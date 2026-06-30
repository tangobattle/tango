//! The on-wire datagram: a generic [`Frame`] over a [`Codec`] — the one type
//! that fixes the [`Element`] each seq slot carries and the per-frame
//! side-channel ([`Meta`]) the rest of the crate is generic over.
//!
//! One datagram is exactly one [`Frame`]: a `base` seq + cumulative `ack`
//! header, a caller-defined [`Meta`] side-channel, and a run of [`Element`]s.
//! There is no envelope tag — a `Frame` *is* the whole message — and no
//! separate ping/pong probe: round-trip latency is derived from the ack
//! round-trip by the caller. Reliability is the receiver's job (see
//! [`crate::stream`]); this module is purely the on-wire (de)serialization of
//! the envelope.
//!
//! Layout of a `Frame` datagram:
//! ```text
//! base   uvarint   always
//! ack    svarint   always; (frontier - base)
//! meta   Meta      always; self-delimiting — Meta::decode reads its own bytes
//! run    Element*  always; each element self-delimits; the run fills the rest
//!                  of the datagram (may be empty)
//! ```
//!
//! rennet itself carries no time-sync field: a `tick_advantage` and the like
//! live in the caller's [`Meta`] type. With the zero-width `()` meta a frame is
//! just `base | ack | run`.
//!
//! The element run is **last**, so the datagram boundary delimits it: rennet
//! decodes elements (each self-delimiting via [`Element::decode`]) until the
//! bytes run out — no length prefix, no count. The `meta` sits between the
//! header and the run, so it too must self-delimit ([`Meta::decode`] reads
//! exactly its own bytes and leaves the rest to the run). There is no
//! data-vs-ack-only distinction on the wire: an "ack-only" frame is simply one
//! whose run is empty. `base` is a plain seq (no reserved sentinel value), so
//! the header needs no `NonZeroU32` trickery.
//!
//! The `ack` rides as a *delta from `base`* rather than as an absolute
//! frontier. Both counters index per-tick streams that advance at the same
//! rate, so at any instant they differ only by the lead/redundancy span
//! (bounded by the rollback horizon) — a small signed number that fits in one
//! svarint byte and an [`i16`], where the absolute frontier grows to three
//! uvarint bytes over a match. The frame stores that delta directly
//! ([`Frame::ack_offset`]); [`Frame::ack`] reconstructs the absolute frontier
//! against `base`. The wire form and the in-memory form are thus the same — no
//! conversion on encode or decode.
use std::io::{self, Read, Write};

/// Cumulative acknowledgement: the receiver's contiguous frontier — the lowest
/// seq it hasn't received yet, i.e. "resend your window from here." That single
/// number is the whole ack: a contiguous resend window is all the sender can act
/// on, so a frontier is all it needs.
pub type Ack = u32;

/// Defines a wire protocol: the [`Element`](Codec::Element) each seq slot
/// carries, the per-frame [`Meta`](Codec::Meta) side-channel, and how each is
/// packed on the wire. One impl — usually a zero-sized marker — is the single
/// type parameter threaded through [`Frame`], [`OutStream`](crate::OutStream),
/// [`InStream`](crate::InStream), and [`Window`](crate::Window), so call sites
/// read `Frame<MyCodec>` rather than carrying each type separately.
///
/// The codec lives here, not as traits on the element/meta types, so those can be
/// plain data — primitives, foreign types, whatever — and one `impl Codec` is
/// all a caller writes. rennet owns the run framing: it calls
/// [`encode_element`](Codec::encode_element) per element on the way out and
/// [`decode_element`](Codec::decode_element) until the datagram's bytes run out
/// on the way in, so **each element must self-delimit**, and the meta likewise (it
/// precedes the run). [`crate::write_svarint`] and friends are the byte-minimal
/// toolkit for the codec methods.
pub trait Codec {
    /// The element each seq slot carries — what the reliability streams
    /// reassemble in order. `Copy` is for the streams' redundancy window; the
    /// `Debug`/`PartialEq`/`Eq` bounds let [`Frame`]/[`Window`](crate::Window)
    /// derive those.
    type Element: Copy + std::fmt::Debug + PartialEq + Eq;
    /// The per-frame side-channel value — a time-sync `tick_advantage`, a flag
    /// word, … — that rennet shuttles but never interprets; use `()` for none.
    /// `Default` is the value the streams report before any frame sets one.
    type Meta: Copy + Default + std::fmt::Debug + PartialEq + Eq;
    /// Hard cap on elements decoded from one datagram. A datagram's size is only
    /// bounded by the transport's max message size, so without this a hostile
    /// peer could make [`Frame::decode`] allocate an enormous run; the rollback
    /// horizon is the natural value (a longer run can't be acted on anyway).
    const MAX_RUN: usize;

    /// Write one element's self-delimiting bytes.
    fn encode_element<W: Write>(element: &Self::Element, w: &mut W) -> io::Result<()>;
    /// Read exactly one element, consuming its own bytes from `r`.
    fn decode_element<R: Read>(r: &mut R) -> io::Result<Self::Element>;
    /// Write the meta's self-delimiting bytes (they precede the run). A `()` meta
    /// writes nothing.
    fn encode_meta<W: Write>(meta: &Self::Meta, w: &mut W) -> io::Result<()>;
    /// Read the meta back, consuming exactly its own bytes from `r`.
    fn decode_meta<R: Read>(r: &mut R) -> io::Result<Self::Meta>;
}

/// One in-match datagram: a `base`/`ack` header, a per-frame [`Meta`], and a run
/// of [`Element`]s. Every frame carries all three; an "ack-only" frame is just
/// one whose run is empty (the reliability streams emit one when there's nothing
/// new to send but an ack to report). The type carries no sentinel and needs no
/// enum tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame<P: Codec> {
    /// Seq of the run's first element (`entries[i]` has seq `base + i`). On an
    /// empty-run frame this is the sender's next unsent seq, carried for
    /// uniformity (there are no elements to place).
    pub base: u32,
    /// Cumulative ack of the peer's stream, stored as a signed offset from
    /// `base` (see the module header) — call [`ack`](Frame::ack) for the
    /// absolute frontier.
    pub ack_offset: i16,
    /// The per-frame side-channel value (see [`Meta`]). The reliability streams
    /// surface the freshest one to the receiver.
    pub meta: P::Meta,
    /// The element run, ascending by seq from `base`; may be empty.
    pub entries: Vec<P::Element>,
}

impl<P: Codec> Frame<P> {
    /// Build a frame from its parts: the `base`/`ack` header (`ack` is the
    /// absolute frontier, stored as an offset from `base`), then the per-frame
    /// `meta` and the element run. For an "ack-only" frame, pass an empty run
    /// (and whatever `meta` the stream currently reports).
    pub fn new(base: u32, ack: Ack, meta: P::Meta, entries: Vec<P::Element>) -> Frame<P> {
        Frame {
            base,
            ack_offset: ack_offset(base, ack),
            meta,
            entries,
        }
    }

    /// The absolute cumulative ack frontier this frame reports, reconstructed
    /// from `base` + [`ack_offset`](Frame::ack_offset). Saturates rather than
    /// wrapping, so a corrupt offset can't fabricate a wild frontier (and the
    /// out-stream clamps the result to what it has sent anyway).
    pub fn ack(&self) -> Ack {
        self.base.saturating_add_signed(self.ack_offset as i32)
    }

    /// Serialize as one whole datagram into `w`. There is no envelope tag — a
    /// frame *is* the message — so this is just the header, the meta, and the
    /// run. [`to_vec`](Frame::to_vec) is the byte-returning convenience form.
    pub fn encode<W: Write>(&self, w: &mut W) -> io::Result<()> {
        write_uvarint(w, self.base as u64)?;
        // The stored `ack_offset` is already the wire form (a signed delta from
        // `base` — see the module header).
        write_svarint(w, self.ack_offset as i64)?;
        P::encode_meta(&self.meta, w)?;
        for e in &self.entries {
            P::encode_element(e, w)?;
        }
        Ok(())
    }

    /// Serialize as one whole datagram into a fresh `Vec`. Convenience over
    /// [`encode`](Frame::encode) for the common "give me the bytes" case —
    /// writing into a `Vec` can't fail.
    pub fn to_vec(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode(&mut out).expect("encoding into a Vec cannot fail");
        out
    }

    /// Decode one whole datagram from `r`: `base`/`ack` header, the
    /// self-delimiting `meta`, then the element run (each element self-delimits;
    /// the run is delimited by the datagram's end). Never leans on a length
    /// prefix or sentinel. `r` must yield exactly one datagram = one frame.
    pub fn decode<R: Read>(r: &mut R) -> io::Result<Frame<P>> {
        // The run is delimited by the datagram's end, so pull the whole datagram
        // in and parse against its length.
        let mut buf = Vec::new();
        r.read_to_end(&mut buf)?;
        let mut c = io::Cursor::new(&buf[..]);
        let base = read_u32(&mut c)?;
        // The ack rides as a signed delta from `base` — stored as-is (see the
        // module header); [`Frame::ack`] reconstructs the absolute frontier.
        let ack_offset =
            i16::try_from(read_svarint(&mut c)?).map_err(|_| invalid("ack offset out of range".to_string()))?;
        // The meta self-delimits, leaving the cursor at the run's first byte.
        let meta = P::decode_meta(&mut c)?;
        // Whatever's left is the run — decode elements until the bytes run out.
        let mut entries = Vec::new();
        while (c.position() as usize) < buf.len() {
            if entries.len() >= P::MAX_RUN {
                return Err(invalid(format!("run exceeds {}-element cap", P::MAX_RUN)));
            }
            entries.push(P::decode_element(&mut c)?);
        }
        Ok(Frame {
            base,
            ack_offset,
            meta,
            entries,
        })
    }
}

/// The signed `base`→frontier delta a frame stores. Bounded by the rollback
/// horizon in practice (both seqs advance together), so it fits an [`i16`].
fn ack_offset(base: u32, ack: Ack) -> i16 {
    let offset = ack as i64 - base as i64;
    assert!(
        i16::try_from(offset).is_ok(),
        "ack offset {offset} exceeds i16 (base={base}, ack={ack}); the two seqs advance together, so it should stay within the rollback horizon"
    );
    offset as i16
}

// --- LEB128 helpers --------------------------------------------------------
//
// Public so an [`Element`] or [`Meta`] impl can pack its fields the same
// byte-minimal way the envelope does.

/// Write `v` as an unsigned LEB128 varint.
pub fn write_uvarint<W: Write>(w: &mut W, mut v: u64) -> io::Result<()> {
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        w.write_all(&[byte])?;
        if v == 0 {
            break;
        }
    }
    Ok(())
}

/// Read an unsigned LEB128 varint, consuming exactly its own bytes from `r`.
pub fn read_uvarint<R: Read>(r: &mut R) -> io::Result<u64> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    loop {
        let mut byte = [0u8; 1];
        r.read_exact(&mut byte)?;
        let b = byte[0];
        if shift >= 64 {
            return Err(invalid("uvarint too long".to_string()));
        }
        value |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    Ok(value)
}

fn read_u32<R: Read>(r: &mut R) -> io::Result<u32> {
    u32::try_from(read_uvarint(r)?).map_err(|_| invalid("value out of u32 range".to_string()))
}

/// Write `v` as a zigzag + LEB128 signed varint.
pub fn write_svarint<W: Write>(w: &mut W, v: i64) -> io::Result<()> {
    write_uvarint(w, zigzag_encode(v))
}

/// Read a zigzag + LEB128 signed varint, consuming exactly its own bytes from `r`.
pub fn read_svarint<R: Read>(r: &mut R) -> io::Result<i64> {
    Ok(zigzag_decode(read_uvarint(r)?))
}

fn zigzag_encode(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

fn zigzag_decode(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

fn invalid(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{El, RawProto, RawUnitProto};

    // Most tests run on `RawProto` (raw-u16 elements + a plain `i16` meta,
    // standing in for a caller's time-sync field; testutil impls [`Meta`] for
    // `i16`).
    type Raw = Frame<RawProto>;

    fn data(base: u32, meta: i16, entries: Vec<El>, ack: Ack) -> Raw {
        Frame::new(base, ack, meta, entries)
    }

    fn roundtrip(f: &Raw) {
        let bytes = f.to_vec();
        let back = Raw::decode(&mut &bytes[..]).expect("decode");
        assert_eq!(f, &back, "roundtrip mismatch; bytes = {bytes:02x?}");
    }

    #[test]
    fn normal_frame_exact_bytes() {
        // base=12345, ack=12345 (delta 0), meta=+2, [Input(0x010), EndOfRound,
        // Input(0x001)]. Header is base + ack-delta, then the meta svarint, then
        // the run running to the datagram end (the test client's raw u16
        // packing, no continuation bits).
        let f = data(
            12345,
            2,
            vec![El::Input(0x010), El::EndOfRound, El::Input(0x001)],
            12345,
        );
        assert_eq!(
            f.to_vec(),
            vec![0xB9, 0x60, 0x00, 0x04, 0x10, 0x00, 0x00, 0x40, 0x01, 0x00]
        );
        roundtrip(&f);
    }

    #[test]
    fn empty_run_frame_carries_meta_and_header() {
        // No elements → an "ack-only" frame. base=5, ack=8 (delta +3), meta=0.
        // With an i16 meta the frame still spends one byte on it: base, ack,
        // meta(0), no run bytes — and it decodes back to an empty run.
        let f = data(5, 0, vec![], 8);
        assert_eq!(f.to_vec(), vec![0x05, 0x06, 0x00]);
        roundtrip(&f);
        assert!(Raw::decode(&mut &f.to_vec()[..]).unwrap().entries.is_empty());
    }

    #[test]
    fn unit_meta_is_zero_width() {
        // With `()` meta a frame is just `base | ack | run`. An empty run then
        // leaves only the two-byte header — there's no separate "ack-only" wire
        // shape, because with nothing in the run and no meta there's nothing to
        // distinguish.
        let f: Frame<RawUnitProto> = Frame::new(5, 8, (), vec![]);
        assert_eq!(f.to_vec(), vec![0x05, 0x06]);
        assert_eq!(Frame::<RawUnitProto>::decode(&mut &f.to_vec()[..]).unwrap(), f);
        // A non-empty run just appends after the (zero-width) meta.
        let g: Frame<RawUnitProto> = Frame::new(1, 1, (), vec![El::Input(0x010)]);
        assert_eq!(g.to_vec(), vec![0x01, 0x00, 0x10, 0x00]);
        assert_eq!(Frame::<RawUnitProto>::decode(&mut &g.to_vec()[..]).unwrap(), g);
    }

    #[test]
    fn ack_is_a_signed_delta_from_base() {
        // base=12345, meta=+2, [Input(0x010)], ack=12340 (5 behind base): the
        // ack is svarint(12340 - 12345) = svarint(-5) = zigzag(9) = one byte,
        // where an absolute frontier would have cost three.
        let f = data(12345, 2, vec![El::Input(0x010)], 12340);
        assert_eq!(f.to_vec(), vec![0xB9, 0x60, 0x09, 0x04, 0x10, 0x00]);
        roundtrip(&f);
        // An ack ahead of base round-trips too (delta is genuinely signed).
        roundtrip(&data(12345, 0, vec![El::Input(0x001)], 12400));
    }

    #[test]
    fn ack_reconstructs_from_offset() {
        // The frame stores the offset; `ack()` adds it back onto `base`.
        for (base, ack) in [(1u32, 1u32), (12345, 12345), (12345, 12340), (12345, 12400), (5, 8)] {
            let f = data(base, 0, vec![El::Input(0)], ack);
            assert_eq!(f.ack_offset as i64, ack as i64 - base as i64);
            assert_eq!(f.ack(), ack);
            // Survives a wire round-trip.
            assert_eq!(Raw::decode(&mut &f.to_vec()[..]).unwrap().ack(), ack);
        }
    }

    #[test]
    fn negative_meta_roundtrips() {
        for m in [-1i16, -2, -64, -300, i16::MIN, i16::MAX, 0, 63, 200] {
            roundtrip(&data(1, m, vec![El::Input(0x3ff)], 1));
        }
    }

    #[test]
    fn large_seqs_roundtrip() {
        roundtrip(&data(1_000_000, 5, vec![El::Input(0x200), El::Input(0x100)], 999_999));
    }

    #[test]
    fn empty_message_errors() {
        assert!(Raw::decode(&mut std::io::empty()).is_err());
    }

    #[test]
    fn truncated_element_errors() {
        // base=1, ack=1 (delta 0), meta=0, then a lone byte — half of a raw-u16
        // element, which `El::decode` (wanting two bytes) rejects.
        let bytes = vec![0x01, 0x00, 0x00, 0x10];
        assert!(Raw::decode(&mut &bytes[..]).is_err());
    }

    #[test]
    fn zigzag_is_invertible() {
        for v in [0i64, 1, -1, 2, -2, i16::MIN as i64, i16::MAX as i64, i64::MAX, i64::MIN] {
            assert_eq!(zigzag_decode(zigzag_encode(v)), v);
        }
    }

    #[test]
    fn uvarint_is_invertible() {
        for v in [0u64, 1, 127, 128, 16383, 16384, 12345, u32::MAX as u64, u64::MAX] {
            let mut out = Vec::new();
            write_uvarint(&mut out, v).unwrap();
            let mut c = io::Cursor::new(&out[..]);
            assert_eq!(read_uvarint(&mut c).unwrap(), v);
            assert_eq!(c.position() as usize, out.len(), "v={v}");
        }
    }
}

//! The on-wire datagram: a generic [`Frame`] over a [`Body`].
//!
//! One datagram is exactly one [`Frame`]: a `base` seq + cumulative `ack`
//! header, optionally followed by a time-sync `tick_advantage` and a [`Body`]
//! of entries. There is no envelope tag — a `Frame` *is* the whole message —
//! and no separate ping/pong probe: round-trip latency is derived from the ack
//! round-trip by the caller. Reliability is the receiver's job (see
//! [`crate::stream`]); this module is purely the on-wire (de)serialization of
//! the envelope.
//!
//! Layout of a `Frame` datagram:
//! ```text
//! base             uvarint   always
//! ack              svarint   always; (frontier - base)
//! tick_advantage  svarint   present iff a body follows
//! body             Body      present iff there are bytes left; runs to the
//!                            end of the datagram
//! ```
//!
//! Because the body is **last**, the datagram boundary delimits it: the body
//! never has to self-delimit, carries no length prefix, and the discriminator
//! between an ack-only frame and a data frame is simply "are there bytes after
//! the header?". `base` is a plain seq (no reserved sentinel value), so the
//! header needs no `NonZeroU32` trickery.
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
use std::io::{self, Read};

/// Cumulative acknowledgement: the receiver's contiguous frontier — the lowest
/// seq it hasn't received yet, i.e. "resend your window from here." That single
/// number is the whole ack: a contiguous resend window is all the sender can act
/// on, so a frontier is all it needs.
pub type Ack = u32;

/// A frame's payload: a run of elements that fills the rest of the datagram.
///
/// rennet never inspects the packing — it just calls [`encode`](Body::encode) /
/// [`decode`](Body::decode) and reads [`elements`](Body::elements) for the
/// reliability streams. Because the body is the last thing in the datagram,
/// [`decode`](Body::decode) is handed exactly its own bytes and
/// [`encode`](Body::encode) just appends — no length prefix, no self-delimiting.
/// An empty body is fine: a data frame is told from an ack-only one by the
/// `tick_advantage` after the header, not by carrying entries, so a zero-entry
/// body just decodes to an empty run.
pub trait Body: Sized {
    /// The entry type this body carries — what the reliability streams
    /// reassemble in seq order.
    type Elem;

    /// Append the run's bytes to `out` (it will be the tail of the datagram).
    fn encode(&self, out: &mut Vec<u8>);

    /// Decode the run from exactly its own bytes (the datagram tail).
    fn decode(bytes: &[u8]) -> io::Result<Self>;

    /// The entries, in seq order.
    fn elements(&self) -> &[Self::Elem];
}

/// One in-match datagram: a `base`/`ack` header, optionally followed by a
/// [`Payload`] (the body and its time-sync advantage). No payload → an ack-only
/// frame; the type carries no sentinel and needs no enum tag, since the
/// datagram either has bytes after the header or it doesn't.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame<B> {
    /// Seq of the payload's first entry (`entries[i]` has seq `base + i`). On an
    /// ack-only frame this is the sender's next unsent seq, carried for
    /// uniformity (there are no entries to place).
    pub base: u32,
    /// Cumulative ack of the peer's stream, stored as a signed offset from
    /// `base` (see the module header) — call [`ack`](Frame::ack) for the
    /// absolute frontier. Present on every frame: it's the whole point of an
    /// ack-only one.
    pub ack_offset: i16,
    /// The entry run plus its time-sync advantage — present iff this frame
    /// carries a body. `None` is an ack-only frame. (A present-but-empty body is
    /// legal too; the reliability streams just never produce one.)
    pub payload: Option<Payload<B>>,
}

/// The body half of a data frame: the entry run and the newest entry's
/// time-sync lead, which ride together (and only when there's a body).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Payload<B> {
    /// The newest entry's time-sync lead.
    pub tick_advantage: i16,
    /// The entry run (see [`Body`]).
    pub body: B,
}

impl<B> Frame<B> {
    /// Build a data frame (header + payload). `ack` is the absolute frontier;
    /// it's stored as an offset from `base`.
    pub fn data(base: u32, tick_advantage: i16, body: B, ack: Ack) -> Frame<B> {
        Frame {
            base,
            ack_offset: ack_offset(base, ack),
            payload: Some(Payload { tick_advantage, body }),
        }
    }

    /// Build an ack-only frame (header, no payload). `base` is the sender's next
    /// unsent seq; `ack` is the absolute frontier.
    pub fn ack_only(base: u32, ack: Ack) -> Frame<B> {
        Frame {
            base,
            ack_offset: ack_offset(base, ack),
            payload: None,
        }
    }

    /// The absolute cumulative ack frontier this frame reports, reconstructed
    /// from `base` + [`ack_offset`](Frame::ack_offset). Saturates rather than
    /// wrapping, so a corrupt offset can't fabricate a wild frontier (and the
    /// out-stream clamps the result to what it has sent anyway).
    pub fn ack(&self) -> Ack {
        self.base.saturating_add_signed(self.ack_offset as i32)
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

impl<B: Body> Frame<B> {
    /// Serialize as one whole datagram. There is no envelope tag — a frame
    /// *is* the message — so this is just the body.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        write_uvarint(&mut out, self.base as u64);
        // The stored `ack_offset` is already the wire form (a signed delta from
        // `base` — see the module header).
        write_svarint(&mut out, self.ack_offset as i64);
        if let Some(p) = &self.payload {
            write_svarint(&mut out, p.tick_advantage as i64);
            p.body.encode(&mut out);
        }
        out
    }

    /// Decode one whole datagram. `base`/`ack` are flat uvarints; any bytes
    /// after them are a `tick_advantage` + body (the body runs to the end), so
    /// this never leans on a length prefix or sentinel. `buf` is one datagram =
    /// one frame.
    pub fn decode(buf: &[u8]) -> io::Result<Frame<B>> {
        let mut c = io::Cursor::new(buf);
        let base = read_u32(&mut c)?;
        // The ack rides as a signed delta from `base` — stored as-is (see the
        // module header); [`Frame::ack`] reconstructs the absolute frontier.
        let ack_offset =
            i16::try_from(read_svarint(&mut c)?).map_err(|_| invalid("ack offset out of range".to_string()))?;
        let payload = if (c.position() as usize) >= buf.len() {
            // Nothing after the header → ack-only.
            None
        } else {
            let tick_advantage =
                i16::try_from(read_svarint(&mut c)?).map_err(|_| invalid("tick_advantage out of range".to_string()))?;
            // Whatever's left is the body — hand it exactly its own bytes.
            let body = B::decode(&buf[c.position() as usize..])?;
            Some(Payload { tick_advantage, body })
        };
        Ok(Frame {
            base,
            ack_offset,
            payload,
        })
    }
}

// --- LEB128 helpers --------------------------------------------------------

fn write_uvarint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if v == 0 {
            break;
        }
    }
}

fn read_uvarint(r: &mut impl Read) -> io::Result<u64> {
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

fn read_u32(r: &mut impl Read) -> io::Result<u32> {
    u32::try_from(read_uvarint(r)?).map_err(|_| invalid("value out of u32 range".to_string()))
}

fn write_svarint(out: &mut Vec<u8>, v: i64) {
    write_uvarint(out, zigzag_encode(v));
}

fn read_svarint(r: &mut impl Read) -> io::Result<i64> {
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
    use crate::testutil::{El, LenBody, RawBody};

    type Raw = Frame<RawBody>;

    fn data(base: u32, fa: i16, entries: Vec<El>, ack: Ack) -> Raw {
        Frame::data(base, fa, RawBody(entries), ack)
    }

    fn roundtrip(f: &Raw) {
        let bytes = f.encode();
        let back = Raw::decode(&bytes).expect("decode");
        assert_eq!(f, &back, "roundtrip mismatch; bytes = {bytes:02x?}");
    }

    #[test]
    fn normal_frame_exact_bytes() {
        // base=12345, ack=12345 (delta 0), adv=+2, [Input(0x010), EndOfRound,
        // Input(0x001)]. Header is base + ack-delta; the body runs to the
        // datagram end with no continuation bits (the test client's raw u16
        // packing).
        let f = data(
            12345,
            2,
            vec![El::Input(0x010), El::EndOfRound, El::Input(0x001)],
            12345,
        );
        assert_eq!(
            f.encode(),
            vec![0xB9, 0x60, 0x00, 0x04, 0x10, 0x00, 0x00, 0x40, 0x01, 0x00]
        );
        roundtrip(&f);
    }

    #[test]
    fn ack_only_frame_exact_bytes() {
        // Just the header: base=5, ack=8 (delta +3). No body bytes follow.
        let f: Raw = Frame::ack_only(5, 8);
        assert_eq!(f.encode(), vec![0x05, 0x06]);
        roundtrip(&f);
    }

    #[test]
    fn ack_is_a_signed_delta_from_base() {
        // base=12345, adv=+2, [Input(0x010)], ack=12340 (5 behind base): the ack
        // is svarint(12340 - 12345) = svarint(-5) = zigzag(9) = one byte, where
        // an absolute frontier would have cost three.
        let f = data(12345, 2, vec![El::Input(0x010)], 12340);
        assert_eq!(f.encode(), vec![0xB9, 0x60, 0x09, 0x04, 0x10, 0x00]);
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
            assert_eq!(Raw::decode(&f.encode()).unwrap().ack(), ack);
        }
    }

    #[test]
    fn negative_tick_advantage_roundtrips() {
        for fa in [-1i16, -2, -64, -300, i16::MIN, i16::MAX, 0, 63, 200] {
            roundtrip(&data(1, fa, vec![El::Input(0x3ff)], 1));
        }
    }

    #[test]
    fn large_seqs_roundtrip() {
        roundtrip(&data(1_000_000, 5, vec![El::Input(0x200), El::Input(0x100)], 999_999));
    }

    #[test]
    fn empty_message_errors() {
        assert!(Raw::decode(&[]).is_err());
    }

    #[test]
    fn empty_body_data_frame_is_distinct_from_ack_only() {
        // A data frame with an empty body still carries a `tick_advantage`, so
        // it's told apart from an ack-only frame and decodes to an empty run —
        // no ">= 1 entry" rule needed.
        let empty: Raw = Frame::data(3, 7, RawBody(vec![]), 5);
        let back = Raw::decode(&empty.encode()).unwrap();
        assert_eq!(back, empty);
        assert!(back.payload.as_ref().unwrap().body.0.is_empty());
        // The ack-only frame with the same header has no payload, and a
        // different wire form (no `tick_advantage` byte).
        let ack: Raw = Frame::ack_only(3, 5);
        assert!(Raw::decode(&ack.encode()).unwrap().payload.is_none());
        assert_ne!(empty.encode(), ack.encode());
    }

    #[test]
    fn body_decode_error_propagates() {
        // base=1, ack=1 (delta 0), adv=0, then a one-byte body the test client's
        // codec rejects (it wants a whole number of u16 entries).
        let bytes = vec![0x01, 0x00, 0x00, 0x10];
        assert!(Raw::decode(&bytes).is_err());
    }

    /// rennet frames the same header around any [`Body`]; swapping the test
    /// client's raw run for a length-prefixed packing needs no change here,
    /// proving the envelope is body-agnostic.
    #[test]
    fn a_different_body_packing_also_round_trips() {
        let f: Frame<LenBody> = Frame::data(
            7,
            -3,
            LenBody(vec![El::Input(0x3ff), El::EndOfRound, El::EndOfMatch]),
            6,
        );
        assert_eq!(Frame::<LenBody>::decode(&f.encode()).unwrap(), f);
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
            write_uvarint(&mut out, v);
            let mut c = io::Cursor::new(&out[..]);
            assert_eq!(read_uvarint(&mut c).unwrap(), v);
            assert_eq!(c.position() as usize, out.len(), "v={v}");
        }
    }
}

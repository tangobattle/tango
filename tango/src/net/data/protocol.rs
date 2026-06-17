//! In-match wire format for the unreliable netplay datagram channel. This is
//! the byte-minimized, loss-tolerant replacement for the per-frame input /
//! end-of-round traffic that used to ride the reliable lobby channel.
//!
//! One datagram is exactly one [`Frame`]: a per-tick input window plus a
//! cumulative ack of the peer's stream (or a bare ack). There is no envelope
//! tag — a `Frame` is the whole message — and no separate ping/pong probe:
//! round-trip latency is derived from the ack round-trip (see
//! [`super::InMatchTx`]). Reliability is the receiver's job — inputs are
//! recovered by a redundancy window keyed on a monotonic seq, round/match
//! boundaries ride in-band as marker entries, and a cumulative ack (the peer's
//! contiguous frontier) drives the sender's window. None of that state lives
//! here; this module is purely the on-wire (de)serialization.
//!
//! Layout of a `Frame` datagram:
//! ```text
//! base             uvarint   0 => ack-only frame (then the ack as a raw
//!                            uvarint frontier); else 1-based seq of entries[0]
//! frame_advantage  svarint   present iff base != 0
//! entries[]        u16 LE    present iff base != 0; >= 1; until CONT clear
//! ack              svarint   present iff base != 0; (frontier - base)
//! ```
//! Each entry `u16`: bit 15 = CONT (more follow), bit 14 = MARK (marker vs
//! input), bits 0..=9 = payload (joyflags, or marker kind). The CONT bit
//! self-delimits the entry run and the mandatory ack closes the frame, so
//! every field is self-describing — no length prefix, no truncation inference.
//!
//! The piggybacked `ack` is encoded as a *delta from `base`* rather than as an
//! absolute frontier. Both counters index per-tick streams that advance at the
//! same ~60 Hz, so at any instant they differ only by the lead/redundancy span
//! (bounded by the rollback horizon) — a small signed number that fits in one
//! svarint byte, where the absolute frontier grows to three uvarint bytes over a
//! match. The in-memory [`Ack`] stays absolute; only the wire form is relative.
//!
use std::io::{self, Read};

/// Bit 15 of a stream entry: another entry follows this one.
const CONT: u16 = 0x8000;
/// Bit 14 of a stream entry: this slot is a marker, not an input.
const MARK: u16 = 0x4000;
/// Bits 0..=9 of a stream entry: joyflags, or a marker kind. The GBA keypad
/// is 10 bits, so the top 6 bits are free for CONT/MARK and stay reserved.
pub const PAYLOAD_MASK: u16 = 0x03ff;

/// Marker kind, carried in an entry's payload when [`MARK`] is set.
const KIND_END_OF_ROUND: u16 = 0;
const KIND_END_OF_MATCH: u16 = 1;

/// Hard cap on entries decoded from one frame. A legitimate redundancy window
/// can't exceed the rollback horizon (the out-stream trims it to that), so a
/// frame claiming more is malformed or hostile. Bounds the decode allocation
/// regardless of transport — UDP is incidentally capped by its recv buffer,
/// but a WebRTC peer's datagram is only bounded by the SCTP max message size.
const MAX_ENTRIES: usize = tango_pvp::battle::MAX_QUEUE_LENGTH;

/// One element of the input stream, occupying a single seq slot: either a
/// tick's input, or a round/match boundary that rides in-band on the seq line.
/// The boundary variants are wire-tagged by [`MARK`] + a `KIND_*` payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element {
    /// Joyflags for this tick (10-bit GBA keypad; the top 6 bits must be 0).
    Input(u16),
    /// End-of-round boundary — the round its preceding inputs belong to ends here.
    EndOfRound,
    /// End-of-match boundary.
    EndOfMatch,
}

/// Cumulative acknowledgement: the receiver's contiguous frontier — the lowest
/// seq it hasn't received yet, i.e. "resend your window from here." That single
/// number is the whole ack: the sender always resends a contiguous run up to
/// its newest input, so a frontier is all it can act on (a bitmap of
/// out-of-order receipts above the frontier couldn't be skipped in a contiguous
/// frame anyway). The window size the sender resends is `newest - frontier + 1`,
/// which it derives from its own counter.
pub type Ack = u32;

/// One in-match datagram's payload: either a tick of input data, or a bare
/// cumulative ack.
///
/// Modeled as a sum type so the wire's `base == 0` ack-only sentinel can't be
/// confused with a data tick: an ack-only frame has no advantage and no
/// entries *by construction*, a data frame's seq is non-zero *by type*, and
/// the throttler can never be fed a synthetic-zero advantage. (The old loose
/// `{ base, frame_advantage: Option, entries, ack }` shape needed a runtime
/// invariant check to keep these in sync.)
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    /// Ack-only: no input this tick, just a cumulative ack of the peer's stream
    /// (encoded as the `base == 0` sentinel). Sent during stalls / as a
    /// reconnect resync.
    Ack(Ack),
    /// A tick of input data.
    Data {
        /// 1-based global seq of `entries[0]` (`entries[i]` has seq
        /// `base + i`). Non-zero by type — `0` is the ack-only sentinel.
        base: std::num::NonZeroU32,
        /// The newest entry's time-sync lead.
        frame_advantage: i16,
        /// Inputs + markers in seq order; non-empty (a window always carries
        /// >= 1 element, and the decoder reads >= 1).
        entries: Vec<Element>,
        /// Piggybacked cumulative ack of the peer's stream, as an absolute
        /// frontier (the wire encodes it as a signed delta from `base`). Every
        /// data frame carries one: the reassembler always has a frontier to
        /// report, so there's never a reason to omit it.
        ack: Ack,
    },
}

impl Frame {
    /// Build a [`Frame::Data`] from a 1-based seq. Panics if `base == 0` —
    /// that value is the ack-only sentinel, never a data seq. Callers source
    /// `base` from the 1-based out-stream window, so it never fires in
    /// practice; it just keeps the `NonZeroU32` construction ergonomic.
    pub fn data(base: u32, frame_advantage: i16, entries: Vec<Element>, ack: Ack) -> Frame {
        Frame::Data {
            base: std::num::NonZeroU32::new(base).expect("data frame seq is 1-based (non-zero)"),
            frame_advantage,
            entries,
            ack,
        }
    }

    /// Serialize as one whole datagram. There is no envelope tag — a frame
    /// *is* the message — so this is just the body.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        encode_frame_body(self, &mut out);
        out
    }

    /// Decode one whole datagram. Every field is self-delimiting now that the
    /// ack is mandatory, so this no longer leans on the exact message length;
    /// `buf` is still one datagram = one frame.
    pub fn decode(buf: &[u8]) -> io::Result<Frame> {
        decode_frame_body(buf)
    }
}

fn encode_frame_body(f: &Frame, out: &mut Vec<u8>) {
    match f {
        Frame::Ack(frontier) => {
            write_uvarint(out, 0); // base == 0 sentinel
            write_uvarint(out, *frontier as u64);
        }
        Frame::Data {
            base,
            frame_advantage,
            entries,
            ack,
        } => {
            debug_assert!(!entries.is_empty(), "data frame needs >= 1 entry");
            write_uvarint(out, base.get() as u64);
            write_svarint(out, *frame_advantage as i64);
            let n = entries.len();
            for (i, e) in entries.iter().enumerate() {
                let mut w = match e {
                    Element::Input(joyflags) => {
                        assert!(
                            joyflags & !PAYLOAD_MASK == 0,
                            "joyflags use reserved high bits: {joyflags:#06x}"
                        );
                        joyflags & PAYLOAD_MASK
                    }
                    Element::EndOfRound => MARK | KIND_END_OF_ROUND,
                    Element::EndOfMatch => MARK | KIND_END_OF_MATCH,
                };
                if i + 1 != n {
                    w |= CONT;
                }
                out.extend_from_slice(&w.to_le_bytes());
            }
            // `ack` is appended raw (no discriminant), as a signed delta from
            // `base` — see the module header — so the common case is one byte.
            write_svarint(out, *ack as i64 - base.get() as i64);
        }
    }
}

fn decode_frame_body(body: &[u8]) -> io::Result<Frame> {
    let mut c = io::Cursor::new(body);

    let Some(base) = std::num::NonZeroU32::new(read_uvarint(&mut c)? as u32) else {
        return Ok(Frame::Ack(read_uvarint(&mut c)? as u32));
    };

    let frame_advantage =
        i16::try_from(read_svarint(&mut c)?).map_err(|_| invalid("frame_advantage out of range".to_string()))?;
    let mut entries = Vec::new();
    loop {
        let w = read_u16le(&mut c)?;
        let element = if w & MARK != 0 {
            match w & PAYLOAD_MASK {
                KIND_END_OF_ROUND => Element::EndOfRound,
                KIND_END_OF_MATCH => Element::EndOfMatch,
                other => return Err(invalid(format!("unknown marker kind: {other}"))),
            }
        } else {
            Element::Input(w & PAYLOAD_MASK)
        };
        entries.push(element);
        if w & CONT == 0 {
            break;
        }
        // Reject a runaway/hostile window before it can grow the allocation.
        if entries.len() >= MAX_ENTRIES {
            return Err(invalid(format!("frame exceeds {MAX_ENTRIES}-entry window cap")));
        }
    }
    // The entries are followed by the mandatory cumulative ack, carried as a
    // signed delta from `base` (see the module header). A frame that stops here
    // is malformed; `read_svarint` bottoms out in `read_exact`, so the missing
    // bytes surface as a decode error on their own.
    let frontier = base.get() as i64 + read_svarint(&mut c)?;
    if !(0..=u32::MAX as i64).contains(&frontier) {
        return Err(invalid(format!("ack delta puts frontier out of range: {frontier}")));
    }
    Ok(Frame::Data {
        base,
        frame_advantage,
        entries,
        ack: frontier as u32,
    })
}

// --- LEB128 + fixed-width helpers ------------------------------------------

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

fn read_u16le(r: &mut impl Read) -> io::Result<u16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}

fn invalid(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(f: &Frame) {
        let bytes = f.encode();
        let back = Frame::decode(&bytes).expect("decode");
        assert_eq!(f, &back, "roundtrip mismatch; bytes = {bytes:02x?}");
    }

    #[test]
    fn normal_frame_exact_bytes() {
        // base=12345, adv=+2, [Right(0x010), EndOfRound, A(0x001)], ack=12345
        // (== base, so the delta is 0 → a single trailing 0x00 byte).
        let f = Frame::data(
            12345,
            2,
            vec![Element::Input(0x010), Element::EndOfRound, Element::Input(0x001)],
            12345,
        );
        assert_eq!(
            f.encode(),
            vec![0xB9, 0x60, 0x04, 0x10, 0x80, 0x00, 0xC0, 0x01, 0x00, 0x00]
        );
        roundtrip(&f);
    }

    #[test]
    fn ack_only_frame_exact_bytes() {
        // base=0 sentinel, then frontier=12340.
        let f = Frame::Ack(12340);
        assert_eq!(f.encode(), vec![0x00, 0xB4, 0x60]);
        roundtrip(&f);
    }

    #[test]
    fn normal_frame_with_ack_roundtrips() {
        roundtrip(&Frame::data(
            12345,
            2,
            vec![Element::Input(0x010), Element::Input(0x011), Element::Input(0x001)],
            12340,
        ));
    }

    #[test]
    fn ack_is_a_signed_delta_from_base() {
        // base=12345, adv=+2, [Right(0x010)], ack=12340 (5 behind base).
        // The ack is svarint(12340 - 12345) = svarint(-5) = zigzag(9) = one byte,
        // where an absolute frontier would have cost three.
        let f = Frame::data(12345, 2, vec![Element::Input(0x010)], 12340);
        assert_eq!(f.encode(), vec![0xB9, 0x60, 0x04, 0x10, 0x00, 0x09]);
        roundtrip(&f);
        // An ack ahead of base round-trips too (delta is genuinely signed).
        roundtrip(&Frame::data(12345, 0, vec![Element::Input(0x001)], 12400));
    }

    #[test]
    fn negative_frame_advantage_roundtrips() {
        for fa in [-1i16, -2, -64, -300, i16::MIN, i16::MAX, 0, 63, 200] {
            roundtrip(&Frame::data(1, fa, vec![Element::Input(0x3ff)], 1));
        }
    }

    #[test]
    fn marker_as_last_entry_roundtrips() {
        roundtrip(&Frame::data(7, 0, vec![Element::Input(0x10), Element::EndOfMatch], 6));
    }

    #[test]
    fn single_entry_window_roundtrips() {
        roundtrip(&Frame::data(1, 0, vec![Element::Input(0)], 1));
    }

    #[test]
    fn large_seqs_roundtrip() {
        roundtrip(&Frame::data(
            1_000_000,
            5,
            vec![Element::Input(0x200), Element::Input(0x100)],
            999_999,
        ));
    }

    #[test]
    fn unknown_marker_kind_errors() {
        // base=1, adv=0, then an entry with MARK set and payload kind 2 (no
        // such marker yet).
        let bytes = vec![0x01, 0x00, 0x02, 0x40];
        assert!(Frame::decode(&bytes).is_err());
    }

    #[test]
    fn empty_message_errors() {
        assert!(Frame::decode(&[]).is_err());
    }

    #[test]
    fn data_frame_missing_ack_errors() {
        // base=1, adv=0, one terminating entry Input(0) — but no trailing ack.
        // The ack is mandatory now, so a frame that stops at the entry run is
        // malformed (it would have decoded as `ack: None` before).
        let bytes = vec![0x01, 0x00, 0x00, 0x00];
        assert!(Frame::decode(&bytes).is_err());
    }

    #[test]
    fn max_window_decodes() {
        // Exactly the cap is legitimate (the last entry terminates the run).
        let f = Frame::data(1, 0, vec![Element::Input(1); MAX_ENTRIES], 1);
        assert_eq!(Frame::decode(&f.encode()).unwrap(), f);
    }

    #[test]
    fn over_long_window_errors() {
        // base=1, adv=0, then a run of CONT-set entries with no terminator
        // inside the cap — a hostile peer trying to force a huge allocation.
        let mut bytes = vec![0x01, 0x00];
        for _ in 0..=MAX_ENTRIES {
            bytes.extend_from_slice(&(CONT | 0x001).to_le_bytes());
        }
        assert!(Frame::decode(&bytes).is_err());
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

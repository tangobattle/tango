//! In-match wire format for the (future) unreliable netplay datagram
//! channel. This is the byte-minimized, loss-tolerant replacement for the
//! per-frame `Packet::Input` / `Packet::EndOfRound` traffic on the
//! reliable lobby channel.
//!
//! A [`Packet`] message is one datagram: a [`Frame`] (the per-tick input
//! window + a block-ack of the peer's stream), or a `Ping`/`Pong` probe.
//! Reliability is the receiver's job — inputs are recovered by a redundancy
//! window keyed on a monotonic seq, round/match boundaries ride in-band as
//! marker entries, and a block-ack drives the sender's window. None of that
//! state lives here; this module is purely the on-wire (de)serialization.
//!
//! Layout of a `Frame` body (after the 1-byte `Packet` tag):
//! ```text
//! base             uvarint   0 => ack-only (no advantage, no entries);
//!                            else 1-based global seq of entries[0]
//! frame_advantage  svarint   present iff base != 0
//! entries[]        u16 LE    present iff base != 0; >= 1; until CONT clear
//! ack (optional)   present iff bytes remain:  ack_base uvarint, ack_bits uvarint
//! ```
//! Each entry `u16`: bit 15 = CONT (more follow), bit 14 = MARK (marker vs
//! input), bits 0..=9 = payload (joyflags, or marker kind). `ack` is the
//! sole truncation-inferred field, which works because the transport hands
//! us one exact-length message per datagram.
//!
use std::io::{self, Read};

/// Bit 15 of a stream entry: another entry follows this one.
const CONT: u16 = 0x8000;
/// Bit 14 of a stream entry: this slot is a marker, not an input.
const MARK: u16 = 0x4000;
/// Bits 0..=9 of a stream entry: joyflags, or a marker kind. The GBA keypad
/// is 10 bits, so the top 6 bits are free for CONT/MARK and stay reserved.
const PAYLOAD_MASK: u16 = 0x03ff;

/// Marker kind, carried in an entry's payload when [`MARK`] is set.
const KIND_END_OF_ROUND: u16 = 0;
const KIND_END_OF_MATCH: u16 = 1;

/// `Wire` envelope tags (first byte of every in-match datagram).
const TAG_FRAME: u8 = 0;
const TAG_PING: u8 = 1;
const TAG_PONG: u8 = 2;

/// Hard cap on entries decoded from one frame. A legitimate redundancy window
/// can't exceed the rollback horizon (the out-stream trims it to that), so a
/// frame claiming more is malformed or hostile. Bounds the decode allocation
/// regardless of transport — UDP is incidentally capped by its recv buffer,
/// but a WebRTC peer's datagram is only bounded by the SCTP max message size.
const MAX_ENTRIES: usize = tango_pvp::battle::MAX_QUEUE_LENGTH;

/// A round/match boundary that occupies a seq slot in the input stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    EndOfRound,
    EndOfMatch,
}

impl Marker {
    fn kind(self) -> u16 {
        match self {
            Marker::EndOfRound => KIND_END_OF_ROUND,
            Marker::EndOfMatch => KIND_END_OF_MATCH,
        }
    }

    fn from_kind(kind: u16) -> io::Result<Marker> {
        match kind {
            KIND_END_OF_ROUND => Ok(Marker::EndOfRound),
            KIND_END_OF_MATCH => Ok(Marker::EndOfMatch),
            other => Err(invalid(format!("unknown marker kind: {other}"))),
        }
    }
}

/// One element of the input stream, occupying a single seq slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element {
    /// Joyflags for this tick (10-bit GBA keypad; the top 6 bits must be 0).
    Input(u16),
    Marker(Marker),
}

/// Selective acknowledgement of the peer's input stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockAck {
    /// Lowest seq not yet contiguously received (the SSN). The peer trims
    /// its retransmit window to start here.
    pub base: u32,
    /// Bit `i` set => seq `base + i` was received out of order. Usually 0.
    pub bits: u32,
}

/// One in-match datagram's payload: either a tick of input data, or a bare
/// block-ack.
///
/// Modeled as a sum type so the wire's `base == 0` ack-only sentinel can't be
/// confused with a data tick: an ack-only frame has no advantage and no
/// entries *by construction*, a data frame's seq is non-zero *by type*, and
/// the throttler can never be fed a synthetic-zero advantage. (The old loose
/// `{ base, frame_advantage: Option, entries, ack }` shape needed a runtime
/// invariant check to keep these in sync.)
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    /// Ack-only: no input this tick, just a block-ack of the peer's stream
    /// (encoded as the `base == 0` sentinel). Sent during stalls / as a
    /// reconnect resync.
    Ack(BlockAck),
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
        /// Optional piggybacked block-ack of the peer's stream. Absent => zero
        /// trailing bytes (inferred from the datagram's exact length).
        ack: Option<BlockAck>,
    },
}

impl Frame {
    /// Build a [`Frame::Data`] from a 1-based seq. Panics if `base == 0` —
    /// that value is the ack-only sentinel, never a data seq. Callers source
    /// `base` from the 1-based out-stream window, so it never fires in
    /// practice; it just keeps the `NonZeroU32` construction ergonomic.
    pub fn data(base: u32, frame_advantage: i16, entries: Vec<Element>, ack: Option<BlockAck>) -> Frame {
        Frame::Data {
            base: std::num::NonZeroU32::new(base).expect("data frame seq is 1-based (non-zero)"),
            frame_advantage,
            entries,
            ack,
        }
    }
}

/// One message on the in-match channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Packet {
    Frame(Frame),
    /// Latency probe carrying the sender's short timestamp (ms, wrapping).
    Ping(u16),
    /// Echo of a `Ping`'s timestamp.
    Pong(u16),
}

impl Packet {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Packet::Frame(f) => {
                out.push(TAG_FRAME);
                encode_frame_body(f, &mut out);
            }
            Packet::Ping(ts) => {
                out.push(TAG_PING);
                out.extend_from_slice(&ts.to_le_bytes());
            }
            Packet::Pong(ts) => {
                out.push(TAG_PONG);
                out.extend_from_slice(&ts.to_le_bytes());
            }
        }
        out
    }

    /// Decode one whole datagram. `buf` must be exactly one message — the
    /// trailing-ack inference relies on the datagram boundary.
    pub fn decode(buf: &[u8]) -> io::Result<Packet> {
        let (&tag, rest) = buf
            .split_first()
            .ok_or_else(|| invalid("empty wire message".to_string()))?;
        match tag {
            TAG_FRAME => Ok(Packet::Frame(decode_frame_body(rest)?)),
            TAG_PING => Ok(Packet::Ping(read_u16_whole(rest)?)),
            TAG_PONG => Ok(Packet::Pong(read_u16_whole(rest)?)),
            other => Err(invalid(format!("unknown wire tag: {other}"))),
        }
    }
}

fn encode_frame_body(f: &Frame, out: &mut Vec<u8>) {
    match f {
        Frame::Ack(ack) => {
            write_uvarint(out, 0); // base == 0 sentinel
            write_uvarint(out, ack.base as u64);
            write_uvarint(out, ack.bits as u64);
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
                    Element::Marker(m) => MARK | m.kind(),
                };
                if i + 1 != n {
                    w |= CONT;
                }
                out.extend_from_slice(&w.to_le_bytes());
            }
            // `ack` is appended raw (no discriminant); its absence is zero bytes.
            if let Some(a) = ack {
                write_uvarint(out, a.base as u64);
                write_uvarint(out, a.bits as u64);
            }
        }
    }
}

fn decode_frame_body(body: &[u8]) -> io::Result<Frame> {
    let mut c = io::Cursor::new(body);
    let base = read_uvarint(&mut c)? as u32;
    let remaining = |c: &io::Cursor<&[u8]>| (c.position() as usize) < body.len();

    if base == 0 {
        // Ack-only: the remainder is the block-ack (required — a base==0 frame
        // exists to carry one; a fully empty frame is rejected).
        if !remaining(&c) {
            return Err(invalid("ack-only frame missing block-ack".to_string()));
        }
        let ack = BlockAck {
            base: read_uvarint(&mut c)? as u32,
            bits: read_uvarint(&mut c)? as u32,
        };
        return Ok(Frame::Ack(ack));
    }

    let base = std::num::NonZeroU32::new(base).expect("base != 0 checked above");
    let frame_advantage =
        i16::try_from(read_svarint(&mut c)?).map_err(|_| invalid("frame_advantage out of range".to_string()))?;
    let mut entries = Vec::new();
    loop {
        let w = read_u16le(&mut c)?;
        let element = if w & MARK != 0 {
            Element::Marker(Marker::from_kind(w & PAYLOAD_MASK)?)
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
    // Whatever is left after the entries is the optional block-ack.
    let ack = if remaining(&c) {
        Some(BlockAck {
            base: read_uvarint(&mut c)? as u32,
            bits: read_uvarint(&mut c)? as u32,
        })
    } else {
        None
    };
    Ok(Frame::Data {
        base,
        frame_advantage,
        entries,
        ack,
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

/// Read a `u16` that must be exactly the whole remaining slice (used for the
/// fixed-size `Ping`/`Pong` payload — reject trailing garbage).
fn read_u16_whole(rest: &[u8]) -> io::Result<u16> {
    match rest {
        [lo, hi] => Ok(u16::from_le_bytes([*lo, *hi])),
        _ => Err(invalid(format!(
            "ping/pong payload must be 2 bytes, got {}",
            rest.len()
        ))),
    }
}

fn invalid(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(w: &Packet) {
        let bytes = w.encode();
        let back = Packet::decode(&bytes).expect("decode");
        assert_eq!(w, &back, "roundtrip mismatch; bytes = {bytes:02x?}");
    }

    #[test]
    fn normal_frame_exact_bytes() {
        // From the spec: base=12345, adv=+2, [Right(0x010), EndOfRound, A(0x001)], no ack.
        let f = Packet::Frame(Frame::data(
            12345,
            2,
            vec![
                Element::Input(0x010),
                Element::Marker(Marker::EndOfRound),
                Element::Input(0x001),
            ],
            None,
        ));
        assert_eq!(
            f.encode(),
            vec![0x00, 0xB9, 0x60, 0x04, 0x10, 0x80, 0x00, 0xC0, 0x01, 0x00]
        );
        roundtrip(&f);
    }

    #[test]
    fn ack_only_frame_exact_bytes() {
        // base=0, ack_base=12340, bits=0.
        let f = Packet::Frame(Frame::Ack(BlockAck { base: 12340, bits: 0 }));
        assert_eq!(f.encode(), vec![0x00, 0x00, 0xB4, 0x60, 0x00]);
        roundtrip(&f);
    }

    #[test]
    fn ping_pong_exact_bytes() {
        assert_eq!(Packet::Ping(0x1234).encode(), vec![0x01, 0x34, 0x12]);
        assert_eq!(Packet::Pong(0x1234).encode(), vec![0x02, 0x34, 0x12]);
        roundtrip(&Packet::Ping(0x1234));
        roundtrip(&Packet::Pong(0));
    }

    #[test]
    fn normal_frame_with_ack_roundtrips() {
        roundtrip(&Packet::Frame(Frame::data(
            12345,
            2,
            vec![Element::Input(0x010), Element::Input(0x011), Element::Input(0x001)],
            Some(BlockAck {
                base: 12340,
                bits: 0b1011,
            }),
        )));
    }

    #[test]
    fn negative_frame_advantage_roundtrips() {
        for fa in [-1i16, -2, -64, -300, i16::MIN, i16::MAX, 0, 63, 200] {
            roundtrip(&Packet::Frame(Frame::data(1, fa, vec![Element::Input(0x3ff)], None)));
        }
    }

    #[test]
    fn marker_as_last_entry_roundtrips() {
        roundtrip(&Packet::Frame(Frame::data(
            7,
            0,
            vec![Element::Input(0x10), Element::Marker(Marker::EndOfMatch)],
            Some(BlockAck { base: 6, bits: 0 }),
        )));
    }

    #[test]
    fn single_entry_window_roundtrips() {
        roundtrip(&Packet::Frame(Frame::data(1, 0, vec![Element::Input(0)], None)));
    }

    #[test]
    fn large_seqs_roundtrip() {
        roundtrip(&Packet::Frame(Frame::data(
            1_000_000,
            5,
            vec![Element::Input(0x200), Element::Input(0x100)],
            Some(BlockAck {
                base: 999_999,
                bits: 0xFFFF_FFFF,
            }),
        )));
    }

    #[test]
    fn unknown_marker_kind_errors() {
        // MARK set with payload kind 2 (no such marker yet).
        let bytes = vec![0x00, 0x01, 0x00, 0x02, 0x40];
        assert!(Packet::decode(&bytes).is_err());
    }

    #[test]
    fn unknown_tag_errors() {
        assert!(Packet::decode(&[0x09, 0x00]).is_err());
    }

    #[test]
    fn empty_message_errors() {
        assert!(Packet::decode(&[]).is_err());
    }

    #[test]
    fn ping_with_trailing_garbage_errors() {
        assert!(Packet::decode(&[TAG_PING, 0x00, 0x00, 0x00]).is_err());
    }

    #[test]
    fn max_window_decodes() {
        // Exactly the cap is legitimate (the last entry terminates the run).
        let f = Packet::Frame(Frame::data(1, 0, vec![Element::Input(1); MAX_ENTRIES], None));
        assert_eq!(Packet::decode(&f.encode()).unwrap(), f);
    }

    #[test]
    fn over_long_window_errors() {
        // base=1, adv=0, then a run of CONT-set entries with no terminator
        // inside the cap — a hostile peer trying to force a huge allocation.
        let mut bytes = vec![TAG_FRAME, 0x01, 0x00];
        for _ in 0..=MAX_ENTRIES {
            bytes.extend_from_slice(&(CONT | 0x001).to_le_bytes());
        }
        assert!(Packet::decode(&bytes).is_err());
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

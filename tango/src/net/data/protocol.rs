//! tango's concrete in-match payload for the unreliable netplay datagram
//! channel: the [`Element`] each seq slot carries, and the [`Entries`] body that
//! packs a run of them onto the wire.
//!
//! The envelope (per-tick seq `base`, the delta-encoded cumulative `ack`, and
//! the `tick_advantage`), the LEB128 codec, and the redundancy-window /
//! cumulative-ack reliability machinery all live in the transport- and
//! packing-agnostic [`rennet`] crate. rennet doesn't know or care how a body is
//! laid out — it just calls [`Body::encode`](rennet::Body::encode) /
//! [`decode`](rennet::Body::decode) and reads its
//! [`elements`](rennet::Body::elements). This module supplies that body.
//!
//! The body is the last thing in the datagram, so rennet hands [`Entries`]
//! exactly its own bytes and the run needs no length prefix — it's read until
//! the bytes run out. Entries are **variable-length**, tagged by the top bit of
//! their first byte:
//!
//! * an **input** is two bytes — the 10-bit joyflags, high byte first (so its
//!   top bit is clear, since joyflags never exceed `0x3ff`);
//! * a **marker** (round/match boundary) is a single byte with the top bit set
//!   and the kind in the low bits.
//!
//! So inputs (the common case) stay two bytes while markers cost just one, and
//! the decoder tells them apart from that first byte alone.

use std::io;

use tango_pvp::input::JOYFLAGS_MASK;

/// Top bit of an entry's first byte: set => a 1-byte marker, clear => the high
/// byte of a 2-byte input (always clear there, as joyflags fit in 10 bits).
const MARKER_FLAG: u8 = 0x80;

/// Marker kind, carried in the low bits of a marker byte.
const KIND_END_OF_ROUND: u8 = 0;
const KIND_END_OF_MATCH: u8 = 1;

/// Hard cap on entries decoded from one body. A legitimate redundancy window
/// can't exceed the rollback horizon (the out-stream trims it to that), so a
/// body claiming more is malformed or hostile.
const MAX_ENTRIES: usize = tango_pvp::battle::MAX_QUEUE_LENGTH;

/// Rollback horizon for the in-match reliability streams: a gap wider than this
/// can't be rolled back to, so the receiver bails. Matches the engine's input
/// buffer cap (`round.rs` bails locally at the same depth).
pub const HORIZON: u32 = tango_pvp::battle::MAX_QUEUE_LENGTH as u32;

/// Cumulative ack frontier — re-exported so callers keep saying `protocol::Ack`.
pub use rennet::Ack;

/// One whole in-match datagram: tango's [`Entries`] body wired into the generic
/// [`rennet::Frame`]. A `Frame` *is* the message — see [`rennet::frame`] for the
/// envelope layout. Build data frames with [`data_frame`] and ack-only frames
/// with [`rennet::Frame::ack_only`].
pub type Frame = rennet::Frame<Entries>;

/// One element of the input stream, occupying a single seq slot: either a
/// tick's input, or a round/match boundary that rides in-band on the seq line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element {
    /// Joyflags for this tick (10-bit GBA keypad; the top 6 bits must be 0).
    Input(u16),
    /// End-of-round boundary — the round its preceding inputs belong to ends here.
    EndOfRound,
    /// End-of-match boundary.
    EndOfMatch,
}

impl Element {
    /// Append this entry's wire bytes (see the module header).
    fn encode_into(self, out: &mut Vec<u8>) {
        match self {
            Element::Input(joyflags) => {
                assert!(
                    joyflags & !JOYFLAGS_MASK == 0,
                    "joyflags use reserved high bits: {joyflags:#06x}"
                );
                // High byte first; its top bit is clear (joyflags <= 0x3ff),
                // which is what tells an input from a marker on the way back in.
                out.push((joyflags >> 8) as u8);
                out.push(joyflags as u8);
            }
            Element::EndOfRound => out.push(MARKER_FLAG | KIND_END_OF_ROUND),
            Element::EndOfMatch => out.push(MARKER_FLAG | KIND_END_OF_MATCH),
        }
    }
}

/// tango's frame body: a variable-length run of [`Element`] entries with no
/// internal framing — the datagram boundary delimits it (see the module
/// header). Exactly the contract [`rennet::Body`] asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entries(pub Vec<Element>);

impl rennet::Body for Entries {
    type Elem = Element;

    fn encode(&self, out: &mut Vec<u8>) {
        for e in &self.0 {
            e.encode_into(out);
        }
    }

    fn decode(bytes: &[u8]) -> io::Result<Self> {
        let mut entries = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let b0 = bytes[i];
            i += 1;
            let element = if b0 & MARKER_FLAG != 0 {
                match b0 & !MARKER_FLAG {
                    KIND_END_OF_ROUND => Element::EndOfRound,
                    KIND_END_OF_MATCH => Element::EndOfMatch,
                    other => return Err(invalid(format!("unknown marker kind: {other}"))),
                }
            } else {
                // Input: this byte's the high half, the next its low half.
                let b1 = *bytes
                    .get(i)
                    .ok_or_else(|| invalid("input entry truncated".to_string()))?;
                i += 1;
                Element::Input(((b0 as u16) << 8 | b1 as u16) & JOYFLAGS_MASK)
            };
            entries.push(element);
            // The body is bounded by the datagram, but a hostile peer's datagram
            // is only capped by the SCTP max message size — reject an over-long
            // run rather than letting it grow the allocation unbounded.
            if entries.len() > MAX_ENTRIES {
                return Err(invalid(format!("frame exceeds {MAX_ENTRIES}-entry window cap")));
            }
        }
        Ok(Entries(entries))
    }

    fn elements(&self) -> &[Element] {
        &self.0
    }
}

fn invalid(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

/// Build an in-match data frame from a run of [`Element`]s. Wraps the entries
/// into [`Entries`] and defers to [`rennet::Frame::data`].
pub fn data_frame(base: u32, tick_advantage: i16, entries: Vec<Element>, ack: Ack) -> Frame {
    rennet::Frame::data(base, tick_advantage, Entries(entries), ack)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tango_entries_exact_bytes() {
        // base=12345, ack=12345 (delta 0), adv=+2, [Right(0x010), EndOfRound,
        // A(0x001)]. Inputs are two bytes (high, low); the marker is one (0x80).
        let f = data_frame(
            12345,
            2,
            vec![Element::Input(0x010), Element::EndOfRound, Element::Input(0x001)],
            12345,
        );
        assert_eq!(f.encode(), vec![0xB9, 0x60, 0x00, 0x04, 0x00, 0x10, 0x80, 0x00, 0x01]);
    }

    #[test]
    fn all_variants_roundtrip() {
        let f = data_frame(
            7,
            -3,
            vec![
                Element::Input(0x3ff),
                Element::EndOfRound,
                Element::Input(0),
                Element::EndOfMatch,
            ],
            6,
        );
        assert_eq!(Frame::decode(&f.encode()).unwrap(), f);
    }

    #[test]
    fn unknown_marker_kind_errors() {
        // base=1, ack=1, adv=0, then a marker byte (top bit set) with an
        // undefined kind (2).
        let bytes = vec![0x01, 0x00, 0x00, 0x82];
        assert!(Frame::decode(&bytes).is_err());
    }

    #[test]
    fn truncated_input_errors() {
        // base=1, ack=1, adv=0, then a lone input high byte with no low byte.
        let bytes = vec![0x01, 0x00, 0x00, 0x03];
        assert!(Frame::decode(&bytes).is_err());
    }
}

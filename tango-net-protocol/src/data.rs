//! tango's concrete in-match payload for the unreliable netplay datagram
//! channel: the [`Element`] each seq slot carries, the [`Meta`] side-channel
//! that rides on every frame, and the [`InMatch`] [`rennet::Codec`] descriptor
//! that pairs them. Moved verbatim from the tango bin crate's
//! `net/data/protocol.rs`, plus the queue-budget constants the horizon is
//! sized from (previously `net/data/mod.rs`).
//!
//! The envelope (per-tick seq `base`, the delta-encoded cumulative `ack`, and
//! the per-frame [`Meta`]), the LEB128 codec, and the redundancy-window /
//! cumulative-ack reliability machinery all live in the transport- and
//! packing-agnostic [`rennet`] crate. rennet owns the run framing too — it
//! concatenates the elements and decodes them back until the datagram runs out,
//! so each [`Element`] only has to self-delimit and the [`Meta`] likewise. This
//! module supplies both (tango's meta is the time-sync `tick_advantage`).
//!
//! The element run is the last thing in the datagram, so it needs no length
//! prefix — rennet reads elements until the bytes run out. Elements are
//! **variable-length**, tagged by the top bit of their first byte:
//!
//! * an **input** is two bytes — the 10-bit joyflags, high byte first (so its
//!   top bit is clear, since joyflags never exceed `0x3ff`);
//! * a **marker** (round/match boundary) is a single byte with the top bit set
//!   and the kind in the low bits.
//!
//! So inputs (the common case) stay two bytes while markers cost just one, and
//! the decoder tells them apart from that first byte alone.

use std::io;

/// The 10-bit GBA joypad mask inputs are packed under. Kept as this
/// crate's own constant so the pure codec crate doesn't drag in the
/// emulator stack; the tango bin crate const-asserts it equal to
/// `tango_match::input::JOYFLAGS_MASK` (it sees both crates).
pub const JOYFLAGS_MASK: u16 = 0x03ff;

/// The reconnect watchdog's trip depth: local inputs buffered with
/// nothing from the peer to match them before the session pauses for a
/// transparent reconnect. 180 frames ≈ 3 s of play (at 60 fps, just
/// above the GBA frame rate).
pub const RECONNECT_QUEUE_LENGTH: usize = 180;

/// Slack between the reconnect trip depth and the hard overflow bail —
/// see [`RECONNECT_QUEUE_LENGTH`]. It need not match the trip depth
/// itself; the slop it has to cover is a handful of frames, far short
/// of the depth's worth of growth. 90 frames ≈ 1.5 s.
const STALL_HEADROOM: usize = 90;

/// Per-side input-queue capacity (the rollback horizon): how many local
/// inputs may sit unmatched against remote ones (and vice versa) before
/// the match bails. Derived from [`RECONNECT_QUEUE_LENGTH`] — the
/// backpressure bound other layers size against (rennet's redundancy
/// window and reorder buffer via [`HORIZON`]).
pub const MAX_QUEUE_LENGTH: usize = RECONNECT_QUEUE_LENGTH + STALL_HEADROOM;

/// Top bit of an element's first byte: set => a 1-byte marker, clear => the high
/// byte of a 2-byte input (always clear there, as joyflags fit in 10 bits).
const MARKER_FLAG: u8 = 0x80;

/// Marker kind, carried in the low bits of a marker byte.
const KIND_END_OF_MATCH: u8 = 1;

/// Hard cap on elements decoded from one datagram. A legitimate redundancy
/// window can't exceed the rollback horizon (the out-stream trims it to that),
/// so a datagram claiming more is malformed or hostile; rennet enforces this in
/// [`Frame::decode`](rennet::Frame::decode) via [`InMatch::MAX_RUN`].
const MAX_ENTRIES: usize = MAX_QUEUE_LENGTH;

/// Rollback horizon for the in-match reliability streams: a gap wider than this
/// can't be rolled back to, so the receiver bails. Sized to the input-buffer
/// budget ([`MAX_QUEUE_LENGTH`]).
pub const HORIZON: u32 = MAX_QUEUE_LENGTH as u32;

/// Cumulative ack frontier — re-exported so callers keep saying `data::Ack`.
pub use rennet::Ack;

/// One whole in-match datagram: tango's [`Element`] run and [`Meta`] wired into
/// the generic [`rennet::Frame`] via the [`InMatch`] descriptor. A `Frame` *is*
/// the message — see [`rennet::frame`] for the envelope layout. Build them with
/// [`data_frame`]; an "ack-only" frame is just one with no entries.
pub type Frame = rennet::Frame<InMatch>;

/// The [`rennet::Protocol`] descriptor for tango's in-match datagrams: pairs the
/// [`Element`] stream with the [`Meta`] side-channel. A zero-sized marker — it's
/// only ever a type parameter (`rennet::Frame<InMatch>`, stream aliases, …).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InMatch;

impl rennet::Protocol for InMatch {
    type Element = Element;
    type Meta = Meta;
    const MAX_RUN: usize = MAX_ENTRIES;
}

/// The per-frame meta tango rides on every in-match datagram. It's a struct
/// rather than a bare `i16` so more synced-once-per-frame fields can join
/// `tick_advantage` later without touching the wire plumbing — add a field here
/// and extend its [`Codec`](rennet::Codec) impl.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    /// The newest local input's time-sync lead, fed to the throttler on the far
    /// side.
    pub tick_advantage: i16,
}

impl rennet::Codec for Meta {
    /// The meta is a single svarint of `tick_advantage`.
    fn encode<W: io::Write>(&self, w: &mut W) -> io::Result<()> {
        rennet::write_svarint(w, self.tick_advantage as i64)
    }

    fn decode<R: io::Read>(r: &mut R) -> io::Result<Option<Self>> {
        // The meta is required: a short read errors (never a clean `None`).
        let tick_advantage =
            i16::try_from(rennet::read_svarint(r)?).map_err(|_| invalid("tick_advantage out of range".to_string()))?;
        Ok(Some(Meta { tick_advantage }))
    }
}

/// One element of the input stream, occupying a single seq slot: either a tick's
/// input, or a round/match boundary that rides in-band on the seq line (see the
/// module header for its wire packing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element {
    /// Joyflags for this tick (10-bit GBA keypad; the top 6 bits must be 0).
    Input(u16),
    /// End-of-match boundary.
    EndOfMatch,
}

impl rennet::Codec for Element {
    /// Write this element's wire bytes (see the module header).
    fn encode<W: io::Write>(&self, w: &mut W) -> io::Result<()> {
        match *self {
            Element::Input(joyflags) => {
                assert!(
                    joyflags & !JOYFLAGS_MASK == 0,
                    "joyflags use reserved high bits: {joyflags:#06x}"
                );
                // High byte first; its top bit is clear (joyflags <= 0x3ff),
                // which is what tells an input from a marker on the way back in.
                w.write_all(&[(joyflags >> 8) as u8, joyflags as u8])
            }
            Element::EndOfMatch => w.write_all(&[MARKER_FLAG | KIND_END_OF_MATCH]),
        }
    }

    /// Read one element off `r` — the top bit of the first byte tells an input
    /// (two bytes) from a marker (one). `None` at a clean EOF (the run's end); a
    /// lone input high byte with no low byte is a truncation error.
    fn decode<R: io::Read>(r: &mut R) -> io::Result<Option<Self>> {
        let mut b0 = [0u8; 1];
        // 0 bytes read = clean EOF at the run boundary.
        if r.read(&mut b0)? == 0 {
            return Ok(None);
        }
        let b0 = b0[0];
        let element = if b0 & MARKER_FLAG != 0 {
            match b0 & !MARKER_FLAG {
                KIND_END_OF_MATCH => Element::EndOfMatch,
                other => return Err(invalid(format!("unknown marker kind: {other}"))),
            }
        } else {
            // Input: this byte's the high half, the next its low half (EOF here
            // is a truncated element, not a clean end).
            let mut b1 = [0u8; 1];
            r.read_exact(&mut b1)?;
            Element::Input(((b0 as u16) << 8 | b1[0] as u16) & JOYFLAGS_MASK)
        };
        Ok(Some(element))
    }
}

fn invalid(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

/// Build an in-match frame from the `base`/`ack` header, the per-frame [`Meta`],
/// and a run of [`Element`]s — a thin [`rennet::Frame::new`] wrapper that pins
/// the [`InMatch`] descriptor. An empty `entries` makes an "ack-only" frame.
pub fn data_frame(base: u32, ack: Ack, meta: Meta, entries: Vec<Element>) -> Frame {
    rennet::Frame::new(base, ack, meta, entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tango_entries_exact_bytes() {
        // base=12345, ack=12345 (delta 0), meta tick_advantage=+2, [Right(0x010),
        // A(0x001)]. Inputs are two bytes (high, low). The meta is a single
        // svarint byte, just as the old inlined tick_advantage was — so the wire
        // form is byte-for-byte unchanged.
        let f = data_frame(
            12345,
            12345,
            Meta { tick_advantage: 2 },
            vec![Element::Input(0x010), Element::Input(0x001)],
        );
        assert_eq!(f.to_vec(), vec![0xB9, 0x60, 0x00, 0x04, 0x00, 0x10, 0x00, 0x01]);
    }

    #[test]
    fn all_variants_roundtrip() {
        let f = data_frame(
            7,
            6,
            Meta { tick_advantage: -3 },
            vec![Element::Input(0x3ff), Element::Input(0), Element::EndOfMatch],
        );
        assert_eq!(Frame::decode(&mut &f.to_vec()[..]).unwrap(), f);
    }

    #[test]
    fn empty_run_is_an_ack_only_frame() {
        // No entries: base=9, ack=9 (delta 0), meta tick_advantage=0, no run.
        let f = data_frame(9, 9, Meta::default(), vec![]);
        assert_eq!(f.to_vec(), vec![0x09, 0x00, 0x00]);
        let back = Frame::decode(&mut &f.to_vec()[..]).unwrap();
        assert_eq!(back, f);
        assert!(back.entries.is_empty());
    }

    #[test]
    fn unknown_marker_kind_errors() {
        // base=1, ack=1, meta=0, then a marker byte (top bit set) with an
        // undefined kind (2).
        let bytes = vec![0x01, 0x00, 0x00, 0x82];
        assert!(Frame::decode(&mut &bytes[..]).is_err());
    }

    #[test]
    fn truncated_input_errors() {
        // base=1, ack=1, meta=0, then a lone input high byte with no low byte.
        let bytes = vec![0x01, 0x00, 0x00, 0x03];
        assert!(Frame::decode(&mut &bytes[..]).is_err());
    }
}

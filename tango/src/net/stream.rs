//! Transport-agnostic reliability state machines for the in-match wire
//! protocol ([`super::wire`]). Two halves, both pure (no async, no I/O):
//!
//! * [`OutStream`] — assigns a monotonic seq to each local element, keeps a
//!   redundancy window of recent unconfirmed elements (`history`), and trims
//!   it as the peer's block-acks confirm receipt. [`OutStream::window`] is
//!   what goes into an outbound [`Frame`].
//! * [`InStream`] — reassembles the peer's stream from possibly-lossy,
//!   reordered, duplicated frames: a reorder buffer feeds elements out in
//!   strict seq order ([`InStream::accept`]), generates the block-ack to
//!   send back ([`InStream::block_ack`]), and bails when a gap grows past
//!   the rollback horizon.
//!
//! Recovery is proactive, not request/response: a lost element is re-sent in
//! the *next* frame's window (cost ~one frame), so single/short losses never
//! pay a round-trip. The block-ack only drives window *trimming* (and would
//! drive selective resend for bursts longer than the window — see
//! [`OutStream::trim`]). Nothing here knows about the engine's `Event` type;
//! the `PvpSender`/`PvpReceiver` adapters map [`Element`] <-> `Event`.
//!
use std::collections::BTreeMap;

use super::wire::{BlockAck, Element, Frame, Marker};

/// Rollback horizon: a gap wider than this can't be rolled back to, so the
/// receiver bails instead of waiting forever. Matches the engine's input
/// buffer cap (`round.rs` bails locally at the same depth).
const HORIZON: u32 = tango_pvp::battle::MAX_QUEUE_LENGTH as u32;

/// Minimum elements every data frame carries, regardless of acks: enough
/// that a single dropped datagram is always covered by the next one without
/// waiting for the peer's ack to report the hole.
pub const MIN_REDUNDANCY: u32 = 2;

/// Sender half: seq assignment + redundancy window.
pub struct OutStream {
    /// Next seq to hand out. Seqs are 1-based so `0` stays free as the
    /// ack-only sentinel in [`Frame`].
    next_seq: u32,
    /// Unconfirmed/redundancy window, ascending by seq. Exactly the set
    /// [`window`](Self::window) emits.
    history: std::collections::VecDeque<(u32, Element)>,
    /// Peer's contiguous frontier (lowest seq it hasn't confirmed). Trims
    /// `history`; only ever advances.
    peer_ack_base: u32,
    /// Newest input's time-sync lead, echoed once per frame.
    latest_advantage: i16,
}

impl Default for OutStream {
    fn default() -> Self {
        Self::new()
    }
}

impl OutStream {
    pub fn new() -> Self {
        Self {
            next_seq: 1,
            history: std::collections::VecDeque::new(),
            peer_ack_base: 1,
            latest_advantage: 0,
        }
    }

    /// Append a local input; returns its seq.
    pub fn push_input(&mut self, joyflags: u16, frame_advantage: i16) -> u32 {
        self.latest_advantage = frame_advantage;
        self.push(Element::Input(joyflags & 0x03ff))
    }

    /// Append a round/match boundary; returns its seq.
    pub fn push_marker(&mut self, marker: Marker) -> u32 {
        self.push(Element::Marker(marker))
    }

    fn push(&mut self, e: Element) -> u32 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.history.push_back((seq, e));
        self.trim();
        seq
    }

    /// Apply a block-ack the peer piggybacked on one of its frames.
    pub fn apply_ack(&mut self, ack: BlockAck) {
        // Acks are cumulative and idempotent; a stale/reordered one must not
        // drag the frontier backwards.
        if ack.base > self.peer_ack_base {
            self.peer_ack_base = ack.base;
            self.trim();
        }
    }

    /// Drop history the peer has confirmed, while keeping at least
    /// [`MIN_REDUNDANCY`] recent elements and no more than a [`HORIZON`]'s
    /// worth (beyond the horizon the peer would bail, so retaining them is
    /// pointless).
    fn trim(&mut self) {
        let newest = match self.history.back() {
            Some(&(seq, _)) => seq,
            None => return,
        };
        let redundancy_floor = newest.saturating_sub(MIN_REDUNDANCY.saturating_sub(1));
        let horizon_floor = newest.saturating_sub(HORIZON.saturating_sub(1));
        let keep_from = self
            .peer_ack_base
            .min(redundancy_floor)
            .max(horizon_floor)
            .max(1);
        while let Some(&(seq, _)) = self.history.front() {
            if seq < keep_from {
                self.history.pop_front();
            } else {
                break;
            }
        }
    }

    /// The data portion of an outbound frame: `(base, frame_advantage,
    /// entries)`. `None` before the first element is pushed — the caller
    /// emits an ack-only frame (`base == 0`) instead.
    pub fn window(&self) -> Option<(u32, i16, Vec<Element>)> {
        let base = self.history.front()?.0;
        let entries = self.history.iter().map(|&(_, e)| e).collect();
        Some((base, self.latest_advantage, entries))
    }

    #[cfg(test)]
    fn window_len(&self) -> usize {
        self.history.len()
    }
}

/// A gap grew past the rollback horizon — the stream can't be recovered;
/// the caller should tear the match down (and, later, attempt reconnect).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HorizonExceeded;

/// Receiver half: reorder buffer + contiguous delivery + block-ack.
pub struct InStream {
    /// Next contiguous seq we still need. Everything below has been
    /// delivered. 1-based to match [`OutStream`].
    recv_base: u32,
    /// Elements received with seq > recv_base that can't be delivered yet
    /// (a gap precedes them). Keyed by seq; re-inserting a seq is a no-op,
    /// which is how redundant copies dedup.
    buffer: BTreeMap<u32, Element>,
    /// Freshest frame-advantage seen, attached to delivered inputs by the
    /// adapter. `None` until the first data frame.
    latest_advantage: Option<i16>,
}

impl Default for InStream {
    fn default() -> Self {
        Self::new()
    }
}

impl InStream {
    pub fn new() -> Self {
        Self {
            recv_base: 1,
            buffer: BTreeMap::new(),
            latest_advantage: None,
        }
    }

    /// Ingest one frame's entries. Returns the elements that became
    /// contiguous (in strict seq order, possibly empty). The frame's
    /// block-ack, if any, is the caller's job to apply to its [`OutStream`].
    pub fn accept(&mut self, frame: &Frame) -> Result<Vec<Element>, HorizonExceeded> {
        if let Some(fa) = frame.frame_advantage {
            self.latest_advantage = Some(fa);
        }
        for (i, &e) in frame.entries.iter().enumerate() {
            let seq = frame.base + i as u32;
            if seq < self.recv_base {
                continue; // already delivered — a redundant/duplicate copy.
            }
            if seq >= self.recv_base.saturating_add(HORIZON) {
                // Too far ahead of our frontier to ever roll back to.
                return Err(HorizonExceeded);
            }
            self.buffer.entry(seq).or_insert(e);
        }
        let mut delivered = Vec::new();
        while let Some(e) = self.buffer.remove(&self.recv_base) {
            delivered.push(e);
            self.recv_base += 1;
        }
        Ok(delivered)
    }

    /// Block-ack to send back: the contiguous frontier plus a bitmap of the
    /// out-of-order arrivals just above it.
    pub fn block_ack(&self) -> BlockAck {
        let mut bits = 0u32;
        let end = self.recv_base.saturating_add(32);
        for (&seq, _) in self.buffer.range(self.recv_base..end) {
            let off = seq - self.recv_base; // recv_base itself is never buffered, so off >= 1
            bits |= 1 << off;
        }
        BlockAck {
            base: self.recv_base,
            bits,
        }
    }

    /// Freshest frame-advantage, for the throttler. `None` => no sample yet
    /// (don't feed a synthetic zero).
    pub fn latest_advantage(&self) -> Option<i16> {
        self.latest_advantage
    }

    #[cfg(test)]
    fn recv_base(&self) -> u32 {
        self.recv_base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::wire::Wire;

    fn input(j: u16) -> Element {
        Element::Input(j)
    }

    /// Build a frame from the out-stream's current window + the in-stream's
    /// ack, then round-trip it through the wire codec (so tests exercise the
    /// real encode/decode too).
    fn make_frame(out: &OutStream, ack: Option<BlockAck>) -> Frame {
        let (base, fa, entries) = out.window().expect("window");
        let frame = Frame {
            base,
            frame_advantage: Some(fa),
            entries,
            ack,
        };
        match Wire::decode(&Wire::Frame(frame).encode()).unwrap() {
            Wire::Frame(f) => f,
            _ => unreachable!(),
        }
    }

    #[test]
    fn window_floor_when_peer_caught_up() {
        let mut out = OutStream::new();
        out.push_input(1, 0);
        out.push_input(2, 0);
        out.push_input(3, 0);
        // Peer has confirmed everything through seq 3 (base = 4).
        out.apply_ack(BlockAck { base: 4, bits: 0 });
        // Still keeps MIN_REDUNDANCY recent elements (seqs are 1..=3).
        assert_eq!(out.window_len(), MIN_REDUNDANCY as usize);
        let (base, _, entries) = out.window().unwrap();
        assert_eq!(base, 3 - (MIN_REDUNDANCY - 1)); // seq of first kept = 2
        assert_eq!(entries.len(), MIN_REDUNDANCY as usize);
    }

    #[test]
    fn window_grows_with_peer_lag() {
        let mut out = OutStream::new();
        for k in 0..10 {
            out.push_input(k, 0);
        }
        // Peer only confirmed through seq 4 (base = 5): seqs 5..=10 unconfirmed.
        out.apply_ack(BlockAck { base: 5, bits: 0 });
        let (base, _, entries) = out.window().unwrap();
        assert_eq!(base, 5);
        assert_eq!(entries.len(), 6); // 5,6,7,8,9,10
    }

    #[test]
    fn ack_never_moves_backwards() {
        let mut out = OutStream::new();
        for k in 0..10 {
            out.push_input(k, 0);
        }
        out.apply_ack(BlockAck { base: 8, bits: 0 });
        out.apply_ack(BlockAck { base: 3, bits: 0 }); // stale/reordered
        let (base, _, _) = out.window().unwrap();
        assert_eq!(base, 8); // didn't regress
    }

    #[test]
    fn in_delivers_contiguously_in_order() {
        let mut out = OutStream::new();
        let mut inn = InStream::new();
        for k in 1..=5u16 {
            out.push_input(k, 0);
        }
        let f = make_frame(&out, None);
        let delivered = inn.accept(&f).unwrap();
        assert_eq!(delivered, (1..=5).map(input).collect::<Vec<_>>());
        assert_eq!(inn.recv_base(), 6);
    }

    #[test]
    fn in_dedups_redundant_copies() {
        let mut out = OutStream::new();
        let mut inn = InStream::new();
        out.push_input(1, 0);
        out.push_input(2, 0);
        let f1 = make_frame(&out, None);
        assert_eq!(inn.accept(&f1).unwrap(), vec![input(1), input(2)]);
        out.push_input(3, 0);
        // f2's window still re-includes 2 (redundancy) plus the new 3.
        let f2 = make_frame(&out, None);
        // 2 is a dup (already delivered), only 3 is new.
        assert_eq!(inn.accept(&f2).unwrap(), vec![input(3)]);
        assert_eq!(inn.recv_base(), 4);
    }

    #[test]
    fn in_recovers_a_dropped_frame_via_redundancy() {
        let mut out = OutStream::new();
        let mut inn = InStream::new();
        // Each "frame" carries the last MIN_REDUNDANCY+ elements. Drop one
        // datagram entirely; the next one's window should still cover it.
        out.push_input(10, 0);
        let f_a = make_frame(&out, None); // window ends at seq1=Input(10)
        assert_eq!(inn.accept(&f_a).unwrap(), vec![input(10)]);

        out.push_input(11, 0);
        let _dropped = make_frame(&out, None); // imagine this is lost

        out.push_input(12, 0);
        let f_c = make_frame(&out, None); // window includes 11 and 12 (redundancy)
        // Frontier was at seq for 11; the lost frame's 11 arrives here.
        assert_eq!(inn.accept(&f_c).unwrap(), vec![input(11), input(12)]);
    }

    #[test]
    fn in_buffers_out_of_order_then_fills_gap() {
        let mut inn = InStream::new();
        // Deliver a frame starting at seq 3 (1 and 2 missing): buffered, none delivered.
        let ahead = Frame {
            base: 3,
            frame_advantage: Some(0),
            entries: vec![input(30), input(40)],
            ack: None,
        };
        assert_eq!(inn.accept(&ahead).unwrap(), vec![]);
        assert_eq!(inn.recv_base(), 1);
        // Block-ack reports the hole: base=1, bits for seq 3 and 4 (offsets 2,3).
        let ba = inn.block_ack();
        assert_eq!(ba.base, 1);
        assert_eq!(ba.bits, (1 << 2) | (1 << 3));
        // Now the gap arrives; everything drains in order.
        let gap = Frame {
            base: 1,
            frame_advantage: Some(0),
            entries: vec![input(10), input(20)],
            ack: None,
        };
        assert_eq!(inn.accept(&gap).unwrap(), vec![input(10), input(20), input(30), input(40)]);
        assert_eq!(inn.recv_base(), 5);
    }

    #[test]
    fn in_bails_past_horizon() {
        let mut inn = InStream::new();
        let way_ahead = Frame {
            base: 1 + HORIZON, // recv_base is 1; this is exactly a horizon away
            frame_advantage: Some(0),
            entries: vec![input(1)],
            ack: None,
        };
        assert_eq!(inn.accept(&way_ahead), Err(HorizonExceeded));
    }

    #[test]
    fn markers_ride_in_band_in_order() {
        let mut out = OutStream::new();
        let mut inn = InStream::new();
        out.push_input(1, 0);
        out.push_marker(Marker::EndOfRound);
        out.push_input(2, 0);
        out.push_marker(Marker::EndOfMatch);
        let f = make_frame(&out, None);
        assert_eq!(
            inn.accept(&f).unwrap(),
            vec![
                Element::Input(1),
                Element::Marker(Marker::EndOfRound),
                Element::Input(2),
                Element::Marker(Marker::EndOfMatch),
            ]
        );
    }

    #[test]
    fn ack_round_trips_to_out_stream() {
        let mut out = OutStream::new();
        let mut inn = InStream::new();
        for k in 1..=4u16 {
            out.push_input(k, 0);
        }
        let f = make_frame(&out, None);
        inn.accept(&f).unwrap();
        // The in-stream now wants seq 5; its ack should advance the peer's
        // out-stream frontier so it trims to the redundancy floor.
        out.apply_ack(inn.block_ack());
        assert_eq!(out.window_len(), MIN_REDUNDANCY as usize);
    }

    #[test]
    fn lossy_stream_converges() {
        // Drive 200 inputs through a flaky link that drops every 3rd
        // datagram, with acks flowing back. Every input must be delivered
        // exactly once, in order, and never bail (loss stays within window).
        let mut out = OutStream::new();
        let mut inn = InStream::new();
        let mut delivered = Vec::new();
        for k in 1..=200u32 {
            out.push_input(k as u16, 0);
            let (base, fa, entries) = out.window().unwrap();
            let frame = Frame {
                base,
                frame_advantage: Some(fa),
                entries,
                ack: None,
            };
            if k % 3 != 0 {
                // delivered: round-trip through the wire and ingest.
                let f = match Wire::decode(&Wire::Frame(frame).encode()).unwrap() {
                    Wire::Frame(f) => f,
                    _ => unreachable!(),
                };
                delivered.extend(f_inputs(inn.accept(&f).unwrap()));
                out.apply_ack(inn.block_ack());
            }
        }
        let expected: Vec<u32> = (1..=200).collect();
        assert_eq!(delivered, expected);
    }

    fn f_inputs(els: Vec<Element>) -> Vec<u32> {
        els.into_iter()
            .filter_map(|e| match e {
                Element::Input(j) => Some(j as u32),
                Element::Marker(_) => None,
            })
            .collect()
    }
}

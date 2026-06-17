//! Transport-agnostic reliability state machines for the in-match wire
//! protocol ([`super::protocol`]). Two halves, both pure (no async, no I/O):
//!
//! * [`OutStream`] — assigns a monotonic seq to each local element, keeps a
//!   redundancy window of recent unconfirmed elements (`history`), and trims
//!   it as the peer's cumulative acks confirm receipt. [`OutStream::window`] is
//!   what goes into an outbound [`Frame`].
//! * [`InStream`] — reassembles the peer's stream from possibly-lossy,
//!   reordered, duplicated frames: a reorder buffer feeds elements out in
//!   strict seq order ([`InStream::accept`]), generates the cumulative ack to
//!   send back ([`InStream::ack`]), and bails when a gap grows past
//!   the rollback horizon.
//!
//! Recovery is proactive, not request/response: a lost element is re-sent in
//! the *next* frame's window (cost ~one frame), so single/short losses never
//! pay a round-trip. The ack only drives window *trimming* (and would
//! drive selective resend for bursts longer than the window — see
//! [`OutStream::trim`]). Nothing here knows about the engine's `Event` type;
//! the `PvpSender`/`PvpReceiver` adapters map [`Element`] <-> `Event`.
//!
use std::collections::BTreeMap;

use super::protocol::{Element, Frame, PAYLOAD_MASK};

/// Rollback horizon: a gap wider than this can't be rolled back to, so the
/// receiver bails instead of waiting forever. Matches the engine's input
/// buffer cap (`round.rs` bails locally at the same depth).
const HORIZON: u32 = tango_pvp::battle::MAX_QUEUE_LENGTH as u32;

/// Default proactive redundancy floor: the minimum elements every data frame
/// carries regardless of acks, so a dropped datagram is covered by the next one
/// without waiting for the peer's ack to report the hole. This is just the
/// starting/typical value — the floor is adaptive (see
/// [`OutStream::set_min_redundancy`]); the adapter raises or lowers it from the
/// measured round-trip.
pub const DEFAULT_REDUNDANCY: u32 = 2;

/// Hard ceiling on the adaptive redundancy floor. Every extra element is bytes
/// on every datagram, and bursts longer than the floor are still recovered by
/// the ack-driven window growth (just a round-trip slower), so the *proactive*
/// floor is capped low.
pub const MAX_REDUNDANCY: u32 = 3;

/// A contiguous run of elements plus the time-sync advantage that rides with
/// them. Both halves of the protocol produce one: [`OutStream::window`] builds
/// the outbound redundancy window (wrapped into a [`Frame::data`]), and
/// [`InStream::accept`] returns the run it just made contiguous, tagged with the
/// freshest advantage, for the receive adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// Seq of `entries[0]`: the frame's `base` outbound, the oldest
    /// just-delivered seq inbound. When `entries` is empty, the next seq the
    /// producer expects (`next_seq` / `recv_base`).
    pub base: u32,
    /// Time-sync lead carried with this run: the newest local input's lead
    /// outbound, the freshest advantage seen inbound.
    pub frame_advantage: i16,
    /// The elements, ascending by seq from `base`.
    pub entries: Vec<Element>,
}

/// Sender half: seq assignment + redundancy window.
pub struct OutStream {
    /// Next seq to hand out. Seqs are 1-based so `0` stays free as the
    /// ack-only sentinel in [`Frame`].
    next_seq: u32,
    /// Unconfirmed/redundancy window, ascending by seq. Exactly the set
    /// [`window`](Self::window) emits. Seqs aren't stored per element: the
    /// window is a contiguous run ending at `next_seq - 1` (only `push` appends,
    /// only `trim` pops the front), so the front's seq is always
    /// [`base`](Self::base) `== next_seq - history.len()`.
    history: std::collections::VecDeque<Element>,
    /// Peer's contiguous frontier (lowest seq it hasn't confirmed). Trims
    /// `history`; only ever advances.
    peer_ack_base: u32,
    /// Newest input's time-sync lead, echoed once per frame.
    latest_advantage: i16,
    /// Current proactive redundancy floor (see [`MIN_REDUNDANCY`]). Adaptive —
    /// the adapter drives it from the measured RTT. Always in `[1,
    /// MAX_REDUNDANCY]`.
    min_redundancy: u32,
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
            min_redundancy: DEFAULT_REDUNDANCY,
        }
    }

    /// Adjust the proactive redundancy floor (see [`MIN_REDUNDANCY`]), clamped to
    /// `[1, MAX_REDUNDANCY]`. The adapter drives this from the measured
    /// round-trip: redundancy exists to recover a lost datagram in ~one frame
    /// instead of waiting for the ack-driven resend, which costs a whole RTT — so
    /// a longer RTT makes a deeper floor worth more, and a sub-frame RTT makes it
    /// worthless (the resend is itself ~one frame). Re-trims immediately so a
    /// lowered floor sheds its now-excess window the same tick.
    pub fn set_min_redundancy(&mut self, n: u32) {
        let n = n.clamp(1, MAX_REDUNDANCY);
        if n != self.min_redundancy {
            self.min_redundancy = n;
            self.trim();
        }
    }

    /// Append a local input; returns its seq. Convenience over [`push`](Self::push)
    /// that also records the time-sync advantage — markers carry none, so they
    /// go straight through `push`.
    pub fn push_input(&mut self, joyflags: u16, frame_advantage: i16) -> u32 {
        self.latest_advantage = frame_advantage;
        self.push(Element::Input(joyflags & PAYLOAD_MASK))
    }

    /// Append any element (an input, or an `EndOf*` boundary) at the next seq;
    /// returns it. Markers ride in-band on the same seq line as inputs.
    pub fn push(&mut self, e: Element) -> u32 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.history.push_back(e);
        self.trim();
        seq
    }

    /// Apply a cumulative ack (the peer's contiguous frontier) piggybacked on
    /// one of its frames.
    pub fn apply_ack(&mut self, frontier: u32) {
        // Clamp to what we've actually sent — a peer can't legitimately ack a
        // seq beyond `next_seq`, and an out-of-range value mustn't pin the
        // frontier into the future (which would starve our own redundancy).
        let acked = frontier.min(self.next_seq);
        // Acks are cumulative and idempotent; a stale/reordered one must not
        // drag the frontier backwards.
        if acked > self.peer_ack_base {
            self.peer_ack_base = acked;
            self.trim();
        }
    }

    /// Drop history the peer has confirmed, while keeping at least the current
    /// redundancy floor ([`min_redundancy`](Self::min_redundancy), default
    /// [`MIN_REDUNDANCY`]) of recent elements and no more than a [`HORIZON`]'s
    /// worth (beyond the horizon the peer would bail, so retaining them is
    /// pointless).
    fn trim(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let newest = self.next_seq - 1;
        let redundancy_floor = newest.saturating_sub(self.min_redundancy.saturating_sub(1));
        let horizon_floor = newest.saturating_sub(HORIZON.saturating_sub(1));
        let keep_from = self.peer_ack_base.min(redundancy_floor).max(horizon_floor).max(1);
        // Each pop advances the front seq (`base` rises as the deque shrinks);
        // `keep_from <= newest`, so this always leaves at least one element.
        while self.base() < keep_from {
            self.history.pop_front();
        }
    }

    /// Seq of the oldest element still in the window — the `base` of an outbound
    /// frame. The window is a contiguous run ending at `next_seq - 1`, so the
    /// front's seq is `next_seq - history.len()`. Equals `next_seq` (one past
    /// the newest) only when the window is empty.
    fn base(&self) -> u32 {
        self.next_seq - self.history.len() as u32
    }

    /// The data portion of an outbound frame — see [`Window`]. `None` before the
    /// first element is pushed: the caller emits an ack-only frame instead.
    pub fn window(&self) -> Option<Window> {
        if self.history.is_empty() {
            return None;
        }
        Some(Window {
            base: self.base(),
            frame_advantage: self.latest_advantage,
            entries: self.history.iter().copied().collect(),
        })
    }

    /// The newest seq handed out so far, or `None` before the first push. The
    /// adapter timestamps this on each send to derive RTT from the peer's ack
    /// of it (see [`super::InMatchTx`]).
    pub fn newest_seq(&self) -> Option<u32> {
        (self.next_seq > 1).then(|| self.next_seq - 1)
    }

    /// The peer's contiguous frontier: the lowest seq it hasn't confirmed, so
    /// every seq below it is acknowledged received. Drives RTT measurement —
    /// when this advances past a timestamped seq, that seq's round-trip is
    /// known.
    pub fn peer_ack_base(&self) -> u32 {
        self.peer_ack_base
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

/// Receiver half: reorder buffer + contiguous delivery + cumulative ack.
pub struct InStream {
    /// Next contiguous seq we still need. Everything below has been
    /// delivered. 1-based to match [`OutStream`].
    recv_base: u32,
    /// Elements received with seq > recv_base that can't be delivered yet
    /// (a gap precedes them). Keyed by seq; re-inserting a seq is a no-op,
    /// which is how redundant copies dedup.
    buffer: BTreeMap<u32, Element>,
    /// Freshest frame-advantage seen; [`accept`](Self::accept) returns it with
    /// every delivery so the adapter can tag the delivered inputs. Persisted
    /// across calls for the reorder guard below.
    latest_advantage: i16,
    /// Newest seq whose frame set [`latest_advantage`](Self::latest_advantage).
    /// Datagrams reorder under jitter, so a *later-arriving* frame can be an
    /// *older* one; without this guard its stale advantage would overwrite the
    /// fresh one and jerk the clock-sync skew backward. Only a frame reaching at
    /// least this far updates the advantage.
    latest_advantage_seq: u32,
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
            latest_advantage: 0,
            latest_advantage_seq: 0,
        }
    }

    /// Ingest one frame's entries. Returns the run that became contiguous (in
    /// strict seq order, possibly empty) as a [`Window`]: `entries` are the
    /// newly-delivered elements, `base` their starting seq, and `frame_advantage`
    /// the freshest advantage seen so far — the reorder guard below keeps a
    /// late-arriving older frame from regressing it. The frame's cumulative ack,
    /// if any, is the caller's job to apply to its [`OutStream`].
    pub fn accept(&mut self, frame: &Frame) -> Result<Window, HorizonExceeded> {
        // Ack-only frames carry no input data — nothing to reassemble.
        let (base, frame_advantage, entries) = match frame {
            Frame::Ack(_) => {
                return Ok(Window {
                    base: self.recv_base,
                    frame_advantage: self.latest_advantage,
                    entries: Vec::new(),
                })
            }
            Frame::Data {
                base,
                frame_advantage,
                entries,
                ..
            } => (base.get(), *frame_advantage, entries),
        };
        // Only the newest-by-seq frame's advantage is fresh; a reordered older
        // frame arriving later must not clobber it (its `frame_advantage` is stale).
        let frame_newest = base.saturating_add(entries.len() as u32).saturating_sub(1);
        if frame_newest >= self.latest_advantage_seq {
            self.latest_advantage_seq = frame_newest;
            self.latest_advantage = frame_advantage;
        }
        for (i, &e) in entries.iter().enumerate() {
            // Saturating: `base` is peer-supplied, so `base + i` mustn't
            // overflow. A saturated seq lands past the horizon and is rejected
            // below, same as any other too-far-ahead value.
            let seq = base.saturating_add(i as u32);
            if seq < self.recv_base {
                continue; // already delivered — a redundant/duplicate copy.
            }
            if seq >= self.recv_base.saturating_add(HORIZON) {
                // Too far ahead of our frontier to ever roll back to.
                return Err(HorizonExceeded);
            }
            self.buffer.entry(seq).or_insert(e);
        }
        // `recv_base` before draining is the seq of the first delivered element.
        let delivered_base = self.recv_base;
        let mut delivered = Vec::new();
        while let Some(e) = self.buffer.remove(&self.recv_base) {
            delivered.push(e);
            self.recv_base += 1;
        }
        Ok(Window {
            base: delivered_base,
            frame_advantage: self.latest_advantage,
            entries: delivered,
        })
    }

    /// Cumulative ack to send back: the contiguous frontier (lowest seq not yet
    /// received). The sender resends its window from here; with a contiguous
    /// resend window that's all it can act on, so there's no bitmap of
    /// out-of-order receipts — those seqs can't be skipped in a contiguous
    /// frame anyway. The reorder `buffer` still tracks them, it's just not
    /// reported.
    pub fn ack(&self) -> u32 {
        self.recv_base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(j: u16) -> Element {
        Element::Input(j)
    }

    /// Build a frame from the out-stream's current window, then round-trip it
    /// through the wire codec (so tests exercise the real encode/decode too).
    /// The ack is irrelevant to these reassembly tests — the in-stream ignores
    /// it — so it's pinned to the initial frontier.
    fn make_frame(out: &OutStream) -> Frame {
        let w = out.window().expect("window");
        let frame = Frame::data(w.base, w.frame_advantage, w.entries, 1);
        Frame::decode(&frame.encode()).unwrap()
    }

    #[test]
    fn window_floor_when_peer_caught_up() {
        let mut out = OutStream::new();
        out.push_input(1, 0);
        out.push_input(2, 0);
        out.push_input(3, 0);
        // Peer has confirmed everything through seq 3 (frontier = 4).
        out.apply_ack(4);
        // Still keeps MIN_REDUNDANCY recent elements (seqs are 1..=3).
        assert_eq!(out.window_len(), DEFAULT_REDUNDANCY as usize);
        let w = out.window().unwrap();
        assert_eq!(w.base, 3 - (DEFAULT_REDUNDANCY - 1)); // seq of first kept = 2
        assert_eq!(w.entries.len(), DEFAULT_REDUNDANCY as usize);
    }

    #[test]
    fn min_redundancy_floor_is_adaptive() {
        let mut out = OutStream::new();
        for k in 0..5 {
            out.push_input(k, 0);
        }
        out.apply_ack(6); // peer caught up: only the floor is retained.
        assert_eq!(out.window_len(), DEFAULT_REDUNDANCY as usize);

        // Lower the floor (clean/low-RTT link): the window sheds down to 1 at once.
        out.set_min_redundancy(1);
        assert_eq!(out.window_len(), 1);

        // Raise it past the default (high-RTT link); clamped at MAX_REDUNDANCY.
        out.set_min_redundancy(99);
        for k in 5..10 {
            out.push_input(k, 0);
        }
        out.apply_ack(11);
        assert_eq!(out.window_len(), MAX_REDUNDANCY as usize);
    }

    #[test]
    fn window_grows_with_peer_lag() {
        let mut out = OutStream::new();
        for k in 0..10 {
            out.push_input(k, 0);
        }
        // Peer only confirmed through seq 4 (frontier = 5): seqs 5..=10 unconfirmed.
        out.apply_ack(5);
        let w = out.window().unwrap();
        assert_eq!(w.base, 5);
        assert_eq!(w.entries.len(), 6); // 5,6,7,8,9,10
    }

    #[test]
    fn ack_never_moves_backwards() {
        let mut out = OutStream::new();
        for k in 0..10 {
            out.push_input(k, 0);
        }
        out.apply_ack(8);
        out.apply_ack(3); // stale/reordered
        let w = out.window().unwrap();
        assert_eq!(w.base, 8); // didn't regress
    }

    #[test]
    fn ack_beyond_sent_is_clamped() {
        let mut out = OutStream::new();
        out.push_input(1, 0);
        out.push_input(2, 0); // next_seq = 3
                              // A peer can't have received a seq we never sent; the bogus frontier
                              // is clamped to next_seq rather than pinned far into the future.
        out.apply_ack(9999);
        assert_eq!(out.peer_ack_base(), 3);
    }

    #[test]
    fn in_delivers_contiguously_in_order() {
        let mut out = OutStream::new();
        let mut inn = InStream::new();
        for k in 1..=5u16 {
            out.push_input(k, 0);
        }
        let f = make_frame(&out);
        let delivered = inn.accept(&f).unwrap();
        assert_eq!(delivered.entries, (1..=5).map(input).collect::<Vec<_>>());
        assert_eq!(inn.ack(), 6);
    }

    #[test]
    fn in_dedups_redundant_copies() {
        let mut out = OutStream::new();
        let mut inn = InStream::new();
        out.push_input(1, 0);
        out.push_input(2, 0);
        let f1 = make_frame(&out);
        assert_eq!(inn.accept(&f1).unwrap().entries, vec![input(1), input(2)]);
        out.push_input(3, 0);
        // f2's window still re-includes 2 (redundancy) plus the new 3.
        let f2 = make_frame(&out);
        // 2 is a dup (already delivered), only 3 is new.
        assert_eq!(inn.accept(&f2).unwrap().entries, vec![input(3)]);
        assert_eq!(inn.ack(), 4);
    }

    #[test]
    fn in_recovers_a_dropped_frame_via_redundancy() {
        let mut out = OutStream::new();
        let mut inn = InStream::new();
        // Each "frame" carries the last MIN_REDUNDANCY+ elements. Drop one
        // datagram entirely; the next one's window should still cover it.
        out.push_input(10, 0);
        let f_a = make_frame(&out); // window ends at seq1=Input(10)
        assert_eq!(inn.accept(&f_a).unwrap().entries, vec![input(10)]);

        out.push_input(11, 0);
        let _dropped = make_frame(&out); // imagine this is lost

        out.push_input(12, 0);
        let f_c = make_frame(&out); // window includes 11 and 12 (redundancy)
                                          // Frontier was at seq for 11; the lost frame's 11 arrives here.
        assert_eq!(inn.accept(&f_c).unwrap().entries, vec![input(11), input(12)]);
    }

    #[test]
    fn in_buffers_out_of_order_then_fills_gap() {
        let mut inn = InStream::new();
        // Deliver a frame starting at seq 3 (1 and 2 missing): buffered, none delivered.
        let ahead = Frame::data(3, 0, vec![input(30), input(40)], 1);
        assert_eq!(inn.accept(&ahead).unwrap().entries, vec![]);
        assert_eq!(inn.ack(), 1);
        // The ack reports the frontier: still seq 1 (the hole), regardless of
        // the out-of-order seqs 3,4 sitting in the reorder buffer.
        assert_eq!(inn.ack(), 1);
        // Now the gap arrives; everything drains in order.
        let gap = Frame::data(1, 0, vec![input(10), input(20)], 1);
        assert_eq!(
            inn.accept(&gap).unwrap().entries,
            vec![input(10), input(20), input(30), input(40)]
        );
        assert_eq!(inn.ack(), 5);
    }

    #[test]
    fn in_bails_past_horizon() {
        let mut inn = InStream::new();
        // ack is 1; this is exactly a horizon away.
        let way_ahead = Frame::data(1 + HORIZON, 0, vec![input(1)], 1);
        assert_eq!(inn.accept(&way_ahead), Err(HorizonExceeded));
    }

    #[test]
    fn markers_ride_in_band_in_order() {
        let mut out = OutStream::new();
        let mut inn = InStream::new();
        out.push_input(1, 0);
        out.push(Element::EndOfRound);
        out.push_input(2, 0);
        out.push(Element::EndOfMatch);
        let f = make_frame(&out);
        assert_eq!(
            inn.accept(&f).unwrap().entries,
            vec![
                Element::Input(1),
                Element::EndOfRound,
                Element::Input(2),
                Element::EndOfMatch,
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
        let f = make_frame(&out);
        inn.accept(&f).unwrap();
        // The in-stream now wants seq 5; its ack should advance the peer's
        // out-stream frontier so it trims to the redundancy floor.
        out.apply_ack(inn.ack());
        assert_eq!(out.window_len(), DEFAULT_REDUNDANCY as usize);
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
            let w = out.window().unwrap();
            let frame = Frame::data(w.base, w.frame_advantage, w.entries, 1);
            if k % 3 != 0 {
                // delivered: round-trip through the wire and ingest.
                let f = Frame::decode(&frame.encode()).unwrap();
                delivered.extend(f_inputs(inn.accept(&f).unwrap().entries));
                out.apply_ack(inn.ack());
            }
        }
        let expected: Vec<u32> = (1..=200).collect();
        assert_eq!(delivered, expected);
    }

    fn f_inputs(els: Vec<Element>) -> Vec<u32> {
        els.into_iter()
            .filter_map(|e| match e {
                Element::Input(j) => Some(j as u32),
                Element::EndOfRound | Element::EndOfMatch => None,
            })
            .collect()
    }
}

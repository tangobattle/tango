//! Transport-agnostic reliability state machines for the [`Frame`] wire
//! protocol ([`crate::frame`]). Two halves, both pure (no async, no I/O) and
//! generic over the element type `E`:
//!
//! * [`OutStream`] — assigns a monotonic seq to each local element, keeps a
//!   redundancy window of recent unconfirmed elements (`history`), and trims
//!   it as the peer's cumulative acks confirm receipt. [`OutStream::window`] is
//!   what goes into an outbound [`Frame`].
//! * [`InStream`] — reassembles the peer's stream from possibly-lossy,
//!   reordered, duplicated frames: a reorder buffer feeds elements out in
//!   strict seq order ([`InStream::accept`]), generates the cumulative ack to
//!   send back ([`InStream::ack`]), and bails when a gap grows past the
//!   rollback horizon.
//!
//! Recovery is proactive, not request/response: a lost element is re-sent in
//! the *next* frame's window (cost ~one frame), so single/short losses never
//! pay a round-trip. The ack only drives window *trimming* (and would drive
//! selective resend for bursts longer than the window — see
//! [`OutStream::trim`]). Nothing here knows what an element *means*; the caller
//! maps elements to its own event type.
//!
//! The rollback horizon is a constructor parameter (`new(horizon)`) rather than
//! a constant: it's a property of the consuming engine's input buffer, not of
//! the protocol.
use std::collections::VecDeque;

use crate::frame::{Frame, Codec};

/// Proactive redundancy floor: the minimum elements every data frame carries
/// regardless of acks, so a dropped datagram is covered by the next one without
/// waiting for the peer's ack to report the hole.
///
/// A fixed floor, not an adaptive knob. The redundancy window is
/// `max(REDUNDANCY, unconfirmed_span)` (see [`OutStream::trim`]), and the
/// unconfirmed span — the genuinely in-flight, not-yet-acked tail, re-sent every
/// frame — already grows with the round-trip, so it recovers loss on its own at
/// any RTT past a frame or two. This floor only bites when the peer is caught up
/// to within `REDUNDANCY` (a sub-frame, LAN-class round-trip), where the span
/// would otherwise collapse below it; there it guarantees a dropped datagram
/// still has a copy in the next one. Two is plenty: a larger floor is inert,
/// since the span overtakes it before the floor would ever apply.
pub const REDUNDANCY: u32 = 2;

/// A contiguous run of elements plus the per-frame [`Meta`] that rides with
/// them. Both halves of the protocol produce one: [`OutStream::window`] builds
/// the outbound redundancy window (wrapped into a [`Frame`]), and
/// [`InStream::accept`] returns the run it just made contiguous, tagged with the
/// freshest meta, for the receive side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window<P: Codec> {
    /// Seq of `entries[0]`: the frame's `base` outbound, the oldest
    /// just-delivered seq inbound. When `entries` is empty, the next seq the
    /// producer expects (`next_seq` / `recv_base`).
    pub base: u32,
    /// Per-frame side-channel carried with this run: the newest local input's
    /// value outbound, the freshest seen inbound.
    pub meta: P::Meta,
    /// The elements, ascending by seq from `base`.
    pub entries: Vec<P::Element>,
}

/// Sender half: seq assignment + redundancy window.
pub struct OutStream<P: Codec> {
    /// Next seq to hand out; the seq line is 0-based (the first element gets
    /// seq 0). There's no reserved sentinel — an ack-only [`Frame`] is told
    /// from a data one by the absence of a body, not by a magic seq.
    next_seq: u32,
    /// Unconfirmed/redundancy window, ascending by seq. Exactly the set
    /// [`window`](Self::window) emits. Seqs aren't stored per element: the
    /// window is a contiguous run ending at `next_seq - 1` (only `push` appends,
    /// only `trim` pops the front), so the front's seq is always
    /// [`base`](Self::base) `== next_seq - history.len()`.
    history: VecDeque<P::Element>,
    /// Peer's contiguous frontier (lowest seq it hasn't confirmed). Trims
    /// `history`; only ever advances.
    peer_ack_base: u32,
    /// Newest input's [`crate::Meta`], echoed once per frame.
    latest_meta: P::Meta,
    /// Rollback horizon: history older than this is dropped (the peer would
    /// bail rather than roll back that far, so retaining it is pointless).
    horizon: u32,
}

impl<P: Codec> OutStream<P> {
    /// Build an out-stream whose redundancy window never retains more than
    /// `horizon` elements (the consuming engine's rollback depth).
    pub fn new(horizon: u32) -> Self {
        Self {
            next_seq: 0,
            history: VecDeque::new(),
            peer_ack_base: 0,
            latest_meta: P::Meta::default(),
            horizon,
        }
    }

    /// Append an element at the next seq and record the [`crate::Meta`] it
    /// carries; returns its seq. The meta-bearing counterpart to
    /// [`push`](Self::push) — use it for elements that update the side-channel
    /// (e.g. inputs carrying a fresh time-sync lead); elements that don't
    /// (markers) go through `push`, which leaves the meta unchanged.
    pub fn push_with_meta(&mut self, e: P::Element, meta: P::Meta) -> u32 {
        self.latest_meta = meta;
        self.push(e)
    }

    /// Append any element at the next seq; returns it. Markers ride in-band on
    /// the same seq line as inputs.
    pub fn push(&mut self, e: P::Element) -> u32 {
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

    /// Drop history the peer has confirmed, while keeping at least the
    /// redundancy floor ([`REDUNDANCY`]) of recent elements and no more than a
    /// horizon's worth (beyond the horizon the peer would bail, so retaining
    /// them is pointless). The window it leaves is thus
    /// `max(REDUNDANCY, unconfirmed_span)`, capped at `horizon`.
    fn trim(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let newest = self.next_seq - 1;
        let redundancy_floor = newest.saturating_sub(REDUNDANCY.saturating_sub(1));
        let horizon_floor = newest.saturating_sub(self.horizon.saturating_sub(1));
        let keep_from = self.peer_ack_base.min(redundancy_floor).max(horizon_floor);
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

    /// The data portion of an outbound frame — see [`Window`]. Always available:
    /// before the first element is pushed the history is empty, so it reports an
    /// empty run at `next_seq` — an "ack-only" frame. The freshest [`crate::Meta`]
    /// rides along regardless.
    pub fn window(&self) -> Window<P> {
        Window {
            base: self.base(),
            meta: self.latest_meta,
            entries: self.history.iter().copied().collect(),
        }
    }

    /// The newest seq handed out so far, or `None` before the first push. The
    /// caller timestamps this on each send to derive RTT from the peer's ack
    /// of it.
    pub fn newest_seq(&self) -> Option<u32> {
        (self.next_seq > 0).then(|| self.next_seq - 1)
    }

    /// The next seq this stream will assign. Used as the `base` of an ack-only
    /// frame, which carries no entries but still reports the sender's position.
    pub fn next_seq(&self) -> u32 {
        self.next_seq
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
pub struct InStream<P: Codec> {
    /// Next contiguous seq we still need. Everything below has been
    /// delivered. Starts at 0, matching [`OutStream`]'s 0-based seq line.
    recv_base: u32,
    /// Elements received ahead of `recv_base` that can't be delivered yet (a
    /// gap precedes them), as a ring indexed by distance from `recv_base`:
    /// slot `k` holds the seq `recv_base + k`, `None` for one not yet received.
    /// Grown on demand and bounded by `horizon`; the front slides up as the
    /// contiguous prefix drains. Replaces a `BTreeMap<seq, E>` — the bounded,
    /// contiguous index means steady-state in-order delivery touches only slot 0
    /// with no per-element node allocation. A redundant copy of an
    /// already-buffered seq is dropped (first copy wins), which is how
    /// duplicates dedup.
    buffer: VecDeque<Option<P::Element>>,
    /// Freshest [`crate::Meta`] seen; [`accept`](Self::accept) returns it with
    /// every delivery so the caller can tag the delivered elements. Persisted
    /// across calls for the reorder guard below.
    latest_meta: P::Meta,
    /// Newest seq whose frame set [`latest_meta`](Self::latest_meta). Datagrams
    /// reorder under jitter, so a *later-arriving* frame can be an *older* one;
    /// without this guard its stale meta would overwrite the fresh one (and, for
    /// a time-sync field, jerk the clock-sync skew backward). Only a frame
    /// reaching at least this far updates the meta.
    latest_meta_seq: u32,
    /// Rollback horizon: a gap wider than this can't be rolled back to, so
    /// [`accept`](Self::accept) bails instead of buffering forever.
    horizon: u32,
}

impl<P: Codec> InStream<P> {
    /// Build an in-stream that bails once a gap grows past `horizon` (the
    /// consuming engine's rollback depth).
    pub fn new(horizon: u32) -> Self {
        Self {
            recv_base: 0,
            buffer: VecDeque::new(),
            latest_meta: P::Meta::default(),
            latest_meta_seq: 0,
            horizon,
        }
    }

    /// Ingest one frame's entries. Returns the run that became contiguous (in
    /// strict seq order, possibly empty) as a [`Window`]: `entries` are the
    /// newly-delivered elements, `base` their starting seq, and `meta` the
    /// freshest per-frame value seen so far — the reorder guard below keeps a
    /// late-arriving older frame from regressing it. An empty-body frame (an
    /// "ack-only") delivers no entries but can still refresh the meta. The
    /// frame's cumulative ack is the caller's job to apply to its [`OutStream`].
    pub fn accept(&mut self, frame: &Frame<P>) -> Result<Window<P>, HorizonExceeded> {
        let base = frame.base;
        let entries = &frame.entries;
        // Only the newest-by-seq frame's meta is fresh; a reordered older frame
        // arriving later must not clobber it (its meta is stale). An empty-body
        // frame reports the sender's newest seq as `base - 1`, so a redundant
        // empty "ack" can refresh the meta but never regress it.
        let frame_newest = base.saturating_add(entries.len() as u32).saturating_sub(1);
        if frame_newest >= self.latest_meta_seq {
            self.latest_meta_seq = frame_newest;
            self.latest_meta = frame.meta;
        }
        for (i, &e) in entries.iter().enumerate() {
            // Saturating: `base` is peer-supplied, so `base + i` mustn't
            // overflow. A saturated seq lands past the horizon and is rejected
            // below, same as any other too-far-ahead value.
            let seq = base.saturating_add(i as u32);
            if seq < self.recv_base {
                continue; // already delivered — a redundant/duplicate copy.
            }
            let offset = (seq - self.recv_base) as usize;
            if offset >= self.horizon as usize {
                // Too far ahead of our frontier to ever roll back to.
                return Err(HorizonExceeded);
            }
            // Slot `offset` holds seq `recv_base + offset`; grow the ring to
            // reach it (the guard means this only ever extends, never truncates),
            // filling any skipped slots with `None` gaps.
            if offset >= self.buffer.len() {
                self.buffer.resize(offset + 1, None);
            }
            // First copy wins, like the old `entry(..).or_insert(..)` — a
            // redundant resend of an already-buffered seq is dropped.
            if self.buffer[offset].is_none() {
                self.buffer[offset] = Some(e);
            }
        }
        // `recv_base` before draining is the seq of the first delivered element.
        let delivered_base = self.recv_base;
        let mut delivered = Vec::new();
        // Pop the contiguous run off the front; each pop slides slot 0 up to the
        // next seq, preserving `slot k == recv_base + k`. Stops at the first gap
        // (a `None` front) or when the ring empties.
        while matches!(self.buffer.front(), Some(Some(_))) {
            delivered.push(self.buffer.pop_front().unwrap().unwrap());
            self.recv_base += 1;
        }
        Ok(Window {
            base: delivered_base,
            meta: self.latest_meta,
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
    use crate::testutil::{El, RawProto};

    /// Horizon used across these tests (the engine's input-buffer cap).
    const HORIZON: u32 = 300;

    // The streams run on `RawProto` (raw-u16 body, plain `i16` meta standing in
    // for a caller's time-sync field — testutil impls [`crate::Meta`] for it).
    fn out() -> OutStream<RawProto> {
        OutStream::new(HORIZON)
    }
    fn inn() -> InStream<RawProto> {
        InStream::new(HORIZON)
    }
    fn input(j: u16) -> El {
        El::Input(j)
    }
    fn push_input(out: &mut OutStream<RawProto>, j: u16, fa: i16) {
        out.push_with_meta(El::Input(j), fa);
    }
    /// Wrap an entry run into a frame.
    fn frame(base: u32, fa: i16, entries: Vec<El>, ack: u32) -> Frame<RawProto> {
        Frame::new(base, ack, fa, entries)
    }

    /// Build a frame from the out-stream's current window, then round-trip it
    /// through the wire codec (so tests exercise the real encode/decode too).
    /// The ack is irrelevant to these reassembly tests — the in-stream ignores
    /// it — so it's pinned to the initial frontier.
    fn make_frame(out: &OutStream<RawProto>) -> Frame<RawProto> {
        let w = out.window();
        Frame::decode(&mut &frame(w.base, w.meta, w.entries, 1).to_vec()[..]).unwrap()
    }

    #[test]
    fn window_floor_when_peer_caught_up() {
        let mut out = out();
        push_input(&mut out, 1, 0);
        push_input(&mut out, 2, 0);
        push_input(&mut out, 3, 0);
        // Peer has confirmed everything through seq 2 (frontier = 3).
        out.apply_ack(3);
        // Still keeps REDUNDANCY recent elements (seqs are 0..=2).
        assert_eq!(out.window_len(), REDUNDANCY as usize);
        let w = out.window();
        assert_eq!(w.base, 2 - (REDUNDANCY - 1)); // seq of first kept = 1
        assert_eq!(w.entries.len(), REDUNDANCY as usize);
    }

    #[test]
    fn window_is_max_of_floor_and_unconfirmed_span() {
        let mut out = out();
        for k in 0..10 {
            push_input(&mut out, k, 0);
        }
        // Peer lagging: frontier 4 leaves seqs 4..=9 unconfirmed (six >
        // REDUNDANCY), so the unconfirmed span governs the window.
        out.apply_ack(4);
        assert_eq!(out.window_len(), 10 - 4); // the whole unconfirmed span

        // Peer catches up (frontier = 10, a monotonic advance): the span
        // collapses below the floor, so the fixed floor governs — exactly
        // REDUNDANCY recent elements remain.
        out.apply_ack(10);
        assert_eq!(out.window_len(), REDUNDANCY as usize);
    }

    #[test]
    fn window_grows_with_peer_lag() {
        let mut out = out();
        for k in 0..10 {
            push_input(&mut out, k, 0);
        }
        // Peer only confirmed through seq 3 (frontier = 4): seqs 4..=9 unconfirmed.
        out.apply_ack(4);
        let w = out.window();
        assert_eq!(w.base, 4);
        assert_eq!(w.entries.len(), 6); // 4,5,6,7,8,9
    }

    #[test]
    fn ack_never_moves_backwards() {
        let mut out = out();
        for k in 0..10 {
            push_input(&mut out, k, 0);
        }
        out.apply_ack(8);
        out.apply_ack(3); // stale/reordered
        let w = out.window();
        assert_eq!(w.base, 8); // didn't regress
    }

    #[test]
    fn ack_beyond_sent_is_clamped() {
        let mut out = out();
        push_input(&mut out, 1, 0);
        push_input(&mut out, 2, 0); // next_seq = 2
                                    // A peer can't have received a seq we never sent; the bogus frontier
                                    // is clamped to next_seq rather than pinned far into the future.
        out.apply_ack(9999);
        assert_eq!(out.peer_ack_base(), 2);
    }

    #[test]
    fn in_delivers_contiguously_in_order() {
        let mut out = out();
        let mut inn = inn();
        for k in 1..=5u16 {
            push_input(&mut out, k, 0);
        }
        let f = make_frame(&out);
        let delivered = inn.accept(&f).unwrap();
        assert_eq!(delivered.entries, (1..=5).map(input).collect::<Vec<_>>());
        assert_eq!(inn.ack(), 5);
    }

    #[test]
    fn in_dedups_redundant_copies() {
        let mut out = out();
        let mut inn = inn();
        push_input(&mut out, 1, 0);
        push_input(&mut out, 2, 0);
        let f1 = make_frame(&out);
        assert_eq!(inn.accept(&f1).unwrap().entries, vec![input(1), input(2)]);
        push_input(&mut out, 3, 0);
        // f2's window still re-includes 2 (redundancy) plus the new 3.
        let f2 = make_frame(&out);
        // 2 is a dup (already delivered), only 3 is new.
        assert_eq!(inn.accept(&f2).unwrap().entries, vec![input(3)]);
        assert_eq!(inn.ack(), 3);
    }

    #[test]
    fn in_recovers_a_dropped_frame_via_redundancy() {
        let mut out = out();
        let mut inn = inn();
        // Each "frame" carries the last REDUNDANCY+ elements. Drop one
        // datagram entirely; the next one's window should still cover it.
        push_input(&mut out, 10, 0);
        let f_a = make_frame(&out); // window ends at seq0=Input(10)
        assert_eq!(inn.accept(&f_a).unwrap().entries, vec![input(10)]);

        push_input(&mut out, 11, 0);
        let _dropped = make_frame(&out); // imagine this is lost

        push_input(&mut out, 12, 0);
        let f_c = make_frame(&out); // window includes 11 and 12 (redundancy)
                                    // Frontier was at seq for 11; the lost frame's 11 arrives here.
        assert_eq!(inn.accept(&f_c).unwrap().entries, vec![input(11), input(12)]);
    }

    #[test]
    fn in_buffers_out_of_order_then_fills_gap() {
        let mut inn = inn();
        // Deliver a frame starting at seq 2 (0 and 1 missing): buffered, none delivered.
        let ahead = frame(2, 0, vec![input(30), input(40)], 1);
        assert_eq!(inn.accept(&ahead).unwrap().entries, vec![]);
        assert_eq!(inn.ack(), 0);
        // The ack reports the frontier: still seq 0 (the hole), regardless of
        // the out-of-order seqs 2,3 sitting in the reorder buffer.
        assert_eq!(inn.ack(), 0);
        // Now the gap arrives; everything drains in order.
        let gap = frame(0, 0, vec![input(10), input(20)], 1);
        assert_eq!(
            inn.accept(&gap).unwrap().entries,
            vec![input(10), input(20), input(30), input(40)]
        );
        assert_eq!(inn.ack(), 4);
    }

    #[test]
    fn in_bails_past_horizon() {
        let mut inn = inn();
        // ack is 0; this is exactly a horizon away.
        let way_ahead = frame(HORIZON, 0, vec![input(1)], 1);
        assert_eq!(inn.accept(&way_ahead), Err(HorizonExceeded));
    }

    #[test]
    fn markers_ride_in_band_in_order() {
        let mut out = out();
        let mut inn = inn();
        push_input(&mut out, 1, 0);
        out.push(El::EndOfRound);
        push_input(&mut out, 2, 0);
        out.push(El::EndOfMatch);
        let f = make_frame(&out);
        assert_eq!(
            inn.accept(&f).unwrap().entries,
            vec![El::Input(1), El::EndOfRound, El::Input(2), El::EndOfMatch]
        );
    }

    #[test]
    fn ack_round_trips_to_out_stream() {
        let mut out = out();
        let mut inn = inn();
        for k in 1..=4u16 {
            push_input(&mut out, k, 0);
        }
        let f = make_frame(&out);
        inn.accept(&f).unwrap();
        // The in-stream now wants seq 4; its ack should advance the peer's
        // out-stream frontier so it trims to the redundancy floor.
        out.apply_ack(inn.ack());
        assert_eq!(out.window_len(), REDUNDANCY as usize);
    }

    #[test]
    fn lossy_stream_converges() {
        // Drive 200 inputs through a flaky link that drops every 3rd
        // datagram, with acks flowing back. Every input must be delivered
        // exactly once, in order, and never bail (loss stays within window).
        let mut out = out();
        let mut inn = inn();
        let mut delivered = Vec::new();
        for k in 1..=200u32 {
            push_input(&mut out, k as u16, 0);
            let w = out.window();
            let dg = frame(w.base, w.meta, w.entries, 1);
            if k % 3 != 0 {
                // delivered: round-trip through the wire and ingest.
                let f = Frame::<RawProto>::decode(&mut &dg.to_vec()[..]).unwrap();
                delivered.extend(f_inputs(inn.accept(&f).unwrap().entries));
                out.apply_ack(inn.ack());
            }
        }
        let expected: Vec<u32> = (1..=200).collect();
        assert_eq!(delivered, expected);
    }

    fn f_inputs(els: Vec<El>) -> Vec<u32> {
        els.into_iter()
            .filter_map(|e| match e {
                El::Input(j) => Some(j as u32),
                El::EndOfRound | El::EndOfMatch => None,
            })
            .collect()
    }
}

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
        self.history.push_back((seq, e));
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
        let newest = match self.history.back() {
            Some(&(seq, _)) => seq,
            None => return,
        };
        let redundancy_floor = newest.saturating_sub(self.min_redundancy.saturating_sub(1));
        let horizon_floor = newest.saturating_sub(HORIZON.saturating_sub(1));
        let keep_from = self.peer_ack_base.min(redundancy_floor).max(horizon_floor).max(1);
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
    /// Freshest frame-advantage seen, attached to delivered inputs by the
    /// adapter. `None` until the first data frame.
    latest_advantage: Option<i16>,
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
            latest_advantage: None,
            latest_advantage_seq: 0,
        }
    }

    /// Ingest one frame's entries. Returns the elements that became
    /// contiguous (in strict seq order, possibly empty). The frame's
    /// cumulative ack, if any, is the caller's job to apply to its [`OutStream`].
    pub fn accept(&mut self, frame: &Frame) -> Result<Vec<Element>, HorizonExceeded> {
        // Ack-only frames carry no input data — nothing to reassemble.
        let (base, frame_advantage, entries) = match frame {
            Frame::Ack(_) => return Ok(Vec::new()),
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
            self.latest_advantage = Some(frame_advantage);
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
        let mut delivered = Vec::new();
        while let Some(e) = self.buffer.remove(&self.recv_base) {
            delivered.push(e);
            self.recv_base += 1;
        }
        Ok(delivered)
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

    fn input(j: u16) -> Element {
        Element::Input(j)
    }

    /// Build a frame from the out-stream's current window + the in-stream's
    /// ack, then round-trip it through the wire codec (so tests exercise the
    /// real encode/decode too).
    fn make_frame(out: &OutStream, ack: Option<u32>) -> Frame {
        let (base, fa, entries) = out.window().expect("window");
        let frame = Frame::data(base, fa, entries, ack);
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
        let (base, _, entries) = out.window().unwrap();
        assert_eq!(base, 3 - (DEFAULT_REDUNDANCY - 1)); // seq of first kept = 2
        assert_eq!(entries.len(), DEFAULT_REDUNDANCY as usize);
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
        out.apply_ack(8);
        out.apply_ack(3); // stale/reordered
        let (base, _, _) = out.window().unwrap();
        assert_eq!(base, 8); // didn't regress
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
        let ahead = Frame::data(3, 0, vec![input(30), input(40)], None);
        assert_eq!(inn.accept(&ahead).unwrap(), vec![]);
        assert_eq!(inn.recv_base(), 1);
        // The ack reports the frontier: still seq 1 (the hole), regardless of
        // the out-of-order seqs 3,4 sitting in the reorder buffer.
        assert_eq!(inn.ack(), 1);
        // Now the gap arrives; everything drains in order.
        let gap = Frame::data(1, 0, vec![input(10), input(20)], None);
        assert_eq!(
            inn.accept(&gap).unwrap(),
            vec![input(10), input(20), input(30), input(40)]
        );
        assert_eq!(inn.recv_base(), 5);
    }

    #[test]
    fn in_bails_past_horizon() {
        let mut inn = InStream::new();
        // recv_base is 1; this is exactly a horizon away.
        let way_ahead = Frame::data(1 + HORIZON, 0, vec![input(1)], None);
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
        let f = make_frame(&out, None);
        assert_eq!(
            inn.accept(&f).unwrap(),
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
        let f = make_frame(&out, None);
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
            let (base, fa, entries) = out.window().unwrap();
            let frame = Frame::data(base, fa, entries, None);
            if k % 3 != 0 {
                // delivered: round-trip through the wire and ingest.
                let f = Frame::decode(&frame.encode()).unwrap();
                delivered.extend(f_inputs(inn.accept(&f).unwrap()));
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

    /// Route two peers' inputs through the *real* `OutStream`/`InStream` — the
    /// growing redundancy window, the cumulative ack (which rides the same lossy
    /// frames), reassembly, and the wire codec — over a bursty-lossy, laggy link,
    /// and watch the clock-sync skew each side computes (extrapolated lead minus
    /// the peer's reported advantage, as getgud does). This exercises the actual
    /// reliability dynamics — window growth while acks are missing, recovery
    /// gated on the ack round-trip — that a fixed-redundancy model can't show.
    #[test]
    fn skew_through_real_data_channel() {
        struct Peer {
            out: OutStream,
            inn: InStream,
            local: u32,           // local inputs produced
            remote: u32,          // remote inputs delivered in order
            ticks_since: u32,     // local ticks since a remote input landed
            last_remote_adv: i32, // freshest advantage the peer reported
        }
        impl Peer {
            fn new() -> Self {
                Peer {
                    out: OutStream::new(),
                    inn: InStream::new(),
                    local: 0,
                    remote: 0,
                    ticks_since: 0,
                    last_remote_adv: 0,
                }
            }
            // getgud's local_tick_advantage: lead minus ticks since we last heard.
            fn synced_lead(&self) -> i32 {
                (self.local as i32 - self.remote as i32) - self.ticks_since as i32
            }
        }

        const FRAMES: u32 = 18_000; // 5 min at 60 Hz
        const LATENCY: u32 = 6; // ~100 ms one way
                                // Variable latency (reorders datagrams). With loss alone the extrapolated
                                // skew is dead flat; jitter is what shows through — both peers' leads
                                // wobble with delivery timing, so the skew (their difference) swings about
                                // ±jitter on each side, i.e. a span ≈ 2 × JITTER. (Sweeping the real
                                // channel: JITTER 3 → span 6, 8 → 16, 15 → 29.) Pinned at 0 here so the
                                // test asserts the loss-taming; the jitter scaling is the residual to smooth.
        const JITTER: u32 = 0;
        const BURST: u64 = 12;
        const LOSS_PCT: u64 = 10;
        let start_ppm = LOSS_PCT * 1_000_000 / ((100 - LOSS_PCT) * BURST);

        let mut peers = [Peer::new(), Peer::new()];
        // (arrival frame, recipient, encoded datagram).
        let mut wire: Vec<(u32, usize, Vec<u8>)> = Vec::new();
        let mut rng = 0x1234_5678_9abc_def0u64;
        let mut bad = [0u64; 2];
        let mut next = || {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            rng >> 40
        };
        let mut skew = [Vec::<i32>::new(), Vec::<i32>::new()];

        for f in 0..FRAMES {
            // Deliver everything scheduled to land this frame.
            let mut k = 0;
            while k < wire.len() {
                if wire[k].0 == f {
                    let (_, dst, bytes) = wire.swap_remove(k);
                    let frame = Frame::decode(&bytes).expect("decode");
                    let ack = match &frame {
                        Frame::Ack(a) => Some(*a),
                        Frame::Data { ack, .. } => *ack,
                    };
                    if let Some(a) = ack {
                        peers[dst].out.apply_ack(a);
                    }
                    let delivered = peers[dst]
                        .inn
                        .accept(&frame)
                        .unwrap_or_else(|_| panic!("InStream bailed (HorizonExceeded) at frame {f}"));
                    let n = delivered.iter().filter(|e| matches!(e, Element::Input(_))).count() as u32;
                    if n > 0 {
                        peers[dst].remote += n;
                        peers[dst].ticks_since = 0;
                    }
                    if let Some(a) = peers[dst].inn.latest_advantage() {
                        peers[dst].last_remote_adv = a as i32;
                    }
                } else {
                    k += 1;
                }
            }
            // Record each side's skew (post-deliver, pre-advance — what round.rs reads).
            for p in 0..2 {
                skew[p].push(peers[p].synced_lead() - peers[p].last_remote_adv);
            }
            // Produce this tick + ship, dropping loss bursts.
            for src in 0..2 {
                let adv = peers[src].synced_lead().clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                peers[src].local += 1;
                peers[src].ticks_since += 1;
                peers[src].out.push_input(0, adv);
                let (base, fa, entries) = peers[src].out.window().unwrap();
                let frame = Frame::data(base, fa, entries, Some(peers[src].inn.ack()));
                let bytes = frame.encode();
                let lost = if bad[src] > 0 {
                    bad[src] -= 1;
                    true
                } else if next() % 1_000_000 < start_ppm {
                    bad[src] = BURST - 1;
                    true
                } else {
                    false
                };
                if !lost {
                    let extra = (next() % (JITTER as u64 + 1)) as u32;
                    wire.push((f + LATENCY + extra, 1 - src, bytes));
                }
            }
        }

        let warmup = 1_200;
        let report = |s: &[i32]| {
            let lo = *s.iter().min().unwrap();
            let hi = *s.iter().max().unwrap();
            (lo, hi, hi - lo)
        };
        let (lo0, hi0, sp0) = report(&skew[0][warmup..]);
        let (lo1, hi1, sp1) = report(&skew[1][warmup..]);
        eprintln!(
            "real data channel @ 100ms / {LOSS_PCT}% bursty loss, {JITTER} jitter — P1 skew [{lo0}, {hi0}] span {sp0}, P2 skew [{lo1}, {hi1}] span {sp1}"
        );
        // Through the real reliability layer, bursty loss alone leaves the
        // extrapolated clock-sync skew dead flat — recovery + extrapolation
        // cancel it. (Jitter is the residual; see JITTER above.)
        assert!(sp0 <= 2 && sp1 <= 2, "loss-only skew should be flat, got {sp0}/{sp1}");
    }
}

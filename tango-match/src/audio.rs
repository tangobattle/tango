//! Where a console's sound comes out: the ring the simulation pushes
//! into and a host's sound callback pulls from.
//!
//! A console's audio has to cross from the thread turning the
//! simulation's crank to the thread the device's callback runs on, and
//! the crossing is the whole problem. Reaching *into* a console means
//! taking the lock its simulation ticks under — where a tick is a
//! console (or two) emulating a whole frame and a rollback puts a
//! multi-megabyte restore in front of that — so a callback can only try
//! the lock and give up. Under real load it gives up essentially every
//! time: a saturated drive loop holds that lock for almost all of wall
//! time, which is exactly when there is the most audio to play. Whole
//! seconds of production sit unreachable and the device gets silence.
//!
//! So the direction inverts. The simulation pushes what each tick
//! produced into [`channel`]'s ring — it is already holding the console,
//! so the push is free — and the callback reads the other end without
//! reaching for a console at all. What the callback can play is whatever
//! has crossed, which is everything the sim produced up to the last
//! tick, rather than whatever it managed to steal a console for.
//!
//! Which leaves the one lock the callback does take: this ring's own.
//! It is never held for longer than a copy of the audio actually
//! crossing — microseconds, against the tens of milliseconds a console's
//! lock is held for — so it cannot be the thing that starves anybody.
//! *Which* lock the callback waits on was always the point, not whether
//! there was one, so the ring is a mutex over a plain `Vec` drained off
//! the front: being lock-free bought nothing a listener could hear, and
//! cost a pile of hand-proved cursor arithmetic to have.
//!
//! # Taking audio back
//!
//! Rollback is what makes this more than a queue. Audio is not machine
//! state — no emulator's savestate carries the buffer a frontend reads
//! from — so when the engine restores a snapshot and re-simulates, the
//! span the speculation already voiced is still sitting there and the
//! re-simulation produces it a second time. The ring answers that by
//! letting the producer *rewind*: everything past the mark that the
//! callback has not read yet is un-published by truncating the queue,
//! which is a length store rather than a copy. What the callback already
//! read cannot be unplayed, so it comes back as a debt
//! ([`AudioIn::revoke_to`]'s return) that the next pushes pay off by
//! dropping the regeneration on the way in.
//!
//! Because the backlog stays in the ring until it is genuinely played,
//! the revocable window is the whole queue depth — much more than a
//! console's own small ring could ever hold.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

/// Interleaved stereo, everywhere audio crosses this seam.
pub const CHANNELS: usize = 2;

/// Ceiling on one [`Side::drain_audio`](crate::Side) call, in frames —
/// a bound on the pump's scratch buffer. One tick of a 65536 Hz cart is
/// about 1100 frames, so a chunk this size takes any single tick's
/// production in one call and the loop below is only ever a guard.
const DRAIN_CHUNK: usize = 4096;

/// A ring of interleaved stereo frames with one producer and one
/// consumer, holding what has been published and not yet played.
///
/// Both ends reach it under the same lock, so neither can see half of
/// what the other did: a fill and a rewind cannot interleave, and what
/// is queued is simply the length of the queue.
struct Ring {
    /// The queue itself, oldest frame first: published, unplayed, and
    /// still revocable from the young end.
    frames: Vec<i16>,
    /// The coordinate the front of `frames` sits at — frames the
    /// consumer has taken, and so the floor a rewind may not go below.
    /// What makes [`AudioIn::produced`] something a snapshot can hold on
    /// to rather than a position that shifts as the queue drains.
    head: u64,
    /// How many frames the queue may hold before a push starts dropping.
    capacity: usize,
    /// The console's production rate in Hz, published by whoever pushes.
    /// Read per fill because a cart can change it at runtime — BN4+ flip
    /// from 32768 to 65536 after boot — and the whole resample ratio is
    /// built on it.
    rate: f64,
}

impl Ring {
    /// Frames queued.
    fn queued(&self) -> usize {
        self.frames.len() / CHANNELS
    }

    /// Frames ever published: the coordinate the young end of the queue
    /// sits at.
    fn published(&self) -> u64 {
        self.head + self.queued() as u64
    }

    /// Hand `frames` over off the old end; the head follows them.
    fn take(&mut self, frames: usize) {
        self.frames.drain(..frames * CHANNELS);
        self.head += frames as u64;
    }
}

/// A ring sized to hold `capacity` frames, as the two ends that share
/// it: [`AudioIn`] for whoever runs the simulation, [`AudioOut`] for
/// whoever plays it.
///
/// Size it well past the queue a host means to hold: a producer that
/// runs out of room drops what will not fit, and the point of the ring
/// is that a burst — a seek chase, a device stall's catch-up — lands
/// somewhere the consumer can shed it deliberately.
pub fn channel(capacity: usize) -> (AudioIn, AudioOut) {
    let capacity = capacity.max(1);
    let ring = Arc::new(Mutex::new(Ring {
        // The whole capacity up front, since a push may not grow it
        // past that and a drain off the front never gives room back: the
        // allocation happens here and never again on either end's path.
        frames: Vec::with_capacity(capacity * CHANNELS),
        head: 0,
        capacity,
        // Stood in for until the first push, and only ever divided by:
        // the GBA's own rate is what every game a session runs produces
        // at, and a zero here would rebase the first fill's resampling.
        rate: 32768.0,
    }));
    (AudioIn { ring: ring.clone() }, AudioOut { ring })
}

/// The producing end, held by whoever turns the simulation's crank.
pub struct AudioIn {
    ring: Arc<Mutex<Ring>>,
}

impl AudioIn {
    /// Publish interleaved stereo frames, dropping whatever will not
    /// fit. Answers with how many landed.
    ///
    /// Dropping the newest rather than making room is deliberate: a full
    /// ring means the consumer is already far past the level it steers
    /// for and is shedding the backlog from the oldest end itself.
    /// Racing it from this end would drop audio out of the middle of
    /// what is about to play.
    pub fn push(&mut self, frames: &[i16]) -> usize {
        let mut ring = self.ring.lock().unwrap();
        // Never underflows: this is the only thing that grows the queue,
        // and it is what holds it to the capacity reserved up front — so
        // the extend below is a copy and never an allocation.
        let n = (frames.len() / CHANNELS).min(ring.capacity - ring.queued());
        ring.frames.extend_from_slice(&frames[..n * CHANNELS]);
        n
    }

    /// Frames ever published — the coordinate a snapshot marks so a
    /// rollback knows what its speculation voiced.
    pub fn produced(&self) -> u64 {
        self.ring.lock().unwrap().published()
    }

    /// Take back everything published since `mark`, and answer with what
    /// could not be taken back.
    ///
    /// What the consumer has not reached is un-published by truncating
    /// the queue — no copying, and the room is taken again by the
    /// re-simulation. What it already read cannot be unplayed, so it
    /// comes back as a frame count: the caller owes that many frames of
    /// the regeneration, and dropping them on the way in is what stops
    /// the listener hearing the span twice.
    pub fn revoke_to(&mut self, mark: u64) -> u64 {
        let mut ring = self.ring.lock().unwrap();
        if ring.published() <= mark {
            return 0;
        }
        // The floor: audio the consumer has taken is gone, whatever the
        // mark says.
        let kept = ring.head.max(mark);
        let keeping = (kept - ring.head) as usize * CHANNELS;
        ring.frames.truncate(keeping);
        kept - mark
    }

    /// Drop everything queued — a seek chase's fast-forward burst, or
    /// the tail of a seat a host just swapped away from, neither of
    /// which anyone wants to hear.
    ///
    /// The head stays where it is, so what comes next is published at
    /// the coordinates the discarded audio held: exactly what a rewind
    /// all the way back to the consumer would have done.
    pub fn clear(&mut self) {
        self.ring.lock().unwrap().frames.clear();
    }

    /// Publish the rate the console is producing at.
    pub fn set_sample_rate(&mut self, hz: f64) {
        if hz > 0.0 {
            self.ring.lock().unwrap().rate = hz;
        }
    }
}

/// The consuming end, held by whoever plays the sound. Every call here
/// takes the ring's lock and nothing else — never a console's — so a
/// device callback may use it directly.
pub struct AudioOut {
    ring: Arc<Mutex<Ring>>,
}

impl AudioOut {
    /// Frames ready to play.
    pub fn available(&self) -> usize {
        self.ring.lock().unwrap().queued()
    }

    /// The rate the console is producing at, in Hz.
    pub fn sample_rate(&self) -> f64 {
        self.ring.lock().unwrap().rate
    }

    /// Take up to `out`'s worth, interleaved. Answers with the frames
    /// copied, which is short only when the ring is short.
    pub fn read(&mut self, out: &mut [i16]) -> usize {
        let mut ring = self.ring.lock().unwrap();
        let n = ring.queued().min(out.len() / CHANNELS);
        out[..n * CHANNELS].copy_from_slice(&ring.frames[..n * CHANNELS]);
        ring.take(n);
        n
    }

    /// Throw away up to `frames` of the oldest audio, answering with how
    /// much went. The backlog shed when a burst has put the queue far
    /// past the level a host steers for.
    pub fn skip(&mut self, frames: usize) -> usize {
        let mut ring = self.ring.lock().unwrap();
        let n = ring.queued().min(frames);
        ring.take(n);
        n
    }
}

/// Empty one console into nothing: the seat nobody is listening to, a
/// priming walk's sound, a seek chase's fast-forward burst. A console's
/// own buffer is small, so what is left in one is not left for long —
/// it is destroyed by the next thing written over it, which is how a
/// stale burst opens the span after it.
pub(crate) fn drop_audio(side: &mut dyn crate::Side, scratch: &mut Vec<i16>) {
    loop {
        scratch.resize(DRAIN_CHUNK * CHANNELS, 0);
        // A drain answers with the console's whole total, so anything
        // over a chunk means there is more behind it.
        if side.drain_audio(scratch) <= DRAIN_CHUNK {
            return;
        }
    }
}

/// Empty both of a pair's consoles into nothing — what a boot and a
/// seek landing want, whether or not anyone is listening to the pair.
pub(crate) fn drop_link_audio(link: &mut dyn crate::Link) {
    let mut scratch = Vec::new();
    for seat in 0..2 {
        drop_audio(&mut *link.side(seat), &mut scratch);
    }
}

/// Moving a console's audio into a ring, once, for every shape that
/// runs one.
///
/// A live pair, a lone console and a playback pair all do the same
/// thing — after the tick, while the console is still in hand, take what
/// it produced and publish it — and all three have the same rollback
/// debt to settle, so it lives here rather than three times over.
pub(crate) struct Pump {
    into: AudioIn,
    /// Which seat is being listened to, read every tick: a host can move
    /// the sound between seats mid-session (training's side swap) and
    /// nothing downstream should be rebuilt when it does.
    seat: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// What [`seat`](Self::seat) said last tick, so a swap can be
    /// noticed and the previous seat's tail dropped.
    playing: usize,
    /// Frames of re-simulated audio still to swallow: the corrected
    /// regeneration of spans whose speculative version already played.
    /// See [`AudioIn::revoke_to`].
    debt: u64,
    /// Landing buffer for the drain. Reused because this runs every
    /// tick.
    scratch: Vec<i16>,
}

impl Pump {
    pub(crate) fn new(into: AudioIn, seat: std::sync::Arc<std::sync::atomic::AtomicUsize>) -> Self {
        let playing = seat.load(Ordering::Relaxed);
        Pump {
            into,
            seat,
            playing,
            debt: 0,
            scratch: Vec::new(),
        }
    }

    /// A pump over a console with no seats to choose between.
    pub(crate) fn lone(into: AudioIn) -> Self {
        Pump::new(into, std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)))
    }

    /// Frames ever published — what a snapshot marks.
    pub(crate) fn produced(&self) -> u64 {
        self.into.produced()
    }

    /// Take back everything published since `mark`, keeping whatever
    /// could not be taken back as debt against the regeneration.
    pub(crate) fn revoke_to(&mut self, mark: u64) {
        self.debt += self.into.revoke_to(mark);
    }

    /// Drop everything queued (a seek chase's burst).
    pub(crate) fn clear(&mut self) {
        self.into.clear();
        self.debt = 0;
    }

    /// Empty a linked pair's consoles: the seat being listened to into
    /// the ring, the other one into nothing.
    ///
    /// The other one still gets emptied, because a console's own ring is
    /// small and holding a whole session's unheard audio in it means a
    /// seat swap would open with however much stale sound the ring
    /// happened to keep.
    pub(crate) fn pump(&mut self, link: &mut dyn crate::Link) {
        let seat = self.listening();
        self.take(&mut *link.side(seat));
        drop_audio(&mut *link.side(1 - seat), &mut self.scratch);
    }

    /// Empty a console booted alone.
    pub(crate) fn pump_console(&mut self, console: &mut dyn crate::Console) {
        self.take(&mut *console.side());
    }

    /// Empty a pair's consoles into nothing, and drop whatever is
    /// already queued: what a priming walk voiced before the session
    /// began, and what a seek chase's fast-forward piled up. Neither is
    /// audio anyone wants to hear.
    pub(crate) fn discard(&mut self, link: &mut dyn crate::Link) {
        drop_link_audio(link);
        self.clear();
    }

    /// The seat to listen to this tick, dropping the old seat's queued
    /// tail if it just changed — that audio belongs to a perspective the
    /// listener has left.
    fn listening(&mut self) -> usize {
        let seat = self.seat.load(Ordering::Relaxed) & 1;
        if seat != self.playing {
            self.playing = seat;
            self.clear();
        }
        seat
    }

    /// Empty one console, publishing what comes out.
    fn take(&mut self, side: &mut dyn crate::Side) {
        self.into.set_sample_rate(side.audio_sample_rate());
        loop {
            self.scratch.resize(DRAIN_CHUNK * CHANNELS, 0);
            // A drain fills as far as it goes and answers with the
            // console's whole total, so what landed is the total or a
            // chunk of it and anything over stays for the next pass.
            let total = side.drain_audio(&mut self.scratch);
            let got = total.min(DRAIN_CHUNK);
            if got == 0 {
                return;
            }
            // The debt comes off the oldest end of the fresh span, so
            // what is published picks up exactly where the listener left
            // off.
            let paid = self.debt.min(got as u64) as usize;
            self.debt -= paid as u64;
            self.into.push(&self.scratch[paid * CHANNELS..got * CHANNELS]);
            if total <= DRAIN_CHUNK {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frames counting up from `base`, so a test can tell which span it
    /// is looking at.
    fn ramp(base: i16, frames: usize) -> Vec<i16> {
        (0..frames as i16)
            .flat_map(|i| [base + i, base + i])
            .collect()
    }

    #[test]
    fn frames_cross_in_order() {
        let (mut into, mut out) = channel(8);
        let mut buf = [0i16; 8 * CHANNELS];
        // Several times the capacity over, so the queue empties and
        // refills again and again.
        for round in 0..20i16 {
            assert_eq!(into.push(&ramp(round * 4, 4)), 4);
            assert_eq!(out.read(&mut buf[..4 * CHANNELS]), 4);
            assert_eq!(buf[..4 * CHANNELS], ramp(round * 4, 4)[..]);
        }
    }

    #[test]
    fn a_full_ring_drops_what_will_not_fit() {
        let (mut into, out) = channel(8);
        assert_eq!(into.push(&ramp(0, 6)), 6);
        assert_eq!(into.push(&ramp(6, 6)), 2);
        assert_eq!(out.available(), 8);
    }

    /// The rollback case: what the listener has not reached is taken
    /// back outright, and the re-simulation's version of it plays
    /// instead.
    #[test]
    fn revoking_unread_audio_takes_it_back_with_no_debt() {
        let (mut into, mut out) = channel(64);
        into.push(&ramp(0, 10));
        let mark = into.produced();
        into.push(&ramp(100, 10));
        assert_eq!(into.revoke_to(mark), 0);
        assert_eq!(out.available(), 10);

        // The corrected span lands where the speculation was.
        into.push(&ramp(200, 10));
        let mut buf = [0i16; 20 * CHANNELS];
        assert_eq!(out.read(&mut buf), 20);
        assert_eq!(buf[..10 * CHANNELS], ramp(0, 10)[..]);
        assert_eq!(buf[10 * CHANNELS..], ramp(200, 10)[..]);
    }

    /// What already played cannot be unplayed, so it comes back as a
    /// debt instead — and paying it is what keeps the regeneration from
    /// queuing as an echo.
    #[test]
    fn revoking_played_audio_comes_back_as_debt() {
        let (mut into, mut out) = channel(64);
        into.push(&ramp(0, 10));
        let mark = into.produced();
        into.push(&ramp(100, 10));

        // The listener got six frames past the mark before the rollback.
        let mut buf = [0i16; 16 * CHANNELS];
        assert_eq!(out.read(&mut buf), 16);
        assert_eq!(into.revoke_to(mark), 6);

        // Four unread frames came back; the six that played are owed.
        assert_eq!(out.available(), 0);
    }

    /// A revoke reaching past everything the listener has left — the
    /// deepest rollback there is — empties the ring rather than
    /// confusing it, and what comes next is heard from where the
    /// listener stopped.
    #[test]
    fn a_revoke_past_the_whole_queue_empties_it() {
        let (mut into, mut out) = channel(64);
        into.push(&ramp(0, 10));
        let mut buf = [0i16; 10 * CHANNELS];
        out.read(&mut buf);
        let mark = into.produced();
        into.push(&ramp(50, 4));
        assert_eq!(into.revoke_to(mark - 6), 6);
        assert_eq!(out.available(), 0);
        into.push(&ramp(50, 3));
        assert_eq!(out.available(), 3);
    }

    /// A console handing over a tick of audio at a time, with a total
    /// bigger than one call can carry.
    struct Console {
        queued: Vec<i16>,
        rate: f64,
    }

    impl crate::Side for Console {
        fn frame(&mut self) -> Option<Vec<u8>> {
            None
        }

        fn audio_sample_rate(&mut self) -> f64 {
            self.rate
        }

        fn drain_audio(&mut self, out: &mut [i16]) -> usize {
            let total = self.queued.len() / CHANNELS;
            let written = total.min(out.len() / CHANNELS);
            out[..written * CHANNELS].copy_from_slice(&self.queued[..written * CHANNELS]);
            self.queued.drain(..written * CHANNELS);
            total
        }
    }

    /// A console with more queued than one drain call can carry still
    /// comes out whole — the pump loops until the console says it is
    /// empty.
    #[test]
    fn the_pump_empties_a_console_bigger_than_one_chunk() {
        let (into, out) = channel(1 << 16);
        let mut pump = Pump::lone(into);
        let mut console = Console {
            queued: ramp(0, DRAIN_CHUNK * 2 + 7),
            rate: 65536.0,
        };
        pump.take(&mut console);
        assert_eq!(out.available(), DRAIN_CHUNK * 2 + 7);
        assert_eq!(out.sample_rate(), 65536.0);
    }

    /// The debt is paid off the oldest end of the regeneration, so what
    /// plays after a rollback picks up exactly where the listener left
    /// off rather than repeating the span.
    #[test]
    fn the_pump_swallows_the_regeneration_it_owes() {
        let (into, mut out) = channel(1 << 12);
        let mut pump = Pump::lone(into);
        let mut console = Console {
            queued: ramp(0, 10),
            rate: 32768.0,
        };
        pump.take(&mut console);

        let mark = pump.produced();
        console.queued = ramp(100, 10);
        pump.take(&mut console);
        let mut buf = [0i16; 16 * CHANNELS];
        assert_eq!(out.read(&mut buf), 16);

        // Six of the speculated frames played, so six of the corrected
        // ten are owed and only the last four should reach the listener.
        pump.revoke_to(mark);
        console.queued = ramp(200, 10);
        pump.take(&mut console);
        assert_eq!(out.available(), 4);
        assert_eq!(out.read(&mut buf), 4);
        assert_eq!(buf[..4 * CHANNELS], ramp(206, 4)[..]);
    }

    /// A seat swap drops what the seat being left had queued: it is a
    /// perspective the listener has moved off, and playing its tail
    /// under the new one is the burst a host would otherwise have to
    /// shed.
    #[test]
    fn a_seat_swap_drops_the_old_seats_tail() {
        let seat = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (into, out) = channel(1 << 12);
        let mut pump = Pump::new(into, seat.clone());
        let mut console = Console {
            queued: ramp(0, 10),
            rate: 32768.0,
        };
        pump.take(&mut console);
        assert_eq!(out.available(), 10);

        seat.store(1, Ordering::Relaxed);
        assert_eq!(pump.listening(), 1);
        assert_eq!(out.available(), 0);
    }
}

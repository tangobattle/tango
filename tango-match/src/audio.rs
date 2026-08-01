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
//! so the push is free — and the callback reads the other end with no
//! lock at all. What the callback can play is whatever has crossed,
//! which is everything the sim produced up to the last tick, rather than
//! whatever it managed to steal the lock for.
//!
//! # Taking audio back
//!
//! Rollback is what makes this more than a queue. Audio is not machine
//! state — no emulator's savestate carries the buffer a frontend reads
//! from — so when the engine restores a snapshot and re-simulates, the
//! span the speculation already voiced is still sitting there and the
//! re-simulation produces it a second time. The ring answers that by
//! letting the producer *rewind*: everything past the mark that the
//! callback has not read yet is un-published by moving the write cursor
//! back, which is an integer store rather than a copy. What the callback
//! already read cannot be unplayed, so it comes back as a debt
//! ([`AudioIn::revoke_to`]'s return) that the next pushes pay off by
//! dropping the regeneration on the way in.
//!
//! Because the backlog stays in the ring until it is genuinely played,
//! the revocable window is the whole queue depth — much more than a
//! console's own small ring could ever hold.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Interleaved stereo, everywhere audio crosses this seam.
pub const CHANNELS: usize = 2;

/// Ceiling on one [`Side::drain_audio`](crate::Side) call, in frames —
/// a bound on the pump's scratch buffer. One tick of a 65536 Hz cart is
/// about 1100 frames, so a chunk this size takes any single tick's
/// production in one call and the loop below is only ever a guard.
const DRAIN_CHUNK: usize = 4096;

/// A ring of interleaved stereo frames with one producer and one
/// consumer.
///
/// Both cursors count frames *ever* crossed rather than positions, so
/// the occupancy is their difference and the slot for frame `n` is
/// `n & mask`. The producer may move `write` backwards (see
/// [`AudioIn::revoke_to`]); the consumer answers a cursor that has moved
/// below its own by snapping back to it, which is why nothing here ever
/// reads a negative occupancy.
struct Ring {
    /// `(mask + 1) * CHANNELS` samples. Cells rather than a plain slice
    /// because the two ends write and read it concurrently; the cursors
    /// are what keep them off the same frames.
    slots: Box<[UnsafeCell<i16>]>,
    /// Capacity minus one, in frames. Capacity is a power of two so the
    /// wrap is a mask.
    mask: usize,
    /// Frames ever published. Rewindable — a rollback moves it back over
    /// audio nobody has heard yet.
    write: AtomicU64,
    /// Frames ever consumed, as the producer sees them: the floor a
    /// rewind may not go below.
    read: AtomicU64,
    /// The console's production rate in Hz, as f64 bits, published by
    /// whoever pushes. Read per fill because a cart can change it at
    /// runtime — BN4+ flip from 32768 to 65536 after boot — and the
    /// whole resample ratio is built on it.
    rate: AtomicU64,
}

// The cells are only ever touched through the cursors, which is what
// keeps the two ends off the same frames.
unsafe impl Send for Ring {}
unsafe impl Sync for Ring {}

impl Ring {
    /// The sample slot frame `n` lives in.
    fn slot(&self, n: u64) -> *mut i16 {
        self.slots[(n as usize & self.mask) * CHANNELS].get()
    }

    /// Copy `frames` frames between the ring and a linear buffer,
    /// splitting at the wrap. `to_ring` picks the direction; `linear` is
    /// interleaved and at least `frames * CHANNELS` long.
    ///
    /// One routine for both directions because the wrap arithmetic is
    /// the only interesting part and it is identical either way.
    fn copy(&self, at: u64, linear: *mut i16, frames: usize, to_ring: bool) {
        let start = at as usize & self.mask;
        let first = (self.mask + 1 - start).min(frames);
        for (offset, count) in [(0, first), (first, frames - first)] {
            if count == 0 {
                continue;
            }
            let ring = self.slot(at + offset as u64);
            let flat = unsafe { linear.add(offset * CHANNELS) };
            let (src, dst) = if to_ring { (flat, ring) } else { (ring, flat) };
            // Disjoint by construction: `ring` points into the cells and
            // `flat` into the caller's own buffer.
            unsafe { std::ptr::copy_nonoverlapping(src, dst, count * CHANNELS) };
        }
    }
}

/// A ring sized to hold `capacity` frames, as the two ends that share
/// it: [`AudioIn`] for whoever runs the simulation, [`AudioOut`] for
/// whoever plays it. Capacity rounds up to a power of two.
///
/// Size it well past the queue a host means to hold: a producer that
/// runs out of room drops what will not fit, and the point of the ring
/// is that a burst — a seek chase, a device stall's catch-up — lands
/// somewhere the consumer can shed it deliberately.
pub fn channel(capacity: usize) -> (AudioIn, AudioOut) {
    let capacity = capacity.next_power_of_two().max(2);
    let ring = Arc::new(Ring {
        slots: (0..capacity * CHANNELS).map(|_| UnsafeCell::new(0)).collect(),
        mask: capacity - 1,
        write: AtomicU64::new(0),
        read: AtomicU64::new(0),
        // Stood in for until the first push, and only ever divided by:
        // the GBA's own rate is what every game a session runs produces
        // at, and a zero here would rebase the first fill's resampling.
        rate: AtomicU64::new(32768.0f64.to_bits()),
    });
    (AudioIn { ring: ring.clone() }, AudioOut { ring, read: 0 })
}

/// The producing end, held by whoever turns the simulation's crank.
pub struct AudioIn {
    ring: Arc<Ring>,
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
        let capacity = self.ring.mask + 1;
        let write = self.ring.write.load(Ordering::Relaxed);
        let queued = write.saturating_sub(self.ring.read.load(Ordering::Acquire)) as usize;
        let n = (frames.len() / CHANNELS).min(capacity - queued.min(capacity));
        if n > 0 {
            self.ring.copy(write, frames.as_ptr().cast_mut(), n, true);
            // Release so the frames are visible before the cursor that
            // hands them over.
            self.ring.write.store(write + n as u64, Ordering::Release);
        }
        n
    }

    /// Frames ever published — the coordinate a snapshot marks so a
    /// rollback knows what its speculation voiced.
    pub fn produced(&self) -> u64 {
        self.ring.write.load(Ordering::Relaxed)
    }

    /// Take back everything published since `mark`, and answer with what
    /// could not be taken back.
    ///
    /// What the consumer has not reached is un-published by moving the
    /// write cursor — no copying, and the frames are simply overwritten
    /// by the re-simulation. What it already read cannot be unplayed, so
    /// it comes back as a frame count: the caller owes that many frames
    /// of the regeneration, and dropping them on the way in is what
    /// stops the listener hearing the span twice.
    pub fn revoke_to(&mut self, mark: u64) -> u64 {
        let write = self.ring.write.load(Ordering::Relaxed);
        if write <= mark {
            return 0;
        }
        // The floor: audio the consumer has taken is gone, whatever the
        // mark says.
        let kept = self.ring.read.load(Ordering::Acquire).max(mark);
        self.ring.write.store(kept, Ordering::Release);
        kept - mark
    }

    /// Drop everything queued — a seek chase's fast-forward burst, or
    /// the tail of a seat a host just swapped away from, neither of
    /// which anyone wants to hear.
    pub fn clear(&mut self) {
        let read = self.ring.read.load(Ordering::Acquire);
        self.ring.write.store(read, Ordering::Release);
    }

    /// Publish the rate the console is producing at.
    pub fn set_sample_rate(&mut self, hz: f64) {
        if hz > 0.0 {
            self.ring.rate.store(hz.to_bits(), Ordering::Relaxed);
        }
    }
}

/// The consuming end, held by whoever plays the sound. Every call here
/// is lock-free — that is the point of the whole file — so a device
/// callback may use it directly.
pub struct AudioOut {
    ring: Arc<Ring>,
    /// This end's own cursor. Mirrored into the ring for the producer to
    /// read; kept here too so the common path is not an atomic load.
    read: u64,
}

impl AudioOut {
    /// Frames ready to play.
    pub fn available(&mut self) -> usize {
        (self.published() - self.read) as usize
    }

    /// The rate the console is producing at, in Hz.
    pub fn sample_rate(&self) -> f64 {
        f64::from_bits(self.ring.rate.load(Ordering::Relaxed))
    }

    /// Take up to `out`'s worth, interleaved. Answers with the frames
    /// copied, which is short only when the ring is short.
    pub fn read(&mut self, out: &mut [i16]) -> usize {
        let n = ((self.published() - self.read) as usize).min(out.len() / CHANNELS);
        if n > 0 {
            self.ring.copy(self.read, out.as_mut_ptr(), n, false);
            self.advance(n);
        }
        n
    }

    /// Throw away up to `frames` of the oldest audio, answering with how
    /// much went. The backlog shed when a burst has put the queue far
    /// past the level a host steers for.
    pub fn skip(&mut self, frames: usize) -> usize {
        let n = ((self.published() - self.read) as usize).min(frames);
        self.advance(n);
        n
    }

    /// The producer's cursor, having first honoured a rewind that landed
    /// below our own: a rollback took back audio while we were mid-read,
    /// so the frames we thought we had are gone. Snapping back is what
    /// keeps the occupancy from reading as an enormous number when the
    /// cursors cross.
    fn published(&mut self) -> u64 {
        let write = self.ring.write.load(Ordering::Acquire);
        if write < self.read {
            self.read = write;
            self.ring.read.store(write, Ordering::Release);
        }
        write
    }

    fn advance(&mut self, frames: usize) {
        self.read += frames as u64;
        // Release so the copy out is done before the producer is told
        // the frames are free.
        self.ring.read.store(self.read, Ordering::Release);
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
    fn frames_cross_in_order_and_wrap() {
        let (mut into, mut out) = channel(8);
        let mut buf = [0i16; 8 * CHANNELS];
        // Several times the capacity, so every push wraps somewhere
        // different.
        for round in 0..20i16 {
            assert_eq!(into.push(&ramp(round * 4, 4)), 4);
            assert_eq!(out.read(&mut buf[..4 * CHANNELS]), 4);
            assert_eq!(buf[..4 * CHANNELS], ramp(round * 4, 4)[..]);
        }
    }

    #[test]
    fn a_full_ring_drops_what_will_not_fit() {
        let (mut into, mut out) = channel(8);
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

    /// A rewind that lands below the consumer's own cursor — a rollback
    /// while a fill was mid-read — must read as an empty ring, not as an
    /// enormous one.
    #[test]
    fn a_cursor_crossing_reads_as_empty_rather_than_wrapping() {
        let (mut into, mut out) = channel(64);
        into.push(&ramp(0, 10));
        let mut buf = [0i16; 10 * CHANNELS];
        out.read(&mut buf);
        // Straight past the consumer, as only a racing revoke can.
        into.ring.write.store(4, Ordering::Release);
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
        let (into, mut out) = channel(1 << 16);
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
        let (into, mut out) = channel(1 << 12);
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

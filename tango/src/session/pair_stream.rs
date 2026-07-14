//! Audio for SIO-engine sessions: the host output stream plays one of
//! the pair's cores directly — [`MGBAStream`]'s pull-and-resample shape
//! and its faux clock (when the host paces the sim below the native
//! tick rate, playback stretches by the same ratio; the fps target is
//! read through a host-supplied closure since there's no mgba-thread
//! sync to read it from). The one MGBAStream ingredient that CANNOT
//! carry over is the audio-sync high water: there, a full buffer pauses
//! the emulator until the callback consumes, but neither a netplay
//! lockstep sim (the match clock owns its pacing) nor a paced replay
//! drive loop may be paused by audio. Buffer-level regulation therefore
//! moves to the consumption side: a small occupancy servo (±0.5%,
//! inaudible) trims the resample ratio to hold the core's queue at a
//! fixed level against timing jitter, and a discard cap sheds the
//! backlog a deep rollback re-simulation dumps (duplicate audio for
//! re-simmed ticks) in one skip instead of replaying it.
//!
//! The pair access locks the same mutex the host's per-tick step takes,
//! so readout interleaves between ticks. A stalled sim (reconnect
//! pause, replay pause) still drains the queue and goes silent —
//! there's genuinely nothing to play.
//!
//! [`MGBAStream`]: crate::platform::audio::MGBAStream

use std::sync::atomic::Ordering;
use std::sync::Arc;

const EXPECTED_FPS: f32 = 60.0;

/// How much source audio to keep queued in the played core's sample
/// buffer, in seconds — the level the servo holds, and the floor on
/// audio latency. Big enough to ride out drive-thread/callback phase
/// jitter and a couple of ticks of rollback burst.
const AUDIO_TARGET_QUEUED_SECS: f64 = 0.05;

/// The occupancy servo's authority: the largest fractional trim it may
/// put on the resample ratio. ±0.5% is ~9 cents of pitch — inaudible —
/// yet re-centers a half-target error in a few seconds.
const AUDIO_MAX_TRIM: f64 = 0.005;

/// Queue level, as a multiple of the target, past which `fill` stops
/// absorbing and discards the oldest samples back down to target. Only
/// deep re-simulation bursts (rollback, seek chases) get here; one skip
/// beats seconds of extra latency (the servo alone would need ~10 s per
/// 50 ms of backlog).
const AUDIO_DISCARD_FACTOR: f64 = 3.0;

/// Cross-thread access to a live [`Pair`](tango_pvp::Pair) for the
/// audio callback — implemented over whatever lock the host keeps its
/// pair behind. A host mid-boot may simply not call `f`; the stream
/// plays silence until the pair exists.
pub trait PairPull: Send {
    fn with_pair(&self, f: &mut dyn FnMut(&mut tango_pvp::Pair));
}

impl PairPull for tango_pvp::PairHandle {
    fn with_pair(&self, f: &mut dyn FnMut(&mut tango_pvp::Pair)) {
        tango_pvp::PairHandle::with_pair(self, |pair| f(pair));
    }
}

pub struct PairStream {
    pair: Box<dyn PairPull>,
    /// Which core to play, re-read every fill (a replay's perspective
    /// swap flips it; PvP pins it to the local player).
    player: Box<dyn Fn() -> usize + Send>,
    /// The host drive loop's current pacing target, f32. Zero or less is
    /// treated as unthrottled (60 fps).
    fps_target: Box<dyn Fn() -> f32 + Send>,
    out_rate: u32,
    resampler: mgba::audio::AudioResampler,
    dest_buffer: mgba::audio::AudioBuffer,
    /// Tracked separately because `mAudioBuffer` doesn't expose
    /// capacity through the Rust binding; grown lazily in `fill`.
    dest_capacity: usize,
    /// Sink for samples dropped at the discard cap.
    discard: Vec<i16>,
}

impl PairStream {
    pub fn new(
        pair: impl PairPull + 'static,
        player: impl Fn() -> usize + Send + 'static,
        fps_target: impl Fn() -> f32 + Send + 'static,
        out_rate: u32,
    ) -> Self {
        let dest_capacity = crate::platform::audio::SAMPLES * 2;
        Self {
            pair: Box::new(pair),
            player: Box::new(player),
            fps_target: Box::new(fps_target),
            out_rate: if out_rate == 0 { 48000 } else { out_rate },
            resampler: mgba::audio::AudioResampler::new(),
            dest_buffer: mgba::audio::AudioBuffer::new(dest_capacity, 2),
            dest_capacity,
            discard: Vec::new(),
        }
    }

    /// A `fps_target` closure over an `f32`-bits atomic — the shape both
    /// hosts publish their pacing through.
    pub fn fps_from_bits(bits: Arc<std::sync::atomic::AtomicU32>) -> impl Fn() -> f32 + Send + 'static {
        move || f32::from_bits(bits.load(Ordering::Relaxed))
    }
}

impl crate::platform::audio::Stream for PairStream {
    fn fill(&mut self, buf: &mut [[i16; 2]]) -> usize {
        let frame_count = buf.len();
        let linear_buf: &mut [i16] = bytemuck::cast_slice_mut(buf);

        let needed = frame_count.saturating_mul(2);
        if needed > self.dest_capacity {
            let new_capacity = needed.next_power_of_two().max(crate::platform::audio::SAMPLES * 2);
            self.dest_buffer = mgba::audio::AudioBuffer::new(new_capacity, 2);
            self.dest_capacity = new_capacity;
        }

        let mut fps_target = (self.fps_target)();
        if fps_target <= 0.0 {
            fps_target = EXPECTED_FPS;
        }
        let player = (self.player)();

        let out_rate = self.out_rate;
        let (resampler, dest_buffer, discard) = (&mut self.resampler, &mut self.dest_buffer, &mut self.discard);
        self.pair.with_pair(&mut |pair| {
            let mut core = pair.core_mut(player);
            // The core's production rate follows the game's SOUNDBIAS
            // resolution and CHANGES at runtime (BN4+ flip from 32768 to
            // 65536 Hz after boot), so it's re-read every fill.
            let rate = core.as_ref().audio_sample_rate() as f64;
            // The faux clock, exactly as MGBAStream: production scales
            // with the sim's pace, so a throttled sim stretches playback
            // by the same ratio instead of starving it.
            let faux_clock = core.as_ref().calculate_framerate_ratio(fps_target as f64);
            let mut source = core.audio_buffer();

            let target = rate * AUDIO_TARGET_QUEUED_SECS;
            let queued = source.available() as f64;
            if queued > target * AUDIO_DISCARD_FACTOR {
                // Deep re-simulation backlog: skip the oldest samples in
                // one go rather than replaying seconds of duplicates.
                let n = (queued - target) as usize;
                discard.resize(n * 2, 0);
                source.read(discard, n);
            }

            // Servo: nudge the claimed source rate so the queue level
            // converges on the target. Claiming the source faster than
            // it is makes each output frame consume more of it (drains);
            // slower, less (refills).
            let queued = source.available() as f64;
            let trim = AUDIO_MAX_TRIM * ((queued - target) / target).clamp(-1.0, 1.0);

            resampler.set_source(&mut source, rate * (1.0 + trim), true);
            resampler.set_destination(dest_buffer, out_rate as f64 * faux_clock);
            resampler.process();
        });

        let available = self.dest_buffer.available().min(frame_count);
        self.dest_buffer.read(&mut linear_buf[..available * 2], available);
        available
    }
}

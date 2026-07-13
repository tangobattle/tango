//! The replay's audio stream: [`MGBAStream`]'s pull-and-resample shape,
//! with the perspective-swap switch. The primary core's buffer is
//! consumed every fill no matter what — its consumption is what
//! un-blocks the emulator's audio sync, so pacing must not depend on
//! whose audio is audible — but while the perspective is swapped, those
//! samples are discarded and the shadow core's buffer (the opponent's
//! perspective: their cursor, their chip sounds) is resampled to the
//! output instead.
//!
//! [`MGBAStream`]: crate::platform::audio::MGBAStream

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tango_pvp::shadow::Shadow;

use crate::platform::audio::{NUM_CHANNELS, SAMPLES};

pub(super) struct ReplayStream {
    handle: mgba::thread::Handle,
    shadow: Arc<Mutex<Shadow>>,
    swap_perspective: Arc<AtomicBool>,
    sample_rate: u32,
    resampler: mgba::audio::AudioResampler,
    dest_buffer: mgba::audio::AudioBuffer,
    shadow_resampler: mgba::audio::AudioResampler,
    shadow_dest_buffer: mgba::audio::AudioBuffer,
    /// Tracked separately because `mAudioBuffer` doesn't expose
    /// capacity through the Rust binding; grown lazily in `fill`.
    dest_capacity: usize,
    /// Sink for the primary's samples while they're inaudible.
    discard: Vec<i16>,
    /// Last fill's swap state — the rising edge drops the shadow
    /// buffer's backlog (it accumulates unread while unswapped, and a
    /// stale burst on toggle would smear old sounds over the handoff).
    was_swapped: bool,
}

impl ReplayStream {
    pub(super) fn new(
        handle: mgba::thread::Handle,
        shadow: Arc<Mutex<Shadow>>,
        swap_perspective: Arc<AtomicBool>,
        sample_rate: u32,
    ) -> Self {
        let dest_capacity = SAMPLES * 2;
        Self {
            handle,
            shadow,
            swap_perspective,
            sample_rate,
            resampler: mgba::audio::AudioResampler::new(),
            dest_buffer: mgba::audio::AudioBuffer::new(dest_capacity, NUM_CHANNELS as u32),
            shadow_resampler: mgba::audio::AudioResampler::new(),
            shadow_dest_buffer: mgba::audio::AudioBuffer::new(dest_capacity, NUM_CHANNELS as u32),
            dest_capacity,
            discard: Vec::new(),
            was_swapped: false,
        }
    }
}

impl crate::platform::audio::Stream for ReplayStream {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize {
        let frame_count = buf.len();
        let linear_buf: &mut [i16] = bytemuck::cast_slice_mut(buf);
        let swapped = self.swap_perspective.load(Ordering::Relaxed);

        // The primary half: identical to MGBAStream — rates, high water,
        // resample-and-consume — because the emulator's audio sync is
        // paced by exactly this consumption.
        let (core_rate, dest_rate) = {
            let mut audio_guard = self.handle.lock_audio();
            let mut fps_target = audio_guard.sync().fps_target();
            if fps_target <= 0.0 {
                fps_target = 1.0;
            }
            let (core_rate, faux_clock) = {
                let core = audio_guard.core_mut();
                (
                    core.as_ref().audio_sample_rate() as f64,
                    core.as_ref().calculate_framerate_ratio(fps_target as f64),
                )
            };
            let dest_rate = self.sample_rate as f64 * faux_clock;
            let high_water = (frame_count as f64 + 16.0 + frame_count as f64 / 64.0) * core_rate / dest_rate;
            audio_guard.sync_mut().set_audio_high_water(high_water as u32);

            let needed = frame_count.saturating_mul(2);
            if needed > self.dest_capacity {
                let new_capacity = needed.next_power_of_two().max(SAMPLES * 2);
                self.dest_buffer = mgba::audio::AudioBuffer::new(new_capacity, NUM_CHANNELS as u32);
                self.shadow_dest_buffer = mgba::audio::AudioBuffer::new(new_capacity, NUM_CHANNELS as u32);
                self.dest_capacity = new_capacity;
            }

            let mut core = audio_guard.core_mut();
            let mut core_buffer = core.audio_buffer();
            self.resampler.set_source(&mut core_buffer, core_rate, true);
            self.resampler.set_destination(&mut self.dest_buffer, dest_rate);
            self.resampler.process();
            (core_rate, dest_rate)
        };

        if !swapped {
            self.was_swapped = false;
            let available = self.dest_buffer.available().min(frame_count);
            self.dest_buffer
                .read(&mut linear_buf[..available * NUM_CHANNELS], available);
            return available;
        }

        // Swapped: the primary's samples pace, the shadow's play.
        let available = self.dest_buffer.available().min(frame_count);
        self.discard.resize(available * NUM_CHANNELS, 0);
        self.dest_buffer.read(&mut self.discard, available);

        let first = !self.was_swapped;
        self.was_swapped = true;
        let shadow = self.shadow.clone();
        shadow.lock().unwrap().with_audio_buffer(|source| {
            if first {
                source.clear();
            }
            // The shadow ticks in lockstep with the primary — same game,
            // same clock — so the primary's rates apply as-is.
            self.shadow_resampler.set_source(source, core_rate, true);
            self.shadow_resampler
                .set_destination(&mut self.shadow_dest_buffer, dest_rate);
            self.shadow_resampler.process();
        });

        let available = self.shadow_dest_buffer.available().min(frame_count);
        self.shadow_dest_buffer
            .read(&mut linear_buf[..available * NUM_CHANNELS], available);
        available
    }
}

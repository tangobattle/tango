//! Training's audio stream: [`MGBAStream`]'s pull-and-resample shape,
//! with a possession switch. The primary core's buffer is consumed every
//! fill no matter what — its consumption is what un-blocks the emulator's
//! audio sync, so pacing must not depend on whose audio is audible — but
//! while the user possesses the dummy, those samples are discarded and
//! the shadow core's buffer (the dummy's perspective: its cursor, its
//! chip sounds) is resampled to the output instead.
//!
//! [`MGBAStream`]: crate::audio::MGBAStream

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::audio::{NUM_CHANNELS, SAMPLES};

use super::PossessState;

pub(super) struct TrainingStream {
    handle: mgba::thread::Handle,
    match_: Arc<tango_pvp::battle::Match>,
    possess: Arc<PossessState>,
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
    /// Last fill's possession state — the rising edge drops the shadow
    /// buffer's backlog (it accumulates unread while unpossessed, and a
    /// stale burst on toggle would smear old sounds over the handoff).
    was_possessing: bool,
}

impl TrainingStream {
    pub(super) fn new(
        handle: mgba::thread::Handle,
        match_: Arc<tango_pvp::battle::Match>,
        possess: Arc<PossessState>,
        sample_rate: u32,
    ) -> Self {
        let dest_capacity = SAMPLES * 2;
        Self {
            handle,
            match_,
            possess,
            sample_rate,
            resampler: mgba::audio::AudioResampler::new(),
            dest_buffer: mgba::audio::AudioBuffer::new(dest_capacity, NUM_CHANNELS as u32),
            shadow_resampler: mgba::audio::AudioResampler::new(),
            shadow_dest_buffer: mgba::audio::AudioBuffer::new(dest_capacity, NUM_CHANNELS as u32),
            dest_capacity,
            discard: Vec::new(),
            was_possessing: false,
        }
    }
}

impl crate::audio::Stream for TrainingStream {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize {
        let frame_count = buf.len();
        let linear_buf: &mut [i16] = bytemuck::cast_slice_mut(buf);
        let possessing = self.possess.active.load(Ordering::Relaxed);

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

        if !possessing {
            self.was_possessing = false;
            let available = self.dest_buffer.available().min(frame_count);
            self.dest_buffer
                .read(&mut linear_buf[..available * NUM_CHANNELS], available);
            return available;
        }

        // Possessed: the primary's samples pace, the shadow's play.
        let available = self.dest_buffer.available().min(frame_count);
        self.discard.resize(available * NUM_CHANNELS, 0);
        self.dest_buffer.read(&mut self.discard, available);

        let first = !self.was_possessing;
        self.was_possessing = true;
        let match_ = self.match_.clone();
        match_.with_shadow_audio_buffer(|source| {
            if first {
                source.clear();
            }
            // The shadow ticks in lockstep with the primary — same game,
            // same clock — so the primary's rates apply as-is.
            self.shadow_resampler.set_source(source, core_rate, true);
            self.shadow_resampler.set_destination(&mut self.shadow_dest_buffer, dest_rate);
            self.shadow_resampler.process();
        });

        let available = self.shadow_dest_buffer.available().min(frame_count);
        self.shadow_dest_buffer
            .read(&mut linear_buf[..available * NUM_CHANNELS], available);
        available
    }
}

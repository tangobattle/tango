//! A console's audio on its way to a device.
//!
//! What comes out of the ring the simulation pushes into is raw: the
//! console's own rate and whatever it has produced. Turning that into
//! device-rate frames at a chosen speed is the host's job, and every
//! console needs the same job done, so it is done once here rather than
//! per emulator.
//!
//! One decision lives in this file and nowhere else: how much to
//! resample. Exactly what the caller is about to play, and no more — the
//! backlog then sits on the unresampled side, which is the side
//! [`Stream`](super::Stream) measures and steers. Converting
//! everything queued instead would put it downstream of the only thing
//! regulating it, where it grows until something has to shed it.
//!
//! For the same reason the backlog is left in the ring: only what a fill
//! is about to convert is pulled out of it. That keeps the ring's own
//! occupancy the whole truth about the queue, and — because a rollback
//! can take back anything the ring still holds — it keeps the revocable
//! window as deep as the queue itself rather than one fill.

use tango_match::AudioOut;

use super::NUM_CHANNELS;

/// Source frames on each side of the cursor the kernel reads: `RADIUS`
/// ahead of the frame the cursor stands on and `RADIUS - 1` behind it.
const RADIUS: usize = 4;

/// The kernel's width in taps.
const TAPS: usize = 2 * RADIUS;

/// Tabulated fractional positions per source frame. The kernel for a
/// cursor between two of them is lerped from both rows, which is what
/// lets the table stay this small.
const PHASES: usize = 256;

/// The normalized sinc, sin(πx)/(πx): the ideal interpolation kernel,
/// which is infinitely wide.
fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    let px = std::f64::consts::PI * x;
    px.sin() / px
}

/// The Blackman window, which tapers the ideal kernel's infinite tails
/// down to the `TAPS` frames actually summed, reaching zero at ±`RADIUS`.
fn blackman(x: f64) -> f64 {
    let t = std::f64::consts::PI * x / RADIUS as f64;
    0.42 + 0.5 * t.cos() + 0.08 * (2.0 * t).cos()
}

/// Windowed-sinc coefficients for every tabulated phase: `PHASES + 1`
/// rows of `TAPS`, the extra row being phase 1.0 so the last real phase
/// still has a next row to lerp toward.
fn kernel_table() -> Vec<f64> {
    let mut table = Vec::with_capacity((PHASES + 1) * TAPS);
    for phase in 0..=PHASES {
        let frac = phase as f64 / PHASES as f64;
        let row_at = table.len();
        let mut sum = 0.0;
        for tap in 0..TAPS {
            let x = (tap as isize - (RADIUS as isize - 1)) as f64 - frac;
            let coeff = sinc(x) * blackman(x);
            sum += coeff;
            table.push(coeff);
        }
        // Truncating the ideal kernel's tails leaves each row summing
        // slightly off 1, by an amount that varies with the phase;
        // dividing it back out pins the gain at exactly 1 so a
        // sustained tone doesn't tremolo at the beat between the rates.
        for coeff in &mut table[row_at..] {
            *coeff /= sum;
        }
    }
    table
}

/// A console's audio on its way to a device: staged, resampled, and
/// accounted for.
///
/// Not a trait and not a wrapper around one — a host owns one of these
/// and drives it. Every console needs the same thing done to its audio,
/// so this is the one implementation rather than something each backend
/// re-answers.
///
/// Windowed-sinc interpolation over a fractional cursor. The cursor
/// still advances by `claimed / destination` per output frame — so the
/// queue drifts exactly the way a servo intends — but each output
/// sample is a `TAPS`-wide kernel over the source rather than a lerp of
/// two frames, which keeps the passband flat and pushes imaging noise
/// down where linear interpolation rolled off highs and let images
/// through.
pub struct Resampler {
    out: AudioOut,
    /// Pulled out of the ring for this fill's conversion, not yet
    /// resampled. Only ever the frames a fill is about to need plus the
    /// history the kernel still reads behind the cursor — the backlog
    /// stays in the ring.
    source: Vec<i16>,
    /// Fractional position into `source`, in frames.
    cursor: f64,
    /// The tabulated kernel, built once at construction so a fill —
    /// which runs in the sound callback — never touches trigonometry.
    kernel: Vec<f64>,
    /// Resampled, waiting for the device — a hand-off buffer, never
    /// more than the last [`process`](Resampler::process) was asked for.
    dest: Vec<i16>,
}

impl Resampler {
    pub fn new(out: AudioOut) -> Self {
        Resampler {
            out,
            source: Vec::new(),
            cursor: 0.0,
            kernel: kernel_table(),
            dest: Vec::new(),
        }
    }

    /// Frames pulled out of the ring that the cursor has not passed yet.
    fn staged(&self) -> usize {
        (self.source.len() / NUM_CHANNELS).saturating_sub(self.cursor as usize)
    }

    /// The rate the console truly produces at, in Hz.
    pub fn sample_rate(&self) -> f64 {
        self.out.sample_rate()
    }

    /// The session's whole audio backlog in source frames — the level a
    /// host's rate control steers, and the audio latency.
    pub fn source_available(&mut self) -> usize {
        self.staged() + self.out.available()
    }

    /// Pull enough out of the ring to cover `frames` more source frames
    /// of conversion, taking what is there and no more.
    fn stage(&mut self, frames: usize) {
        let have = self.source.len() / NUM_CHANNELS;
        let want = frames.saturating_sub(have).min(self.out.available());
        if want == 0 {
            return;
        }
        let at = self.source.len();
        // A sound callback must not allocate; after the first few fills
        // this is a length change inside capacity the vec already has.
        self.source.resize(at + want * NUM_CHANNELS, 0);
        let got = self.out.read(&mut self.source[at..]);
        self.source.truncate(at + got * NUM_CHANNELS);
    }

    pub fn process(&mut self, claimed_source_rate: f64, destination_rate: f64, frames: usize) {
        let step = claimed_source_rate / destination_rate;
        // A non-positive or non-finite step would never advance the
        // cursor, so the loop below would push forever.
        if !step.is_finite() || step <= 0.0 {
            return;
        }
        let want = frames.saturating_sub(self.dest.len() / NUM_CHANNELS);
        // What converting `want` output frames can reach: the cursor
        // ends up at most `cursor + step * want` in, and the kernel
        // reads `RADIUS` frames past wherever it stands. Asking for
        // exactly that is what leaves the rest of the backlog in the
        // ring, where the servo can see it and a rollback can still take
        // it back.
        let reach = (self.cursor + step * want as f64).ceil() as usize + RADIUS + 1;
        self.stage(reach);
        let available = self.source.len() / NUM_CHANNELS;
        for _ in 0..want {
            let i = self.cursor as usize;
            if i + RADIUS >= available {
                break;
            }
            // The kernel for this exact fraction sits between two
            // tabulated rows; lerp them tap by tap, once for both
            // channels.
            let phase = (self.cursor - i as f64) * PHASES as f64;
            let row = phase as usize;
            let between = phase - row as f64;
            let mut taps = [0.0f64; TAPS];
            for (tap, coeff) in taps.iter_mut().enumerate() {
                let a = self.kernel[row * TAPS + tap];
                let b = self.kernel[(row + 1) * TAPS + tap];
                *coeff = a + (b - a) * between;
            }
            // Taps run from `RADIUS - 1` frames behind the frame the
            // cursor stands on to `RADIUS` ahead; until any history
            // exists — at stream start and after a discard — the
            // leading ones clamp onto the head, repeating the edge
            // frame.
            let first = i as isize - (RADIUS as isize - 1);
            for channel in 0..NUM_CHANNELS {
                let mut acc = 0.0;
                for (tap, coeff) in taps.iter().enumerate() {
                    let frame = (first + tap as isize).max(0) as usize;
                    acc += self.source[frame * NUM_CHANNELS + channel] as f64 * coeff;
                }
                // The saturating cast absorbs the kernel's ringing on
                // hard edges, which can carry a near-full-scale step
                // past i16's range.
                self.dest.push(acc as i16);
            }
            self.cursor += step;
        }
        // Drop what the cursor has passed, keeping the `RADIUS - 1`
        // frames of history the kernel still reads behind it. The
        // loop's last step can carry the cursor past the end of what was
        // staged (any step over 1 gets there — the fast-forward fold
        // divides the destination rate, so a 65536 Hz cart at 4x steps
        // ~5.5 frames at a time); the drain must not follow it out of
        // bounds. The overshoot stays in the cursor as a debt the next
        // batch of source pays off.
        let consumed = (self.cursor as usize).saturating_sub(RADIUS - 1).min(available);
        if consumed > 0 {
            self.source.drain(..consumed * NUM_CHANNELS);
            self.cursor -= consumed as f64;
        }
    }

    pub fn available(&self) -> usize {
        self.dest.len() / NUM_CHANNELS
    }

    pub fn discard_source(&mut self, frames: usize) {
        // What was staged for this fill first, then through to the ring
        // — which is where the backlog being shed almost entirely sits.
        // The frames behind the cursor were already played and were kept
        // only as kernel history, so they don't count against `frames`;
        // a discard is a discontinuity, so there is nothing left worth
        // interpolating with and they go too.
        let staged = frames.min(self.staged());
        let played = self.source.len() / NUM_CHANNELS - self.staged();
        self.source.drain(..(played + staged) * NUM_CHANNELS);
        // The cursor indexed into what was just dropped; the point of
        // discarding is to jump forward, so it restarts at the head.
        self.cursor = 0.0;
        self.out.skip(frames - staged);
    }

    pub fn read(&mut self, out: &mut [i16], frames: usize) -> usize {
        let frames = frames.min(self.dest.len() / NUM_CHANNELS).min(out.len() / NUM_CHANNELS);
        out[..frames * NUM_CHANNELS].copy_from_slice(&self.dest[..frames * NUM_CHANNELS]);
        self.dest.drain(..frames * NUM_CHANNELS);
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 32768.0;

    /// A ring with `frames` of a console's audio already in it, plus the
    /// producing end so a test can feed it more.
    fn ring(rate: f64, frames: usize) -> (tango_match::AudioIn, Resampler) {
        let (mut into, out) = tango_match::audio::channel(1 << 17);
        into.set_sample_rate(rate);
        into.push(&vec![1i16; frames * NUM_CHANNELS]);
        (into, Resampler::new(out))
    }

    /// The resampled queue is a hand-off buffer, never a backlog: it
    /// holds what the fill asked for and no more, however much source is
    /// waiting. Converting everything available instead put the
    /// session's whole audio backlog here — downstream of the servo,
    /// which measures the *source* queue — so it grew unchecked (until a
    /// cap started shedding whole spans, audible as skipping) while the
    /// servo sat pinned at full authority chasing a level it could not
    /// move.
    #[test]
    fn the_resampled_queue_holds_only_what_was_asked_for() {
        let (mut into, mut pull) = ring(RATE, 0);
        for _ in 0..200 {
            into.push(&vec![1i16; 4096 * NUM_CHANNELS]);
            pull.source_available();
            // 4x fast-forward: 48 kHz device, 192 kHz claimed.
            pull.process(RATE, 192_000.0, 512);
            let mut out = [0i16; 1024];
            pull.read(&mut out, 512);
        }
        assert!(
            pull.available() <= 512,
            "resampled queue grew to {} frames",
            pull.available()
        );
    }

    /// The backlog stays in the ring: a fill pulls out what it is about
    /// to convert and leaves the rest.
    ///
    /// Two things ride on that. The level the servo steers stays exactly
    /// measurable wherever the audio physically sits — an earlier design
    /// left the backlog in the console, where melonDS's ~43 ms SPU ring
    /// saturated below the target the servo was steering toward, so the
    /// queue never filled, priming never released, and the session
    /// played silence forever. And a rollback can take back anything the
    /// ring still holds, so leaving it there is what makes the revocable
    /// window the whole queue instead of one fill.
    #[test]
    fn the_backlog_stays_in_the_ring() {
        let (_into, mut pull) = ring(RATE, 20_000);
        assert_eq!(pull.source_available(), 20_000);

        pull.process(RATE, 48_000.0, 512);
        let mut out = [0i16; 1024];
        assert_eq!(pull.read(&mut out, 512), 512);

        // Nothing lost in the move: the backlog is what was there minus
        // what played.
        assert_eq!(pull.source_available(), 20_000 - 512 * 32768 / 48_000);
        // And almost none of it was pulled out to get that fill played.
        assert!(
            pull.source.len() / NUM_CHANNELS < 600,
            "{} frames staged for a 512-frame fill",
            pull.source.len() / NUM_CHANNELS
        );
    }

    /// Shedding a burst reaches through the staged frames into the ring,
    /// since that is where nearly all of a backlog sits.
    #[test]
    fn discarding_reaches_through_the_staging_into_the_ring() {
        let (_into, mut pull) = ring(RATE, 20_000);
        pull.process(RATE, 48_000.0, 512);
        let before = pull.source_available();
        let staged = pull.source.len() / NUM_CHANNELS;
        assert!(staged > 0 && staged < 15_000, "{staged} staged — nothing to reach past");

        pull.discard_source(15_000);
        assert_eq!(pull.source_available(), before - 15_000);
    }

    /// The resample loop's last step can carry the cursor past the end
    /// of what was staged whenever the step exceeds 1 — under the
    /// fast-forward fold a 65536 Hz cart against a 48 kHz device at 4x
    /// steps ~5.5 source frames per output frame. The drain after the
    /// loop must not follow the cursor out of bounds: with 8 frames
    /// queued the cursor lands past 10, and draining to there panicked
    /// in the sound callback (which aborts the process).
    #[test]
    fn cursor_overshoot_does_not_drain_past_the_source_end() {
        let (_into, mut pull) = ring(65536.0, 8);
        pull.source_available();
        // The speed folds into the destination rate:
        // 48000 × (59.7275 / 240).
        pull.process(65536.0, 11945.5, 512);
        // Only the first cursor position has the RADIUS frames of
        // lookahead the kernel reads; the second stands at ~5.5 of 8.
        assert_eq!(pull.available(), 1);
    }

    /// The kernel rows are normalized: a constant signal comes out at
    /// the same level for every cursor fraction, rather than rippling
    /// at the beat between the two rates.
    #[test]
    fn a_constant_signal_passes_through_at_unity_gain() {
        let (mut into, mut pull) = ring(RATE, 0);
        into.push(&vec![1000i16; 4096 * NUM_CHANNELS]);
        pull.source_available();
        pull.process(RATE, 48_000.0, 2048);
        let mut out = vec![0i16; 2048 * NUM_CHANNELS];
        let got = pull.read(&mut out, 2048);
        assert_eq!(got, 2048);
        for &sample in &out[..got * NUM_CHANNELS] {
            assert!((sample - 1000).abs() <= 1, "constant 1000 came out as {sample}");
        }
    }

    /// A zero or negative step can never advance the cursor, so the
    /// resample loop would push output forever.
    #[test]
    fn a_degenerate_rate_produces_nothing_rather_than_hanging() {
        let (_into, mut pull) = ring(RATE, 4096);
        pull.source_available();
        pull.process(0.0, 48_000.0, 512);
        assert_eq!(pull.available(), 0);
        pull.process(RATE, 0.0, 512);
        assert_eq!(pull.available(), 0);
    }
}

//! WSOLA time stretching: change how long audio takes to play without
//! changing its pitch.
//!
//! [`CoreStream`](super::CoreStream) reaches for this when the sim runs
//! at a real speed multiple (fast-forward, slow-mo): the resampler
//! stays pitch-true and the stretcher absorbs the tempo difference,
//! instead of folding the speed into the resample ratio and shifting 4×
//! up two octaves.
//!
//! Waveform-similarity overlap-add: output is stitched from
//! [`SEGMENT_MS`]-long input segments. Each segment nominally starts
//! where the speed ratio dictates, but is nudged within ±[`SEARCH_MS`]
//! to wherever its head best continues the previous segment's
//! waveform, then spliced in under an [`OVERLAP_MS`] crossfade. At
//! ratio 1 the best continuation IS the previous segment's own tail,
//! the crossfade blends identical samples, and the output is bit-exact
//! passthrough — artifacts appear only as the ratio forces real
//! splices.

use std::collections::VecDeque;

/// Frames of output emitted per synthesis hop. Long enough that
/// splices stay sparse (tonal game audio smears if spliced too often),
/// short enough that transients aren't audibly doubled or dropped.
const SEGMENT_MS: usize = 21;

/// Crossfade length at each splice.
const OVERLAP_MS: usize = 8;

/// Search radius around the nominal segment start — the room the
/// similarity search has to line waveforms up. Comfortably over one
/// period of the lowest tone worth aligning (8 ms ≈ 125 Hz).
const SEARCH_MS: usize = 8;

pub struct TimeStretcher {
    /// [`SEGMENT_MS`] / [`OVERLAP_MS`] / [`SEARCH_MS`] in frames at the
    /// device rate.
    hs: usize,
    overlap: usize,
    search: usize,
    /// Device-rate input window: stereo for output, summed mono for
    /// the similarity search.
    input: Vec<[f32; 2]>,
    mono: Vec<f32>,
    /// Nominal start of the next segment, in (fractional) frames from
    /// the head of `input` — advanced by `hs × ratio` per hop.
    pos: f64,
    /// Where the previously emitted segment would naturally continue
    /// in `input` — the waveform the next splice must match. `None`
    /// right after a reset; the first segment is a straight copy.
    cont: Option<usize>,
    out: VecDeque<[i16; 2]>,
}

impl TimeStretcher {
    pub fn new(out_rate: u32) -> Self {
        let per_ms = (out_rate as usize / 1000).max(8);
        let overlap = OVERLAP_MS * per_ms;
        Self {
            hs: (SEGMENT_MS * per_ms).max(overlap * 2),
            overlap,
            search: SEARCH_MS * per_ms,
            input: Vec::new(),
            mono: Vec::new(),
            pos: 0.0,
            cont: None,
            out: VecDeque::new(),
        }
    }

    pub fn reset(&mut self) {
        self.input.clear();
        self.mono.clear();
        self.pos = 0.0;
        self.cont = None;
        self.out.clear();
    }

    /// Append interleaved-stereo device-rate frames to the input window.
    pub fn push(&mut self, samples: &[i16]) {
        for frame in samples.chunks_exact(2) {
            let (l, r) = (frame[0] as f32, frame[1] as f32);
            self.input.push([l, r]);
            self.mono.push(l + r);
        }
    }

    /// How many more input frames [`Self::pop`] needs before it can
    /// deliver `want` output frames at `ratio` input-frames-per-output-
    /// frame. Deliberately a slight over-ask (it assumes every hop
    /// needs the full search window and continuation reference): an
    /// under-ask would starve a hop, short the fill, and false-latch
    /// the stream's re-priming into a silence loop.
    pub fn input_deficit(&self, want: usize, ratio: f64) -> usize {
        let missing = want.saturating_sub(self.out.len());
        if missing == 0 {
            return 0;
        }
        let hops = missing.div_ceil(self.hs);
        let last = self.pos + (hops as f64 - 1.0) * (self.hs as f64 * ratio);
        let need = last.ceil().max(0.0) as usize + self.search + self.hs + self.overlap + 2;
        need.saturating_sub(self.input.len())
    }

    /// Synthesize up to `dst.len()` output frames; returns how many
    /// were delivered (short only when the input window can't cover
    /// another hop).
    pub fn pop(&mut self, dst: &mut [[i16; 2]], ratio: f64) -> usize {
        while self.out.len() < dst.len() && self.hop(ratio) {}
        self.trim();
        let n = dst.len().min(self.out.len());
        for frame in dst[..n].iter_mut() {
            *frame = self.out.pop_front().unwrap();
        }
        n
    }

    /// One synthesis hop: pick the next segment, splice, emit `hs`
    /// frames. False if the input window can't cover it yet.
    fn hop(&mut self, ratio: f64) -> bool {
        let advance = self.hs as f64 * ratio;
        let Some(cont) = self.cont else {
            // First segment after a reset: nothing to match, copy.
            if self.input.len() < self.hs {
                return false;
            }
            for frame in &self.input[..self.hs] {
                self.out.push_back(quantize(*frame));
            }
            self.cont = Some(self.hs);
            self.pos = advance;
            return true;
        };

        let lo = (self.pos - self.search as f64).floor().max(0.0) as usize;
        let hi = (self.pos + self.search as f64).ceil() as usize;
        if self.input.len() < (hi + self.hs).max(cont + self.overlap) {
            return false;
        }

        // Find the candidate start whose head best matches the previous
        // segment's continuation (normalized cross-correlation on the
        // mono mix).
        let reference = &self.mono[cont..cont + self.overlap];
        let e_ref: f32 = reference.iter().map(|x| x * x).sum();
        let mut best = lo;
        let mut best_score = f32::MIN;
        for candidate in lo..=hi {
            let mut dot = 0.0f32;
            let mut energy = 0.0f32;
            for (a, b) in self.mono[candidate..candidate + self.overlap].iter().zip(reference) {
                dot += a * b;
                energy += a * a;
            }
            let score = dot / (energy * e_ref).sqrt().max(1e-6);
            if score > best_score {
                best_score = score;
                best = candidate;
            }
        }

        // Splice: fade the continuation out against the new segment's
        // head (raised-cosine, equal-gain — the search made them
        // correlated), then copy the rest of the segment straight.
        for i in 0..self.overlap {
            let w = 0.5 - 0.5 * (std::f32::consts::PI * (i as f32 + 0.5) / self.overlap as f32).cos();
            let a = self.input[cont + i];
            let b = self.input[best + i];
            self.out
                .push_back(quantize([a[0] + (b[0] - a[0]) * w, a[1] + (b[1] - a[1]) * w]));
        }
        for frame in &self.input[best + self.overlap..best + self.hs] {
            self.out.push_back(quantize(*frame));
        }

        self.cont = Some(best + self.hs);
        self.pos += advance;
        true
    }

    /// Drop input frames no future hop can reach.
    fn trim(&mut self) {
        let mut cut = (self.pos - self.search as f64).floor() as isize;
        if let Some(cont) = self.cont {
            cut = cut.min(cont as isize);
        }
        if cut <= 0 {
            return;
        }
        let cut = cut as usize;
        self.input.drain(..cut);
        self.mono.drain(..cut);
        self.pos -= cut as f64;
        if let Some(cont) = &mut self.cont {
            *cont -= cut;
        }
    }
}

fn quantize(frame: [f32; 2]) -> [i16; 2] {
    [
        frame[0].round().clamp(-32768.0, 32767.0) as i16,
        frame[1].round().clamp(-32768.0, 32767.0) as i16,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48000;
    const HZ: f32 = 440.0;

    /// Interleaved-stereo sine frames `[start, start + frames)` of a
    /// phase-continuous tone.
    fn sine(start: usize, frames: usize) -> Vec<i16> {
        (start..start + frames)
            .flat_map(|i| {
                let s = ((i as f32 * HZ * std::f32::consts::TAU / RATE as f32).sin() * 12000.0) as i16;
                [s, s]
            })
            .collect()
    }

    /// Positive-going zero crossings of the left channel — a cycle
    /// counter, so `crossings / seconds ≈ tone frequency`.
    fn cycles(frames: &[[i16; 2]]) -> usize {
        frames.windows(2).filter(|w| w[0][0] < 0 && w[1][0] >= 0).count()
    }

    fn assert_pitch_preserved(delivered: &[[i16; 2]]) {
        let expected = HZ * delivered.len() as f32 / RATE as f32;
        let got = cycles(delivered) as f32;
        assert!(
            (got - expected).abs() / expected < 0.08,
            "expected ~{expected} cycles, counted {got}"
        );
    }

    #[test]
    fn unity_ratio_is_passthrough() {
        let mut stretcher = TimeStretcher::new(RATE);
        let src = sine(0, 24000);
        stretcher.push(&src);
        let mut out = vec![[0i16; 2]; 20000];
        assert_eq!(stretcher.pop(&mut out, 1.0), out.len());
        for (i, frame) in out.iter().enumerate() {
            assert_eq!(*frame, [src[i * 2], src[i * 2 + 1]], "frame {i}");
        }
    }

    #[test]
    fn double_speed_preserves_pitch() {
        let mut stretcher = TimeStretcher::new(RATE);
        stretcher.push(&sine(0, 96000));
        let mut out = vec![[0i16; 2]; 40000];
        let n = stretcher.pop(&mut out, 2.0);
        assert!(n > 30000, "only {n} frames from 96000 at 2×");
        assert_pitch_preserved(&out[..n]);
    }

    #[test]
    fn half_speed_preserves_pitch() {
        let mut stretcher = TimeStretcher::new(RATE);
        stretcher.push(&sine(0, 48000));
        let mut out = vec![[0i16; 2]; 80000];
        let n = stretcher.pop(&mut out, 0.5);
        assert!(n > 70000, "only {n} frames from 48000 at 0.5×");
        assert_pitch_preserved(&out[..n]);
    }

    /// The deficit estimate must always cover the hops a pop needs —
    /// an under-ask would short the fill and false-latch the stream's
    /// re-priming into silence. Drive feed-exactly-the-deficit loops at
    /// ratios on both sides of 1 (0.2 exercises the overlap term:
    /// segment starts advance slower than continuations there).
    #[test]
    fn input_deficit_is_sufficient() {
        for ratio in [0.2, 0.5, 1.0, 3.0, 4.0] {
            let mut stretcher = TimeStretcher::new(RATE);
            let mut fed = 0usize;
            let mut out = vec![[0i16; 2]; 512];
            for i in 0..200 {
                let deficit = stretcher.input_deficit(out.len(), ratio);
                stretcher.push(&sine(fed, deficit));
                fed += deficit;
                assert_eq!(stretcher.pop(&mut out, ratio), out.len(), "iteration {i} at {ratio}×");
            }
        }
    }
}

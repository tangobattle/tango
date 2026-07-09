//! Synthesized menu sound effects. No asset files: each sound is a
//! tiny additive synth (sine + a couple of harmonics under a fast
//! exponential decay — the GBA-flavored "chip blip" register)
//! rendered once into a sample bank on first use. [`play`] just
//! queues a voice; [`LateBinder::fill`] calls [`mix_into`] on the
//! audio thread to ride the active voices on top of whatever the
//! host stream is carrying (silence between sessions, the emulator
//! during one).
//!
//! Volume is its own knob (`config.ui_sfx_volume`, the "Menu
//! sounds" slider), independent of the emulator's master volume —
//! 0 disables the sounds entirely and short-circuits [`play`].
//!
//! [`LateBinder::fill`]: super::LateBinder

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex};

/// Banks are rendered at the SDL backend's fixed output rate
/// (`sdl::TARGET_SAMPLE_RATE`); there's no resampling path here.
const RATE: f32 = 48_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sfx {
    /// Pointer entered an options row — a barely-there tick.
    Hover = 0,
    /// Selection moved (tab switch, section switch).
    Move = 1,
    /// Something was committed (menu action picked, lobby ready).
    Confirm = 2,
    /// Something was backed out of (lobby left).
    Cancel = 3,
    /// The big one: an opponent connected to the lobby.
    Sting = 4,
}

const BANK_COUNT: usize = 5;

/// One in-flight sound: which bank, and how far through it we are.
struct Voice {
    bank: usize,
    cursor: usize,
}

/// `config.ui_sfx_volume` as raw f32 bits — UI thread stores, audio
/// thread loads per fill. Starts at 0 (silent) until `App::new`
/// pushes the configured value.
static VOLUME: AtomicU32 = AtomicU32::new(0);

static VOICES: Mutex<Vec<Voice>> = Mutex::new(Vec::new());

/// Additive-synth one note into `buf` at `start` seconds:
/// fundamental plus quieter 2nd/3rd harmonics for the chip timbre,
/// under a 2 ms attack, an exponential decay across the note, and a
/// short linear tail so the cutoff never clicks.
fn note_into(buf: &mut Vec<f32>, start: f32, freq: f32, dur: f32, amp: f32) {
    use std::f32::consts::TAU;
    let s0 = (start * RATE) as usize;
    let n = (dur * RATE) as usize;
    if buf.len() < s0 + n {
        buf.resize(s0 + n, 0.0);
    }
    for i in 0..n {
        let t = i as f32 / RATE;
        let w = (TAU * freq * t).sin() + 0.28 * (TAU * freq * 2.0 * t).sin() + 0.12 * (TAU * freq * 3.0 * t).sin();
        let env = (1.0 - (-t / 0.002).exp()) * (-3.0 * t / dur).exp() * ((dur - t) / 0.005).min(1.0);
        buf[s0 + i] += w * env * amp;
    }
}

fn render(notes: &[(f32, f32, f32, f32)]) -> Vec<f32> {
    let mut buf = Vec::new();
    for &(start, freq, dur, amp) in notes {
        note_into(&mut buf, start, freq, dur, amp);
    }
    buf
}

/// The sample banks, indexed by `Sfx as usize`. Pitches walk an
/// E-major-ish ladder so every cue sounds like family; amplitudes
/// are pre-baked conservative and scaled by [`VOLUME`] at mix time.
static BANKS: LazyLock<[Vec<f32>; BANK_COUNT]> = LazyLock::new(|| {
    [
        // Hover: a soft high tick.
        render(&[(0.0, 2200.0, 0.025, 0.045)]),
        // Move: one mid blip — the classic cursor sound.
        render(&[(0.0, 880.0, 0.055, 0.10)]),
        // Confirm: two quick rising notes (E5 → B5).
        render(&[(0.0, 659.25, 0.05, 0.10), (0.05, 987.77, 0.10, 0.10)]),
        // Cancel: two falling ones (D5 → G4).
        render(&[(0.0, 587.33, 0.05, 0.10), (0.05, 392.0, 0.11, 0.10)]),
        // Sting: rising arpeggio E5–G5–B5–E6, last note ringing out.
        render(&[
            (0.0, 659.25, 0.10, 0.11),
            (0.08, 783.99, 0.10, 0.11),
            (0.16, 987.77, 0.12, 0.11),
            (0.24, 1318.51, 0.30, 0.12),
        ]),
    ]
});

/// Set the menu-sounds volume. Clamped to `[0.0, 1.0]`; single
/// atomic store, callable from the UI thread at any time.
pub fn set_volume(v: f32) {
    VOLUME.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
}

fn volume() -> f32 {
    f32::from_bits(VOLUME.load(Ordering::Relaxed))
}

/// Queue a sound. Cheap and fire-and-forget: callable from update
/// handlers and widget internals alike. Re-triggers of the same
/// sound within its first ~40 ms coalesce into the existing voice
/// (hover sweeps and slider drags re-fire much faster than that),
/// and the voice pool is capped so a pathological caller can't
/// stack a wall of sound.
pub fn play(sfx: Sfx) {
    if volume() <= 0.0 {
        return;
    }
    let bank = sfx as usize;
    let mut voices = VOICES.lock().unwrap();
    const COALESCE_SAMPLES: usize = (RATE * 0.04) as usize;
    if voices.len() >= 8 || voices.iter().any(|v| v.bank == bank && v.cursor < COALESCE_SAMPLES) {
        return;
    }
    voices.push(Voice { bank, cursor: 0 });
}

/// Mix all active voices into `buf` (both channels), advancing
/// their cursors. Returns whether anything was mixed — the caller
/// uses that to decide if the tail past the session stream's fill
/// count now carries signal. Audio-thread side; the lock is only
/// contended against [`play`]'s push.
pub fn mix_into(buf: &mut [[i16; super::NUM_CHANNELS]]) -> bool {
    let mut voices = VOICES.lock().unwrap();
    if voices.is_empty() {
        return false;
    }
    let vol = volume();
    for v in voices.iter_mut() {
        let bank = &BANKS[v.bank];
        let n = buf.len().min(bank.len() - v.cursor);
        for (frame, s) in buf[..n].iter_mut().zip(&bank[v.cursor..v.cursor + n]) {
            let add = (s * vol * 32767.0) as i32;
            for ch in frame.iter_mut() {
                *ch = (*ch as i32 + add).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            }
        }
        v.cursor += n;
    }
    voices.retain(|v| v.cursor < BANKS[v.bank].len());
    true
}

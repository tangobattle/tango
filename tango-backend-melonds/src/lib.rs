//! The melonDS backend: [`tango_match::Backend`] over a pair of
//! emulated DSes on emulated local wireless.
//!
//! This is one half of what a DS game needs. The engine-specific half
//! is here — how a pair ticks, snapshots, restores and draws — while
//! the game-specific half (priming a link into that game's link battle)
//! stays in the game's own crate.
//!
//! Re-exports the pieces a game crate needs so it can depend on this
//! rather than on the emulator directly.


pub mod audio;

use tango_match::{Backend, Screen, ScreenLayout};

/// The rate the SPU hands samples out at.
///
/// Not the DS's own 32823.6328 Hz: melonDS resamples internally, and
/// what it resamples *to* is `NDSArgs::OutputSampleRate`, which the shim
/// leaves at its 48 kHz default. Claiming the console's native rate
/// instead stretches playback by 48000/32823.6 — about six semitones
/// flat — and drains the source queue half again as fast as it fills,
/// which underruns into a crackle on top of the wrong pitch.
pub const SAMPLE_RATE: f64 = 48_000.0;

/// The DS's video framerate, which is also the rate audio production
/// scales against when a host paces the simulation faster or slower.
pub const FPS: f64 = 33_513_982.0 / 560_190.0;

/// How long a second of this console's production lasts once the host
/// paces the simulation at `fps_target`.
///
/// Native over target, *not* the other way round: at twice the DS's
/// framerate the SPU emits twice the audio per wall-clock second, so
/// what it produced in a second now has to play in half of one and the
/// ratio falls. Same shape as mgba's
/// `clockRate / (desiredFrameRate * frameCycles)`, which is what the
/// stream's faux clock is written against — it scales the resampler's
/// destination rate by this directly, so inverting it makes a
/// fast-forward play *slower* rather than faster.
pub fn framerate_ratio(fps_target: f64) -> f64 {
    if fps_target > 0.0 {
        FPS / fps_target
    } else {
        1.0
    }
}

/// The DS presents two identically-sized screens. Listed in the order
/// [`MelonDs::frame`] lays them out, which is the console's top screen
/// then its bottom (touch) one — left to right in the composed frame
/// rather than the console's own physical stack.
const SCREENS: [Screen; 2] = [
    Screen {
        width: 256,
        height: 192,
    },
    Screen {
        width: 256,
        height: 192,
    },
];

/// Marker type: the backend is all associated types and free
/// functions, so it never needs a value.
pub enum MelonDs {}

impl Backend for MelonDs {
    type Link = Link;
    type Snapshot = Snapshot;
    type Input = Input;
    type Error = melonds::Error;

    fn boot(
        roms: [&[u8]; 2],
        saves: [Option<&[u8]>; 2],
        rtc: std::time::SystemTime,
    ) -> Result<Link, melonds::Error> {
        // Both consoles run the same cart — a DS link is one game, two
        // consoles — so the second ROM is ignored.
        let _ = roms[1];
        Link::new(roms[0], saves, rtc_parts(rtc))
    }

    fn input_of(host: tango_match::HostInput) -> Input {
        input_from_host(host)
    }

    fn host_of(input: Input) -> tango_match::HostInput {
        tango_match::HostInput {
            keys: input.keys,
            touch: input.touch,
        }
    }

    fn tick(link: &mut Link, inputs: [Input; 2]) {
        link.tick(inputs);
    }

    fn snapshot(link: &mut Link, recycled: Option<Snapshot>) -> Result<Snapshot, melonds::Error> {
        link.snapshot_into(recycled)
    }

    fn restore(link: &mut Link, snapshot: &Snapshot) -> Result<(), melonds::Error> {
        link.restore(snapshot)
    }

    fn frame(link: &mut Link, player: usize) -> Option<Vec<u8>> {
        let (top, bottom) = link.console(player).framebuffers()?;
        let mut rgba = Vec::with_capacity(SCREENS.iter().map(Screen::len).sum());
        // Side by side, so a row of the composite is a row of the top
        // screen followed by the same row of the bottom one. Stacked
        // would be the cheaper `top` then `bottom` — concatenation is a
        // vertical stack for free when the widths match — but a 256x384
        // pane wastes most of the width of any display it is drawn into.
        let (width, height) = (SCREENS[0].width as usize, SCREENS[0].height as usize);
        for row in 0..height {
            for screen in [top, bottom] {
                for &pixel in &screen[row * width..(row + 1) * width] {
                    // The core hands out BGRA words; hosts want RGBA bytes.
                    let [b, g, r, _] = pixel.to_le_bytes();
                    rgba.extend_from_slice(&[r, g, b, 0xff]);
                }
            }
        }
        Some(rgba)
    }

    fn screen_layout() -> ScreenLayout {
        ScreenLayout::new(SCREENS)
    }

    fn set_render(link: &mut Link, player: usize, on: bool) {
        link.console(player).set_render(on);
    }

    fn audio(
        link: std::sync::Arc<std::sync::Mutex<Link>>,
        player: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> Box<dyn tango_match::AudioDrain> {
        Box::new(crate::audio::pull(link, player))
    }

}

/// Translate the seam's input word into the console's.
///
/// The DS pad is the GBA's plus X and Y, and Tango's own bit order
/// already matches the console's for the buttons both share, so the pad
/// half is a mask rather than a remap. The touch position clamps into
/// the bottom screen rather than trusting the host's mapping
/// arithmetic: an out-of-range sample is simulation state here, and
/// both peers must derive the same one.
pub fn input_from_host(input: tango_match::HostInput) -> Input {
    Input {
        keys: input.keys & 0xfff,
        touch: input.touch.map(|(x, y)| {
            (
                x.min(SCREENS[1].width as u16 - 1),
                y.min(SCREENS[1].height as u16 - 1),
            )
        }),
    }
}

/// Re-exported so a game crate can name a link, a snapshot, an input or
/// a session without depending on the emulator crates itself.


/// Split an instant into the fields a cart RTC takes. Both peers pass
/// the same one, so both consoles agree without a date library.
fn rtc_parts(rtc: std::time::SystemTime) -> (i32, i32, i32, i32, i32, i32) {
    let secs = rtc
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        year as i32,
        month as i32,
        day as i32,
        (rem / 3_600) as i32,
        (rem % 3_600 / 60) as i32,
        (rem % 60) as i32,
    )
}

#[cfg(test)]
mod tests {
    /// The shared rollback loop instantiates for this backend — the
    /// point of the seam. A DS match is `Match<MelonDs>`, a GBA match is
    /// `Match<Mgba>`, and neither engine reimplements the loop.
    #[test]
    fn the_shared_engine_accepts_this_backend() {
        fn assert_usable<B: tango_match::Backend>() {}
        assert_usable::<super::MelonDs>();
        let _: fn(super::Link, usize, u32) -> Result<tango_match::engine::Match<super::MelonDs>, melonds::Error> =
            tango_match::engine::Match::<super::MelonDs>::new;
    }
}

/// Re-exported so a game crate can name a link, a snapshot, an input
/// or a session without depending on the emulator crates itself.
pub use melonds_rollback::{session::Session, Input, Link, Snapshot};

/// One console of a pair. A game crate needs this to reach past the link
/// when priming: execution traps are installed per console.
pub use melonds::Nds;

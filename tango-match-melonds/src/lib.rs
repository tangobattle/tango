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

use tango_match::{Backend, Screen, ScreenLayout};

/// The DS presents two identically-sized screens, top then bottom.
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
        for screen in [top, bottom] {
            for &pixel in screen {
                // The core hands out BGRA words; hosts want RGBA bytes.
                let [b, g, r, _] = pixel.to_le_bytes();
                rgba.extend_from_slice(&[r, g, b, 0xff]);
            }
        }
        Some(rgba)
    }

    fn screen_layout() -> ScreenLayout {
        ScreenLayout::new(SCREENS)
    }
}

/// Translate Tango's joyflag word into DS keys.
///
/// The DS pad is the GBA's plus X and Y, and Tango's own bit order
/// already matches the console's for the buttons both share, so this is
/// a mask rather than a remap. The stylus has no binding yet: nothing
/// in a NetBattle needs it once the match is running.
pub fn input_from_joyflags(joyflags: u32) -> Input {
    Input::keys(joyflags & 0xfff)
}

/// Re-exported so a game crate can name a link, a snapshot, an input or
/// a session without depending on the emulator crates itself.
pub use melonds_rollback::{session::Session, Input, Link, Snapshot};

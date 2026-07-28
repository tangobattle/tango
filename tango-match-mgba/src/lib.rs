//! The mgba backend: [`tango_match::Backend`] over a pair of emulated
//! GBAs on an emulated link cable.
//!
//! The sibling of `tango-match-melonds`. Both answer the same
//! questions — how does a pair tick, snapshot, restore and draw — for
//! very different hardware, which is what lets one host drive either.

use tango_match::{Backend, Screen, ScreenLayout};

/// The GBA's single screen.
const SCREEN: Screen = Screen {
    width: 240,
    height: 160,
};

/// Marker type: the backend is all associated types and free functions,
/// so it never needs a value.
pub enum Mgba {}

impl Backend for Mgba {
    type Link = mgba_rollback::Link;
    type Snapshot = mgba_rollback::Snapshot;
    /// One side's joypad keys for one tick.
    type Input = u32;
    type Error = mgba::Error;

    fn tick(link: &mut Self::Link, inputs: [u32; 2]) {
        link.tick(&inputs);
    }

    fn snapshot(link: &mut Self::Link, _recycled: Option<Self::Snapshot>) -> Result<Self::Snapshot, mgba::Error> {
        // GBA states are small enough (~0.5 MB against the DS's ~6 MB)
        // that recycling buffers has never been worth the plumbing.
        link.save()
    }

    fn restore(link: &mut Self::Link, snapshot: &Self::Snapshot) -> Result<(), mgba::Error> {
        link.load(snapshot)
    }

    fn frame(link: &mut Self::Link, player: usize) -> Option<Vec<u8>> {
        // Cores render BGR555; expanding here is what keeps the console's
        // native pixel format from leaking out to hosts.
        let native = link.video_buffer(player)?;
        let mut rgba = vec![0u8; native.len() * 2];
        mgba::gba::bgr555_to_rgba8(native, &mut rgba);
        Some(rgba)
    }

    fn screen_layout() -> ScreenLayout {
        ScreenLayout::new([SCREEN])
    }
}

/// Re-exported so a game crate can name a link or a snapshot without
/// depending on the emulator crates itself.
pub use mgba_rollback::{Link, Snapshot};

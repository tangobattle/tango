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

/// Starts GBA matches for one game registration.
///
/// A registration holds one of these as its `pvp`, so the app starts a
/// match without knowing which emulator is underneath. `support`
/// resolves a variant of this family to that variant's engine support:
/// the game's own crate has every variant's statics in scope, which is
/// how the peer's side gets resolved without the seam knowing anything
/// about registries.
pub struct GbaMatchFactory {
    /// This registration's variant.
    pub variant: u8,
    /// Variant of this family -> its engine support.
    pub support: fn(u8) -> &'static (dyn crate::GameSupport + Send + Sync),
}

impl tango_match::MatchFactory for GbaMatchFactory {
    fn screen_layout(&self) -> tango_match::ScreenLayout {
        <Mgba as tango_match::Backend>::screen_layout()
    }

    fn start(
        &self,
        config: tango_match::StartConfig,
    ) -> Result<Box<dyn tango_match::RunningMatch>, tango_match::Error> {
        // Engine support is indexed by seat, not by who is local.
        let mut support: [&dyn crate::GameSupport; 2] =
            [(self.support)(self.variant), (self.support)(config.peer_variant)];
        if config.local_player == 1 {
            support.swap(0, 1);
        }
        let match_ = crate::engine::Match::new(crate::engine::MatchConfig {
            roms: [config.roms[0].to_vec(), config.roms[1].to_vec()],
            saves: [
                config.saves[0].unwrap_or_default().to_vec(),
                config.saves[1].unwrap_or_default().to_vec(),
            ],
            support,
            match_type: config.match_type,
            rng_seed: config.rng_seed,
            rtc: config.rtc,
            local_player: config.local_player,
            present_delay: config.present_delay,
            disable_bgm: config.disable_bgm,
        })
        .map_err(tango_match::Error::from)?;
        Ok(Box::new(match_))
    }
}

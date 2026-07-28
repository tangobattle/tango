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

    fn boot(
        roms: [&[u8]; 2],
        saves: [Option<&[u8]>; 2],
        rtc: std::time::SystemTime,
    ) -> Result<Self::Link, mgba::Error> {
        mgba_rollback::Link::with_options(mgba_rollback::LinkOptions {
            sides: (0..2)
                .map(|i| mgba_rollback::SideOptions {
                    rom: roms[i].to_vec(),
                    save: saves[i].map(|s| s.to_vec()),
                })
                .collect(),
            rtc: Some(rtc),
            peripheral: mgba_rollback::Peripheral::Cable,
        })
    }

    fn input_from_keys(keys: u32) -> u32 {
        keys
    }

    fn keys_of(input: u32) -> u32 {
        input
    }

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

    fn audio(
        link: std::sync::Arc<std::sync::Mutex<Self::Link>>,
        player: usize,
    ) -> Box<dyn tango_match::AudioPull> {
        Box::new(tango_match::Resampled::new(SharedPairAudio { link, player }))
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
        let match_ = crate::r#match::engine::Match::new(crate::r#match::engine::MatchConfig {
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

#[cfg(test)]
mod tests {
    /// The shared rollback loop instantiates for this backend too.
    #[test]
    fn the_shared_engine_accepts_this_backend() {
        fn assert_usable<B: tango_match::Backend>() {}
        assert_usable::<super::Mgba>();
        let _: fn(
            mgba_rollback::Link,
            usize,
            u32,
        ) -> Result<tango_match::engine::Match<super::Mgba>, mgba::Error> =
            tango_match::engine::Match::<super::Mgba>::new;
    }
}


/// Start a GBA match from both sides' engine support.
///
/// A match can span two variants of a family (Gregar against Falzar),
/// so the engine needs each seat's support — and only something holding
/// both registrations can supply that, which is why this takes them
/// rather than hanging off one game.
pub fn start_match(
    local: &'static (dyn crate::GameSupport + Send + Sync),
    peer: &'static (dyn crate::GameSupport + Send + Sync),
    config: tango_match::StartConfig,
) -> Result<Box<dyn tango_match::RunningMatch>, tango_match::Error> {
    // Support is indexed by seat, not by who is local.
    let mut support: [&dyn crate::GameSupport; 2] = [local, peer];
    if config.local_player == 1 {
        support.swap(0, 1);
    }
    let match_ = crate::r#match::engine::Match::new(crate::r#match::engine::MatchConfig {
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


/// A pair behind the seam's shared handle, as something the shared
/// resampler can drain. (`crate::audio::ConsoleAudio` is the same thing
/// over mgba-rollback's own handle, which the legacy engine hands out.)
struct SharedPairAudio {
    link: std::sync::Arc<std::sync::Mutex<mgba_rollback::Link>>,
    player: usize,
}

impl tango_match::AudioDrain for SharedPairAudio {
    fn sample_rate(&self) -> f64 {
        self.link.lock().unwrap().core_mut(self.player).audio_sample_rate() as f64
    }

    fn framerate_ratio(&self, fps_target: f64) -> f64 {
        self.link
            .lock()
            .unwrap()
            .core_mut(self.player)
            .calculate_framerate_ratio(fps_target)
    }

    fn drain(&mut self, out: &mut [i16]) -> usize {
        let mut link = self.link.lock().unwrap();
        let buffer = link.core_mut(self.player).audio_buffer();
        let frames = (out.len() / 2).min(buffer.available());
        buffer.read(out, frames)
    }
}

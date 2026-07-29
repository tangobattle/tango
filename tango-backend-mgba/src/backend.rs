//! This engine as a game registration holds it.
//!
//! A `Game` registration cannot name an emulator — that is the whole
//! point of [`tango_match::Backend`] — so everything a host asks
//! of the mgba engine arrives through one object per registered
//! cartridge. [`GbaBackend`] is that object: it closes over the
//! cartridge's own [`GameSupport`](crate::GameSupport) and, for the
//! seat it does not own, resolves the peer's out of the family table
//! its crate hands it.
//!
//! Netplay, a single-player ride, and replay playback all come out of
//! here, so `tango-session` drives a GBA game through exactly the same
//! calls it drives a DS one through, and never learns which is which.

use crate::playback;

/// One cartridge in a family, keyed as its ROM header names it.
pub type Seat = (&'static [u8; 4], u8, &'static (dyn crate::GameSupport + Send + Sync));

/// A GBA cartridge's engine support, as the engine-neutral backend its
/// registration holds.
///
/// A match needs *both* seats' support and a backend hangs off one
/// game, so the peer arrives as a [`PeerRom`](tango_match::PeerRom)
/// looked up in `family` — the table its own crate declares, which is
/// the only place that knows what its siblings are. Crossplay is why
/// this is a lookup rather than a constant: a Japanese cart links with
/// an American one, and each seat needs the support for the ROM
/// actually in it.
pub struct GbaBackend {
    local: &'static (dyn crate::GameSupport + Send + Sync),
    family: &'static [Seat],
}

impl GbaBackend {
    pub const fn new(local: &'static (dyn crate::GameSupport + Send + Sync), family: &'static [Seat]) -> Self {
        GbaBackend { local, family }
    }

    /// The peer's support, or the local cart's if the family doesn't
    /// list what the peer says it is running. Falling back rather than
    /// failing keeps a mismatched revision playable-if-desynced instead
    /// of unstartable, which is what the engine did before this lookup
    /// existed.
    fn peer(&self, peer: tango_match::PeerRom) -> &'static (dyn crate::GameSupport + Send + Sync) {
        self.family
            .iter()
            .find(|(code, revision, _)| **code == peer.code && *revision == peer.revision)
            .map(|(_, _, support)| *support)
            .unwrap_or(self.local)
    }

    /// Both seats' support in seat order, which is not the same as
    /// local-and-peer: seat 0 is player 0 whoever that is.
    fn seats(&self, config: &tango_match::StartConfig) -> [&'static (dyn crate::GameSupport + Send + Sync); 2] {
        let mut seats = [self.local, self.peer(config.peer_rom)];
        if config.local_player == 1 {
            seats.swap(0, 1);
        }
        seats
    }

    fn boot_config(&self, config: &tango_match::ReplayConfig) -> playback::BootConfig {
        let mut support = [self.local, self.peer(config.peer_rom)];
        if config.local_player == 1 {
            support.swap(0, 1);
        }
        playback::BootConfig {
            roms: config.roms.clone(),
            saves: config.saves.clone(),
            support,
            match_type: config.match_type,
            rng_seed: config.rng_seed,
            rtc: config.rtc,
            disable_bgm: false,
        }
    }
}

impl tango_match::Backend for GbaBackend {
    fn screen_layout(&self) -> tango_match::ScreenLayout {
        crate::link::screen_layout()
    }

    fn expected_fps(&self) -> f64 {
        crate::link::EXPECTED_FPS
    }

    fn start(&self, config: tango_match::StartConfig) -> Result<tango_match::Match, tango_match::Error> {
        crate::engine::start(crate::engine::MatchConfig {
            roms: [config.roms[0].to_vec(), config.roms[1].to_vec()],
            saves: [
                config.saves[0].unwrap_or_default().to_vec(),
                config.saves[1].unwrap_or_default().to_vec(),
            ],
            support: self.seats(&config).map(|s| s as &dyn crate::GameSupport),
            match_type: config.match_type,
            rng_seed: config.rng_seed,
            rtc: config.rtc,
            local_player: config.local_player,
            present_delay: config.present_delay,
            disable_bgm: config.disable_bgm,
        })
    }

    fn stats_builder(&self, rom: &[u8]) -> tango_match::analysis::StatsBuilder {
        tango_match::analysis::StatsBuilder::new(self.local.usage_fold(rom))
    }

    fn start_solo(&self, config: tango_match::SoloConfig) -> Result<tango_match::Solo, tango_match::Error> {
        Ok(tango_match::Solo::new(
            crate::solo::SoloConsole::new(config.rom, config.save, config.rtc).map_err(tango_match::Error::from)?,
        ))
    }

    fn open_replay(&self, config: tango_match::ReplayConfig) -> Result<tango_match::ReplaySet, tango_match::Error> {
        let boot = self.boot_config(&config);
        // The pass reads chip use off the *local* seat's cart, which is
        // the one whose statistics a viewer is being shown.
        let usage = config
            .want_stats
            .then(|| boot.support[config.local_player].usage_fold(&boot.roms[config.local_player]));
        Ok(tango_match::ReplaySet::new(&config, usage, playback::Boot(boot)))
    }
}

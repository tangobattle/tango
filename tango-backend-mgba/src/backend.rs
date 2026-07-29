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
//!
//! The live-match boot lives in [`Backend::start`] itself: build the
//! two-player pair, prime both games to their link battle, wire the
//! RAM-poll telemetry, and start the seam's rollback
//! [`Match`](tango_match::Match) over it. The rollback loop itself is
//! `tango-match`'s — one engine for the cable and the DS's wireless
//! alike. What is mgba-shaped here is everything before the session's
//! first tick: the boot, the primer traps, and the audio bring-up.
//!
//! [`Backend::start`]: tango_match::Backend::start
//!
//! (mgba-rollback links go up to four players, but every game tango
//! supports is a two-player link battle, so this engine is two-player
//! throughout: the pair of cores IS the link.)

use mgba_rollback::{LinkOptions, Peripheral, SideOptions};

use crate::playback;
use crate::telemetry::Telemetry;
use crate::{GameSupport, PrimeConfig};

/// One cartridge in a family, keyed as its ROM header names it.
pub type Seat = (&'static [u8; 4], u8, &'static (dyn GameSupport + Send + Sync));

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
    local: &'static (dyn GameSupport + Send + Sync),
    family: &'static [Seat],
}

impl GbaBackend {
    pub const fn new(local: &'static (dyn GameSupport + Send + Sync), family: &'static [Seat]) -> Self {
        GbaBackend { local, family }
    }

    /// The peer's support, or the local cart's if the family doesn't
    /// list what the peer says it is running. Falling back rather than
    /// failing keeps a mismatched revision playable-if-desynced instead
    /// of unstartable, which is what the engine did before this lookup
    /// existed.
    fn peer(&self, peer: tango_match::PeerRom) -> &'static (dyn GameSupport + Send + Sync) {
        self.family
            .iter()
            .find(|(code, revision, _)| **code == peer.code && *revision == peer.revision)
            .map(|(_, _, support)| *support)
            .unwrap_or(self.local)
    }

    /// Both seats' support in seat order, which is not the same as
    /// local-and-peer: seat 0 is player 0 whoever that is.
    fn seats(&self, config: &tango_match::StartConfig) -> [&'static (dyn GameSupport + Send + Sync); 2] {
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

    /// Boot the pair, prime both games to their link battle, and start
    /// the rollback session. Priming runs identically on both peers (it
    /// is a pure function of ROM/save/rtc), so both reach the same
    /// state before the session — and therefore the same session
    /// initial state.
    fn start(&self, config: tango_match::StartConfig) -> Result<tango_match::Match, tango_match::Error> {
        assert!(config.local_player < 2);
        let support = self.seats(&config);

        crate::install_logger();
        let mut pair = mgba_rollback::Link::with_options(LinkOptions {
            sides: vec![
                SideOptions {
                    rom: config.roms[0].to_vec(),
                    save: Some(config.saves[0].unwrap_or_default().to_vec()),
                },
                SideOptions {
                    rom: config.roms[1].to_vec(),
                    save: Some(config.saves[1].unwrap_or_default().to_vec()),
                },
            ],
            rtc: Some(config.rtc),
            peripheral: Peripheral::Cable,
        })
        .map_err(crate::Error::from)?;

        let prime_config = PrimeConfig {
            match_type: config.match_type,
            rng_seed: config.rng_seed,
            disable_bgm: config.disable_bgm,
        };
        let lifecycle = crate::telemetry::LifecycleSink::new();
        let primed = [crate::PrimedLatch::new(), crate::PrimedLatch::new()];
        // The cores own their primer traps (see [`mgba_rollback::Link::set_traps`]):
        // core teardown walks the trap component, so the traps must live
        // exactly as long as their cores. They stay installed for the
        // pair's life — inert once primed, since their boot/menu addresses
        // never execute in battle.
        pair.set_traps(0, support[0].primer_traps(&prime_config, 0, &lifecycle, &primed[0]));
        pair.set_traps(1, support[1].primer_traps(&prime_config, 1, &lifecycle, &primed[1]));

        // Prime both cores to their link battle. The traps do all the
        // driving (each core's walk its own menu state machine); the
        // pads stay idle throughout. Priming is done when both games'
        // own battle-start code has fired.
        let mut prime_ticks = 0;
        while !(primed[0].is_set() && primed[1].is_set()) {
            if prime_ticks >= MAX_PRIME_TICKS {
                return Err(tango_match::Error::PrimeTimeout(MAX_PRIME_TICKS));
            }
            pair.tick(&[0, 0]);
            prime_ticks += 1;
        }
        log::info!("pvp: primed to link battle in {prime_ticks} ticks");

        // Audio bring-up, host-side only (sample buffers aren't in
        // savestates, so the simulation is unaffected — but do it
        // identically for both cores anyway, keeping the pairs
        // configured bit-identically across the peers):
        //   * deepen the buffers from mgba's 2048-sample default — the
        //     host's queue-level rate control wants ~50 ms queued plus
        //     headroom for rollback re-simulation bursts, and 2048
        //     doesn't even hold the 50 ms at the 65536 Hz rate BN4+
        //     run at;
        //   * drop what priming piled up (it ran far faster than real
        //     time with nothing draining), so the session doesn't open
        //     on a stale burst of boot/menu sound.
        for i in 0..2 {
            let core = pair.core_mut(i);
            core.set_audio_buffer_size(16384);
            core.audio_buffer().clear();
        }

        let (telemetry, handle) = Telemetry::new([support[0].core_poller(0), support[1].core_poller(1)], lifecycle);
        let mut match_ = tango_match::Match::new(
            crate::link::Link::new(pair, Some(telemetry)),
            config.local_player,
            config.present_delay,
        )?;
        match_.set_telemetry(handle);
        Ok(match_)
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

/// Cap on priming ticks before we give up bringing the games to their
/// link battle. Real bring-up is ~470 ticks (BN6); this is generous
/// headroom for slower families without hanging forever on a wedge.
const MAX_PRIME_TICKS: u32 = 3600;

//! This engine as a game registration holds it.
//!
//! A `Game` registration cannot name an emulator — that is the whole
//! point of [`tango_match::Backend`] — so everything a host asks of the
//! melonDS engine arrives through one object per registered cartridge.
//! [`DsBackend`] is that object. The one thing the engine cannot know
//! is how to get a booted pair into a game's link battle, so that
//! arrives through [`GameSupport`] — the game crate's half of the deal,
//! the DS counterpart of `tango-backend-mgba`'s trait of the same name.

use std::sync::atomic::AtomicBool;

use crate::link::Link;

/// Per-game support for the melonDS engine, implemented in the game's
/// own crate: walking a freshly booted pair into its link battle.
pub trait GameSupport: Sync {
    /// Walk a booted pair into the game's link battle. The walk must be
    /// a pure function of ROM/save/rtc/`match_type` — both peers run it
    /// on their own pairs and must reach identical state, because
    /// priming is simulation state and a difference here is a desync.
    /// Flipping `cancel` mid-walk fails it with
    /// [`Error::Cancelled`](tango_match::Error::Cancelled) — replay
    /// boots run on host worker threads whose teardown joins them.
    ///
    /// `match_type` is the mode the two players agreed on, indexed as
    /// the registration's `match_types` table lists it. Which console
    /// takes the game's own host seat is the walk's business, not the
    /// caller's: the pair is symmetric, and both peers walk both
    /// consoles, so the walk simply assigns it.
    fn prime(
        &self,
        link: &mut Link,
        match_type: (u8, u8),
        cancel: Option<&AtomicBool>,
    ) -> Result<(), tango_match::Error>;

    /// The telemetry reader for one console running this game. `player`
    /// is which console (and player) this poller answers for. Pure
    /// reads over that console's RAM, `None` while the game has no live
    /// battle state — and that `None` is load-bearing beyond the data
    /// side: the engine derives its round-start events from the poll
    /// coming back (see the link's session watch), because a game whose
    /// battle block is torn down between rounds answers `None` across
    /// exactly the boundary. The default reads nothing, for a game
    /// that hasn't wired telemetry up: no battle values, no round
    /// marks; the engine's wireless-drop match end still fires.
    fn core_poller(&self, player: usize) -> Box<dyn tango_match::telemetry::CorePoller<crate::Nds>> {
        let _ = player;
        Box::new(|_: &mut crate::Nds| None)
    }
}

/// A DS cartridge's engine support, as the engine-neutral backend its
/// registration holds — the counterpart of `tango-backend-mgba`'s
/// `GbaBackend`. No family table here: a DS link is one cart in two
/// consoles, so there is no crossplay seat to resolve.
pub struct DsBackend {
    support: &'static (dyn GameSupport + Send + Sync),
}

impl DsBackend {
    pub const fn new(support: &'static (dyn GameSupport + Send + Sync)) -> Self {
        DsBackend { support }
    }
}

impl tango_match::Backend for DsBackend {
    fn screen_layout(&self) -> tango_match::ScreenLayout {
        crate::link::screen_layout()
    }

    fn frame_timing(&self) -> tango_match::FrameTiming {
        // The DS frame clock: 280095 scanline-pair cycles against its
        // 16.756991 MHz half-rate tick — the exact rational behind
        // [`EXPECTED_FPS`](crate::link::EXPECTED_FPS).
        tango_match::FrameTiming {
            timescale: 16_756_991,
            frame_duration: 280_095,
        }
    }

    fn start(&self, config: tango_match::StartConfig) -> Result<tango_match::Match, tango_match::Error> {
        // Both consoles run the same cart, so the pair takes one image;
        // the saves still differ, since each player brings their own.
        let mut link = Link::new(config.roms[0], config.saves, config.rtc)
            .map_err(|e| tango_match::Error::Backend(Box::new(e)))?;

        self.support.prime(&mut link, config.match_type, None)?;

        // The session watch: the game's battle pollers, plus the two
        // lifecycle facts the engine derives itself (round starts off
        // the poll gate's edge, match end off the wireless teardown).
        // Armed only now, because priming is what brings the wireless
        // up in the first place.
        let lifecycle = tango_match::telemetry::LifecycleSink::new();
        let (telemetry, handle) = tango_match::telemetry::Telemetry::new(
            [self.support.core_poller(0), self.support.core_poller(1)],
            lifecycle.clone(),
        );
        link.watch(telemetry, lifecycle);

        // The rollback loop is the seam's — this engine contributes the
        // boot, not another copy of it.
        let mut match_ = tango_match::Match::new(link, config.local_player, config.present_delay)?;
        match_.set_telemetry(handle);
        Ok(match_)
    }

    fn start_solo(&self, config: tango_match::SoloConfig) -> Result<tango_match::Solo, tango_match::Error> {
        // One console, no pair: the game runs from power-on exactly as
        // a lone cart would, and the ride is the seam's.
        let rtc = config.rtc.unwrap_or_else(std::time::SystemTime::now);
        let console = crate::solo::SoloConsole::new(config.rom, config.save, rtc)
            .map_err(|e| tango_match::Error::Backend(Box::new(e)))?;
        Ok(tango_match::Solo::new(console))
    }

    fn open_replay(&self, config: tango_match::ReplayConfig) -> Result<tango_match::ReplaySet, tango_match::Error> {
        // `usage` stays `None`: no game on this engine derives usage
        // events (chip uses, buster counts) yet, so the stats pass
        // folds HP/rounds and lays the seek keyframes.
        let boot = Boot {
            support: self.support,
            rom: config.roms[0].clone(),
            saves: config.saves.clone(),
            rtc: config.rtc,
            match_type: config.match_type,
        };
        Ok(tango_match::ReplaySet::new(&config, None, boot))
    }
}

/// The engine's replay boot ([`tango_match::ReplayBoot`]): boot the
/// pair from the recording's saves and prime it exactly as the live
/// match did. The recording's input stream picks up where priming left
/// off, so the walk must reproduce the one the match ran — same saves,
/// same clock, same deterministic route.
struct Boot {
    support: &'static (dyn GameSupport + Send + Sync),
    /// The cart image — one, because both consoles run the same cart.
    rom: Vec<u8>,
    /// Per-seat saves as they stood when the match started.
    saves: [Vec<u8>; 2],
    /// The negotiated match clock, so the re-sim lands where the
    /// original did.
    rtc: std::time::SystemTime,
    /// The mode the recording was played in, so the walk picks the
    /// same one out of the game's menus.
    match_type: (u8, u8),
}

impl tango_match::ReplayBoot for Boot {
    fn boot(&self, observe: bool, cancel: &AtomicBool) -> Result<tango_match::BootedReplay, tango_match::Error> {
        let mut link = Link::new(
            &self.rom,
            [Some(self.saves[0].as_slice()), Some(self.saves[1].as_slice())],
            self.rtc,
        )
        .map_err(|e| tango_match::Error::Backend(Box::new(e)))?;
        self.support.prime(&mut link, self.match_type, Some(cancel))?;
        let handle = if observe {
            // The stats pass wants the game observed: the same session
            // watch a live match arms.
            let lifecycle = tango_match::telemetry::LifecycleSink::new();
            let (telemetry, handle) = tango_match::telemetry::Telemetry::new(
                [self.support.core_poller(0), self.support.core_poller(1)],
                lifecycle.clone(),
            );
            link.watch(telemetry, lifecycle);
            Some(handle)
        } else {
            // The display pair pays for no pollers.
            None
        };
        Ok(tango_match::BootedReplay {
            link: Box::new(link),
            telemetry: handle,
        })
    }
}

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
    /// a pure function of ROM/save/rtc — both peers run it on their own
    /// pairs and must reach identical state, because priming is
    /// simulation state and a difference here is a desync. Flipping
    /// `cancel` mid-walk fails it with
    /// [`Error::Cancelled`](tango_match::Error::Cancelled) — replay
    /// boots run on host worker threads whose teardown joins them.
    fn prime(&self, link: &mut Link, cancel: Option<&AtomicBool>) -> Result<(), tango_match::Error>;
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

    fn expected_fps(&self) -> f64 {
        crate::link::EXPECTED_FPS
    }

    fn start(&self, config: tango_match::StartConfig) -> Result<tango_match::Match, tango_match::Error> {
        // Both consoles run the same cart, so the pair takes one image;
        // the saves still differ, since each player brings their own.
        let mut link = Link::new(config.roms[0], config.saves, config.rtc)
            .map_err(|e| tango_match::Error::Backend(Box::new(e)))?;

        self.support.prime(&mut link, None)?;

        // The rollback loop is the seam's — this engine contributes the
        // boot, not another copy of it.
        tango_match::Match::new(link, config.local_player, config.present_delay)
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
        // No RAM pollers on this engine yet, so `usage` stays `None`:
        // the stats pass lays the seek keyframes and nothing else.
        let boot = Boot {
            support: self.support,
            rom: config.roms[0].clone(),
            saves: config.saves.clone(),
            rtc: config.rtc,
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
}

impl tango_match::ReplayBoot for Boot {
    // `observe` is ignored: this engine has no telemetry to wire yet,
    // so the stats pass gets no store and just lays keyframes.
    fn boot(&self, _observe: bool, cancel: &AtomicBool) -> Result<tango_match::BootedReplay, tango_match::Error> {
        let mut link = Link::new(
            &self.rom,
            [Some(self.saves[0].as_slice()), Some(self.saves[1].as_slice())],
            self.rtc,
        )
        .map_err(|e| tango_match::Error::Backend(Box::new(e)))?;
        self.support.prime(&mut link, Some(cancel))?;
        Ok(tango_match::BootedReplay {
            link: Box::new(link),
            telemetry: None,
        })
    }
}

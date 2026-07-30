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
    /// battle state. The default reads nothing, for a game that hasn't
    /// wired telemetry up.
    fn core_poller(&self, player: usize) -> Box<dyn tango_match::telemetry::CorePoller<crate::Nds>> {
        let _ = player;
        Box::new(|_: &mut crate::Nds| None)
    }

    /// The game's lifecycle, read from its RAM once per tick on console
    /// 0: where the match stands, as a [`Phase`] level. This engine's
    /// stand-in for the mgba families' trap anchors — a standing
    /// melonDS trap holds the console to the interpreter, so round and
    /// match boundaries are declared as instantaneous RAM facts instead
    /// of PC sites, and the telemetry collector turns the levels into
    /// events. Must be a pure function of console state: it runs on
    /// speculative ticks and again on their re-simulation. The default
    /// reads nothing: no round marks, and — because [`Phase::Over`] is
    /// the engine's only match-end signal — no automatic session end.
    ///
    /// [`Phase`]: tango_match::telemetry::Phase
    /// [`Phase::Over`]: tango_match::telemetry::Phase::Over
    fn phase_poller(&self) -> Option<Box<dyn FnMut(&mut crate::Nds) -> tango_match::telemetry::Phase + Send>> {
        None
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

        prime_dark(self.support, &mut link, config.match_type, None)?;
        let handle = observe(&mut link, self.support);

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
        // No game on this engine derives usage events (chip uses,
        // buster counts) yet, so a stats pass gets the inert fold —
        // HP, custom spans and outcomes still fold as usual.
        let usage = config.want_stats.then(tango_match::analysis::inert_usage_fold);
        let boot = Boot {
            support: self.support,
            rom: config.roms[0].clone(),
            saves: config.saves.clone(),
            rtc: config.rtc,
            match_type: config.match_type,
        };
        Ok(tango_match::ReplaySet::new(&config, usage, boot))
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
    fn boot(&self, want_stats: bool, cancel: &AtomicBool) -> Result<tango_match::BootedReplay, tango_match::Error> {
        let mut link = self.pair()?;
        prime_dark(self.support, &mut link, self.match_type, Some(cancel))?;
        // Session tick numbering starts after the walk, observed or
        // not (the walk drives the pair through the seam's tick, so it
        // counts otherwise). Captures then carry session ticks, which
        // is what lets the stats pass land its own pair on the display
        // pair's first one.
        link.zero_clock();
        // The stats pass wants the game observed, exactly as a live
        // match is; the display pair pays for no pollers.
        let handle = want_stats.then(|| observe(&mut link, self.support));
        Ok(tango_match::BootedReplay {
            link: Box::new(link),
            telemetry: handle,
        })
    }

    /// A bare pair for the stats pass to land on the display pair's
    /// primed capture ([`tango_match::ReplaySet::stats_reusing_playback`]),
    /// skipping the second priming walk. Sound on this engine because
    /// the walk leaves nothing a snapshot doesn't carry: its traps are
    /// all off again before `prime` returns (both processors handed
    /// back to the JIT), and everything else it changed is console
    /// state.
    fn boot_unprimed(
        &self,
        want_stats: bool,
        _cancel: &AtomicBool,
    ) -> Result<Option<tango_match::BootedReplay>, tango_match::Error> {
        let mut link = self.pair()?;
        let handle = want_stats.then(|| observe(&mut link, self.support));
        Ok(Some(tango_match::BootedReplay {
            link: Box::new(link),
            telemetry: handle,
        }))
    }
}

impl Boot {
    /// The pair at power-on, from the recording's own header.
    fn pair(&self) -> Result<Link, tango_match::Error> {
        Link::new(
            &self.rom,
            [Some(self.saves[0].as_slice()), Some(self.saves[1].as_slice())],
            self.rtc,
        )
        .map_err(|e| tango_match::Error::Backend(Box::new(e)))
    }
}

/// Run the walk with both framebuffers dark. Nothing displays a priming
/// pair, and compositing frames for it anyway is about a sixth of the
/// walk's wall clock. Emulated state is bit-identical either way — the
/// toggle skips only the framebuffer nobody reads — so peers and
/// recordings can't tell, whichever side of this change they're on.
/// Render comes back on before the pair is handed over: the boot leaves
/// the consoles as it found them, and per-seat visibility stays the
/// session's business.
fn prime_dark(
    support: &'static (dyn GameSupport + Send + Sync),
    link: &mut Link,
    match_type: (u8, u8),
    cancel: Option<&AtomicBool>,
) -> Result<(), tango_match::Error> {
    for player in 0..2 {
        link.console(player).set_render(false);
    }
    let walked = support.prime(link, match_type, cancel);
    for player in 0..2 {
        link.console(player).set_render(true);
    }
    walked
}

/// Arm a primed pair's telemetry: the game's battle pollers plus its
/// phase read, which carries the round and match lifecycle (see
/// [`GameSupport::phase_poller`]). Armed only after priming, so the
/// boot's screens predate the watch. Returns the handle the backend
/// installs on the match for the host to read.
fn observe(
    link: &mut Link,
    support: &'static (dyn GameSupport + Send + Sync),
) -> tango_match::telemetry::TelemetryHandle {
    let lifecycle = tango_match::telemetry::LifecycleSink::new();
    let (mut telemetry, handle) =
        tango_match::telemetry::Telemetry::new([support.core_poller(0), support.core_poller(1)], lifecycle);
    if let Some(phases) = support.phase_poller() {
        telemetry.set_phase_poller(phases);
    }
    link.set_telemetry(telemetry);
    handle
}

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
    /// The game half of a registration's
    /// [`sim_version`](tango_match::Backend::sim_version): bump it when
    /// this game's own support starts producing a different match out of
    /// the same inputs — where the walk hands off, what it writes on the
    /// way, where a round or a match is read to begin and end. The
    /// implementations carry a line per bump, because "which recordings
    /// did this cost, and what for" is only answerable if it was written
    /// down at the time.
    fn sim_version(&self) -> u16;

    /// Walk a booted pair into the game's link battle. The walk must be
    /// a pure function of ROM/save/rtc/`match_type`/`rng_seed` — both
    /// peers run it on their own pairs and must reach identical state,
    /// because priming is simulation state and a difference here is a
    /// desync. Flipping `cancel` mid-walk fails it with
    /// [`Error::Cancelled`](tango_match::Error::Cancelled) — replay
    /// boots run on host worker threads whose teardown joins them.
    ///
    /// `match_type` is the mode the two players agreed on, indexed as
    /// the registration's `match_types` table lists it. Which console
    /// takes the game's own host seat is the walk's business, not the
    /// caller's: the pair is symmetric, and both peers walk both
    /// consoles, so the walk simply assigns it.
    ///
    /// `rng_seed` is the negotiated match seed
    /// ([`StartConfig::rng_seed`](tango_match::StartConfig::rng_seed);
    /// a replay boot passes the recording's). The pair boots
    /// bit-identically every match, so a game whose randomness free-runs
    /// from power-on would deal the same draws match after match — the
    /// walk reseeds the game's own RNG state from values derived here,
    /// exactly as the mgba families' primers do.
    fn prime(
        &self,
        link: &mut Link,
        match_type: (u8, u8),
        rng_seed: [u8; 16],
        cancel: Option<&AtomicBool>,
    ) -> Result<(), tango_match::Error>;

    /// The telemetry reader for one console running this game. `player`
    /// is which console (and player) this poller answers for. Pure
    /// reads over that console's RAM, `None` while the game has no live
    /// battle state.
    ///
    /// Everything this game reports goes through the poller: the tick's
    /// levels come back as the observation, and the edges — chip uses,
    /// and this engine's round/match lifecycle — go into the sink as
    /// the poller catches them. A standing melonDS trap is what the
    /// mgba families use for lifecycle; this engine's anchors are RAM
    /// facts instead, so console 0's poller reads where the match
    /// stands and reports the transitions (see
    /// [`CorePoller`](tango_match::telemetry::CorePoller) for the
    /// rollback contract, including the scratch that keeps edge
    /// detection re-simulation-exact). The default reads nothing, for a
    /// game that hasn't wired telemetry up — which also means no
    /// automatic session end, the match-end report being the engine's
    /// only end signal.
    fn core_poller(&self, player: usize) -> Box<dyn tango_match::telemetry::CorePoller<crate::Nds>> {
        let _ = player;
        Box::new(|_: &mut crate::Nds| None)
    }

    /// Silence this console's battle BGM, for a host that asked for
    /// silent battles ([`StartConfig::disable_bgm`], and a replay's own
    /// [`ReplayConfig::disable_bgm`]). Called on both consoles of a
    /// freshly primed pair, once, before anyone watches it.
    ///
    /// The setting is **local**: peers are not told, and neither are
    /// recordings, so a muted console and an unmuted one have to stay
    /// the same *game* from here on. That rules out the mgba families'
    /// answer — skipping the battle-start music call — on this engine,
    /// where starting a sequence loads it off the cartridge and costs
    /// the game a frame it never gets back: a muted peer would run one
    /// frame ahead of an unmuted one for the rest of the match
    /// (measured, on both carts here — the game's own frame counter
    /// stands exactly one apart). What a cart does instead is turn its
    /// music down where the driver reads the volume, so the sequence
    /// still loads, still starts, and still costs what it always did.
    ///
    /// The default does nothing, for a cart with no lever found yet:
    /// the setting is then inert rather than unmeasured.
    ///
    /// [`StartConfig::disable_bgm`]: tango_match::StartConfig::disable_bgm
    /// [`ReplayConfig::disable_bgm`]: tango_match::ReplayConfig::disable_bgm
    fn silence_bgm(&self, nds: &mut crate::Nds) {
        let _ = nds;
    }

    /// Which of the console's screens this cart's *link battle* uses,
    /// in the mode the pair was primed into.
    ///
    /// A game that spends a netbattle on one screen names just that one
    /// and the rest follows — the pane, the video exports and the
    /// host's stylus area all come off the
    /// [`ScreenLayout`](tango_match::ScreenLayout) it produces, and a
    /// single-screen layout leaves the DS arrangement settings nothing
    /// to arrange. Per mode because a cart can differ between its own:
    /// BN5DS's Team Battle uses the touch screen where its plain
    /// subtypes don't.
    ///
    /// Played alone the cart always gets its whole console, so this is
    /// the link answer only. The default is the whole console, for a
    /// game whose battle uses the stylus throughout.
    fn pvp_screens(&self, match_type: (u8, u8)) -> crate::link::Screens {
        let _ = match_type;
        crate::link::Screens::BOTH
    }
}

/// Turn one SDAT sequence's volume down to nothing, wherever the
/// archive's INFO block happens to be living. Answers whether it
/// landed.
///
/// A DS cart's music is sequences in an SDAT archive, and the
/// archive's INFO block — one 12-byte record per sequence, its volume
/// at +6 — is copied into main RAM at sound init and read from there
/// each time a sequence starts. Zeroing that byte *before* the start
/// is therefore a mute the sound driver applies for itself: the
/// request, the file load and the start all still happen and still
/// cost what they cost, and only the mixing comes out silent. Zeroing
/// it after a sequence has started does nothing — the volume is read
/// at the start, which is why this is a boot-time write and not a
/// standing trap.
///
/// `record` is the record as the cart's own ROM carries it. The
/// block's address is a heap allocation rather than a fixed one, so
/// the record is found rather than named: it is matched on the fields
/// that identify it — the file it plays, its bank, its player — with
/// the volume byte itself skipped, so that a pair muted once is still
/// matched if it is muted again.
pub fn mute_sequence(nds: &mut crate::Nds, record: [u8; 12]) -> bool {
    /// Everything in the record except the volume at +6 and the two
    /// bytes of padding at +10, which say nothing about which sequence
    /// this is.
    const IDENTIFYING: [usize; 9] = [0, 1, 2, 3, 4, 5, 7, 8, 9];
    let found = {
        let ram = nds.main_ram();
        ram.windows(record.len())
            .position(|w| IDENTIFYING.iter().all(|&i| w[i] == record[i]))
    };
    let Some(offset) = found else { return false };
    nds.write8(0x0200_0000 + offset as u32 + 6, 0);
    true
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

/// The engine half of every DS registration's
/// [`sim_version`](tango_match::Backend::sim_version) — this crate and
/// melonDS under it, which every cart here shares. Bump it when a match
/// on this engine stops running the way it used to for reasons that
/// have nothing to do with which cart it is: what a tick of emulated
/// time is and how it gets filled, whether execution is compiled or
/// interpreted, what a savestate carries, what the cartridge and
/// wireless servers cost, what the BIOS answers.
///
/// Every one of those has already happened, and each is written into
/// *both* games' halves below because there was nowhere else to put it
/// — the same emulator change spelled twice, once as EXE OSS 2 and once
/// as BN5DS 5. The next one is spelled here instead, once, and costs
/// exactly what it always did: every DS recording, and every DS peer on
/// an older build.
///
/// 2: the last of those. FreeBIOS's sqrt SWI takes an unsigned integer
/// and compared it signed, so every argument with bit 31 set failed the
/// very first comparison and the call came back 0. The blob the console
/// actually boots is rebuilt from the fixed source, so a cart that ever
/// takes the root of a large number gets a different answer here than
/// it did on any build before this one.
const BACKEND_SIM_VERSION: u16 = 2;

impl tango_match::Backend for DsBackend {
    fn sim_version(&self) -> u32 {
        ((BACKEND_SIM_VERSION as u32) << 16) | self.support.sim_version() as u32
    }

    fn screen_layout(&self, mode: tango_match::SessionMode) -> tango_match::ScreenLayout {
        match mode {
            // What `start`/`open_replay` set on the pair below.
            tango_match::SessionMode::PvP { match_type } => self.support.pvp_screens(match_type),
            // A cart played alone gets the console it shipped for.
            tango_match::SessionMode::Solo => crate::link::Screens::BOTH,
        }
        .layout()
    }

    fn keys_mask(&self) -> u32 {
        crate::link::KEYS_MASK
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
        link.set_screens(self.support.pvp_screens(config.match_type));

        prime_dark(
            self.support,
            &mut link,
            config.match_type,
            config.rng_seed,
            config.cancel,
        )?;
        silence(&mut link, self.support, config.disable_bgm);
        let handle = observe(&mut link, self.support);

        // The rollback loop is the seam's — this engine contributes the
        // boot, not another copy of it.
        let mut match_ = tango_match::Match::new(link, config.local_player, config.present_delay, config.audio)?;
        match_.set_telemetry(handle);
        Ok(match_)
    }

    fn start_solo(&self, config: tango_match::SoloConfig) -> Result<tango_match::Solo, tango_match::Error> {
        // One console, no pair: the game runs from power-on exactly as
        // a lone cart would, and the ride is the seam's.
        let rtc = config.rtc.unwrap_or_else(std::time::SystemTime::now);
        let console = crate::solo::SoloConsole::new(config.rom, config.save, rtc)
            .map_err(|e| tango_match::Error::Backend(Box::new(e)))?;
        Ok(tango_match::Solo::new(console, config.audio))
    }

    fn open_replay(&self, config: tango_match::ReplayConfig) -> Result<tango_match::ReplaySet, tango_match::Error> {
        let boot = Boot {
            support: self.support,
            rom: config.roms[0].clone(),
            saves: config.saves.clone(),
            rtc: config.rtc,
            match_type: config.match_type,
            rng_seed: config.rng_seed,
            disable_bgm: config.disable_bgm,
        };
        Ok(tango_match::ReplaySet::new(&config, boot))
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
    /// The recording's match seed, so the re-primed walk seeds the
    /// game's rngs exactly as the live one did.
    rng_seed: [u8; 16],
    /// Whether this viewer asked for the music off. Applied to every
    /// pair this boot makes — the playback pair AND the stats pair,
    /// which lands on the playback pair's own captures and would
    /// otherwise be re-simulating a differently-silenced console.
    disable_bgm: bool,
}

impl tango_match::ReplayBoot for Boot {
    fn boot(&self, want_stats: bool, cancel: &AtomicBool) -> Result<tango_match::BootedReplay, tango_match::Error> {
        let mut link = self.pair()?;
        prime_dark(self.support, &mut link, self.match_type, self.rng_seed, Some(cancel))?;
        silence(&mut link, self.support, self.disable_bgm);
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
    /// skipping the second priming walk.
    ///
    /// Pure construction: the pair comes back at power-on, having run
    /// nothing, and the caller's restore is what puts it where it
    /// belongs.
    ///
    /// It used to run one throwaway frame first, on the theory that a
    /// console which has never run one is not equivalent to a console
    /// that has. What was really being papered over was melonDS keeping
    /// state a savestate did not carry — the CPUs' fetch-timing scratch,
    /// and the derived timing tables a load only rebuilt when the
    /// registers it could see had changed — which the JIT then baked
    /// into compiled blocks. The frame moved the symptom around without
    /// fixing it, and a landed pair still parted ways on screen about a
    /// minute into a recording. With that carried properly, landing
    /// straight out of construction is exact for as long as bn5ds's
    /// `landing_probe` runs a recording (`fresh` mode).
    fn boot_unprimed(&self, want_stats: bool) -> Result<tango_match::BootedReplay, tango_match::Error> {
        let mut link = self.pair()?;
        silence(&mut link, self.support, self.disable_bgm);
        let handle = want_stats.then(|| observe(&mut link, self.support));
        Ok(tango_match::BootedReplay {
            link: Box::new(link),
            telemetry: handle,
        })
    }
}

impl Boot {
    /// The pair at power-on, from the recording's own header. Composes
    /// the screens the live match did, so a replay of it fills the same
    /// pane and exports at the same size.
    fn pair(&self) -> Result<Link, tango_match::Error> {
        let mut link = Link::new(
            &self.rom,
            [Some(self.saves[0].as_slice()), Some(self.saves[1].as_slice())],
            self.rtc,
        )
        .map_err(|e| tango_match::Error::Backend(Box::new(e)))?;
        link.set_screens(self.support.pvp_screens(self.match_type));
        Ok(link)
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
    rng_seed: [u8; 16],
    cancel: Option<&AtomicBool>,
) -> Result<(), tango_match::Error> {
    for player in 0..2 {
        link.console(player).set_render(false);
    }
    let walked = support.prime(link, match_type, rng_seed, cancel);
    for player in 0..2 {
        link.console(player).set_render(true);
    }
    walked
}

/// Turn both consoles' battle music down, for a host that asked for
/// silent battles (see [`GameSupport::silence_bgm`]).
///
/// Both, not just the seat the host watches: the pair is one
/// simulation, its two consoles are snapshotted and restored together,
/// and a session mixes what both produce.
fn silence(link: &mut Link, support: &'static (dyn GameSupport + Send + Sync), disable_bgm: bool) {
    if !disable_bgm {
        return;
    }
    for player in 0..2 {
        support.silence_bgm(link.console(player));
    }
}

/// Arm a primed pair's telemetry: the game's pollers, which carry both
/// the battle levels and — on console 0 — the poll-derived round and
/// match lifecycle (see [`GameSupport::core_poller`]). Armed only after
/// priming, so the boot's screens predate the watch. Returns the handle
/// the backend installs on the match for the host to read.
fn observe(
    link: &mut Link,
    support: &'static (dyn GameSupport + Send + Sync),
) -> tango_match::telemetry::TelemetryHandle {
    let events = tango_match::telemetry::EventSink::new();
    let (telemetry, handle) =
        tango_match::telemetry::Telemetry::new([support.core_poller(0), support.core_poller(1)], events);
    link.set_telemetry(telemetry);
    handle
}

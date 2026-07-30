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
//! Replay playback and offline re-analysis re-run the same walk
//! ([`boot_pair`]) — priming is a pure function of ROM/save/rtc, so a
//! re-simulation reaches the state the live match started from.
//!
//! [`Backend::start`]: tango_match::Backend::start
//!
//! (mgba-rollback links go up to four players, but every game tango
//! supports is a two-player link battle, so this engine is two-player
//! throughout: the pair of cores IS the link.)

use std::sync::atomic::{AtomicBool, Ordering};

use mgba_rollback::{LinkOptions, Peripheral, SideOptions};

use tango_match::telemetry::{EventSink, Telemetry};
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
    fn seats(
        &self,
        peer_rom: tango_match::PeerRom,
        local_player: usize,
    ) -> [&'static (dyn GameSupport + Send + Sync); 2] {
        let mut seats = [self.local, self.peer(peer_rom)];
        if local_player == 1 {
            seats.swap(0, 1);
        }
        seats
    }
}

impl tango_match::Backend for GbaBackend {
    fn screen_layout(&self) -> tango_match::ScreenLayout {
        crate::link::screen_layout()
    }

    fn frame_timing(&self) -> tango_match::FrameTiming {
        // The GBA frame clock: 280896 cycles at 2^24 Hz — the exact
        // rational behind [`EXPECTED_FPS`](crate::link::EXPECTED_FPS).
        tango_match::FrameTiming {
            timescale: 16_777_216,
            frame_duration: 280_896,
        }
    }

    /// Boot the pair, prime both games to their link battle, and start
    /// the rollback session. Priming runs identically on both peers (it
    /// is a pure function of ROM/save/rtc), so both reach the same
    /// state before the session — and therefore the same session
    /// initial state.
    fn start(&self, config: tango_match::StartConfig) -> Result<tango_match::Match, tango_match::Error> {
        assert!(config.local_player < 2);
        let support = self.seats(config.peer_rom, config.local_player);

        let (mut pair, events) = boot_pair(
            [config.roms[0].to_vec(), config.roms[1].to_vec()],
            [
                config.saves[0].unwrap_or_default().to_vec(),
                config.saves[1].unwrap_or_default().to_vec(),
            ],
            support.map(|s| s as &dyn GameSupport),
            &PrimeConfig {
                match_type: config.match_type,
                rng_seed: config.rng_seed,
                disable_bgm: config.disable_bgm,
            },
            config.rtc,
            true,
            None,
        )?;

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

        let (telemetry, handle) = Telemetry::new([support[0].core_poller(0), support[1].core_poller(1)], events);
        let mut match_ = tango_match::Match::new(
            crate::link::Link::new(pair, Some(telemetry)),
            config.local_player,
            config.present_delay,
        )?;
        match_.set_telemetry(handle);
        Ok(match_)
    }

    fn start_solo(&self, config: tango_match::SoloConfig) -> Result<tango_match::Solo, tango_match::Error> {
        Ok(tango_match::Solo::new(
            crate::solo::SoloConsole::new(config.rom, config.save, config.rtc).map_err(tango_match::Error::from)?,
        ))
    }

    fn open_replay(&self, config: tango_match::ReplayConfig) -> Result<tango_match::ReplaySet, tango_match::Error> {
        let support = self.seats(config.peer_rom, config.local_player);
        let boot = Boot {
            roms: config.roms.clone(),
            saves: config.saves.clone(),
            support,
            prime: PrimeConfig {
                match_type: config.match_type,
                rng_seed: config.rng_seed,
                disable_bgm: config.disable_bgm,
            },
            rtc: config.rtc,
        };
        Ok(tango_match::ReplaySet::new(&config, boot))
    }
}

/// Cap on priming ticks before we give up bringing the games to their
/// link battle. Real bring-up is ~470 ticks (BN6); this is generous
/// headroom for slower families without hanging forever on a wedge.
const MAX_PRIME_TICKS: u32 = 3600;

/// Build a pair and prime both games to their link battle — the walk
/// every simulation of a match starts with: live netplay, replay
/// playback, and offline re-analysis ([`crate::analysis::analyze`]).
///
/// The traps do all the driving (each core's walk its own menu state
/// machine); the pads stay idle throughout. Priming is done when both
/// games' own battle-start code has fired. With `render` unset both
/// cores skip rasterization, for a pass nothing watches. Flipping
/// `cancel` mid-walk fails it with [`Error::Cancelled`](crate::Error).
///
/// The returned [`EventSink`] is the one the primer traps report
/// round lifecycle into — hand it to [`Telemetry::new`] to observe the
/// simulation, or drop it to leave it a write-only stub.
pub(crate) fn boot_pair(
    roms: [Vec<u8>; 2],
    saves: [Vec<u8>; 2],
    support: [&dyn GameSupport; 2],
    prime: &PrimeConfig,
    rtc: std::time::SystemTime,
    render: bool,
    cancel: Option<&AtomicBool>,
) -> Result<(mgba_rollback::Link, EventSink), crate::Error> {
    let (mut pair, events, primed) = assemble_pair(roms, saves, support, prime, rtc, render)?;

    // Prime both cores to their link battle.
    let mut prime_ticks = 0;
    while !(primed[0].is_set() && primed[1].is_set()) {
        if prime_ticks >= MAX_PRIME_TICKS {
            return Err(crate::Error::PrimeTimeout(MAX_PRIME_TICKS));
        }
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(crate::Error::Cancelled);
        }
        pair.tick(&[0, 0]);
        prime_ticks += 1;
    }
    log::info!("pvp: primed to link battle in {prime_ticks} ticks");
    Ok((pair, events))
}

/// Build the pair with its primer traps installed but the walk not yet
/// run — where [`boot_pair`]'s priming loop starts from, and what a
/// caller about to restore an already-primed capture takes as is.
fn assemble_pair(
    roms: [Vec<u8>; 2],
    saves: [Vec<u8>; 2],
    support: [&dyn GameSupport; 2],
    prime: &PrimeConfig,
    rtc: std::time::SystemTime,
    render: bool,
) -> Result<(mgba_rollback::Link, EventSink, [crate::PrimedLatch; 2]), crate::Error> {
    crate::install_logger();
    let [rom0, rom1] = roms;
    let [save0, save1] = saves;
    let mut pair = mgba_rollback::Link::with_options(LinkOptions {
        sides: vec![
            SideOptions {
                rom: rom0,
                save: Some(save0),
            },
            SideOptions {
                rom: rom1,
                save: Some(save1),
            },
        ],
        rtc: Some(rtc),
        peripheral: Peripheral::Cable,
    })?;
    if !render {
        pair.set_frameskip(0, i32::MAX);
        pair.set_frameskip(1, i32::MAX);
    }

    let events = EventSink::new();
    let primed = [crate::PrimedLatch::new(), crate::PrimedLatch::new()];
    // The cores own their primer traps (see [`mgba_rollback::Link::set_traps`]):
    // core teardown walks the trap component, so the traps must live
    // exactly as long as their cores. They stay installed for the
    // pair's life — inert once primed, since their boot/menu addresses
    // never execute in battle.
    pair.set_traps(0, support[0].primer_traps(prime, 0, &events, &primed[0]));
    pair.set_traps(1, support[1].primer_traps(prime, 1, &events, &primed[1]));
    Ok((pair, events, primed))
}

/// The engine's replay boot ([`tango_match::ReplayBoot`]): prime a
/// pair per the recording's own header, exactly as the live match did,
/// and hand it over as the seam's link — with the game's telemetry
/// wired when the stats pass asks for it. This is all the engine
/// contributes to replays; the machinery above the boot is
/// [`tango_match::ReplaySet`]'s.
struct Boot {
    /// Per-seat images as the recording stood, in absolute player
    /// order (core 0 runs player 0's game).
    roms: [Vec<u8>; 2],
    saves: [Vec<u8>; 2],
    support: [&'static (dyn GameSupport + Send + Sync); 2],
    prime: PrimeConfig,
    rtc: std::time::SystemTime,
}

impl tango_match::ReplayBoot for Boot {
    fn boot(&self, observe: bool, cancel: &AtomicBool) -> Result<tango_match::BootedReplay, tango_match::Error> {
        let (pair, events) = boot_pair(
            self.roms.clone(),
            self.saves.clone(),
            self.support.map(|s| s as &dyn GameSupport),
            &self.prime,
            self.rtc,
            true,
            Some(cancel),
        )
        .map_err(tango_match::Error::from)?;
        Ok(self.wrap(pair, observe.then_some(events)))
    }

    /// A bare pair for the stats pass to land on the display pair's
    /// primed capture ([`tango_match::ReplaySet::stats_reusing_playback`]),
    /// skipping the second priming walk. Sound on this engine because
    /// the walk leaves nothing a snapshot doesn't carry: the primer
    /// traps stay installed either way (installed here too, since they
    /// double as the round-lifecycle anchors), the walk's pokes are
    /// core state, and the lockstep blobs ride in the snapshot.
    fn boot_unprimed(&self, observe: bool) -> Result<tango_match::BootedReplay, tango_match::Error> {
        let (pair, events, _primed) = assemble_pair(
            self.roms.clone(),
            self.saves.clone(),
            self.support.map(|s| s as &dyn GameSupport),
            &self.prime,
            self.rtc,
            true,
        )
        .map_err(tango_match::Error::from)?;
        // The capture this pair is about to land on is round 1 already
        // started — the walk's handoff fired on the pair that took it,
        // leaving the sink latched for `Telemetry::new`'s baseline
        // `Started` at tick 0. Reproduce the latch, since no walk runs
        // here to fire it. The primed latches stay unset: only the
        // priming loop reads them, and the walk traps gate on game
        // state a battle capture leaves inert.
        events.round_started();
        Ok(self.wrap(pair, observe.then_some(events)))
    }
}

impl Boot {
    /// The booted pair as the seam takes it — observed (pollers + the
    /// trap-fed lifecycle sink) when the stats pass asked for it; the
    /// display pair passes `None`, paying for no pollers and leaving
    /// its sink a write-only stub.
    fn wrap(&self, pair: mgba_rollback::Link, events: Option<EventSink>) -> tango_match::BootedReplay {
        let (telemetry, handle) = match events {
            Some(events) => {
                let (telemetry, handle) = Telemetry::new(
                    [self.support[0].core_poller(0), self.support[1].core_poller(1)],
                    events,
                );
                (Some(telemetry), Some(handle))
            }
            None => (None, None),
        };
        tango_match::BootedReplay {
            link: Box::new(crate::Link::new(pair, telemetry)),
            telemetry: handle,
        }
    }
}

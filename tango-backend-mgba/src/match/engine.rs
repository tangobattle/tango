//! Booting a live SIO match: build the two-player pair, prime both
//! games to their link battle, wire the RAM-poll telemetry, and start
//! the seam's rollback [`Match`](tango_match::Match) over it.
//!
//! The rollback loop itself is `tango-match`'s — one engine for the
//! cable and the DS's wireless alike. What is mgba-shaped here is
//! everything before the session's first tick: the boot, the primer
//! traps, and the audio bring-up.
//!
//! (mgba-rollback links go up to four players, but every game tango
//! supports is a two-player link battle, so this engine is two-player
//! throughout: the pair of cores IS the link.)

use mgba_rollback::{LinkOptions, Peripheral, SideOptions};

use crate::r#match::telemetry::Telemetry;
use crate::{GameSupport, PrimeConfig};

/// Cap on priming ticks before we give up bringing the games to their
/// link battle. Real bring-up is ~470 ticks (BN6); this is generous
/// headroom for slower families without hanging forever on a wedge.
const MAX_PRIME_TICKS: u32 = 3600;

/// Everything [`start`] needs. Both peers pass identical values
/// except [`local_player`](Self::local_player): the pair is symmetric, so
/// core 0 always runs `player0`'s game and core 1 `player1`'s, on both
/// peers.
pub struct MatchConfig<'a> {
    /// Per-core ROM images (already patched). `roms[i]` runs on core `i`.
    pub roms: [Vec<u8>; 2],
    /// Per-core SRAM images.
    pub saves: [Vec<u8>; 2],
    /// Per-core game support (priming + telemetry). `support[i]` drives core `i`.
    pub support: [&'a dyn GameSupport; 2],
    /// Link-battle mode selection, passed to both games' primers.
    pub match_type: (u8, u8),
    /// The negotiated match seed, for the primers' per-core RNG reseed
    /// (see [`PrimeConfig::rng_seed`](crate::PrimeConfig)).
    pub rng_seed: [u8; 16],
    /// The negotiated match clock, pinned into both carts' RTC.
    pub rtc: std::time::SystemTime,
    /// Which core this peer controls (0 or 1).
    pub local_player: usize,
    /// How many ticks behind the local frontier to present. Purely local.
    pub present_delay: u32,
    /// Silence the battle BGM (see [`PrimeConfig::disable_bgm`]). Purely
    /// local.
    pub disable_bgm: bool,
}

/// Boot the pair, prime both games to their link battle, and start the
/// rollback session. Priming runs identically on both peers (it is a
/// pure function of ROM/save/rtc), so both reach the same state before
/// the session — and therefore the same session initial state.
pub fn start(config: MatchConfig) -> Result<tango_match::Match, tango_match::Error> {
    let MatchConfig {
        roms,
        saves,
        support,
        match_type,
        rng_seed,
        rtc,
        local_player,
        present_delay,
        disable_bgm,
    } = config;
    assert!(local_player < 2);
    let [rom0, rom1] = roms;
    let [save0, save1] = saves;

    crate::install_logger();
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
    })
    .map_err(crate::Error::from)?;

    let prime_config = PrimeConfig {
        match_type,
        rng_seed,
        disable_bgm,
    };
    let lifecycle = crate::r#match::telemetry::LifecycleSink::new();
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
        local_player,
        present_delay,
    )?;
    match_.set_telemetry(handle);
    Ok(match_)
}

//! Offline re-analysis: boot a pair, replay recorded input through it,
//! and fold the telemetry with the same code the live session uses — so
//! live stats and re-analysis stay byte-equivalent.

use mgba_rollback::session::TickObserver;
pub use tango_match::analysis::*;

// ---------------------------------------------------------------------------
// Replay re-analysis: linear re-simulation + the telemetry -> stats fold
// shared with the live session (so live stats and offline re-analysis stay
// byte-equivalent).

use crate::r#match::telemetry::{self, Telemetry};
use crate::{GameSupport, PrimeConfig};

/// Cap on priming ticks, mirroring the live engine's bound.
const MAX_PRIME_TICKS: u32 = 3600;

/// Everything [`analyze`] needs. All fields are in **absolute** player
/// order (core 0 runs player 0's game), which is how the caller should
/// orient the replay's local/remote pairs using
/// `tango_replay::Replay::local_player_index`.
pub struct AnalyzeConfig<'a> {
    pub roms: [Vec<u8>; 2],
    pub saves: [Vec<u8>; 2],
    pub support: [&'a dyn GameSupport; 2],
    pub match_type: (u8, u8),
    pub rng_seed: [u8; 16],
    pub rtc: std::time::SystemTime,
    /// Which side the stats should be from the perspective of.
    pub local_player: usize,
    /// `[p0, p1]` joypad pairs, one per pair tick from session start.
    pub inputs: &'a [[u32; 2]],
    /// Chip-report semantics + buster counting for the local game (see
    /// [`Hooks::chip_semantics`](crate::hooks::Hooks::chip_semantics)).
    pub chip_semantics: crate::r#match::analysis::ChipSemantics,
    pub counts_buster: bool,
}

/// Re-simulate an SIO replay and fold its telemetry into [`MatchStats`].
/// `on_progress` is called once per simulated tick with `(done, total)`
/// and the in-flight builder; flipping `cancel` aborts with an error and
/// nothing partial.
pub fn analyze(
    config: AnalyzeConfig<'_>,
    on_progress: &mut dyn FnMut(u32, u32, &StatsBuilder),
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<MatchStats, crate::Error> {
    let AnalyzeConfig {
        roms,
        saves,
        support,
        match_type,
        rng_seed,
        rtc,
        local_player,
        inputs,
        chip_semantics,
        counts_buster,
    } = config;
    let [rom0, rom1] = roms;
    let [save0, save1] = saves;

    crate::install_logger();
    let mut pair = mgba_rollback::Link::with_options(mgba_rollback::LinkOptions {
        sides: vec![
            mgba_rollback::SideOptions {
                rom: rom0,
                save: Some(save0),
            },
            mgba_rollback::SideOptions {
                rom: rom1,
                save: Some(save1),
            },
        ],
        rtc: Some(rtc),
        peripheral: mgba_rollback::Peripheral::Cable,
    })?;
    // Nothing reads the pixels — skip rasterization on both cores.
    pair.set_frameskip(0, i32::MAX);
    pair.set_frameskip(1, i32::MAX);

    let prime_config = PrimeConfig {
        match_type,
        rng_seed,
        // Presentation-only (audio is never read here), and gameplay-neutral
        // either way — see `PrimeConfig::disable_bgm`.
        disable_bgm: false,
    };
    let lifecycle = crate::r#match::telemetry::LifecycleSink::new();
    let primed = [crate::PrimedLatch::new(), crate::PrimedLatch::new()];
    // Cores own their primer traps — see [`mgba_rollback::Link::set_traps`]
    // for why any other ownership dangles at core teardown.
    pair.set_traps(0, support[0].primer_traps(&prime_config, 0, &lifecycle, &primed[0]));
    pair.set_traps(1, support[1].primer_traps(&prime_config, 1, &lifecycle, &primed[1]));

    let mut prime_ticks = 0;
    while !(primed[0].is_set() && primed[1].is_set()) {
        if prime_ticks >= MAX_PRIME_TICKS {
            return Err(crate::Error::PrimeTimeout(MAX_PRIME_TICKS));
        }
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(crate::Error::Cancelled);
        }
        pair.tick(&[0, 0]);
        prime_ticks += 1;
    }

    let (mut observer, store) = crate::r#match::telemetry::Telemetry::new([support[0].core_poller(0), support[1].core_poller(1)], lifecycle);
    let mut builder = StatsBuilder::new(chip_semantics, counts_buster);
    let total = inputs.len() as u32;
    for (i, &keys) in inputs.iter().enumerate() {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(crate::Error::Cancelled);
        }
        let tick = i as u32 + 1;
        pair.tick(&keys);
        // Everything is final on a linear re-sim — fold as we go.
        let obs0 = observer.poll(0, pair.core_mut(0));
        let obs1 = observer.poll(1, pair.core_mut(1));
        observer.observe(obs0, obs1, tick);
        let (samples, events) = store.lock().unwrap().drain_confirmed(tick);
        fold_confirmed(&mut builder, local_player, samples, events, &mut |t| {
            (t == tick).then_some(keys)
        });
        on_progress(tick, total, &builder);
    }

    Ok(builder.finish())
}


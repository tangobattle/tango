//! Match telemetry and the statistics it folds into, as this engine
//! drives them.
//!
//! Both halves are the seam's — the collector and the fold are the
//! same arithmetic for any console. What is engine-specific is knowing
//! where the two cores are: the live path drives the collector from
//! inside [`crate::link::Link`]'s tick, and the offline paths
//! ([`analyze`], the probe harnesses) drive it directly off the pair
//! they step ([`observe_pair`]). Re-analysis folds with the same code
//! the live session uses, so live stats and re-analysis stay
//! byte-equivalent.

pub use tango_match::analysis::*;

use tango_match::telemetry::Telemetry;

/// Drive the collector one tick off a bare pair — what the live link
/// does internally, for the offline paths (replay re-analysis, the
/// probe harnesses) that step a pair by hand.
pub fn observe_pair(telemetry: &mut Telemetry<mgba::core::Core>, pair: &mut mgba_rollback::Link, tick: u32) {
    let obs0 = telemetry.poll(0, pair.core_mut(0));
    let obs1 = telemetry.poll(1, pair.core_mut(1));
    telemetry.observe(obs0, obs1, tick);
}

// ---------------------------------------------------------------------------
// Replay re-analysis: linear re-simulation + the telemetry -> stats fold
// shared with the live session (so live stats and offline re-analysis stay
// byte-equivalent).

use crate::{GameSupport, PrimeConfig};

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
    } = config;
    let (mut pair, events) = crate::backend::boot_pair(
        roms,
        saves,
        support,
        &PrimeConfig {
            match_type,
            rng_seed,
            // Presentation-only (audio is never read here), and gameplay-neutral
            // either way — see `PrimeConfig::disable_bgm`.
            disable_bgm: false,
        },
        rtc,
        // Nothing reads the pixels — skip rasterization on both cores.
        false,
        Some(cancel),
    )?;

    let (mut observer, store) = Telemetry::new([support[0].core_poller(0), support[1].core_poller(1)], events);
    let mut builder = StatsBuilder::new();
    let total = inputs.len() as u32;
    for (i, &keys) in inputs.iter().enumerate() {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(crate::Error::Cancelled);
        }
        let tick = i as u32 + 1;
        pair.tick(&keys);
        // Everything is final on a linear re-sim — fold as we go.
        observe_pair(&mut observer, &mut pair, tick);
        let (samples, events) = store.lock().unwrap().drain_confirmed(tick);
        fold_confirmed(&mut builder, local_player, samples, events);
        on_progress(tick, total, &builder);
    }

    Ok(builder.finish())
}


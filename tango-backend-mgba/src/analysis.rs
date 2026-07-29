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
    // No phase read: this engine's round and match lifecycle is
    // trap-driven.
    telemetry.observe(obs0, obs1, None, tick);
}

// ---------------------------------------------------------------------------
// The standard usage-event folds. The seam ([`UsageFold`]) lives with the
// stats machinery in `tango_match`; the building blocks live here, next to
// [`GameSupport::usage_fold`], because what a chip report means is game
// knowledge — most games share these two, and the ones that don't (BCC,
// vanilla exe45) bring their own from their gamesupport.

/// Low bits of a chip report that carry the chip id; the rest is a
/// per-game tag (the loaded-chip contract's fire-sequence counter — see
/// [`loaded_chip_uses`]).
pub const CHIP_ID_MASK: u16 = 0x0fff;

/// The loaded-chip decode — the contract the mainline BN families
/// report: each report is the player's loaded chip as `id | (seq << 12)`,
/// [`NO_CHIP`] when none — the id the player will use next, tagged with
/// a fire-sequence counter so back-to-back duplicate picks still produce
/// a visible transition per use (bn5/bn6 report a raw cell with seq 0 —
/// no counter is known for them). A value departing = that chip being
/// used, EXCEPT the first departure after each custom close, which is
/// the new selection landing on top of whatever was left (see the
/// per-game chip-block docs).
pub fn loaded_chip_uses(samples: &[RoundSample], custom: &[(u32, u32)]) -> [Vec<(u32, u16)>; 2] {
    let mut chip_uses: [Vec<(u32, u16)>; 2] = [vec![], vec![]];
    for side in 0..2 {
        let mut prev_chip = NO_CHIP;
        // Index of the next custom span whose close hasn't had its
        // selection-load transition yet.
        let mut next_load_span = 0usize;
        for s in samples {
            let chip = s.chips[side];
            if chip != prev_chip {
                // Skip spans whose load window has fully passed
                // (a side that picked nothing produces no load
                // transition).
                while next_load_span < custom.len()
                    && custom.get(next_load_span + 1).is_some_and(|&(s2, _)| s.tick >= s2)
                {
                    next_load_span += 1;
                }
                // A load consumes the pending span: the selection
                // can land while the span is still open (bn6
                // writes the block mid-pick; bn1-3's local-only
                // spans outlast the other side's commit) or right
                // after its close. Anything else departing a real
                // chip is a use.
                let is_load = custom.get(next_load_span).is_some_and(|&(s0, _)| s.tick >= s0);
                if is_load {
                    next_load_span += 1;
                } else if next_load_span > 0 && prev_chip != NO_CHIP {
                    // next_load_span == 0 means no selection has
                    // landed yet — the cell still holds round-init
                    // garbage (bn5 inits it to 0 before first
                    // flipping to the sentinel), which can't be a
                    // real use.
                    chip_uses[side].push((s.tick, prev_chip & CHIP_ID_MASK));
                }
                prev_chip = chip;
            }
        }
    }
    chip_uses
}

/// The standard buster fold: B-press edges outside the custom screen,
/// per side. A game whose B is not a buster (vanilla exe45 — B is a
/// menu key there) simply leaves it out of its fold.
pub fn buster_presses(samples: &[RoundSample]) -> [Vec<u32>; 2] {
    let mut buster: [Vec<u32>; 2] = [vec![], vec![]];
    for side in 0..2 {
        let b_bit = if side == 0 {
            tango_match::battle::BUTTON_LOCAL_B
        } else {
            tango_match::battle::BUTTON_REMOTE_B
        };
        let mut prev_buttons = 0u8;
        for s in samples {
            if !s.custom && s.buttons & b_bit != 0 && prev_buttons & b_bit == 0 {
                buster[side].push(s.tick);
            }
            prev_buttons = s.buttons;
        }
    }
    buster
}

/// Both standard folds together — the [`GameSupport::usage_fold`]
/// default: loaded-chip uses plus buster presses.
pub fn standard_usage_fold() -> UsageFold {
    Box::new(|samples, custom| UsageEvents {
        chip_uses: loaded_chip_uses(samples, custom),
        buster: buster_presses(samples),
    })
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
    /// The local game's usage-event fold (see
    /// [`GameSupport::usage_fold`]).
    pub usage: UsageFold,
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
        usage,
    } = config;
    let (mut pair, lifecycle) = crate::backend::boot_pair(
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

    let (mut observer, store) = Telemetry::new([support[0].core_poller(0), support[1].core_poller(1)], lifecycle);
    let mut builder = StatsBuilder::new(usage);
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
        fold_confirmed(&mut builder, local_player, samples, events, &mut |t| {
            (t == tick).then_some(keys)
        });
        on_progress(tick, total, &builder);
    }

    Ok(builder.finish())
}


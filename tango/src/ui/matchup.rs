//! The stats-to-chart cooking for the match-analysis HP graph: the
//! engine's round verdicts are restated as the toolkit's
//! [`RoundOutcome`] and chip ids resolve through a loaded save's baked
//! [`chips`](tango_gamesupport::LoadedSave::chips) table, so
//! `tango_ui::widgets::hp_match_graph` can stay game- and engine-free.
//! Lives here because it is the one place that speaks both the engine's
//! stats and the toolkit's chart shapes, and it needs no more of a save
//! than that public table. Re-exported into [`crate::ui::widgets`] so
//! call sites read the same as the toolkit's own widgets.

use tango_ui::widgets::*;

/// Cook one side's chip-use events (`(tick, chip id)` as the stats
/// record them) into [`ChipUseMark`]s: x normalized via `x_of`, name and
/// icon resolved through `loaded`'s assets (`"???"`/no icon when no save
/// is available or the game doesn't know that id). The pre-baked icon
/// handles are Arc-backed — cloning one per use is a refcount bump.
fn chip_use_marks(
    uses: &[(u32, u16)],
    loaded: Option<&tango_gamesupport::LoadedSave>,
    x_of: impl Fn(u32) -> f32,
) -> Vec<ChipUseMark> {
    uses.iter()
        .map(|&(t, id)| {
            let chip = loaded.and_then(|l| l.chips.get(id as usize)).cloned().unwrap_or_default();
            ChipUseMark {
                x: x_of(t),
                name: chip.name.unwrap_or_else(|| "???".to_string()),
                icon: chip.icon,
            }
        })
        .collect()
}

/// The engine's verdict, restated for the engine-free chart.
fn round_outcome(o: tango_match::analysis::BattleOutcome) -> RoundOutcome {
    match o {
        tango_match::analysis::BattleOutcome::Win => RoundOutcome::Win,
        tango_match::analysis::BattleOutcome::Loss => RoundOutcome::Loss,
        tango_match::analysis::BattleOutcome::Draw => RoundOutcome::Draw,
    }
}

/// Cook a match's stats for [`hp_match_graph`], returning the rounds and
/// the match-wide max HP the traces were normalized against (the chart's
/// hover readout multiplies back through it). This is the one
/// stats-to-chart path — the replays detail pane and the post-match
/// results card both go through it. `loadeds` resolve chip names/icons
/// per side (`[you, opponent]`); pass the local side's twice when only
/// it is available.
///
/// `planned` fixes the layout upfront: per-round tick counts from the
/// recording's round markers (the recorded input pairs per round).
/// Stats ticks are session-absolute — the recording is one contiguous
/// stream — so the markers' cumulative boundaries are the rounds'
/// positions on that same timebase: round `i` spans
/// `[Σ planned[..i], Σ planned[..=i])`, and every chart cooked with the
/// same plan shares the scrubber's tick scale by construction. Rounds
/// the stats don't cover yet are padded in as empty segments at their
/// planned width, so a live analysis draws into a stable frame instead
/// of continually rescaling it. Without a plan, each round anchors at
/// its own first sample.
pub fn cook_hp_rounds(
    stats: &tango_match::analysis::MatchStats,
    loadeds: [Option<&tango_gamesupport::LoadedSave>; 2],
    planned: Option<&[u32]>,
) -> (Vec<CookedHpRound>, f32) {
    let max_hp = stats
        .rounds
        .iter()
        .flat_map(|r| r.hp.iter())
        .map(|p| p.local.max(p.remote))
        .max()
        .unwrap_or(0)
        .max(1) as f32;
    let n = stats.rounds.len().max(planned.map(|p| p.len()).unwrap_or(0));
    // Cumulative round boundaries on the recording's timebase.
    let starts: Vec<u32> = planned
        .map(|p| {
            p.iter()
                .scan(0u32, |acc, &len| {
                    let start = *acc;
                    *acc += len;
                    Some(start)
                })
                .collect()
        })
        .unwrap_or_default();
    let rounds = (0..n)
        .map(|i| {
            let full = planned.and_then(|p| p.get(i)).map(|&t| t as f32);
            let Some(r) = stats.rounds.get(i) else {
                // Not simulated yet: an empty segment at its final width.
                return CookedHpRound {
                    outcome: None,
                    trace: vec![],
                    custom: vec![],
                    chip_uses: [vec![], vec![]],
                    weight: full.unwrap_or(0.0),
                };
            };
            let last = match r.hp.last() {
                Some(last) if r.hp.len() >= 2 => last,
                _ => {
                    return CookedHpRound {
                        outcome: r.outcome.map(round_outcome),
                        trace: vec![],
                        custom: vec![],
                        chip_uses: [vec![], vec![]],
                        weight: full.unwrap_or(0.0),
                    };
                }
            };
            // With a planned layout the span is exactly the planned tick
            // count, so the frame is identical across empty -> partial ->
            // final states of the same round. x runs from the round's
            // real start: the input-less round intro reads as trace-free
            // space before the first sample — the same convention the
            // playback transport draws (its strip shares the scrubber's
            // linear tick timeline), so the chart starts identically
            // everywhere. The simulation runs PAST the recorded input
            // count (the round-end animation isn't recorded, and its
            // length is only known by simulating), so the plan is always
            // overshot: those tail ticks clamp onto the segment's right
            // edge instead of widening it. The tail is inert (post-KO:
            // HP frozen, no chips, no customs), so nothing visible
            // distorts. Without a plan, normalize to the sampled extent.
            // The round's position on the recording's timebase: its
            // marker boundary when the plan carries one, else its own
            // first sample.
            let base = starts
                .get(i)
                .copied()
                .unwrap_or_else(|| r.hp.first().map(|p| p.tick).unwrap_or(0));
            let span = match full {
                Some(f) => f.max(1.0),
                None => (last.tick.saturating_sub(base) as f32).max(1.0),
            };
            let x_of = |tick: u32| (tick.saturating_sub(base) as f32 / span).clamp(0.0, 1.0);
            CookedHpRound {
                outcome: r.outcome.map(round_outcome),
                trace: r
                    .hp
                    .iter()
                    .map(|p| (x_of(p.tick), p.local as f32 / max_hp, p.remote as f32 / max_hp))
                    .collect(),
                custom: r.custom.iter().map(|&(a, b)| (x_of(a), x_of(b))).collect(),
                chip_uses: [
                    chip_use_marks(&r.chip_uses[0], loadeds[0], x_of),
                    chip_use_marks(&r.chip_uses[1], loadeds[1], x_of),
                ],
                weight: span,
            }
        })
        .collect();
    (rounds, max_hp)
}

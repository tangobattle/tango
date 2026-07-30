//! The stats-to-chart cooking for the match-analysis HP graph: the
//! engine's round verdicts are restated as the toolkit's
//! [`RoundOutcome`] and chip ids resolve through a loaded save's baked
//! [`chips`](tango_gamesupport::LoadedSave::chips) table, so
//! `tango_ui::widgets::hp_match_graph` can stay game- and engine-free.
//! Lives here because it is the one place that speaks both the engine's
//! stats and the toolkit's chart shapes, and it needs no more of a save
//! than that public table. Re-exported into [`crate::ui::widgets`] so
//! call sites read the same as the toolkit's own widgets.

use super::widgets::*;

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
/// The stats are one flat series per kind on the recording's timebase
/// with the rounds marking it up, so cooking is a matter of cutting each
/// series at those marks and normalizing x within the round. A round's
/// segment runs its full tick span, which means the input-less round
/// intro reads as trace-free space before the first sample — the same
/// convention the playback transport draws, so the chart starts
/// identically everywhere.
///
/// `total_ticks` is the recording's length, and giving it fixes the
/// chart's timeline: the segments' weights then sum to the whole
/// recording no matter how much of it has been analyzed, and since a
/// segment's width is its span while x inside it is the offset over that
/// same span, every tick sits at the same place throughout. A fold
/// finding its next round splits the last segment in two and moves
/// nothing already drawn. Without it, the last round stops at its final
/// reading and the chart covers only what was analyzed.
pub fn cook_hp_rounds(
    stats: &tango_match::analysis::MatchStats,
    loadeds: [Option<&tango_gamesupport::LoadedSave>; 2],
    total_ticks: Option<u32>,
) -> (Vec<CookedHpRound>, f32) {
    let max_hp = stats
        .hp
        .iter()
        .map(|p| p.local.max(p.remote))
        .max()
        .unwrap_or(0)
        .max(1) as f32;
    let rounds = (0..stats.rounds.len())
        .map(|i| {
            let outcome = stats.rounds[i].outcome.map(|(_, o)| round_outcome(o));
            let Some((start, end)) = stats.round_span(i, total_ticks) else {
                return CookedHpRound {
                    outcome,
                    trace: vec![],
                    custom: vec![],
                    chip_uses: [vec![], vec![]],
                    weight: 0.0,
                };
            };
            let span = (end - start).max(1) as f32;
            let x_of = |tick: u32| (tick.saturating_sub(start) as f32 / span).clamp(0.0, 1.0);
            // The simulation runs PAST the last round's span — the
            // round-end animation isn't in the recording, and its length
            // is only known by simulating — so that round keeps
            // everything from its start on and `x_of` clamps the
            // overshoot onto the segment's right edge instead of
            // widening it. The tail is inert (post-KO the HP is frozen
            // and nothing else fires), so nothing visible distorts.
            let last = i + 1 == stats.rounds.len();
            let within = |tick: u32| tick >= start && (tick < end || last);
            let trace: Vec<_> = stats
                .hp
                .iter()
                .filter(|p| within(p.tick))
                .map(|p| (x_of(p.tick), p.local as f32 / max_hp, p.remote as f32 / max_hp))
                .collect();
            CookedHpRound {
                outcome,
                // One point draws nothing but implies a shape; leave the
                // segment bare rather than plotting a lone dot.
                trace: if trace.len() >= 2 { trace } else { vec![] },
                custom: stats
                    .custom
                    .iter()
                    .filter(|&&(a, _)| within(a))
                    .map(|&(a, b)| (x_of(a), x_of(b)))
                    .collect(),
                chip_uses: [0, 1].map(|side| {
                    let uses: Vec<_> = stats.chip_uses[side]
                        .iter()
                        .filter(|&&(t, _)| within(t))
                        .copied()
                        .collect();
                    chip_use_marks(&uses, loadeds[side], x_of)
                }),
                weight: span,
            }
        })
        .collect();
    (rounds, max_hp)
}

//! The post-match results card's model: a finished PvP match cooked into
//! owned data at teardown, which outlives the session it came from.

use super::update::pvp::PvpPanes;
use super::{pvp, view, Session};
use crate::ui::{anim, widgets};

/// How the match on the results screen came to its end. The disconnect
/// variant renders the same card at rest — no reveal choreography, and a
/// "connection lost" headline instead of a verdict (the match never
/// finished, so declaring victory or defeat would be a lie).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MatchEnd {
    /// Natural end: the deciding round finished and the runout elapsed.
    Completed,
    /// The remote vanished mid-match: their channel EOF'd or the
    /// reconnect window expired.
    Disconnected,
}

/// Snapshot of a finished PvP match, taken at the session teardown
/// (`is_ended`) and shown as the post-match results screen until dismissed:
/// on a natural end, and on a remote disconnect (the match state as it
/// stood — see [`MatchEnd`]). Owned data only — the session (and everything
/// network-side) is already gone while this is on screen. User-initiated
/// quits (Esc hold, disconnect confirm) skip the capture: the player chose
/// to leave, so they go straight back to the menu.
pub struct MatchResults {
    pub remote_nickname: String,
    /// How the match ended — picks the card's dress (verdict reveal vs
    /// the quiet disconnect layout).
    pub end: MatchEnd,
    /// Per-round outcome + presentation-ready HP trace, in play order —
    /// including a round the match never decided, which carries its trace
    /// with no outcome. Empty only when the match tore down before any
    /// round was sampled at all (e.g. a comm error in the intro) — the
    /// screen shows a neutral headline then.
    pub rounds: Vec<RoundCard>,
    /// Session start to local completion.
    pub duration: std::time::Duration,
    /// The replay recorded for this match, for the Watch button. `None` if
    /// the writer failed to open at match start.
    pub replay_path: Option<std::path::PathBuf>,
    /// The match-wide HP scale the round traces were normalized against —
    /// the chart's hover readout multiplies back through it.
    pub max_hp: f32,
    /// When the results screen was put up — the zero point of its reveal
    /// choreography (per-round HP sweeps, then the verdict stamp). One-shot:
    /// returning from a replay watch finds it long elapsed, so the card sits
    /// at rest instead of replaying its entrance.
    pub revealed_at: iced::time::Instant,
}

/// One round on the results card: the outcome plus the cooked series for
/// the round graph. `trace` points are `(x, you, opponent)`, all normalized —
/// x over the round's sampled ticks, HP against the match-wide maximum so
/// every round shares one vertical scale; `custom` is the normalized
/// `[start, end)` x spans where the custom screen stood open. Empty when the
/// round produced no HP samples (torn down mid-intro).
pub struct RoundCard {
    /// `None` for a round the match never decided — a mid-round
    /// disconnect keeps the round and its trace, it just has no verdict
    /// to report.
    pub outcome: Option<widgets::RoundOutcome>,
    pub trace: Vec<(f32, f32, f32)>,
    pub custom: Vec<(f32, f32)>,
    /// Chip-use events per side (`[you, opponent]`), cooked for the
    /// graph's event lanes. Names/icons are resolved at capture time —
    /// the session (and both sides' loadeds) is gone while the card is
    /// on screen — each side through its own LoadedSave, the opponent
    /// falling back to the local game's table when they blinded their
    /// setup. Empty on games whose traps don't report chips (bn1).
    pub chip_uses: [Vec<widgets::ChipUseMark>; 2],
    /// Tick span of the round — its share of the continuous timeline.
    pub weight: f32,
}

impl MatchResults {
    fn capture(pvp: &pvp::PvpSession, panes: Option<&PvpPanes>, end: MatchEnd) -> Self {
        // The same aggregation the replay sidecar gets: the match folded
        // each round into its MatchStatsBuilder as it ended, so this snapshot
        // can never disagree with what the Replays tab later shows for
        // the same match.
        let stats = pvp.stats_snapshot();
        let local_loaded = panes.and_then(|p| p.local_loaded.as_ref());
        let loadeds = [
            local_loaded,
            panes.and_then(|p| p.opponent_loaded.as_ref()).or(local_loaded),
        ];
        // No recording length to pin the timeline to — the match just
        // ended and its replay is still flushing — so the cards run to
        // the last reading.
        let (cooked, max_hp) = crate::ui::matchup::cook_hp_rounds(&stats, loadeds, None);
        let rounds = cooked
            .into_iter()
            // Every round the match simulated is on the card, decided or
            // not: the last one of a mid-round disconnect comes through
            // with its trace and no outcome.
            .map(|c| RoundCard {
                outcome: c.outcome,
                trace: c.trace,
                custom: c.custom,
                chip_uses: c.chip_uses,
                weight: c.weight,
            })
            .collect::<Vec<_>>();
        let results = Self {
            remote_nickname: pvp.remote_nickname.clone(),
            end,
            rounds,
            duration: pvp.match_duration(),
            replay_path: pvp.replay_path.clone(),
            max_hp,
            revealed_at: iced::time::Instant::now(),
        };
        anim::kick(view::results::reveal_duration(&results));
        results
    }
}

/// Post-match results for the results screen, snapshotted at teardown
/// (right before the `is_ended` close drops the session) — `None` for
/// everything but a PvP match that ran to completion or lost its
/// remote: on a natural end the results card comes up with its reveal
/// choreography, on a remote disconnect (their channel EOF'd or the
/// reconnect window expired) in its disconnect dress with the match as
/// it stood. Our own quit paths (Esc hold, disconnect confirm) set
/// neither flag and go straight back to the menu: the player chose to
/// leave.
pub(super) fn capture_results(session: &dyn Session, panes: Option<&PvpPanes>) -> Option<MatchResults> {
    let pvp = session.downcast_ref::<pvp::PvpSession>()?;
    if pvp.is_completed() {
        Some(MatchResults::capture(pvp, panes, MatchEnd::Completed))
    } else if pvp.remote_disconnected() {
        Some(MatchResults::capture(pvp, panes, MatchEnd::Disconnected))
    } else {
        None
    }
}

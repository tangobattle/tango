//! The post-match results screen: a centered card over the cyber
//! backdrop summarizing the PvP match that just ended. Rendered by the
//! App whenever no session is active but [`MatchResults`] is set, i.e.
//! from a PvP session's natural end until the user dismisses it.
//!
//! When every round carries an HP trace the card replays the match in
//! miniature: each round's graph sweeps in left to right (a step-line of
//! both navis' HP over the round), the score tallies up as rounds
//! complete, and the verdict stamps in last. All choreography is drawn
//! transforms over a layout that never changes — slots are reserved up
//! front, so nothing shifts under the cursor while the sequence runs.
//! Rounds without traces (older sessions, a round torn down mid-intro)
//! fall back to a static card with per-round outcome marks.

use crate::i18n::t;
use crate::style::{STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION, TEXT_DISPLAY};
use crate::widgets;
use iced::widget::{button, container, text};
use iced::{Alignment, Color, Element, Fill, Length, Theme};
use lucide_icons::Icon;
use sweeten::widget::{column, row};
use tango_pvp::stepper::BattleOutcome;
use unic_langid::LanguageIdentifier;

use super::super::{MatchResults, Message};

/// Point size of the two score numerals — the card's centerpiece.
const SCORE_SIZE: f32 = 44.0;

// Reveal timeline: one linear clock from `MatchResults::revealed_at`.
// The whole match sweeps once across the continuous chart (each round's
// share proportional to its length); the score ticks pop as the sweep
// crosses each round boundary, and the verdict stamps in last.
const SWEEP_MS: f32 = 850.0;
const POP_MS: f32 = 250.0;
const STAMP_MS: f32 = 400.0;

/// Reserved heights for the elements that appear mid-choreography, so the
/// card's layout is complete from the first frame.
const HEADLINE_SLOT_H: f32 = 30.0;
const DRAWS_SLOT_H: f32 = 16.0;
const GRAPH_H: f32 = 48.0;

/// How long [`MatchResults::capture`] must keep redraws flowing to play the
/// whole reveal.
pub(crate) fn reveal_duration(results: &MatchResults) -> std::time::Duration {
    let sweeps = if animated(results) { results.rounds.len() } else { 0 };
    std::time::Duration::from_millis((sweeps as f32 * SWEEP_MS + STAMP_MS) as u64)
}

/// The choreographed card needs every round to have a trace; any round
/// without one drops the whole card back to the static marks layout.
fn animated(results: &MatchResults) -> bool {
    !results.rounds.is_empty() && results.rounds.iter().all(|r| !r.trace.is_empty())
}

pub fn results_view<'a>(lang: &'a LanguageIdentifier, results: &'a MatchResults) -> Element<'a, Message> {
    let now = iced::time::Instant::now();
    let elapsed_ms = now.duration_since(results.revealed_at).as_secs_f32() * 1000.0;
    let animated = animated(results);

    // Verdict = round majority, matching the game's own call. A match that
    // tore down before any round was decided (comm error mid-round-1) gets a
    // neutral headline instead of a fake draw.
    let no_contest = results.rounds.is_empty();
    let wins = count(results, BattleOutcome::Win);
    let losses = count(results, BattleOutcome::Loss);
    let draws = count(results, BattleOutcome::Draw);
    let (headline, headline_style) = if no_contest {
        (t!(lang, "session-results-no-contest"), muted_style())
    } else if wins > losses {
        (
            t!(lang, "session-results-victory"),
            widgets::success_text_style as fn(&iced::Theme) -> iced::widget::text::Style,
        )
    } else if losses > wins {
        (t!(lang, "session-results-defeat"), widgets::danger_text_style as _)
    } else {
        (t!(lang, "session-results-draw"), muted_style())
    };

    // "vs <opponent> · m:ss" — one quiet context line under the verdict.
    let secs = results.duration.as_secs();
    let context = format!(
        "{} · {}:{:02}",
        t!(lang, "session-results-vs", nickname = results.remote_nickname.as_str()),
        secs / 60,
        secs % 60
    );

    let mut body = column![].spacing(4).align_x(Alignment::Center);

    // Verdict headline. On the choreographed card its slot is reserved and
    // the text stamps in (scale settling to rest, no fade) once every round
    // graph has finished sweeping; on the static card it's just there.
    let headline_text = || text(headline.clone()).size(TEXT_DISPLAY).style(headline_style);
    if animated {
        let stamp = segment(elapsed_ms, results.rounds.len() as f32 * SWEEP_MS, STAMP_MS);
        let slot: Element<'_, Message> = if stamp <= 0.0 {
            iced::widget::Space::new().into()
        } else if stamp >= 1.0 {
            headline_text().into()
        } else {
            let scale = 1.0 + 0.45 * cubic_out_inv(stamp);
            iced::widget::float(headline_text()).scale(scale).into()
        };
        body = body.push(
            container(slot)
                .height(Length::Fixed(HEADLINE_SLOT_H))
                .align_y(Alignment::Center),
        );
    } else {
        body = body.push(headline_text());
    }
    body = body.push(text(context).size(TEXT_BODY).style(widgets::muted_text_style));

    if no_contest {
        body = body.push(iced::widget::Space::new().height(10)).push(
            text(t!(lang, "session-results-no-rounds"))
                .size(TEXT_CAPTION)
                .style(widgets::muted_text_style),
        );
    } else if animated {
        // Score tallies up as the sweep crosses each round's boundary; a
        // freshly-counted round bounces the numeral it incremented.
        let total_sweep_ms = results.rounds.len() as f32 * SWEEP_MS;
        let total_weight: f32 = results.rounds.iter().map(|r| r.weight.max(1.0)).sum::<f32>().max(1.0);
        let mut shown = [0usize; 3]; // wins, losses, draws
        let mut pops = [1.0f32; 3];
        let mut cum_weight = 0.0f32;
        for round in results.rounds.iter() {
            cum_weight += round.weight.max(1.0);
            let pop = segment(elapsed_ms, cum_weight / total_weight * total_sweep_ms, POP_MS);
            if pop <= 0.0 {
                break;
            }
            let k = match round.outcome {
                BattleOutcome::Win => 0,
                BattleOutcome::Loss => 1,
                BattleOutcome::Draw => 2,
            };
            shown[k] += 1;
            pops[k] = pop;
        }

        let side = |n: usize, pop: f32, label: String| {
            let numeral: Element<'_, Message> = if pop < 1.0 {
                crate::anim::pop(text(n.to_string()).size(SCORE_SIZE), pop, 4.0)
            } else {
                text(n.to_string()).size(SCORE_SIZE).into()
            };
            column![numeral, text(label).size(TEXT_CAPTION).style(widgets::muted_text_style)]
                .spacing(2)
                .align_x(Alignment::Center)
        };
        body = body.push(iced::widget::Space::new().height(8)).push(
            row![
                side(shown[0], pops[0], t!(lang, "session-results-you")),
                text("–").size(SCORE_SIZE * 0.6).style(widgets::muted_text_style),
                side(shown[1], pops[1], results.remote_nickname.clone()),
            ]
            .spacing(24)
            .align_y(Alignment::Center),
        );
        // Draws line: only matches that had any get the slot, and the count
        // appears once the first drawn round has been tallied.
        if draws > 0 {
            let line: Element<'_, Message> = if shown[2] > 0 {
                let caption = text(t!(lang, "session-results-draws", count = shown[2] as i64))
                    .size(TEXT_CAPTION)
                    .style(widgets::muted_text_style);
                if pops[2] < 1.0 {
                    crate::anim::pop(caption, pops[2], 3.0)
                } else {
                    caption.into()
                }
            } else {
                iced::widget::Space::new().into()
            };
            body = body.push(
                container(line)
                    .height(Length::Fixed(DRAWS_SLOT_H))
                    .align_y(Alignment::Center),
            );
        }

        // The whole match on one continuous chart — rounds side by side in
        // proportion to their length, swept once left to right. A legend row
        // names the two traces — text carries identity in ink, the colored
        // chips beside it carry the hue.
        let legend_entry = |color: fn(&Theme) -> Color, label: String| {
            row![
                container(iced::widget::Space::new().width(10).height(3)).style(move |theme: &Theme| {
                    iced::widget::container::Style {
                        background: Some(color(theme).into()),
                        border: iced::border::rounded(1.5),
                        ..Default::default()
                    }
                }),
                text(label).size(TEXT_CAPTION).style(widgets::muted_text_style),
            ]
            .spacing(5)
            .align_y(Alignment::Center)
        };
        body = body.push(iced::widget::Space::new().height(10)).push(
            row![
                legend_entry(widgets::hp_you_color, t!(lang, "session-results-you")),
                legend_entry(widgets::hp_opponent_color, results.remote_nickname.clone()),
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        );
        let sweep = segment(elapsed_ms, 0.0, total_sweep_ms);
        let chart_rounds: Vec<widgets::HpGraphRound<'_>> = results
            .rounds
            .iter()
            .map(|r| widgets::HpGraphRound {
                trace: &r.trace,
                custom: &r.custom,
                outcome: Some(r.outcome),
                weight: r.weight,
            })
            .collect();
        body = body
            .push(iced::widget::Space::new().height(6))
            .push(widgets::hp_match_graph(chart_rounds, sweep, GRAPH_H));
    } else {
        // Static fallback: the pre-trace layout — full score up front, plus a
        // marks row when there was more than one round to sequence.
        let side = |n: usize, label: String| {
            column![
                text(n.to_string()).size(SCORE_SIZE),
                text(label).size(TEXT_CAPTION).style(widgets::muted_text_style),
            ]
            .spacing(2)
            .align_x(Alignment::Center)
        };
        body = body.push(iced::widget::Space::new().height(14)).push(
            row![
                side(wins, t!(lang, "session-results-you")),
                text("–").size(SCORE_SIZE * 0.6).style(widgets::muted_text_style),
                side(losses, results.remote_nickname.clone()),
            ]
            .spacing(24)
            .align_y(Alignment::Center),
        );
        if draws > 0 {
            body = body.push(
                text(t!(lang, "session-results-draws", count = draws as i64))
                    .size(TEXT_CAPTION)
                    .style(widgets::muted_text_style),
            );
        }
        if results.rounds.len() > 1 {
            let mut marks = row![].spacing(10).align_y(Alignment::Center);
            for round in &results.rounds {
                let (icon, style) = widgets::outcome_mark(round.outcome);
                marks = marks.push(icon.widget().size(TEXT_BODY).style(style));
            }
            body = body.push(iced::widget::Space::new().height(8)).push(marks);
        }
    }

    // The ways out. Watch replay is the secondary action (and absent when
    // the recorder never opened); Done carries the primary weight.
    let mut actions = row![].spacing(8).align_y(Alignment::Center);
    if results.replay_path.is_some() {
        actions = actions.push(widgets::labeled_icon_button(
            Icon::Play,
            t!(lang, "session-results-watch-replay"),
            Message::WatchResultsReplay,
            STANDARD_PADDING,
            widgets::neutral,
        ));
    }
    actions = actions.push(
        button(text(t!(lang, "session-results-done")))
            .padding(STANDARD_PADDING)
            .style(widgets::primary_button)
            .on_press(Message::DismissResults),
    );
    body = body.push(iced::widget::Space::new().height(20)).push(actions);

    container(
        container(body.width(Fill))
            .style(widgets::panel)
            .padding(28)
            .width(Length::Fixed(400.0)),
    )
    .center(Fill)
    .into()
}

/// Progress of the timeline segment starting at `start_ms` lasting `len_ms`,
/// clamped to 0..=1.
fn segment(elapsed_ms: f32, start_ms: f32, len_ms: f32) -> f32 {
    ((elapsed_ms - start_ms) / len_ms).clamp(0.0, 1.0)
}

/// Remaining travel of an ease-out-cubic at progress `t` — 1 at the start,
/// 0 at rest. Shapes the verdict stamp's settle.
fn cubic_out_inv(t: f32) -> f32 {
    (1.0 - t).powi(3)
}

fn count(results: &MatchResults, which: BattleOutcome) -> usize {
    results.rounds.iter().filter(|r| r.outcome == which).count()
}

/// Plain-muted text style with the same fn-pointer type as the
/// success/danger styles, so the verdict styling stays one `match`.
fn muted_style() -> fn(&iced::Theme) -> iced::widget::text::Style {
    widgets::muted_text_style
}

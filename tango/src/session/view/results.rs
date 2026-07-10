//! The post-match results screen: a centered card over the cyber
//! backdrop summarizing the PvP match that just ended — verdict
//! headline, round score, per-round outcome marks, and the ways out
//! (replay playback / back to the menu). Rendered by the App whenever
//! no session is active but [`MatchResults`] is set, i.e. from a PvP
//! session's natural end until the user dismisses it.

use crate::i18n::t;
use crate::style::{STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION, TEXT_DISPLAY};
use crate::widgets;
use iced::widget::{button, container, text};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use sweeten::widget::{column, row};
use tango_pvp::stepper::BattleOutcome;
use unic_langid::LanguageIdentifier;

use super::super::{MatchResults, Message};

/// Point size of the two score numerals — the card's centerpiece.
const SCORE_SIZE: f32 = 44.0;

pub fn results_view<'a>(lang: &'a LanguageIdentifier, results: &'a MatchResults) -> Element<'a, Message> {
    let wins = count(&results.outcomes, BattleOutcome::Win);
    let losses = count(&results.outcomes, BattleOutcome::Loss);
    let draws = count(&results.outcomes, BattleOutcome::Draw);

    // Verdict = round majority, matching the game's own call. A match that
    // tore down before any round was decided (comm error mid-round-1) gets a
    // neutral headline instead of a fake draw.
    let no_contest = results.outcomes.is_empty();
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

    let mut body = column![
        text(headline).size(TEXT_DISPLAY).style(headline_style),
        text(context).size(TEXT_BODY).style(widgets::muted_text_style),
    ]
    .spacing(4)
    .align_x(Alignment::Center);

    if no_contest {
        body = body.push(iced::widget::Space::new().height(10)).push(
            text(t!(lang, "session-results-no-rounds"))
                .size(TEXT_CAPTION)
                .style(widgets::muted_text_style),
        );
    } else {
        // Round score, our side left. The labels beneath the numerals carry
        // the orientation so the numbers stay clean.
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
        // Per-round marks, in play order — only worth a row when there was
        // more than one round to sequence.
        if results.outcomes.len() > 1 {
            let mut marks = row![].spacing(10).align_y(Alignment::Center);
            for outcome in &results.outcomes {
                let (icon, style) = match outcome {
                    BattleOutcome::Win => (
                        Icon::Check,
                        widgets::success_text_style as fn(&iced::Theme) -> iced::widget::text::Style,
                    ),
                    BattleOutcome::Loss => (Icon::X, widgets::danger_text_style as _),
                    BattleOutcome::Draw => (Icon::Minus, widgets::muted_text_style as _),
                };
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

fn count(outcomes: &[BattleOutcome], which: BattleOutcome) -> usize {
    outcomes.iter().filter(|o| **o == which).count()
}

/// Plain-muted text style with the same fn-pointer type as the
/// success/danger styles, so the verdict styling stays one `match`.
fn muted_style() -> fn(&iced::Theme) -> iced::widget::text::Style {
    widgets::muted_text_style
}

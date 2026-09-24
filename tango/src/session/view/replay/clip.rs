//! The replay bar's clip strip: marking a span of the recording and
//! exporting it.

use super::*;
// Explicit so this wins over iced's prelude macro; see the view module.
use sweeten::widget::row;

/// Fixed height of the expanded clip strip (chips + the row spacing
/// above it come out of [`clip_lift`] too, so the floats above the
/// bar ride up in step).
const CLIP_ROW_H: f32 = 28.0;

/// How much the expanded clip strip grows the bar — added to the
/// bottom-anchored floats' resting lift ([`POPOVER_LIFT`]).
pub(super) fn clip_lift(state: &State) -> f32 {
    if state.scrub.tools_open {
        CLIP_ROW_H
    } else {
        0.0
    }
}

/// The clip strip: mark-in/mark-out stamps, the marked span's
/// wallclock readout, export quality, clear, and the export CTA —
/// swapped wholesale for a progress line while an export job is
/// running. Lives between the scrubber and the transport row, only
/// while the bar's scissors toggle is on.
pub(super) fn clip_strip<'a>(
    lang: &'a LanguageIdentifier,
    state: &'a State,
    export_scale: u8,
    job: Option<ClipJob<'a>>,
) -> Element<'a, Message> {
    let chip = |icon: Icon, lit: bool, tip: String, msg: Option<Message>| -> Element<'a, Message> {
        let style = lit_plate_button(lit);
        iced::widget::tooltip(
            button(
                container(icon.widget().size(14.0))
                    .width(iced::Length::Fixed(16.0))
                    .height(iced::Length::Fixed(16.0))
                    .center(Fill),
            )
            .padding(0)
            .width(iced::Length::Fixed(26.0))
            .height(iced::Length::Fixed(26.0))
            .style(style)
            .on_press_maybe(msg),
            widgets::tooltip_bubble(tip),
            iced::widget::tooltip::Position::Top,
        )
        .gap(4)
        .into()
    };

    // A running export replaces the tools with its progress — the
    // strip is the player-side face of the same per-replay job the
    // replays tab shows, so there's exactly one of these at a time.
    if let Some(job) = job.filter(|j| j.result.is_none()) {
        let pct = if job.total > 0 {
            (job.completed as f32 / job.total as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let caption = if job.cancelling {
            t!(lang, "replays-export-cancelling")
        } else {
            format!(
                "{} {}%",
                t!(lang, "replays-export-progress"),
                (pct * 100.0).round() as u32
            )
        };
        let cancel = chip(
            Icon::X,
            false,
            t!(lang, "replays-export-cancel"),
            (!job.cancelling).then_some(Message::CancelClipExport),
        );
        return container(
            row![
                text(caption).size(TEXT_CAPTION).style(widgets::muted_text_style),
                iced::widget::progress_bar(0.0..=1.0, pct)
                    .girth(Length::Fixed(4.0))
                    .style(widgets::slim_progress_bar),
                cancel,
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .height(iced::Length::Fixed(CLIP_ROW_H))
        .align_y(iced::alignment::Vertical::Center)
        .into();
    }

    let (mark_in, mark_out) = (state.scrub.mark_in, state.scrub.mark_out);
    // Mark stamps ride beside their chips in the transport's numeral
    // treatment; an unset mark shows a muted placeholder so setting
    // one never reflows the row.
    let stamp = |mark: Option<u32>| {
        let (label, style): (String, fn(&iced::Theme) -> iced::widget::text::Style) = match mark {
            Some(m) => (crate::session::format_tick(m), |theme: &iced::Theme| {
                iced::widget::text::Style {
                    color: Some(theme.palette().primary),
                }
            }),
            None => ("–:––".to_string(), widgets::muted_text_style),
        };
        text(label).size(12).font(iced::Font::MONOSPACE).style(style)
    };
    let mut strip = row![
        chip(
            Icon::ArrowRightFromLine,
            mark_in.is_some(),
            t!(lang, "playback-clip-start"),
            Some(Message::SetClipStart),
        ),
        stamp(mark_in),
        chip(
            Icon::ArrowRightToLine,
            mark_out.is_some(),
            t!(lang, "playback-clip-end"),
            Some(Message::SetClipEnd),
        ),
        stamp(mark_out),
        chip(
            Icon::Eraser,
            false,
            t!(lang, "playback-clip-clear"),
            (mark_in.is_some() || mark_out.is_some()).then_some(Message::ClearClipMarks),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    // The marked span's length, once it exists.
    if let (Some(a), Some(b)) = (mark_in, mark_out) {
        strip = strip.push(
            text(format!("({})", crate::session::format_tick(b - a)))
                .size(12)
                .style(widgets::muted_text_style),
        );
    }
    strip = strip.push(iced::widget::space::horizontal());
    // The last job's outcome, quietly, with the full line in a
    // tooltip when it failed.
    if let Some(job) = job {
        match job.result {
            Some(Ok(())) => {
                strip = strip.push(
                    text(t!(lang, "replays-export-success"))
                        .size(TEXT_CAPTION)
                        .style(widgets::muted_text_style),
                );
            }
            Some(Err(e)) => {
                strip = strip.push(
                    iced::widget::tooltip(
                        text(t!(lang, "replays-export-error", error = "…"))
                            .size(TEXT_CAPTION)
                            .style(widgets::muted_text_style),
                        widgets::tooltip_bubble(e.describe(lang)),
                        iced::widget::tooltip::Position::Top,
                    )
                    .gap(4),
                );
            }
            None => {}
        }
    }
    // Full replay exports use this exact picker and state too.
    let quality_menu = widgets::replay_export_scale_picker(
        lang,
        export_scale,
        Message::SetClipExportScale,
        Some(Message::BarMenuToggled),
    );
    strip = strip.push(quality_menu);
    // The one CTA in the strip: primary once a valid span exists.
    let export_msg = match (mark_in, mark_out) {
        (Some(start), Some(end)) if start < end => Some(Message::ExportClip { start, end }),
        _ => None,
    };
    strip = strip.push(
        button(text(t!(lang, "playback-clip-export")).size(12))
            .padding([4, 10])
            .height(iced::Length::Fixed(26.0))
            .style(widgets::primary_button)
            .on_press_maybe(export_msg),
    );
    container(strip)
        .height(iced::Length::Fixed(CLIP_ROW_H))
        .align_y(iced::alignment::Vertical::Center)
        .into()
}

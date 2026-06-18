use super::*;
// Explicit so these win over iced's prelude `column!`/`row!` macros, which
// would otherwise clash with the sweeten ones re-exported via `super::*`.
use sweeten::widget::{column, mouse_area, row};

/// Live frame-delay control: a turtle-icon heading naming it, over the lobby's
/// frame-delay row (slider, fixed-width numeric readout, latency-driven
/// "suggest" wand). Lifting the title into the heading frees the control line so
/// the slider gets lobby-like width even in the compact panel. Frame delay is
/// purely local display lag, so dragging it mid-match takes effect on the next
/// rendered frame with no peer coordination.
fn frame_delay_control<'a>(lang: &'a LanguageIdentifier, pvp: &'a pvp::PvpSession) -> Element<'a, Message> {
    let fd = pvp.frame_delay();

    // Heading: turtle glyph + title, both muted — matches the metric-card
    // captions above so the control reads as part of the same panel.
    let heading = row![
        Icon::Turtle.widget().size(TEXT_BODY).style(widgets::muted_text_style),
        text(t!(lang, "settings-netplay-frame-delay"))
            .size(TEXT_CAPTION)
            .style(widgets::muted_text_style),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Fill);

    // Slider fills the row; the value + wand take their natural sizes.
    let slider = iced::widget::slider(MIN_FRAME_DELAY..=MAX_FRAME_DELAY, fd, Message::SetFrameDelay)
        .style(widgets::chunky_slider)
        .width(Length::Fill);

    // "Suggest" button — same formula as the lobby: one-way frames + 1,
    // clamped to the slider range, off the median ping. Enabled whenever the
    // link is live (`latency()` is `Some`); before the first ping that reads
    // `Some(ZERO)`, which just suggests the minimum frame delay.
    let suggest_msg = match pvp.latency() {
        Some(rtt) => Some(Message::SetFrameDelay(suggest_frame_delay(rtt))),
        _ => None,
    };
    let suggest = widgets::icon_button_maybe(
        Icon::Wand,
        t!(lang, "lobby-frame-delay-suggest"),
        suggest_msg,
        crate::style::STANDARD_PADDING,
    );

    let control = row![
        slider,
        text(format!("{}", fd)).size(TEXT_BODY).width(Length::Fixed(18.0)),
        suggest,
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Fill);

    column![heading, control]
        .spacing(3)
        .width(Length::Fixed(PANEL_W))
        .into()
}

// Panel + sparkline geometry. The cards are all `PANEL_W` wide so the metrics
// line up; the metric value reads in a fixed `VALUE_W` column on the right
// (sized to the widest readout, `NNN ms`) so every number right-aligns and
// every chart ends at the same x, with the chart filling everything to its
// left. The frame-delay control spans the same width: a turtle-icon heading
// over a lobby-style slider row.
const PANEL_W: f32 = 228.0;
const VALUE_W: f32 = 50.0;
const SPARK_H: f32 = 24.0;
// Each metric's full-height value span (sample saturates into it). Chosen to
// line up with the tone thresholds so a point's height roughly tracks its color.
const TPS_SPAN: f32 = 8.0; // fps below target = floor of the chart
const SKEW_SPAN: i32 = 8; // ± about parity; 0 sits mid-height
const LEAD_SPAN: i32 = 24; // ± about zero; saturates well before the overflow bail
const DEPTH_SPAN: u32 = 8;
const PING_SPAN: u128 = 200;

/// A compact per-metric history chart for the match-settings panel. Each
/// retained sample is `(height fraction in 0..=1, tone)`, plotted left→right
/// (oldest→newest) as a thin line whose every segment and vertex is colored by
/// that sample's health tone — so the trend tells the same green/amber/red
/// story as the readout, point by point, instead of one flat color for the
/// whole line. `None` slots are gaps (e.g. skew/depth between rounds) and break
/// the line.
struct Sparkline {
    points: Vec<Option<(f32, StatTone)>>,
    /// Whether to wash the area below the trace (down to the chart floor) with a
    /// faint tint of each segment's tone. On for the one-sided metrics (tps,
    /// depth, ping); off for skew, which is bidirectional about its midline.
    fill_under: bool,
    /// Height fraction (0 = bottom, 1 = top) of a reference line to draw, or
    /// `None` for no line. Parity (mid-height) for skew, the value-0 floor for
    /// depth/ping — and `None` for tps, whose displayed floor is `target − 8`,
    /// not 0, so a "zero" line there would mislead.
    zero: Option<f32>,
}

impl Sparkline {
    fn view<'a>(self) -> Element<'a, Message> {
        // Fill the card's chart area; height is fixed so the row lays out cleanly.
        Canvas::new(self)
            .width(Length::Fill)
            .height(Length::Fixed(SPARK_H))
            .into()
    }
}

impl canvas::Program<Message> for Sparkline {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();
        let text_color = theme.palette().text;
        let n = self.points.len();
        let w = bounds.width;
        let h = bounds.height;
        // Inset vertically so points at the extremes (yf 0 or 1) keep the line
        // width fully on-canvas instead of clipping at the edge.
        const PAD: f32 = 2.0;
        let y_at = |yf: f32| PAD + (1.0 - yf.clamp(0.0, 1.0)) * (h - 2.0 * PAD);

        // Recessed background so the chart area reads as its own inset panel.
        let bg = Path::rounded_rectangle(Point::new(0.0, 0.0), bounds.size(), 3.0.into());
        frame.fill(
            &bg,
            Color {
                a: if palette.is_dark { 0.10 } else { 0.05 },
                ..text_color
            },
        );

        // Fixed rolling window: samples sit a fixed pixel step apart with the
        // newest pinned to the right edge, so the trace scrolls in from the
        // right at full scale instead of stretching to fill while the buffer is
        // still filling up.
        let dx = w / (METRIC_HISTORY_LEN.saturating_sub(1).max(1) as f32);
        let x_at = |i: usize| w - (n.saturating_sub(1) - i) as f32 * dx;

        // Tone wash below the trace, down to the chart floor, per segment.
        if self.fill_under {
            let base = y_at(0.0);
            for i in 0..n.saturating_sub(1) {
                if let (Some((y0, _)), Some((y1, tone))) = (self.points[i], self.points[i + 1]) {
                    let (x0, x1) = (x_at(i), x_at(i + 1));
                    let area = Path::new(|p| {
                        p.move_to(Point::new(x0, y_at(y0)));
                        p.line_to(Point::new(x1, y_at(y1)));
                        p.line_to(Point::new(x1, base));
                        p.line_to(Point::new(x0, base));
                        p.close();
                    });
                    frame.fill(
                        &area,
                        Color {
                            a: 0.3,
                            ..stat_tone_color(theme, tone)
                        },
                    );
                }
            }
        }

        // Reference line where one is meaningful (parity for skew, the value-0
        // floor for depth/ping). Drawn over the fill so it stays visible, under
        // the trace.
        if let Some(z) = self.zero {
            let zero_y = y_at(z);
            frame.stroke(
                &Path::line(Point::new(0.0, zero_y), Point::new(w, zero_y)),
                Stroke::default()
                    .with_color(Color { a: 0.22, ..text_color })
                    .with_width(1.0),
            );
        }

        // The trace itself: one hairline segment per adjacent pair of samples,
        // each colored by the newer endpoint's tone, breaking across `None`
        // gaps. No vertices/dots — the connected segments are the whole chart.
        for i in 0..n.saturating_sub(1) {
            if let (Some((y0, _)), Some((y1, tone))) = (self.points[i], self.points[i + 1]) {
                let seg = Path::line(Point::new(x_at(i), y_at(y0)), Point::new(x_at(i + 1), y_at(y1)));
                frame.stroke(
                    &seg,
                    Stroke::default()
                        .with_color(stat_tone_color(theme, tone))
                        .with_width(1.0)
                        .with_line_cap(LineCap::Round),
                );
            }
        }

        vec![frame.into_geometry()]
    }
}

/// One telemetry card: `icon caption` on top, `control value` below — the shape
/// shared by every metric (control = sparkline) and the frame-delay knob
/// (control = slider). Icon + caption ride muted; `control` fills the row while
/// `value` sits right-aligned in a fixed [`VALUE_W`] column, so every readout
/// lines up and every chart ends at the same x. Fixed at [`PANEL_W`] so the
/// cards align with one another.
fn telemetry_card<'a>(
    icon: Icon,
    caption: String,
    control: Element<'a, Message>,
    value: Element<'a, Message>,
) -> Element<'a, Message> {
    let caption_row = row![
        icon.widget().size(TEXT_BODY).style(widgets::muted_text_style),
        text(caption).size(TEXT_CAPTION).style(widgets::muted_text_style),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Fill);
    let value_row = row![
        control,
        container(value)
            .width(Length::Fixed(VALUE_W))
            .align_x(iced::alignment::Horizontal::Right),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .width(Fill);
    column![caption_row, value_row]
        .spacing(3)
        .width(Length::Fixed(PANEL_W))
        .into()
}

/// A right-aligned monospace value readout, tinted by `tone` (or default text
/// when `None`, e.g. the frame-delay number).
fn value_text<'a>(s: String, tone: Option<StatTone>) -> Element<'a, Message> {
    text(s)
        .size(TEXT_BODY)
        .font(iced::Font::MONOSPACE)
        .style(move |theme: &iced::Theme| iced::widget::text::Style {
            color: Some(tone.map_or_else(|| theme.palette().text, |t| stat_tone_color(theme, t))),
        })
        .into()
}

/// TPS readout: current rate over its live cap, stacked to stay narrow. The
/// current rate carries the health tone; the cap rides muted underneath.
fn tps_value<'a>(tps: f32, fps_target: f32, tone: StatTone) -> Element<'a, Message> {
    use iced::widget::text::LineHeight;
    column![
        text(format!("{:.2}", tps))
            .size(TEXT_BODY)
            .font(iced::Font::MONOSPACE)
            .line_height(LineHeight::Relative(1.0))
            .style(move |theme: &iced::Theme| iced::widget::text::Style {
                color: Some(stat_tone_color(theme, tone)),
            }),
        text(format!("{:.2}", fps_target))
            .size(TEXT_CAPTION)
            .font(iced::Font::MONOSPACE)
            .line_height(LineHeight::Relative(1.0))
            .style(widgets::muted_text_style),
    ]
    .spacing(2)
    .align_x(Alignment::End)
    .into()
}

/// One metric card: build its sparkline series by mapping every retained sample
/// through `point` (returning `None` for slots with no reading, which become
/// gaps), and read the current value off the newest sample via `value` (showing
/// `—` muted when there's nothing yet, e.g. skew/depth between rounds).
fn metric_card<'a>(
    icon: Icon,
    caption: String,
    fill_under: bool,
    zero: Option<f32>,
    history: &std::collections::VecDeque<MetricSample>,
    point: impl Fn(&MetricSample) -> Option<(f32, StatTone)>,
    value: impl Fn(&MetricSample) -> Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let points = history.iter().map(&point).collect();
    let value = history
        .back()
        .and_then(value)
        .unwrap_or_else(|| value_text("—".to_string(), Some(StatTone::Muted)));
    telemetry_card(
        icon,
        caption,
        Sparkline {
            points,
            fill_under,
            zero,
        }
        .view(),
        value,
    )
}

/// Contents of the match-settings panel: a sparkline card per live metric
/// (TPS, skew, lead, depth, ping) stacked above the frame-delay card. Each chart
/// reads its window from `history` and its current value from the newest
/// sample.
fn match_settings_content<'a>(
    lang: &'a LanguageIdentifier,
    pvp: &'a pvp::PvpSession,
    history: &std::collections::VecDeque<MetricSample>,
) -> Element<'a, Message> {
    // `zero` is the reference line: parity (mid-height) for skew, the value-0
    // floor for depth/ping, and `None` for tps (its floor is `target − 8`, so a
    // "zero" line there would mislead).
    let tps_card = metric_card(
        Icon::Gauge,
        t!(lang, "session-stat-tps"),
        true,
        None,
        history,
        |s| {
            (s.fps_target > 0.0).then(|| {
                let yf = (s.tps - (s.fps_target - TPS_SPAN)) / TPS_SPAN;
                (yf.clamp(0.0, 1.0), tone_for_tps(s.tps, s.fps_target))
            })
        },
        |s| (s.fps_target > 0.0).then(|| tps_value(s.tps, s.fps_target, tone_for_tps(s.tps, s.fps_target))),
    );

    let skew_card = metric_card(
        Icon::ArrowLeftRight,
        t!(lang, "session-stat-skew"),
        false,
        Some(0.5),
        history,
        |s| {
            s.round.map(|(skew, _, _)| {
                let yf = (skew.clamp(-SKEW_SPAN, SKEW_SPAN) as f32 + SKEW_SPAN as f32) / (2.0 * SKEW_SPAN as f32);
                (yf, tone_for_skew(skew))
            })
        },
        |s| {
            s.round
                .map(|(skew, _, _)| value_text(fmt_skew(skew), Some(tone_for_skew(skew))))
        },
    );

    let lead_card = metric_card(
        Icon::SportShoe,
        t!(lang, "session-stat-lead"),
        false,
        Some(0.5),
        history,
        |s| {
            s.round.map(|(_, _, lead)| {
                let yf = (lead.clamp(-LEAD_SPAN, LEAD_SPAN) as f32 + LEAD_SPAN as f32) / (2.0 * LEAD_SPAN as f32);
                (yf, tone_for_lead(lead))
            })
        },
        |s| {
            s.round
                .map(|(_, _, lead)| value_text(fmt_lead(lead), Some(tone_for_lead(lead))))
        },
    );

    let depth_card = metric_card(
        Icon::GitMergeConflict,
        t!(lang, "session-stat-depth"),
        true,
        Some(0.0),
        history,
        |s| {
            s.round
                .map(|(_, depth, _)| (depth.min(DEPTH_SPAN) as f32 / DEPTH_SPAN as f32, tone_for_depth(depth)))
        },
        |s| {
            s.round
                .map(|(_, depth, _)| value_text(fmt_depth(depth), Some(tone_for_depth(depth))))
        },
    );

    let ping_card = metric_card(
        Icon::ChevronsLeftRightEllipsis,
        t!(lang, "session-stat-ping"),
        true,
        Some(0.0),
        history,
        |s| {
            Some((
                s.ping_ms.min(PING_SPAN) as f32 / PING_SPAN as f32,
                tone_for_ping(s.ping_ms),
            ))
        },
        |s| Some(value_text(fmt_ping(s.ping_ms), Some(tone_for_ping(s.ping_ms)))),
    );

    // Faint rule separating the read-only metrics from the frame-delay knob.
    let rule =
        container(iced::widget::Space::new().width(Fill).height(Length::Fixed(1.0))).style(|theme: &iced::Theme| {
            let p = theme.extended_palette();
            iced::widget::container::Style {
                background: Some(iced::Background::Color(Color {
                    a: if p.is_dark { 0.16 } else { 0.13 },
                    ..theme.palette().text
                })),
                ..Default::default()
            }
        });

    column![
        tps_card,
        skew_card,
        lead_card,
        depth_card,
        ping_card,
        rule,
        frame_delay_control(lang, pvp)
    ]
    .spacing(8)
    .width(Length::Fixed(PANEL_W))
    .into()
}

/// Semantic tone for a PvP telemetry value. The icon always rides
/// muted; only the value picks up `Good`/`Warn`/`Bad` so color reads
/// as "this number means something is healthy / borderline / wrong"
/// rather than mere decoration.
#[derive(Clone, Copy)]
enum StatTone {
    Muted,
    Good,
    Warn,
    Bad,
}

fn stat_tone_color(theme: &iced::Theme, tone: StatTone) -> iced::Color {
    match tone {
        StatTone::Muted => widgets::muted_color(theme),
        StatTone::Good => theme.extended_palette().success.strong.color,
        // Amber lives outside iced's default palette, so hardcode a
        // tone that reads on both the dark navy and light parchment
        // HUD plates.
        StatTone::Warn => iced::Color::from_rgb(0.92, 0.67, 0.18),
        StatTone::Bad => theme.extended_palette().danger.strong.color,
    }
}

// Health tone per metric. Shared by the instrument-panel cells and the
// popover sparklines so the value readout and the chart points always agree
// on green/amber/red.

/// TPS vs the live fps target: green at/near rate, amber as it dips, red when
/// it falls well behind (visible netplay stutter). Muted before a target exists.
fn tone_for_tps(tps: f32, fps_target: f32) -> StatTone {
    if fps_target <= 0.0 {
        StatTone::Muted
    } else if tps >= fps_target - 1.0 {
        StatTone::Good
    } else if tps >= fps_target - 5.0 {
        StatTone::Warn
    } else {
        StatTone::Bad
    }
}

/// Clock skew: green near parity, amber drifting, red far out, by `|skew|`.
fn tone_for_skew(skew: i32) -> StatTone {
    match skew.unsigned_abs() {
        0..=3 => StatTone::Good,
        4..=7 => StatTone::Warn,
        _ => StatTone::Bad,
    }
}

/// Local lead by `|lead|`: green at a healthy steady lead, amber as it climbs,
/// red when it runs far from zero in either direction (the remote is lagging and
/// we're heading toward the bail, or we've fallen behind it).
fn tone_for_lead(lead: i32) -> StatTone {
    match lead.unsigned_abs() {
        0..=8 => StatTone::Good,
        9..=16 => StatTone::Warn,
        _ => StatTone::Bad,
    }
}

/// Rollback depth: green shallow, amber climbing, red when speculation runs deep.
fn tone_for_depth(depth: u32) -> StatTone {
    match depth {
        0..=2 => StatTone::Good,
        3..=5 => StatTone::Warn,
        _ => StatTone::Bad,
    }
}

/// Latency band: green under 80 ms, amber under 140 ms, red beyond.
fn tone_for_ping(ping_ms: u128) -> StatTone {
    if ping_ms < 80 {
        StatTone::Good
    } else if ping_ms < 140 {
        StatTone::Warn
    } else {
        StatTone::Bad
    }
}

// Value formatting for the telemetry readouts.

/// Signed skew in a 3-wide field; bare `0` at parity reads calmer than `+0`.
fn fmt_skew(skew: i32) -> String {
    if skew == 0 {
        "0".to_string()
    } else {
        format!("{skew:+}")
    }
}
/// Signed local lead in ticks; bare `0` at zero reads calmer than `+0`.
fn fmt_lead(lead: i32) -> String {
    if lead == 0 {
        "0".to_string()
    } else {
        format!("{lead:+}")
    }
}
/// Rollback depth.
fn fmt_depth(depth: u32) -> String {
    format!("{depth}")
}
/// Latency in ms.
fn fmt_ping(ping_ms: u128) -> String {
    format!("{ping_ms} ms")
}

/// One telemetry cell: a label `icon` and the current `value`, both
/// color-coded by the health `tone`. The full metric name lives in the
/// match-settings panel's captions, so the cell carries no hover tooltip.
/// Flat plate behind the telemetry deck — a faint fill + hairline
/// border so the readout reads as one grouped module without drawing
/// attention to itself. Realized as a button style (not a static
/// container) because the instrument panel is clickable: a subtle
/// hover/press brighten marks it as the trigger for the match-settings
/// popover. PvP-only.
fn telemetry_plate_button(theme: &iced::Theme, status: iced::widget::button::Status) -> iced::widget::button::Style {
    use iced::widget::button::Status;
    let p = theme.extended_palette();
    let text = theme.palette().text;
    let bg = theme.palette().background;
    // Mostly-opaque scrim in the page background color — same
    // recipe as [`hud_chip_plate`], so every floating HUD button
    // reads over live game pixels. Hover/press nudge the plate
    // toward the text color.
    let plate = match status {
        Status::Hovered => widgets::mix(bg, text, 0.10),
        Status::Pressed => widgets::mix(bg, text, 0.16),
        _ => bg,
    };
    iced::widget::button::Style {
        background: Some(iced::Background::Color(iced::Color { a: 0.85, ..plate })),
        text_color: text,
        border: iced::Border {
            radius: 6.0.into(),
            width: 1.0,
            color: iced::Color {
                a: if p.is_dark { 0.10 } else { 0.08 },
                ..text
            },
        },
        ..Default::default()
    }
}

/// [`telemetry_plate_button`] variant for the overlay's Close X:
/// the same quiet floating chip at rest, but hover and press flip
/// to a solid danger plate with a white glyph — the titlebar-close
/// idiom (`widgets::window_close`), adapted to sit over live game
/// pixels instead of the nav bar.
fn overlay_close_button(theme: &iced::Theme, status: iced::widget::button::Status) -> iced::widget::button::Style {
    use iced::widget::button::Status;
    let danger = theme.palette().danger;
    match status {
        Status::Hovered | Status::Pressed => iced::widget::button::Style {
            background: Some(iced::Background::Color(if matches!(status, Status::Pressed) {
                widgets::mix(danger, iced::Color::BLACK, 0.15)
            } else {
                danger
            })),
            text_color: iced::Color::WHITE,
            border: iced::Border {
                radius: 6.0.into(),
                width: 1.0,
                color: iced::Color::TRANSPARENT,
            },
            ..Default::default()
        },
        _ => telemetry_plate_button(theme, status),
    }
}

/// Container twin of [`telemetry_plate_button`]'s resting plate —
/// the flat translucent fill + hairline border the floating chips
/// use, for surfaces that aren't buttons (the replay transport
/// bar). Keeps every floating HUD piece in one visual family.
fn hud_chip_plate(theme: &iced::Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    let text = theme.palette().text;
    // A mostly-opaque scrim in the page background color — the
    // chips' sheer text-tint wash is fine behind one icon, but
    // the bar carries readouts and a scrubber over live game
    // pixels, where it was too transparent to read against.
    iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color {
            a: 0.85,
            ..theme.palette().background
        })),
        text_color: Some(text),
        border: iced::Border {
            radius: 6.0.into(),
            width: 1.0,
            color: iced::Color {
                a: if p.is_dark { 0.10 } else { 0.08 },
                ..text
            },
        },
        ..Default::default()
    }
}

/// Docked sidebar surface for the PvP setup drawers — flush with
/// its screen edge, full height, square corners, no border or
/// shadow. A near-opaque scrim in the page background so the save
/// view inside stays readable over the bezel art without reading
/// as a floating card.
fn setup_sidebar_plate(theme: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color {
            a: 0.95,
            ..theme.palette().background
        })),
        text_color: Some(theme.palette().text),
        ..Default::default()
    }
}

/// Pick-list twin of [`telemetry_plate_button`] — flat translucent
/// plate + hairline border instead of the chunky gradient, so the
/// replay bar's speed picker reads as the same family as the
/// floating chips around it.
fn flat_pick_list(
    theme: &iced::Theme,
    status: sweeten::widget::pick_list::Status,
) -> sweeten::widget::pick_list::Style {
    use sweeten::widget::pick_list::Status;
    let p = theme.extended_palette();
    let text = theme.palette().text;
    let base = if p.is_dark { 0.06 } else { 0.05 };
    let fill = match status {
        Status::Hovered => base + 0.06,
        Status::Opened { .. } => base + 0.10,
        Status::Active => base,
    };
    sweeten::widget::pick_list::Style {
        text_color: text,
        placeholder_color: widgets::muted_color(theme),
        handle_color: widgets::muted_color(theme),
        background: iced::Background::Color(iced::Color { a: fill, ..text }),
        border: iced::Border {
            radius: 6.0.into(),
            width: 1.0,
            color: iced::Color {
                a: if p.is_dark { 0.10 } else { 0.08 },
                ..text
            },
        },
    }
}

/// Render the active session — framebuffer, header, and (for replays
/// only) the transport row with play/pause + scrubber + prefetch %.
/// Pass the App's `session: State` borrow.
/// Vertical clearance that floats a bottom-anchored popover just
/// above the replay transport bar (bottom margin + strip padding
/// + control height + plate border + gap).
const POPOVER_LIFT: f32 = 12.0 + 16.0 + 32.0 + 2.0 + 6.0;

/// Width of a PvP setup side pane (the save view inside the
/// drawer).
const SETUP_PANE_WIDTH: f32 = 420.0;

/// Total travel of a setup drawer — what the sidebar slides
/// through on open/close and how far its edge handle rides
/// inward. Equal to the pane width: the sidebar docks flush with
/// its screen edge.
const SETUP_DRAWER_TRAVEL: f32 = SETUP_PANE_WIDTH;

/// How far the floating controls sink when hiding — past the
/// window's bottom edge (panel height + bottom margin, with a
/// little extra for the drop shadow).
const CONTROLS_SLIDE: f32 = 90.0;

pub fn view<'a>(
    lang: &'a LanguageIdentifier,
    state: &'a State,
    fractional_scaling: bool,
    hide_emulator_border: bool,
    effect: &'static Effect,
) -> Element<'a, Message> {
    let Some(session) = state.active.as_ref() else {
        return iced::widget::Space::new().width(Fill).height(Fill).into();
    };

    let frame = framebuffer_view(state, fractional_scaling, effect);
    let mut layout = column![].spacing(0).width(Fill).height(Fill);
    layout = layout.push(emulator_body(session, state, frame, hide_emulator_border));

    // The controls live in a floating bar over the emulator (no
    // reserved bottom strip), sliding away after the cursor sits
    // still — see `floating_controls`. When fully hidden it isn't
    // in the tree at all, so no invisible buttons linger where it
    // used to be.
    let mut stacked = stack![Element::from(layout)];
    // A drawer pane mid-animation draws in iced's floating layer,
    // above every base stack layer — so for those moments the
    // telemetry plate is hoisted into the floating layer too, where
    // tree order puts it back on top of the moving pane. See
    // `keep_above_drawers` for why it isn't hoisted permanently.
    // The top-right commands stay un-hoisted on purpose: the
    // drawers are supposed to cover them.
    let now = iced::time::Instant::now();
    let drawer_moving = state.self_panel_anim.is_animating(now) || state.opponent_panel_anim.is_animating(now);
    if state.controls_anim.visible(now) {
        // Replay: transport bar; PvP: setup-drawer edge handles.
        // SP has nothing down here.
        if !matches!(session, ActiveSession::SinglePlayer(_)) {
            stacked = stacked.push(floating_controls(lang, session, state));
        }
        // Every session: Settings + tear-down, top-right (PvP's
        // tear-down routes through the disconnect confirm).
        // Pushed BEFORE the setup drawers so an open drawer layers
        // over them rather than the buttons intruding on the pane.
        stacked = stacked.push(corner_commands_overlay(lang, session, state));
    }
    // PvP setup drawers — above the corner commands, below the
    // telemetry plate (see `setup_drawers_overlay`).
    for pane in setup_drawers_overlay(lang, session, state) {
        stacked = stacked.push(pane);
    }
    if let Some(o) = scrub_thumbnail_overlay(session, state) {
        stacked = stacked.push(o);
    }
    // PvP signal indicator / expanded telemetry graph, bottom-right.
    // Deliberately outside the floating-controls gate — connection
    // health stays glanceable even when the controls tuck away.
    if let Some(o) = telemetry_overlay(lang, session, state) {
        stacked = stacked.push(keep_above_drawers(o, drawer_moving));
    }
    if let Some(o) = disconnect_overlay(lang, session, state) {
        stacked = stacked.push(o);
    }
    // Any cursor movement over the session wakes the controls.
    // iced's mouse_area, not sweeten's: sweeten 0.14 gates all its
    // enter/move/exit dispatches on the cursor being inside the
    // bounds, which makes `on_exit` unreachable (the cursor is
    // outside by definition when it fires).
    iced::widget::mouse_area(stacked)
        .on_move(|_| Message::MouseMoved)
        .into()
}

/// The floating controls bar: the transport / toggles strip in a
/// [`widgets::panel`] plate, bottom-anchored over the emulator.
/// Hiding slides it past the window's bottom edge — iced has no
/// subtree opacity to fade with, but fully clearing the edge
/// reads the same. The bar's own hover pin keeps it up while the
/// cursor rests on it.
fn floating_controls<'a>(
    lang: &'a LanguageIdentifier,
    session: &'a ActiveSession,
    state: &'a State,
) -> Element<'a, Message> {
    let now = iced::time::Instant::now();
    let hide_progress = state.controls_anim.progress(now);
    // Replay transport carries a Fill-width scrubber, so its bar
    // spans the window; PvP's setup toggles ride the screen edges
    // as drawer handles instead — see `setup_handles_overlay`.
    let Some(r) = session.as_replay() else {
        return setup_handles_overlay(lang, session, state, hide_progress);
    };
    let panel = container(replay_bar(lang, r, state)).width(Fill).style(hud_chip_plate);
    // iced's mouse_area — sweeten's `on_exit` never fires (see the
    // note in `view`), which left the hover pin stuck and the bar
    // permanently visible.
    let hover_pin = iced::widget::mouse_area(panel)
        .on_enter(Message::ControlsHovered(true))
        .on_exit(Message::ControlsHovered(false));
    let slid = anim::slide_in(hover_pin, hide_progress, iced::Vector::new(0.0, CONTROLS_SLIDE));
    container(slid)
        .width(Fill)
        .height(Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(12)
        .into()
}

/// The live framebuffer, rendered through a custom wgpu shader widget
/// (one persistent GPU texture, written in place each vblank) instead
/// of a per-frame `image` handle. The shader fills the widget's
/// bounds, so the widget is sized to the framebuffer rect — an exact
/// integer multiple (crisp, the default) or a smooth aspect-fit —
/// using `responsive` for the pane size both need. Before the first
/// frame, a 1×1 black placeholder keeps the pane opaque.
fn framebuffer_view<'a>(state: &'a State, fractional_scaling: bool, effect: &'static Effect) -> Element<'a, Message> {
    // Post-filter framebuffer dimensions. Drive the scale math below;
    // match the (w, h) `build_frame_pixels` stamps into the frame the
    // `framebuffer` shader uploads.
    // The widget is sized to native·scale — the same rectangle the old CPU
    // upscalers produced — and the effect's fragment shader magnifies the
    // native texture to fill it.
    let scale = effect.scale;
    let img_w = (replay::SCREEN_WIDTH * scale) as f32;
    let img_h = (replay::SCREEN_HEIGHT * scale) as f32;

    iced::widget::responsive(move |size| {
        let raw = (size.width / img_w).min(size.height / img_h);
        let scale = if fractional_scaling {
            raw.max(0.0)
        } else {
            raw.floor().max(1.0)
        };
        let (w, h) = (img_w * scale, img_h * scale);

        let frame = state
            .current_frame
            .clone()
            .unwrap_or_else(crate::video::framebuffer::Frame::black);
        let fb = iced::widget::shader::Shader::new(crate::video::framebuffer::Program::new(frame))
            .width(Length::Fixed(w))
            .height(Length::Fixed(h));

        let centered = |content: Element<'a, Message>| -> Element<'a, Message> {
            iced::widget::container(content)
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .into()
        };

        if fractional_scaling {
            // Smooth aspect-fit, centered, no drop shadow.
            centered(fb.into())
        } else {
            // Tight container around the Fixed-size framebuffer so the
            // shadow style traces its edges, not the surrounding pane.
            let framed = iced::widget::container(fb)
                .width(Length::Fixed(w))
                .height(Length::Fixed(h))
                .style(|_theme: &iced::Theme| iced::widget::container::Style {
                    shadow: iced::Shadow {
                        color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.55),
                        offset: iced::Vector::new(0.0, 8.0),
                        blur_radius: 24.0,
                    },
                    ..Default::default()
                });
            centered(framed.into())
        }
    })
    .into()
}

/// Body: framebuffer + optional setup panes layered over the game's
/// BNLC background art (cover-fit, crops as needed) or a pure-black
/// backdrop when BNLC isn't installed. The backdrop spans the full
/// body width so the setup panes float on top of the same bezel art.
fn emulator_body<'a>(
    session: &'a ActiveSession,
    state: &'a State,
    frame: Element<'a, Message>,
    hide_emulator_border: bool,
) -> Element<'a, Message> {
    let frame_container = container(frame).center(Fill);
    let bnlc_bg = if hide_emulator_border {
        None
    } else {
        background_handle(session.local_game())
    };
    let backdrop: Element<'a, Message> = match bnlc_bg {
        Some(bg_handle) => iced::widget::image(bg_handle)
            .width(Fill)
            .height(Fill)
            .content_fit(iced::ContentFit::Cover)
            .into(),
        None => container(iced::widget::Space::new().width(Fill).height(Fill))
            .style(|_: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(iced::Color::BLACK)),
                ..Default::default()
            })
            .into(),
    };

    // Left/right drawer SLOTS for PvP. The panes themselves render
    // as overlay layers in [`view`] (`setup_drawers_overlay`) so
    // they can layer above the corner commands; the row only claims
    // their width so the emulator docks aside. The space is claimed
    // eagerly and handed back eagerly: an OPEN drawer holds its slot
    // (while the pane slides in over it), but the moment it starts
    // closing the slot collapses — the emulator expands right away —
    // and the exit slide plays out over the reflowed body. The
    // matching edge handle rides the drawer's inner edge either way
    // (`setup_handles_overlay`).
    let drawer_slot = || {
        iced::widget::Space::new()
            .width(iced::Length::Fixed(SETUP_PANE_WIDTH))
            .height(Fill)
    };
    let mut content_row = row![].spacing(0).height(Fill).width(Fill);
    if let ActiveSession::PvP(s) = session {
        if s.local_loaded.is_some() && state.show_self_panel {
            content_row = content_row.push(drawer_slot());
        }
    }
    content_row = content_row.push(container(frame_container).width(Fill).height(Fill));
    if let ActiveSession::PvP(s) = session {
        if s.opponent_loaded.is_some() && state.show_opponent_panel {
            content_row = content_row.push(drawer_slot());
        }
    }
    let body = stack![backdrop, Element::from(content_row)];
    container(body).width(Fill).height(Fill).into()
}

/// The PvP setup drawers, one overlay layer per visible pane. Each
/// is a docked sidebar flush with its screen edge — full height,
/// square corners, only the content padded — whose width
/// `emulator_body`'s drawer slots reserve in the layout while it's
/// open. Rendered as stack layers (rather than row members) so the
/// drawers sit ABOVE the corner commands — an open drawer covers
/// the Settings / Close buttons instead of having them intrude on
/// its content — and BELOW the telemetry plate, which stays
/// glanceable over an open drawer (see the layer order in [`view`]).
/// Mid-slide the panes draw in iced's floating layer, above every
/// base layer (see `anim::slide_in` / `keep_above_drawers`).
fn setup_drawers_overlay<'a>(
    lang: &'a LanguageIdentifier,
    session: &'a ActiveSession,
    state: &'a State,
) -> Vec<Element<'a, Message>> {
    let ActiveSession::PvP(s) = session else {
        return Vec::new();
    };
    let now = iced::time::Instant::now();
    let setup_pane = |panel: Element<'a, Message>, from_dx: f32, progress: f32| -> Element<'a, Message> {
        let pane = container(panel)
            .width(iced::Length::Fixed(SETUP_PANE_WIDTH))
            .height(Fill)
            .padding(style::PANE_PADDING)
            .style(setup_sidebar_plate);
        // An opaque plate must be opaque to the mouse too: iced's
        // Stack lets the cursor reach lower layers anywhere the
        // upper one reports no interaction, so without this a
        // click on a quiet patch of the pane would press the
        // corner commands hidden beneath it.
        let pane = iced::widget::mouse_area(pane).interaction(iced::mouse::Interaction::Idle);
        anim::slide_in(pane, progress, iced::Vector::new(from_dx, 0.0))
    };
    let mut panes: Vec<Element<'a, Message>> = Vec::new();
    if s.local_loaded.is_some() && (state.show_self_panel || state.self_panel_anim.is_animating(now)) {
        let me = s.local_loaded.as_ref().unwrap();
        let panel = save_view::view(lang, me, &s.local_save_view, true, None, false, false)
            .map(Message::SelfSaveViewAction);
        let pane = setup_pane(panel, -SETUP_DRAWER_TRAVEL, state.self_panel_anim.progress(now));
        panes.push(
            container(pane)
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Left)
                .into(),
        );
    }
    if s.opponent_loaded.is_some() && (state.show_opponent_panel || state.opponent_panel_anim.is_animating(now)) {
        let opponent = s.opponent_loaded.as_ref().unwrap();
        let panel = save_view::view(lang, opponent, &s.opponent_save_view, true, None, false, false)
            .map(Message::OpponentSaveViewAction);
        let pane = setup_pane(panel, SETUP_DRAWER_TRAVEL, state.opponent_panel_anim.progress(now));
        panes.push(
            container(pane)
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .into(),
        );
    }
    panes
}

/// The replay bar's strip: full transport (play/pause + scrubber +
/// tick readouts) plus the options trigger, at the chunky
/// BAR_CONTROL_HEIGHT sizing. SP/PvP don't use this — their few
/// controls live in compact corner chips ([`corner_chips`]).
fn replay_bar<'a>(
    lang: &'a LanguageIdentifier,
    r: &'a replay::ReplaySession,
    state: &'a State,
) -> sweeten::widget::Row<'a, Message> {
    // No ellipsis popover for replays — the speed picker sits
    // directly in the bar, and Settings + Close float top-right
    // (see `corner_commands_overlay`).
    let current = r.speed();
    let speed_options: Vec<widgets::Choice<u32>> = [0.5f32, 1.0, 2.0, 4.0]
        .iter()
        .map(|&v| {
            let label = if (v - v.trunc()).abs() < 1e-3 {
                format!("{}×", v as i32)
            } else {
                format!("{:.1}×", v)
            };
            widgets::Choice::new((v * 10.0) as u32, label)
        })
        .collect();
    let selected = speed_options
        .iter()
        .find(|c| c.value == (current * 10.0) as u32)
        .cloned();
    let speed_picker = sweeten::widget::pick_list(speed_options, selected, |c: widgets::Choice<u32>| {
        Message::SetSpeed(c.value as f32 / 10.0)
    })
    .padding([6.0, 10.0])
    .width(Length::Fixed(78.0))
    .style(flat_pick_list);

    let controls = row![].spacing(10).align_y(Alignment::Center).padding([8, 8]);
    let controls = replay_transport(lang, r, state, controls);
    controls.push(speed_picker)
}

/// Hoist a persistent chrome layer into iced's floating layer —
/// an invisible sub-pixel translation — so it keeps drawing above
/// a drawer pane mid-animation (which floats, and floats render
/// above every base stack layer; among floats, tree order wins,
/// and these layers come after the drawer). Used by the telemetry
/// plate, the one piece of chrome that outranks the drawers — the
/// top-right commands deliberately layer UNDER them instead. Only
/// applied while a drawer is actually moving: hoisted permanently,
/// the chrome would also paint over the settings/disconnect modals.
fn keep_above_drawers(el: Element<'_, Message>, drawer_moving: bool) -> Element<'_, Message> {
    if drawer_moving {
        iced::widget::float(el)
            .translate(|_bounds, _viewport| iced::Vector::new(0.0, 0.001))
            .into()
    } else {
        el
    }
}

/// The unified session command cluster, top-right in every
/// session type: the Settings gear and the tear-down button —
/// direct Close for replay/SP, the disconnect confirm for PvP.
/// Rides the same auto-hide transition as the rest of the
/// controls, sliding up past the top edge when the cursor goes
/// idle.
fn corner_commands_overlay<'a>(
    lang: &'a LanguageIdentifier,
    session: &'a ActiveSession,
    state: &'a State,
) -> Element<'a, Message> {
    let now = iced::time::Instant::now();
    let cmd = |icon: Icon,
               label: String,
               msg: Message,
               style: fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style|
     -> Element<'a, Message> {
        let btn = button(icon.widget().size(16.0)).padding([6.0, 8.0]).style(style).on_press(msg);
        iced::widget::tooltip(
            btn,
            widgets::tooltip_bubble(label),
            iced::widget::tooltip::Position::Bottom,
        )
        .gap(4)
        .into()
    };
    // Same X + "Close" tooltip in every session type. A live PvP
    // match routes through the disconnect confirm (whose copy
    // carries the unplug framing); once the link is already gone
    // (`latency()` = None ⇒ remote dropped) there's nothing left
    // to protect, so it closes directly.
    let tear_down_msg = match session {
        ActiveSession::PvP(pvp) if pvp.latency().is_some() => Message::OpenDisconnectConfirm,
        _ => Message::Close,
    };
    let tear_down = cmd(Icon::X, t!(lang, "playback-close"), tear_down_msg, overlay_close_button);
    let cluster = row![
        cmd(
            Icon::Settings,
            t!(lang, "tab-settings"),
            Message::OpenSettings,
            telemetry_plate_button
        ),
        tear_down,
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let pinned = iced::widget::mouse_area(cluster)
        .on_enter(Message::ControlsHovered(true))
        .on_exit(Message::ControlsHovered(false));
    // While the opponent drawer is open the cluster sits behind it
    // (see the layer order in `view`) — skip the auto-hide slide
    // then. The slide draws in iced's floating layer
    // (`anim::slide_in`), which would pop the buttons OVER the
    // drawer they're supposed to be under for the length of the
    // animation; at rest behind the drawer the slide is invisible
    // anyway.
    let behind_drawer = match session {
        ActiveSession::PvP(pvp) => pvp.opponent_loaded.is_some() && state.show_opponent_panel,
        _ => false,
    };
    let progress = if behind_drawer {
        1.0
    } else {
        state.controls_anim.progress(now)
    };
    let slid = anim::slide_in(pinned, progress, iced::Vector::new(0.0, -CONTROLS_SLIDE));
    container(slid)
        .width(Fill)
        .height(Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Top)
        .padding(12)
        .into()
}

/// PvP setup-panel handles, riding the screen edges they control:
/// the red "my setup" handle vertically centered on the LEFT edge
/// (the side its pane occupies), the blue opponent handle on the
/// RIGHT. Each is drawn as a tab emerging from its edge — square
/// against the edge, rounded on the inner corners — with a
/// chevron that points the way the click will move the drawer
/// (inward to open, back out to close) tinted in the side's
/// accent. They ride the shared auto-hide, slipping out through
/// their own edges; the opponent handle isn't rendered at all
/// while the peer has blinded their setup.
fn setup_handles_overlay<'a>(
    lang: &'a LanguageIdentifier,
    session: &'a ActiveSession,
    state: &'a State,
    hide_progress: f32,
) -> Element<'a, Message> {
    let ActiveSession::PvP(pvp) = session else {
        return iced::widget::Space::new().into();
    };

    let now = iced::time::Instant::now();

    // `on_left`: which screen edge the tab grows out of.
    // `drawer_progress`: how far the tab's drawer is out (0..1) —
    // the tab rides the drawer's moving inner edge.
    let handle = |on_left: bool,
                  open: bool,
                  drawer_progress: f32,
                  accent: Color,
                  label: String,
                  msg: Option<Message>|
     -> Element<'a, Message> {
        // Open chevrons point back toward the edge (push the
        // drawer shut), closed ones inward (pull it open).
        let icon = match (on_left, open) {
            (true, false) | (false, true) => Icon::ChevronRight,
            (true, true) | (false, false) => Icon::ChevronLeft,
        };
        // Square against the edge, rounded inner corners.
        let radius = if on_left {
            iced::border::Radius {
                top_left: 0.0,
                top_right: 8.0,
                bottom_right: 8.0,
                bottom_left: 0.0,
            }
        } else {
            iced::border::Radius {
                top_left: 8.0,
                top_right: 0.0,
                bottom_right: 0.0,
                bottom_left: 8.0,
            }
        };
        let enabled = msg.is_some();
        let style = move |theme: &iced::Theme, status: iced::widget::button::Status| {
            let mut st = if open {
                // Lit accent plate while the pane is out.
                widgets::tinted_button(theme, status, accent)
            } else {
                telemetry_plate_button(theme, status)
            };
            if !open {
                // The chevron carries the side's identity even at
                // rest; a disabled handle goes muted instead.
                st.text_color = if enabled { accent } else { widgets::muted_color(theme) };
            }
            st.border.radius = radius;
            // No glow shadow on a flush tab — it would paint a
            // halo onto the screen edge.
            st.shadow = iced::Shadow::default();
            st
        };
        let mut btn = button(
            container(icon.widget().size(14.0))
                .width(Fill)
                .height(Fill)
                .center(Fill),
        )
        .padding(0)
        .width(iced::Length::Fixed(18.0))
        .height(iced::Length::Fixed(56.0))
        .style(style);
        if let Some(m) = msg {
            btn = btn.on_press(m);
        }
        let tip = iced::widget::tooltip(
            btn,
            widgets::tooltip_bubble(label),
            if on_left {
                iced::widget::tooltip::Position::Right
            } else {
                iced::widget::tooltip::Position::Left
            },
        )
        .gap(4);
        let pinned = iced::widget::mouse_area(tip)
            .on_enter(Message::ControlsHovered(true))
            .on_exit(Message::ControlsHovered(false));
        // Riding the drawer's inner edge is LAYOUT (edge padding),
        // not a Float translation: a floating element draws in
        // iced's overlay layer above everything, so a permanently
        // translated handle would sit on top of the settings /
        // disconnect modals whenever a drawer is open.
        let ride = drawer_progress * SETUP_DRAWER_TRAVEL;
        let positioned: Element<'a, Message> = container(pinned)
            .padding(iced::Padding {
                top: 0.0,
                right: if on_left { 0.0 } else { ride },
                bottom: 0.0,
                left: if on_left { ride } else { 0.0 },
            })
            .into();
        // The auto-hide slide through the screen edge stays a
        // Float (it needs to go off-screen), but it's transient —
        // suppressed entirely while the drawer is out at all,
        // where a tab twitching toward the edge would read as a
        // glitch (and an open drawer needs its close affordance).
        let hide = if drawer_progress > 0.0 {
            0.0
        } else {
            (1.0 - hide_progress) * if on_left { -28.0 } else { 28.0 }
        };
        if hide == 0.0 {
            positioned
        } else {
            iced::widget::float(positioned)
                .translate(move |_bounds, _viewport| iced::Vector::new(hide, 0.0))
                .into()
        }
    };

    const FIELD_RED: Color = Color::from_rgb(0.85, 0.22, 0.28);
    const FIELD_BLUE: Color = Color::from_rgb(0.18, 0.40, 0.85);

    let mut edges = row![].width(Fill).align_y(Alignment::Center);
    if pvp.local_loaded.is_some() {
        edges = edges.push(handle(
            true,
            state.show_self_panel,
            state.self_panel_anim.progress(now),
            FIELD_RED,
            t!(lang, "session-self"),
            Some(Message::ToggleSelfPanel),
        ));
    }
    edges = edges.push(horizontal_space());
    // No tab at all when the peer blinded their setup — a
    // permanently dead handle is just clutter on the edge.
    if pvp.opponent_loaded.is_some() {
        edges = edges.push(handle(
            false,
            state.show_opponent_panel,
            state.opponent_panel_anim.progress(now),
            FIELD_BLUE,
            t!(lang, "session-opponent"),
            Some(Message::ToggleOpponentPanel),
        ));
    }

    container(edges)
        .width(Fill)
        .height(Fill)
        .align_y(iced::alignment::Vertical::Center)
        .into()
}

/// The replay transport: circular play/pause, current tick, scrubber,
/// total tick — pushed onto the strip in that order.
fn replay_transport<'a>(
    lang: &'a LanguageIdentifier,
    r: &'a replay::ReplaySession,
    state: &State,
    controls: sweeten::widget::Row<'a, Message>,
) -> sweeten::widget::Row<'a, Message> {
    let total = r.total_ticks().max(1);
    // Playhead priority: the tick under an active drag, else the target
    // of an in-flight seek (so the handle doesn't snap back while the
    // chase catches up), else the emulator's actual position.
    let cur = state
        .scrub_preview
        .or_else(|| r.pending_seek_target())
        .unwrap_or_else(|| r.current_tick())
        .min(total);
    let prefetched = r.prefetch_progress().min(total);
    // The mgba thread is paused for the duration of a scrub drag and
    // the seek chase that follows it, but when playback resumes on
    // landing the session is logically still *playing* — flipping the
    // button to "Play" mid-scrub reads as a stuck pause.
    let logically_playing = (state.scrub_preview.is_some() && state.scrub_resume) || r.seek_will_resume();
    let (play_pause_icon, play_pause_label, paused) = if r.is_paused() && !logically_playing {
        (Icon::Play, t!(lang, "playback-play"), true)
    } else {
        (Icon::Pause, t!(lang, "playback-pause"), false)
    };
    let scrub = replay::scrubber::Scrubber::new(
        cur,
        total,
        prefetched,
        Message::ScrubPreview,
        Message::ScrubCommit,
        Message::ScrubHover,
    )
    .round_boundaries(r.round_boundaries())
    .view();

    // Play/Pause is the transport's centerpiece — promote to
    // the primary-button style when paused (the affordance
    // the user is most likely looking for at rest) and keep
    // it neutral while playing. Either way it sits a notch
    // bigger than the other strip controls and is rendered
    // as a perfect circle (square padding + huge radius) so
    // it reads as a console transport button instead of a
    // generic pill.
    let base_style: fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style = if paused {
        // Paused keeps the one accent in the bar — Play is the
        // affordance the user is looking for at rest.
        widgets::primary_button
    } else {
        // Playing rides the same flat plate as the floating chips.
        telemetry_plate_button
    };
    let play_pause_style = move |theme: &iced::Theme, status: iced::widget::button::Status| {
        let mut style = base_style(theme, status);
        style.border.radius = 999.0.into();
        style
    };
    // Compact circle, a notch bigger than the chip buttons so it
    // still reads as the transport's centerpiece.
    let play_pause_btn = iced::widget::tooltip(
        button(
            iced::widget::container(play_pause_icon.widget().size(16.0))
                .width(iced::Length::Fixed(18.0))
                .height(iced::Length::Fixed(18.0))
                .center(Fill),
        )
        .padding(0)
        .width(iced::Length::Fixed(32.0))
        .height(iced::Length::Fixed(32.0))
        .style(play_pause_style)
        .on_press(Message::TogglePlay),
        widgets::tooltip_bubble(play_pause_label),
        iced::widget::tooltip::Position::Top,
    )
    .gap(4);

    // Tick readouts: monospaced + bumped one tier above caption
    // so they read as digital-clock numerals rather than
    // metadata, primary-tinted so the eye picks them up as
    // playback state.
    let tick_style = |theme: &iced::Theme| iced::widget::text::Style {
        color: Some(theme.palette().primary),
    };
    controls
        .push(play_pause_btn)
        .push(
            text(format_tick(cur))
                .size(14)
                .font(iced::Font::MONOSPACE)
                .style(tick_style),
        )
        .push(scrub)
        .push(
            text(format_tick(total))
                .size(14)
                .font(iced::Font::MONOSPACE)
                .style(widgets::muted_text_style),
        )
}

/// PvP-only telemetry deck: P1/P2 tag, TPS, frame skew, rollback
/// depth, ping — each metric drawn next to its current value, colored
/// by health (green/amber/red), gathered into one hairline-divided
/// plate. The plate is a button: clicking the instrument panel toggles
/// the match-settings popover anchored above it. Gated on a live
/// latency reading: `latency_raw()` is `Some` while the link is up
/// (even at 0 ms on LAN) and `None` the moment the remote drops — at
/// which point the telemetry is frozen and meaningless, so the panel
/// retires itself.
/// Sync-skew band → signal-bars icon. Full bars at parity,
/// dropping as the two sides drift apart — same bands as
/// [`tone_for_skew`], so the bars and the tint always agree.
fn signal_icon(skew: i32) -> Icon {
    match skew.unsigned_abs() {
        0..=3 => Icon::SignalHigh,
        4..=7 => Icon::SignalMedium,
        _ => Icon::SignalLow,
    }
}

/// Bottom-right telemetry overlay (PvP-only). At rest it's a
/// small, permanently-visible signal indicator — latency band as
/// colored signal bars, deliberately outside the auto-hide group
/// so connection health stays glanceable. Clicking it expands the
/// full graph view above the corner (P1/P2 sides, metric
/// sparklines, frame-delay knob); the chevron in the panel's
/// header collapses it back (Esc works too).
fn telemetry_overlay<'a>(
    lang: &'a LanguageIdentifier,
    session: &'a ActiveSession,
    state: &'a State,
) -> Option<Element<'a, Message>> {
    let ActiveSession::PvP(pvp) = session else {
        return None;
    };
    // Same link-up gate as `latency()`: nothing to show before the
    // first pong.
    pvp.latency_raw()?;
    let now = iced::time::Instant::now();

    let content: Element<'a, Message> = if state.match_settings_anim.visible(now) {
        // Expanded graph view. The header carries the players and
        // the collapse chevron — no latency readout here, the ping
        // sparkline below already shows it.
        //
        // The red/blue dots are FIELD sides (same coding as the
        // setup toggles and the matchup pane: red = your half,
        // blue = the opponent's), so your row always leads with
        // the red dot; the seat assignment rides in the P1/P2
        // label next to it.
        let collapse = button(Icon::ChevronDown.widget().size(14.0))
            .padding([4.0, 8.0])
            .style(widgets::neutral)
            .on_press(Message::ToggleMatchSettings);
        let side = |accent: Color, seat: &'static str, name: String| -> Element<'a, Message> {
            let dot = container(
                iced::widget::Space::new()
                    .width(Length::Fixed(8.0))
                    .height(Length::Fixed(8.0)),
            )
            .style(move |_: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(accent)),
                border: iced::Border {
                    radius: 999.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
            row![
                dot,
                text(seat).size(TEXT_CAPTION).font(iced::Font::MONOSPACE),
                text(name).size(TEXT_CAPTION).style(widgets::muted_text_style),
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
        };
        const FIELD_RED: Color = Color::from_rgb(0.85, 0.22, 0.28);
        const FIELD_BLUE: Color = Color::from_rgb(0.18, 0.40, 0.85);
        let (local_seat, remote_seat) = if pvp.local_player_index() == 0 {
            ("P1", "P2")
        } else {
            ("P2", "P1")
        };
        let players = row![
            side(FIELD_RED, local_seat, t!(lang, "play-you")),
            side(FIELD_BLUE, remote_seat, t!(lang, "play-opponent")),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        let header = row![players, horizontal_space(), collapse]
            .spacing(8)
            .align_y(Alignment::Center);

        // Pin the column to the cards' width — the Fill spacer in
        // the header would otherwise stretch the panel out to the
        // whole window.
        let panel = container(
            column![header, match_settings_content(lang, pvp, &state.metric_history)]
                .spacing(8)
                .width(Length::Fixed(PANEL_W)),
        )
        .padding(12)
        .style(widgets::panel);
        anim::pop(panel, state.match_settings_anim.progress(now), 8.0)
    } else {
        // Collapsed: signal bars showing the SYNC health — how far
        // the two sides have drifted (skew) — with the live frame
        // count as a tooltip. Between rounds there's no skew
        // reading; ride muted full bars until the next one starts.
        let (icon, tone, reading) = match pvp.round_stats() {
            Some(stats) => (
                signal_icon(stats.skew),
                tone_for_skew(stats.skew),
                format!("{:+}", stats.skew),
            ),
            None => (Icon::SignalHigh, StatTone::Muted, "—".to_string()),
        };
        let icon_el = icon
            .widget()
            .size(18.0)
            .style(move |theme: &iced::Theme| iced::widget::text::Style {
                color: Some(stat_tone_color(theme, tone)),
            });
        let chip = button(icon_el)
            .padding([5.0, 8.0])
            .style(telemetry_plate_button)
            .on_press(Message::ToggleMatchSettings);
        iced::widget::tooltip(
            chip,
            widgets::tooltip_bubble(reading),
            iced::widget::tooltip::Position::Left,
        )
        .gap(4)
        .into()
    };

    Some(
        container(content)
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Bottom)
            .padding(12)
            .into(),
    )
}

/// Match-settings popover (PvP-only), anchored above the telemetry
/// plate that triggers it. Currently holds just the live frame-delay
/// control (moved here from the footer), but it's the home for any
/// future in-match knobs. Like the options menu it owns no dismiss
/// backdrop — clicking the plate again or pressing Esc closes it. No
/// heading: the frame-delay row already labels itself.
/// Floating keyframe thumbnail + timestamp, hovering above the scrub
/// bar while the cursor rests on it (replay-only). Centered on the
/// cursor and clamped to the window edges (with a small margin so it
/// never sits flush against the border), lifted to the same height as
/// the bottom-anchored popovers. `responsive` is how the clamp learns
/// the window width — the overlay layer spans the whole session view.
/// Pure presentation — no mouse handlers anywhere in the chain, so it
/// never steals events from the transport below.
fn scrub_thumbnail_overlay<'a>(session: &'a ActiveSession, state: &'a State) -> Option<Element<'a, Message>> {
    session.as_replay()?;
    let h = state.scrub_hover?;
    let (_, handle) = state.scrub_thumb.as_ref()?;
    let handle = handle.clone();
    // Native 240×160 at 0.75 — big enough to read the scene, small
    // enough not to feel like a second screen.
    const THUMB_W: f32 = 180.0;
    const THUMB_H: f32 = 120.0;
    const CARD_PAD: f32 = 4.0;
    const EDGE_MARGIN: f32 = 8.0;
    Some(
        iced::widget::responsive(move |size| {
            let img = iced::widget::image(handle.clone())
                .width(Length::Fixed(THUMB_W))
                .height(Length::Fixed(THUMB_H));
            // Same numeral treatment as the transport's tick readouts
            // so the hover timestamp reads as playback state.
            let stamp = text(format_tick(h.tick))
                .size(TEXT_CAPTION)
                .font(iced::Font::MONOSPACE)
                .style(|theme: &iced::Theme| iced::widget::text::Style {
                    color: Some(theme.palette().primary),
                });
            // Same flat scrim plate as the transport bar below it.
            let card = container(column![img, stamp].spacing(2).align_x(Alignment::Center))
                .padding(CARD_PAD)
                .style(hud_chip_plate);
            let card_w = THUMB_W + CARD_PAD * 2.0;
            let hi = (size.width - EDGE_MARGIN - card_w).max(EDGE_MARGIN);
            let left = (h.x - card_w / 2.0).clamp(EDGE_MARGIN.min(hi), hi);
            container(card)
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Left)
                .align_y(iced::alignment::Vertical::Bottom)
                .padding(iced::Padding {
                    top: 0.0,
                    right: 0.0,
                    bottom: POPOVER_LIFT,
                    left,
                })
                .into()
        })
        .into(),
    )
}

/// Disconnect confirmation modal (PvP-only). Centered panel with a
/// dimmed click-to-dismiss backdrop — same shape as app.rs's
/// in-session Settings modal so the two read as the same family
/// of "this interrupts what you're doing" dialogs. Sits above
/// the options popover in the stack so it covers the menu if
/// the user somehow re-opened it.
fn disconnect_overlay<'a>(
    lang: &'a LanguageIdentifier,
    session: &'a ActiveSession,
    state: &'a State,
) -> Option<Element<'a, Message>> {
    let now = iced::time::Instant::now();
    if !(state.disconnect_anim.visible(now) && matches!(session, ActiveSession::PvP(_))) {
        return None;
    }
    let progress = state.disconnect_anim.progress(now);
    let title = text(t!(lang, "playback-disconnect-prompt")).size(TEXT_BODY + 4.0);
    let body_text = text(t!(lang, "playback-disconnect-detail")).style(widgets::muted_text_style);
    let cancel_btn = widgets::labeled_icon_button(
        Icon::X,
        t!(lang, "playback-cancel"),
        Message::CloseDisconnectConfirm,
        [8.0, 14.0],
        widgets::neutral,
    );
    let disconnect_btn = widgets::labeled_icon_button(
        Icon::Unplug,
        t!(lang, "playback-disconnect"),
        Message::Close,
        [8.0, 14.0],
        widgets::danger_button,
    );
    let buttons = row![horizontal_space(), cancel_btn, disconnect_btn]
        .spacing(8)
        .align_y(Alignment::Center);
    let panel = container(column![title, body_text, buttons].spacing(14).width(Fill))
        .width(iced::Length::Fixed(420.0))
        .padding(20)
        .style(widgets::panel);
    // Swallow clicks on the panel's inert regions (title,
    // body) so they don't fall through to the backdrop's
    // dismiss-on-press handler. Buttons inside the panel
    // still capture their own events.
    let panel_swallow = mouse_area(anim::pop(panel, progress, 8.0)).on_press(|_| Message::NoOp);
    let placement = container(panel_swallow)
        .width(Fill)
        .height(Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center);
    // Backdrop dim fades with the panel; the dismiss handler is
    // only armed while the modal is actually open so a click during
    // the fade-out can't re-fire the close.
    let mut backdrop = mouse_area(
        container(iced::widget::Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill)
            .style(anim::backdrop_style(0.55 * progress)),
    );
    if state.disconnect_anim.shown() {
        backdrop = backdrop.on_press(|_| Message::CloseDisconnectConfirm);
    }
    Some(iced::widget::stack![Element::from(backdrop), Element::from(placement)].into())
}


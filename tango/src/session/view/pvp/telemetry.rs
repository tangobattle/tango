//! PvP telemetry: one always-visible, single-row bottom status bar. P1/P2 sit
//! beside five tiny inline sparklines (TPS, frame skew, local lead, rollback
//! depth, ping), each in an equal-width lane whose value sizes naturally. A
//! trailing frame-delay chip opens its slider in a small
//! popover, keeping the resting bar shallow. Split out of the session view so
//! the emulator/drawer layout in `mod.rs` isn't sharing a file with the
//! charting + tone math.

use super::super::*;
use super::{Message, PvpSession};
// Explicit so these win over iced's prelude `column!`/`row!` macros (see mod.rs).
use sweeten::widget::{column, row};

/// Live frame-delay control: a turtle-icon heading naming it, over the lobby's
/// frame-delay row (slider, fixed-width numeric readout, latency-driven
/// "suggest" wand). Lifting the title into the heading frees the control line so
/// the slider gets lobby-like width even in the compact panel. Frame delay is
/// purely local display lag, so dragging it mid-match takes effect on the next
/// rendered frame with no peer coordination.
fn frame_delay_control<'a>(lang: &'a LanguageIdentifier, pvp: &'a PvpSession) -> Element<'a, Message> {
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
    let suggest_msg = pvp
        .latency()
        .map(|rtt| Message::SetFrameDelay(suggest_frame_delay(rtt)));
    let suggest = widgets::icon_button_maybe(
        Icon::Wand,
        t!(lang, "lobby-frame-delay-suggest"),
        suggest_msg,
        crate::ui::style::STANDARD_PADDING,
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
        .width(Length::Fixed(FRAME_DELAY_W))
        .into()
}

// Status-bar geometry. The five metric lanes divide the available width; each
// value takes its natural width and the sparkline flexes around it.
const FRAME_DELAY_W: f32 = 260.0;
const FRAME_DELAY_CLEARANCE: f32 = 52.0;
const SPARK_H: f32 = 16.0;
// Each metric's full-height value span (sample saturates into it). Chosen to
// line up with the tone thresholds so a point's height roughly tracks its color.
const TPS_SPAN: f32 = 8.0; // fps below target = floor of the chart
const SKEW_SPAN: i32 = 8; // ± about parity; 0 sits mid-height
const LEAD_SPAN: i32 = 24; // ± about zero; saturates well before the overflow bail
const DEPTH_SPAN: u32 = 8;
const PING_SPAN: u128 = 200;

/// A compact per-metric history chart for the persistent PvP panel. Each
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

/// One telemetry lane in the compact bar: icon, tiny sparkline, naturally sized
/// current value. The full localized caption moves to a tooltip, preserving the
/// meaning without adding a second line to the bar.
fn telemetry_card<'a>(
    icon: Icon,
    caption: String,
    control: Element<'a, Message>,
    value: Element<'a, Message>,
) -> Element<'a, Message> {
    let lane = row![
        icon.widget().size(14.0).style(widgets::muted_text_style),
        control,
        value,
    ]
    .spacing(5)
    .align_y(Alignment::Center)
    .width(Fill);
    iced::widget::tooltip(
        lane,
        widgets::tooltip_bubble(caption),
        iced::widget::tooltip::Position::Top,
    )
    .gap(5)
    .into()
}

/// A right-aligned monospace value readout, tinted by `tone`.
fn value_text<'a>(s: String, tone: Option<StatTone>) -> Element<'a, Message> {
    text(s)
        .size(TEXT_CAPTION)
        .font(iced::Font::MONOSPACE)
        .style(move |theme: &iced::Theme| iced::widget::text::Style {
            color: Some(tone.map_or_else(|| theme.palette().text, |t| stat_tone_color(theme, t))),
        })
        .into()
}

/// TPS readout: current rate over its live cap, compressed to fit a status lane.
fn tps_value<'a>(tps: f32, fps_target: f32, tone: StatTone) -> Element<'a, Message> {
    row![
        text(format!("{:5.2}", tps))
            .size(TEXT_CAPTION)
            .font(iced::Font::MONOSPACE)
            .style(move |theme: &iced::Theme| iced::widget::text::Style {
                color: Some(stat_tone_color(theme, tone)),
            }),
        text(format!("/{:5.2}", fps_target))
            .size(TEXT_CAPTION)
            .font(iced::Font::MONOSPACE)
            .style(widgets::muted_text_style),
    ]
    .spacing(2)
    .align_y(Alignment::Center)
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

/// The five compact metric lanes. Each chart reads its rolling window from
/// `history` and its current value from the newest sample.
fn telemetry_content<'a>(
    lang: &'a LanguageIdentifier,
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
            s.ping_ms
                .map(|ping_ms| (ping_ms.min(PING_SPAN) as f32 / PING_SPAN as f32, tone_for_ping(ping_ms)))
        },
        |s| {
            s.ping_ms
                .map(|ping_ms| value_text(fmt_ping(ping_ms), Some(tone_for_ping(ping_ms))))
        },
    );

    row![
        tps_card,
        status_divider(),
        skew_card,
        status_divider(),
        lead_card,
        status_divider(),
        depth_card,
        status_divider(),
        ping_card,
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .width(Fill)
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

// Health tone per metric. Shared by the current readout and sparkline so the
// number and chart points always agree
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
        "  0".to_string()
    } else {
        format!("{skew:+3}")
    }
}
/// Signed local lead in ticks; bare `0` at zero reads calmer than `+0`.
fn fmt_lead(lead: i32) -> String {
    if lead == 0 {
        "  0".to_string()
    } else {
        format!("{lead:+3}")
    }
}
/// Rollback depth.
fn fmt_depth(depth: u32) -> String {
    format!("{depth:3}")
}
/// Latency in ms.
fn fmt_ping(ping_ms: u128) -> String {
    format!("{ping_ms:3} ms")
}

/// Persistent player/seat legend above the charts. Red and blue describe field
/// halves everywhere except games that color players by seat; those keep the
/// game-native P1/P2 color assignment while You/Opponent follows the local seat.
fn players_header<'a>(lang: &'a LanguageIdentifier, pvp: &'a PvpSession) -> Element<'a, Message> {
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

    use widgets::{FIELD_BLUE, FIELD_RED};
    let local_is_p1 = pvp.local_player_index() == 0;
    let (you, opponent) = (t!(lang, "play-you"), t!(lang, "play-opponent"));
    if pvp.local_game().family.players_colored_by_seat {
        let (p1, p2) = if local_is_p1 { (you, opponent) } else { (opponent, you) };
        row![side(FIELD_RED, "P1", p1), side(FIELD_BLUE, "P2", p2)]
    } else {
        let (local_seat, remote_seat) = if local_is_p1 { ("P1", "P2") } else { ("P2", "P1") };
        row![
            side(FIELD_RED, local_seat, you),
            side(FIELD_BLUE, remote_seat, opponent),
        ]
    }
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

/// Hairline between the player legend, telemetry lanes, and frame-delay chip.
fn status_divider<'a>() -> Element<'a, Message> {
    container(
        iced::widget::Space::new()
            .width(Length::Fixed(1.0))
            .height(Length::Fixed(20.0)),
    )
    .style(|theme: &iced::Theme| iced::widget::container::Style {
        background: Some(iced::Background::Color(Color {
            a: 0.14,
            ..theme.palette().text
        })),
        ..Default::default()
    })
    .into()
}

/// Resting frame-delay control: current value and disclosure chevron only. The
/// full slider is intentionally out of the bar until requested.
fn frame_delay_chip<'a>(lang: &'a LanguageIdentifier, pvp: &'a PvpSession, open: bool) -> Element<'a, Message> {
    let chip = button(
        row![
            Icon::Turtle.widget().size(14.0),
            text(pvp.frame_delay().to_string())
                .size(TEXT_CAPTION)
                .font(iced::Font::MONOSPACE)
                .width(Length::Fixed(14.0)),
            if open { Icon::ChevronDown } else { Icon::ChevronUp }
                .widget()
                .size(11.0)
                .style(widgets::muted_text_style),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .padding([3.0, 6.0])
    .style(widgets::neutral)
    .on_press(Message::ToggleFrameDelayControl);
    iced::widget::tooltip(
        chip,
        widgets::tooltip_bubble(t!(lang, "settings-netplay-frame-delay")),
        iced::widget::tooltip::Position::Top,
    )
    .gap(5)
    .into()
}

/// Always-visible PvP telemetry. P1/P2 and all five tiny charts occupy one
/// stable row; the trailing chip is the only disclosure control.
pub(super) fn telemetry_panel<'a>(
    lang: &'a LanguageIdentifier,
    pvp: &'a PvpSession,
    state: &'a State,
) -> Element<'a, Message> {
    let bar = row![
        players_header(lang, pvp),
        status_divider(),
        telemetry_content(lang, &state.metric_history),
        status_divider(),
        frame_delay_chip(lang, pvp, state.frame_delay_control.shown()),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .width(Fill);
    let panel = container(bar).padding([6.0, 8.0]).width(Fill).style(hud_chip_plate);

    container(panel)
        .width(Fill)
        .padding(iced::Padding {
            top: 0.0,
            right: 12.0,
            bottom: 8.0,
            left: 12.0,
        })
        .into()
}

/// Small frame-delay popover anchored immediately above the persistent bar.
/// It is the only collapsible piece of telemetry; opening it never changes the
/// bar's size or the game-region layout.
pub(super) fn frame_delay_overlay<'a>(
    lang: &'a LanguageIdentifier,
    pvp: &'a PvpSession,
    state: &'a State,
) -> Option<Element<'a, Message>> {
    let now = iced::time::Instant::now();
    if !state.frame_delay_control.visible(now) {
        return None;
    }
    let popup = container(frame_delay_control(lang, pvp))
        .padding(12)
        .style(widgets::panel);
    Some(
        container(anim::pop(popup, state.frame_delay_control.progress(now), 8.0))
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Bottom)
            .padding(iced::Padding {
                top: 0.0,
                right: 12.0,
                bottom: FRAME_DELAY_CLEARANCE,
                left: 0.0,
            })
            .into(),
    )
}

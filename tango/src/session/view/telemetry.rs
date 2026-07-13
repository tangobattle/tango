//! PvP telemetry: the bottom-right signal indicator and the
//! match-settings popover it expands into. The deck tags P1/P2,
//! draws a per-metric sparkline (TPS, frame skew, local lead,
//! rollback depth, ping) next to its current value colored by health
//! (green/amber/red), and stacks the live frame-delay knob beneath.
//! The plate is a button: clicking the instrument panel toggles the
//! popover, which is gated on a live latency reading and retires
//! itself the moment the remote drops. Split out of the session view
//! so the emulator/drawer/overlay layout in `mod.rs` isn't sharing a
//! file with ~500 lines of charting + tone math.

use super::super::*;
use super::*;
// Explicit so these win over iced's prelude `column!`/`row!` macros (see mod.rs).
use sweeten::widget::{column, row};

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
pub(super) fn telemetry_overlay<'a>(
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

    let content: Element<'a, Message> = if state.match_settings.visible(now) {
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
        use widgets::{FIELD_BLUE, FIELD_RED};
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
        anim::pop(panel, state.match_settings.progress(now), 8.0)
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

//! The floating HUD every session screen shares: the plate styles its
//! chips are drawn in, the corner command cluster, and the Esc
//! hold-to-quit countdown.

use super::*;
// Explicit so these win over iced's prelude macros; see the parent module.
use sweeten::widget::{column, row};

/// Flat plate behind the telemetry deck — a faint fill + hairline
/// border so the readout reads as one grouped module without drawing
/// attention to itself. Realized as a button style (not a static
/// container) because the instrument panel is clickable: a subtle
/// hover/press brighten marks it as the trigger for the match-settings
/// popover. PvP-only.
pub(super) fn telemetry_plate_button(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
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
pub(super) fn overlay_close_button(
    theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
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

/// [`telemetry_plate_button`], lit while `lit`: primary text and a
/// primary hairline — the identity-in-the-glyph treatment the floating
/// bars give an engaged toggle or an off-default menu, not a full CTA
/// fill.
pub(super) fn lit_plate_button(
    lit: bool,
) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style + Copy {
    move |theme, status| {
        let mut st = telemetry_plate_button(theme, status);
        if lit {
            let primary = theme.palette().primary;
            st.text_color = primary;
            st.border.color = iced::Color { a: 0.35, ..primary };
        }
        st
    }
}

/// Container twin of [`telemetry_plate_button`]'s resting plate —
/// the flat translucent fill + hairline border the floating chips
/// use, for surfaces that aren't buttons (the replay transport
/// bar). Keeps every floating HUD piece in one visual family.
pub(super) fn hud_chip_plate(theme: &iced::Theme) -> iced::widget::container::Style {
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

/// How far the floating controls sink when hiding — past the
/// window's bottom edge (panel height + bottom margin, with a
/// little extra for the drop shadow).
pub(super) const CONTROLS_SLIDE: f32 = 120.0;

/// The unified session command cluster, top-right in every
/// session type: the Settings gear and the tear-down button —
/// `tear_down_msg` is direct Close for replay/SP, the disconnect
/// confirm for a live PvP link. Rides the same auto-hide transition
/// as the rest of the controls, sliding up past the top edge when
/// the cursor goes idle — unless `behind_drawer` (PvP: the opponent
/// drawer covers the cluster), which pins it instead.
pub(super) fn corner_commands_overlay<'a>(
    lang: &'a LanguageIdentifier,
    state: &'a State,
    tear_down_msg: Message,
    behind_drawer: bool,
) -> Element<'a, Message> {
    let now = iced::time::Instant::now();
    let cmd = |icon: Icon,
               label: String,
               msg: Message,
               style: fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style|
     -> Element<'a, Message> {
        let btn = button(icon.widget().size(16.0))
            .padding([6.0, 8.0])
            .style(style)
            .on_press(msg);
        iced::widget::tooltip(
            btn,
            widgets::tooltip_bubble(label),
            iced::widget::tooltip::Position::Bottom,
        )
        .gap(4)
        .into()
    };
    // Same X + "Close" tooltip in every session type.
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
    // (see the layer order in [`pvp::view`]) — skip the auto-hide
    // slide then. The slide draws in iced's floating layer
    // (`anim::slide_in`), which would pop the buttons OVER the
    // drawer they're supposed to be under for the length of the
    // animation; at rest behind the drawer the slide is invisible
    // anyway.
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

/// Diameter of the exit chip's countdown dial.
const HOLD_RING_SIZE: f32 = 28.0;

/// Stroke width of the dial's track and arc.
const HOLD_RING_WIDTH: f32 = 3.0;

/// Countdown dial for the exit chip: a faint full-circle track with a
/// danger-toned arc sweeping clockwise from 12 o'clock as the hold
/// progresses — the radial twin of a hold-to-confirm button fill.
struct HoldRing {
    /// Arc fill fraction, 0 (just appeared) ..= 1 (quit fires).
    progress: f32,
}
impl<M> canvas::Program<M> for HoldRing {
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
        let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        // Inset by the stroke so the arc's full width stays on-canvas.
        let radius = (bounds.width.min(bounds.height) - HOLD_RING_WIDTH) / 2.0;
        // Faint track so the dial's full extent reads before the arc
        // fills it in.
        frame.stroke(
            &Path::circle(center, radius),
            Stroke::default().with_width(HOLD_RING_WIDTH).with_color(iced::Color {
                a: 0.20,
                ..theme.palette().text
            }),
        );
        let sweep = self.progress.clamp(0.0, 1.0) * std::f32::consts::TAU;
        if sweep > 0.0 {
            let arc = Path::new(|p| {
                p.arc(canvas::path::Arc {
                    center,
                    radius,
                    start_angle: iced::Radians(-std::f32::consts::FRAC_PI_2),
                    end_angle: iced::Radians(-std::f32::consts::FRAC_PI_2 + sweep),
                });
            });
            frame.stroke(
                &arc,
                Stroke::default()
                    .with_width(HOLD_RING_WIDTH)
                    .with_color(theme.palette().danger)
                    .with_line_cap(LineCap::Round),
            );
        }
        vec![frame.into_geometry()]
    }
}

/// The hold-Esc-to-quit readout: appears the moment the hold arms
/// and counts down to the [`ESC_QUIT_HOLD`] deadline, where
/// [`State::update`]'s wrapper closes the session. Deliberately NOT
/// a modal — no dim wash, no panel, no buttons — but a compact
/// top-center chip in the floating HUD family ([`hud_chip_plate`]),
/// with a [`HoldRing`] dial filling around the close X: it's a
/// transient status readout the user is already acting on, and
/// releasing Esc disarms the hold and takes the chip with it (a bare
/// tap just flashes it — feedback that the key registered). Pushed
/// last in [`view`]: the countdown must read over every other layer,
/// the reconnect modal included (holding Esc through a stalled
/// reconnect is exactly the bail-out case).
pub(super) fn exit_hold_overlay<'a>(lang: &'a LanguageIdentifier, state: &'a State) -> Option<Element<'a, Message>> {
    let held = state.esc_hold?.elapsed();
    let progress = held.as_secs_f32() / ESC_QUIT_HOLD.as_secs_f32();
    // The close X centered in the dial — same glyph as the corner
    // tear-down button this hold is a shortcut for, danger-tinted to
    // carry the destructive framing.
    let dial = stack![
        Canvas::new(HoldRing {
            progress: progress.min(1.0)
        })
        .width(Length::Fixed(HOLD_RING_SIZE))
        .height(Length::Fixed(HOLD_RING_SIZE)),
        container(Icon::X.widget().size(12.0).style(|theme: &iced::Theme| {
            iced::widget::text::Style {
                color: Some(theme.palette().danger),
            }
        }))
        .center(Fill),
    ];
    let copy = column![
        text(t!(lang, "playback-exit-hold")).size(TEXT_BODY),
        text(t!(lang, "playback-exit-hold-detail"))
            .size(TEXT_CAPTION)
            .style(widgets::muted_text_style),
    ]
    .spacing(2);
    let chip = container(row![Element::from(dial), copy].spacing(10).align_y(Alignment::Center))
        .padding([8, 12])
        .style(hud_chip_plate);
    Some(
        container(chip)
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Top)
            .padding(12)
            .into(),
    )
}

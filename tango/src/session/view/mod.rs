use super::*;
// Explicit so these win over iced's prelude `column!`/`row!` macros, which
// would otherwise clash with the sweeten ones re-exported via `super::*`.
use sweeten::widget::{column, row};

use crate::session::replay::{SCREEN_HEIGHT, SCREEN_WIDTH};

pub mod pvp;
pub mod replay;
pub mod results;
pub mod singleplayer;
pub use results::results_view;

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

/// Width of a PvP setup side pane (the save view inside the
/// drawer) — the width [`emulator_body`]'s drawer slots reserve.
const SETUP_PANE_WIDTH: f32 = 420.0;

/// How far the floating controls sink when hiding — past the
/// window's bottom edge (panel height + bottom margin, with a
/// little extra for the drop shadow).
const CONTROLS_SLIDE: f32 = 120.0;

/// Pre-digested view of the watched replay's export job, for the
/// transport bar's clip strip. The job itself lives in the replays
/// tab's per-replay state (the App owns it and its canceller); the
/// session view only renders what it's handed.
#[derive(Clone, Copy)]
pub struct ClipJob<'a> {
    pub completed: usize,
    pub total: usize,
    /// Set once the export finished: `Ok` = saved, `Err` = the
    /// failure line.
    pub result: Option<Result<(), &'a str>>,
    /// Cancel was clicked but the encoder thread hasn't wound down
    /// yet — "Cancelling…" chrome.
    pub cancelling: bool,
}

/// Everything a session's [`view`](ActiveSession::view) needs from
/// the app, bundled so the trait hook stays one argument wide.
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub lang: &'a LanguageIdentifier,
    pub state: &'a State,
    pub fractional_scaling: bool,
    pub hide_emulator_border: bool,
    pub show_replay_inputs: bool,
    pub clip_job: Option<ClipJob<'a>>,
    pub effect: &'static Effect,
}

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    lang: &'a LanguageIdentifier,
    state: &'a State,
    fractional_scaling: bool,
    hide_emulator_border: bool,
    show_replay_inputs: bool,
    clip_job: Option<ClipJob<'a>>,
    effect: &'static Effect,
) -> Element<'a, Message> {
    let Some(session) = state.active.as_deref() else {
        return iced::widget::Space::new().width(Fill).height(Fill).into();
    };
    // Each session kind assembles its own screen — see
    // [`replay::view`] / [`singleplayer::view`] / [`pvp::view`].
    session.view(Ctx {
        lang,
        state,
        fractional_scaling,
        hide_emulator_border,
        show_replay_inputs,
        clip_job,
        effect,
    })
}

/// Shared closer for every session screen: the topmost Esc
/// hold-to-quit chip, then the cursor-wake mouse area.
/// iced's mouse_area, not sweeten's: sweeten 0.14 gates all its
/// enter/move/exit dispatches on the cursor being inside the
/// bounds, which makes `on_exit` unreachable (the cursor is
/// outside by definition when it fires).
fn finish_session_stack<'a>(
    lang: &'a LanguageIdentifier,
    state: &'a State,
    mut stacked: iced::widget::Stack<'a, Message>,
) -> Element<'a, Message> {
    // Topmost: the Esc hold-to-quit countdown chip (see
    // `exit_hold_overlay` for why it outranks even the reconnect
    // modal).
    if let Some(o) = exit_hold_overlay(lang, state) {
        stacked = stacked.push(o);
    }
    iced::widget::mouse_area(stacked)
        .on_move(|_| Message::MouseMoved)
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
    let img_w = (SCREEN_WIDTH * scale) as f32;
    let img_h = (SCREEN_HEIGHT * scale) as f32;

    iced::widget::responsive(move |size| {
        let raw = (size.width / img_w).min(size.height / img_h);
        let scale = if fractional_scaling {
            raw.max(0.0)
        } else {
            raw.floor().max(1.0)
        };
        let (w, h) = (img_w * scale, img_h * scale);

        let mut frame = state
            .current_frame
            .clone()
            .unwrap_or_else(crate::platform::video::framebuffer::Frame::black);
        // The uploaded texture is always the native frame; the effect is just
        // the draw-time pipeline pick. Take it live from config here (not from
        // whatever was current when the frame was produced) so switching the
        // video filter re-renders immediately — even on a paused replay that
        // isn't producing new frames.
        frame.effect = effect;
        let fb = iced::widget::shader::Shader::new(crate::platform::video::framebuffer::Program::new(frame))
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
/// `slots` are the PvP setup-drawer slots (`[left, right]`) — see the
/// comment on `drawer_slot` below; always `[false, false]` outside
/// PvP.
fn emulator_body<'a>(
    game: &'static crate::library::game::Game,
    frame: Element<'a, Message>,
    hide_emulator_border: bool,
    slots: [bool; 2],
) -> Element<'a, Message> {
    let frame_container = container(frame).center(Fill);
    let bnlc_bg = if hide_emulator_border {
        None
    } else {
        background_handle(game)
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
    if slots[0] {
        content_row = content_row.push(drawer_slot());
    }
    content_row = content_row.push(container(frame_container).width(Fill).height(Fill));
    if slots[1] {
        content_row = content_row.push(drawer_slot());
    }
    let body = stack![backdrop, Element::from(content_row)];
    container(body).width(Fill).height(Fill).into()
}

/// The unified session command cluster, top-right in every
/// session type: the Settings gear and the tear-down button —
/// `tear_down_msg` is direct Close for replay/SP, the disconnect
/// confirm for a live PvP link. Rides the same auto-hide transition
/// as the rest of the controls, sliding up past the top edge when
/// the cursor goes idle — unless `behind_drawer` (PvP: the opponent
/// drawer covers the cluster), which pins it instead.
fn corner_commands_overlay<'a>(
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
fn exit_hold_overlay<'a>(lang: &'a LanguageIdentifier, state: &'a State) -> Option<Element<'a, Message>> {
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

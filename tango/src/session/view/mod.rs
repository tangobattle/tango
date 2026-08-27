use super::*;
// Explicit so these win over iced's prelude `column!`/`row!` macros, which
// would otherwise clash with the sweeten ones re-exported via `super::*`.
use sweeten::widget::{column, row};


pub mod pvp;
pub mod replay;
pub mod results;
pub mod singleplayer;
pub mod training;
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

/// How wide a PvP setup side pane is allowed to get by dragging its
/// inner edge. The floor keeps the save view's tab strip legible; the
/// ceiling keeps the emulator from being squeezed off a modest window
/// with both drawers out. The resting width is the user's — persisted
/// as `config.pvp_setup_pane_widths` and carried on `PvpPanes`.
pub(crate) const SETUP_PANE_MIN_WIDTH: f32 = 300.0;
pub(crate) const SETUP_PANE_MAX_WIDTH: f32 = 720.0;

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

/// Everything a session's view needs from the app, bundled so each
/// kind's entry point stays one argument wide.
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub lang: &'a LanguageIdentifier,
    pub state: &'a State,
    pub fractional_scaling: bool,
    pub hide_emulator_border: bool,
    pub show_replay_inputs: bool,
    /// How modes with two perspectives present the auxiliary surface.
    /// Read live from config so replay and training switch immediately.
    pub opponent_view: crate::config::OpponentView,
    /// How a DS session's two screens stack in the pane. Read live
    /// from config, so the switch re-lays out an active session.
    pub ds_screen_stacking: crate::config::DsScreenStacking,
    /// Which DS screen leads the arrangement — live from config, like
    /// the stacking.
    pub ds_primary_screen: crate::config::DsPrimaryScreen,
    /// Quality mode used by replay exports: `0` is raw output at native
    /// resolution; `1..=10` is lossy at that integer upscale. Owned by
    /// the replays tab, but surfaced in the replay clip strip too.
    pub clip_export_scale: u8,
    pub clip_job: Option<ClipJob<'a>>,
    /// How many replays are waiting behind this one. The queue itself lives
    /// in the replays tab; the transport bar only needs the count, to say so
    /// and to offer the skip. `0` = nothing queued, and the bar shows neither.
    pub queued: usize,
    pub effect: &'static Effect,
}

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    lang: &'a LanguageIdentifier,
    state: &'a State,
    fractional_scaling: bool,
    hide_emulator_border: bool,
    show_replay_inputs: bool,
    opponent_view: crate::config::OpponentView,
    ds_screen_stacking: crate::config::DsScreenStacking,
    ds_primary_screen: crate::config::DsPrimaryScreen,
    clip_export_scale: u8,
    clip_job: Option<ClipJob<'a>>,
    queued: usize,
    effect: &'static Effect,
) -> Element<'a, Message> {
    let Some(session) = state.active.as_deref() else {
        return iced::widget::Space::new().width(Fill).height(Fill).into();
    };
    let ctx = Ctx {
        lang,
        state,
        fractional_scaling,
        hide_emulator_border,
        show_replay_inputs,
        opponent_view,
        ds_screen_stacking,
        ds_primary_screen,
        clip_export_scale,
        clip_job,
        queued,
        effect,
    };
    // Each session kind assembles its own screen. The engine-side
    // trait deliberately knows nothing about rendering, so this is the
    // one place a session's concrete kind picks its view.
    if let Some(s) = session.downcast_ref::<crate::session::replay::ReplaySession>() {
        replay::view(s, ctx)
    } else if let Some(s) = session.downcast_ref::<crate::session::pvp::PvpSession>() {
        pvp::view(s, ctx)
    } else if let Some(s) = session.downcast_ref::<crate::session::singleplayer::SinglePlayerSession>() {
        singleplayer::view(s, ctx)
    } else if let Some(s) = session.downcast_ref::<crate::session::training::TrainingSession>() {
        training::view(s, ctx)
    } else {
        // Unreachable today — the three kinds above are the only
        // Session impls anywhere.
        iced::widget::Space::new().width(Fill).height(Fill).into()
    }
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

/// Localized label for one opponent-surface presentation. Shared by the
/// replay and training menus so the same setting reads identically in both.
fn opponent_view_label(lang: &LanguageIdentifier, view: crate::config::OpponentView) -> String {
    match view {
        crate::config::OpponentView::Off => t!(lang, "opponent-view-off"),
        crate::config::OpponentView::PictureInPicture => t!(lang, "opponent-view-picture-in-picture"),
        crate::config::OpponentView::StackHorizontally => t!(lang, "opponent-view-stack-horizontally"),
        crate::config::OpponentView::StackVertically => t!(lang, "opponent-view-stack-vertically"),
    }
}

/// The four checked rows used by each opponent-view dropdown.
fn opponent_view_items<M>(
    lang: &LanguageIdentifier,
    selected: crate::config::OpponentView,
    message: impl Fn(crate::config::OpponentView) -> M,
) -> Vec<widgets::MenuItem<M>> {
    use crate::config::OpponentView::{Off, PictureInPicture, StackHorizontally, StackVertically};
    [Off, PictureInPicture, StackHorizontally, StackVertically]
        .into_iter()
        .map(|view| widgets::MenuItem::toggle(opponent_view_label(lang, view), message(view), view == selected))
        .collect()
}

/// Glyph for the menu's current presentation. Off uses a neutral multi-view
/// affordance; the three active choices name their actual geometry.
fn opponent_view_icon(view: crate::config::OpponentView) -> Icon {
    match view {
        crate::config::OpponentView::Off => Icon::GalleryHorizontal,
        crate::config::OpponentView::PictureInPicture => Icon::PictureInPicture2,
        crate::config::OpponentView::StackHorizontally => Icon::Columns,
        crate::config::OpponentView::StackVertically => Icon::Rows,
    }
}

/// Where the main perspective sits inside its half of a stacked layout.
/// Docking both frames against their shared seam prevents integer scaling
/// from leaving an empty band between them.
fn main_frame_alignment(
    view: crate::config::OpponentView,
) -> (iced::alignment::Horizontal, iced::alignment::Vertical) {
    match view {
        crate::config::OpponentView::StackHorizontally => (
            iced::alignment::Horizontal::Right,
            iced::alignment::Vertical::Center,
        ),
        crate::config::OpponentView::StackVertically => (
            iced::alignment::Horizontal::Center,
            iced::alignment::Vertical::Bottom,
        ),
        _ => (
            iced::alignment::Horizontal::Center,
            iced::alignment::Vertical::Center,
        ),
    }
}

/// The live framebuffer, rendered through a custom wgpu shader widget
/// (one persistent GPU texture, written in place each vblank) instead
/// of a per-frame `image` handle. The shader fills the widget's
/// bounds, so the widget is sized to the framebuffer rect — an exact
/// integer multiple (crisp, the default) or a smooth aspect-fit —
/// using `responsive` for the pane size both need. Before the first
/// frame, a 1×1 black placeholder keeps the pane opaque.
fn framebuffer_view<'a>(
    ctx: Ctx<'a>,
    // A recorded touch to draw at its spot on the touch screen (the
    // replay input display); `None` everywhere else.
    touch_spot: Option<(u16, u16)>,
    // Stacked layouts dock the two frames against their center seam; every
    // other presentation centers the main frame in its pane.
    horizontal_alignment: iced::alignment::Horizontal,
    vertical_alignment: iced::alignment::Vertical,
) -> Element<'a, Message> {
    let state = ctx.state;
    let (fractional_scaling, effect) = (ctx.fractional_scaling, ctx.effect);
    // Resolved out here, where the language is still in hand; the
    // closure below only lays it out (see `priming_notice`).
    let priming = priming_copy(ctx.lang, state);
    // Post-filter framebuffer dimensions. Drive the scale math below;
    // match the (w, h) `build_frame_pixels` stamps into the frame the
    // `framebuffer` shader uploads.
    // The widget is sized to native·scale — the same rectangle the old CPU
    // upscalers produced — and the effect's fragment shader magnifies the
    // native texture to fill it. Native size comes from the session,
    // which knows its console's screens from boot — a DS puts two
    // 256-wide screens where a GBA has one — so the pane holds the
    // right shape before the first frame lands.
    let scale = effect.scale;
    let layout = state.active.as_ref().map(|s| s.screen_layout());
    // The session always composes multiple screens side by side, upper
    // screen first; the arrangement settings are this view's re-layout
    // of that frame (see `rearrange_screens`), taken live from config
    // like the effect so flipping either re-lays out an active session
    // immediately.
    let multi = layout.as_ref().is_some_and(|layout| layout.screens.len() > 1);
    let touch_first = multi && ctx.ds_primary_screen == crate::config::DsPrimaryScreen::Touch;
    let (native_w, native_h) = match &layout {
        Some(layout) if multi && ctx.ds_screen_stacking == crate::config::DsScreenStacking::Vertical => (
            layout.screens.iter().map(|s| s.width).max().unwrap_or(1),
            layout.screens.iter().map(|s| s.height).sum(),
        ),
        Some(layout) if multi && ctx.ds_screen_stacking == crate::config::DsScreenStacking::PrimaryOnly => {
            let screen = layout.screens[touch_first as usize];
            (screen.width, screen.height)
        }
        // Horizontal keeps the session's own composition, so its size
        // is the frame's.
        _ => state
            .active
            .as_ref()
            .map(|s| s.frame_size())
            // Unreachable in practice — the app only renders this view over
            // an active session — and an absent one draws only the black
            // placeholder, where shape doesn't matter.
            .unwrap_or((1, 1)),
    };
    let img_w = (native_w * scale) as f32;
    let img_h = (native_h * scale) as f32;

    let touch_screen = layout
        .as_ref()
        .and_then(|layout| touch_screen_placement(layout, ctx.ds_screen_stacking, touch_first));

    let base_frame = state
        .current_frame
        .clone()
        .unwrap_or_else(crate::platform::video::framebuffer::Frame::black);
    let base_frame = match (&layout, multi) {
        (Some(layout), true) => present_frame(base_frame, layout, ctx.ds_screen_stacking, ctx.ds_primary_screen),
        _ => base_frame,
    };

    // The arrangement is settled here, so this is where the session
    // learns which screens are worth composing. Cheap enough to repeat
    // every repaint (one atomic store), which is also what makes a
    // setting flipped mid-session take effect straight away.
    if let (Some(layout), Some(active)) = (&layout, state.active.as_ref()) {
        active.set_displayed_screens(presented_mask(layout, ctx.ds_screen_stacking, touch_first));
    }

    // The recorded touch as a fraction of the pane: through the same
    // origin as the stylus mapping above, so the spot lands on the
    // touch screen wherever the arrangement puts it.
    let spot = touch_spot.and_then(|(tx, ty)| {
        touch_screen.map(|((origin_x, origin_y), _)| {
            (
                (origin_x + tx as f32 + 0.5) / native_w as f32,
                (origin_y + ty as f32 + 0.5) / native_h as f32,
            )
        })
    });

    iced::widget::responsive(move |size| {
        let raw = (size.width / img_w).min(size.height / img_h);
        let scale = if fractional_scaling {
            raw.max(0.0)
        } else {
            raw.floor().max(1.0)
        };
        let (w, h) = (img_w * scale, img_h * scale);

        let mut frame = base_frame.clone();
        // The uploaded texture is always the native frame; the effect is just
        // the draw-time pipeline pick. Take it live from config here (not from
        // whatever was current when the frame was produced) so switching the
        // video filter re-renders immediately — even on a paused replay that
        // isn't producing new frames.
        frame.effect = effect;
        let fb = iced::widget::shader::Shader::new(crate::platform::video::framebuffer::Program::new(frame))
            .width(Length::Fixed(w))
            .height(Length::Fixed(h));

        let mut fb: Element<'a, Message> = fb.into();
        // The recorded touch, over the frame and under the stylus
        // area. A canvas with no event handling, so the pointer passes
        // straight through it.
        if let Some((fx, fy)) = spot {
            let overlay = Canvas::new(TouchSpot {
                fx,
                fy,
                rf: TOUCH_SPOT_R / native_w as f32,
            })
            .width(Length::Fixed(w))
            .height(Length::Fixed(h));
            fb = stack![fb, overlay].into();
        }
        // The stylus: pointer events over the widget, handed to the
        // session mapped into the touch screen's own pixels. The whole
        // surface reports (so a drag can scrape along the screen's
        // edges); whether a *press* lands on the touch screen travels
        // with each move as `inside`.
        if let Some(((origin_x, origin_y), screen)) = touch_screen {
            fb = iced::widget::mouse_area(fb)
                .on_move(move |p| {
                    let nx = p.x / w * native_w as f32 - origin_x;
                    let ny = p.y / h * native_h as f32 - origin_y;
                    let inside =
                        (0.0..screen.width as f32).contains(&nx) && (0.0..screen.height as f32).contains(&ny);
                    let pos = (
                        nx.clamp(0.0, (screen.width - 1) as f32) as u16,
                        ny.clamp(0.0, (screen.height - 1) as f32) as u16,
                    );
                    Message::Stylus(StylusEvent::Moved { pos, inside })
                })
                .on_press(Message::Stylus(StylusEvent::Pressed))
                .on_release(Message::Stylus(StylusEvent::Released))
                .on_exit(Message::Stylus(StylusEvent::Released))
                .into();
        }

        // The priming notice, bounded to the frame rect itself so it
        // reads as part of the screen rather than of the window. Last
        // in, so it sits over the stylus area: while it's up there is
        // no battle to point at, and its own dismissal (the failure
        // case) has to be the thing a press lands on.
        if let Some(copy) = priming.as_ref() {
            fb = stack![fb, priming_notice(copy, w, h)].into();
        }

        let centered = |content: Element<'a, Message>| -> Element<'a, Message> {
            iced::widget::container(content)
                .width(Fill)
                .height(Fill)
                .align_x(horizontal_alignment)
                .align_y(vertical_alignment)
                .into()
        };

        if fractional_scaling {
            // Smooth aspect-fit, centered, no drop shadow.
            centered(fb)
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

/// The auxiliary opponent framebuffer as a full pane. This uses the PiP
/// primitive type so iced gives it an independent resident GPU texture, but
/// otherwise mirrors the main framebuffer's arrangement, scaling, filter,
/// touch marker, and shadow treatment.
fn opponent_framebuffer_view<'a>(
    ctx: Ctx<'a>,
    touch_spot: Option<(u16, u16)>,
    horizontal_alignment: iced::alignment::Horizontal,
    vertical_alignment: iced::alignment::Vertical,
) -> Element<'a, Message> {
    let state = ctx.state;
    let (fractional_scaling, effect) = (ctx.fractional_scaling, ctx.effect);
    let layout = state.active.as_ref().map(|s| s.screen_layout());
    let multi = layout.as_ref().is_some_and(|layout| layout.screens.len() > 1);
    let touch_first = multi && ctx.ds_primary_screen == crate::config::DsPrimaryScreen::Touch;
    let (native_w, native_h) = match &layout {
        Some(layout) if multi && ctx.ds_screen_stacking == crate::config::DsScreenStacking::Vertical => (
            layout.screens.iter().map(|s| s.width).max().unwrap_or(1),
            layout.screens.iter().map(|s| s.height).sum(),
        ),
        Some(layout) if multi && ctx.ds_screen_stacking == crate::config::DsScreenStacking::PrimaryOnly => {
            let screen = layout.screens[touch_first as usize];
            (screen.width, screen.height)
        }
        _ => state.active.as_ref().map(|s| s.frame_size()).unwrap_or((1, 1)),
    };
    let (img_w, img_h) = ((native_w * effect.scale) as f32, (native_h * effect.scale) as f32);
    let touch_screen = layout
        .as_ref()
        .and_then(|layout| touch_screen_placement(layout, ctx.ds_screen_stacking, touch_first));

    // Keep the equal pane mounted while its first frame is being captured.
    // The black pixel stretches into the same aspect-sized widget, avoiding a
    // one-frame layout jump when the session publishes the auxiliary surface.
    let base_frame = state
        .pip_frame
        .clone()
        .unwrap_or_else(crate::platform::video::framebuffer::Frame::black);
    let base_frame = match (&layout, multi) {
        (Some(layout), true) => present_frame(base_frame, layout, ctx.ds_screen_stacking, ctx.ds_primary_screen),
        _ => base_frame,
    };
    let spot = touch_spot.and_then(|(tx, ty)| {
        touch_screen.map(|((origin_x, origin_y), _)| {
            (
                (origin_x + tx as f32 + 0.5) / native_w as f32,
                (origin_y + ty as f32 + 0.5) / native_h as f32,
            )
        })
    });

    iced::widget::responsive(move |size| {
        let raw = (size.width / img_w).min(size.height / img_h);
        let scale = if fractional_scaling {
            raw.max(0.0)
        } else {
            raw.floor().max(1.0)
        };
        let (w, h) = (img_w * scale, img_h * scale);
        let mut frame = base_frame.clone();
        frame.effect = effect;
        let fb = iced::widget::shader::Shader::new(crate::platform::video::framebuffer::PipProgram::new(frame))
            .width(Length::Fixed(w))
            .height(Length::Fixed(h));
        let mut fb: Element<'a, Message> = fb.into();
        if let Some((fx, fy)) = spot {
            let marker = Canvas::new(TouchSpot {
                fx,
                fy,
                rf: TOUCH_SPOT_R / native_w as f32,
            })
            .width(Length::Fixed(w))
            .height(Length::Fixed(h));
            fb = stack![fb, marker].into();
        }

        let centered = |content: Element<'a, Message>| -> Element<'a, Message> {
            container(content)
                .width(Fill)
                .height(Fill)
                .align_x(horizontal_alignment)
                .align_y(vertical_alignment)
                .into()
        };
        if fractional_scaling {
            centered(fb)
        } else {
            let framed = container(fb)
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

/// Split the emulator body into two equal perspective panes along the
/// selected axis. Both arrangements have a zero-width center seam.
fn stacked_framebuffers<'a>(
    ctx: Ctx<'a>,
    main: Element<'a, Message>,
    opponent_touch: Option<(u16, u16)>,
    view: crate::config::OpponentView,
) -> Element<'a, Message> {
    match view {
        crate::config::OpponentView::StackHorizontally => {
            let opponent = opponent_framebuffer_view(
                ctx,
                opponent_touch,
                iced::alignment::Horizontal::Left,
                iced::alignment::Vertical::Center,
            );
            row![
                container(main).width(Length::FillPortion(1)).height(Fill),
                container(opponent).width(Length::FillPortion(1)).height(Fill),
            ]
            .spacing(0)
            .padding([0, 12])
            .width(Fill)
            .height(Fill)
            .into()
        }
        crate::config::OpponentView::StackVertically => {
            let opponent = opponent_framebuffer_view(
                ctx,
                opponent_touch,
                iced::alignment::Horizontal::Center,
                iced::alignment::Vertical::Top,
            );
            column![
                container(main).width(Fill).height(Length::FillPortion(1)),
                container(opponent).width(Fill).height(Length::FillPortion(1)),
            ]
            .spacing(0)
            .padding([12, 0])
            .width(Fill)
            .height(Fill)
            .into()
        }
        _ => main,
    }
}

/// The layout's screens in the order this arrangement lays them out,
/// as indices into it: canonical, or with the touch screen pulled to
/// the front when it leads. Shared by the placement below and the
/// re-pack in [`rearrange_screens`] so the two can't disagree about
/// where a screen ended up.
/// Which of `layout`'s screens this arrangement actually puts in front
/// of the player, as a bitmask over the layout's own order.
///
/// Handed to the session so the console can stop composing a screen
/// nobody is shown - on the DS that is a whole 2D engine, and
/// primary-only is exactly the arrangement that drops one.
fn presented_mask(
    layout: &tango_match::ScreenLayout,
    stacking: crate::config::DsScreenStacking,
    touch_first: bool,
) -> u8 {
    let order = presented_order(layout, touch_first);
    let shown = match stacking {
        crate::config::DsScreenStacking::PrimaryOnly => &order[..1.min(order.len())],
        _ => &order[..],
    };
    shown.iter().fold(0u8, |m, &i| m | 1 << i)
}

fn presented_order(layout: &tango_match::ScreenLayout, touch_first: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..layout.screens.len()).collect();
    if touch_first {
        if let Some(touch) = layout.touch {
            // `order` starts as the identity, so the touch screen sits
            // at its own index; the rest keep their relative order.
            order.remove(touch);
            order.insert(0, touch);
        }
    }
    order
}

/// Where the stylus target sits in the presented frame: (where it
/// starts, its size), all in native pixels. `None` when this
/// arrangement puts no touch screen on the pane — neither the stylus
/// area nor a touch spot goes up without a screen to touch.
///
/// The layout names which of its screens the stylus points at rather
/// than the pane inferring it from a count: a session composes
/// whichever screens its mode uses, so the touch screen may lead the
/// frame, trail it, or not be in it at all — as in a primary-only
/// arrangement led by the upper screen, or a game whose link battle
/// never leaves that screen.
fn touch_screen_placement(
    layout: &tango_match::ScreenLayout,
    stacking: crate::config::DsScreenStacking,
    touch_first: bool,
) -> Option<((f32, f32), tango_match::Screen)> {
    let touch = layout.touch?;
    let order = presented_order(layout, touch_first);
    // Primary-only shows the leading screen and drops the rest.
    let shown = match stacking {
        crate::config::DsScreenStacking::PrimaryOnly => &order[..1],
        _ => &order[..],
    };
    let at = shown.iter().position(|&i| i == touch)?;
    // Everything ahead of it along the axis the stacking runs; the
    // other axis stays at the frame's edge.
    let vertical = stacking == crate::config::DsScreenStacking::Vertical;
    let ahead: u32 = shown[..at]
        .iter()
        .map(|&i| {
            if vertical {
                layout.screens[i].height
            } else {
                layout.screens[i].width
            }
        })
        .sum();
    let origin = if vertical {
        (0.0, ahead as f32)
    } else {
        (ahead as f32, 0.0)
    };
    Some((origin, layout.screens[touch]))
}

/// A multi-screen frame as the arrangement settings present it:
/// screens reordered so the primary one leads, then packed side by
/// side, as a vertical stack, or cut down to the primary screen
/// alone. Pure presentation: the session's composition stays
/// canonical, so replays, exports and the wire never see this.
///
/// The revision is always remapped into a per-arrangement space, even
/// when the pixels pass through untouched: dimensions don't
/// distinguish every pair of arrangements (a horizontal swap keeps
/// them), and the GPU pipeline skips uploads on revision equality
/// alone — so two arrangements of one source revision must never
/// share a presented revision.
fn present_frame(
    frame: crate::platform::video::framebuffer::Frame,
    layout: &tango_match::ScreenLayout,
    stacking: crate::config::DsScreenStacking,
    primary: crate::config::DsPrimaryScreen,
) -> crate::platform::video::framebuffer::Frame {
    let touch_first = primary == crate::config::DsPrimaryScreen::Touch;
    // Only a horizontal pair in canonical order passes through
    // untouched.
    let rearranges = stacking != crate::config::DsScreenStacking::Horizontal || touch_first;
    // Also guards the 1×1 black placeholder before the first frame,
    // which has no screens to rearrange.
    let canonical = (frame.width, frame.height) == tango_session::composite_size(layout);
    let mut frame = if canonical && rearranges {
        rearrange_screens(&frame, layout, stacking, touch_first)
    } else {
        frame
    };
    let arrangement = match stacking {
        crate::config::DsScreenStacking::Vertical => 0u64,
        crate::config::DsScreenStacking::Horizontal => 1,
        crate::config::DsScreenStacking::PrimaryOnly => 2,
    };
    frame.revision = frame
        .revision
        .wrapping_mul(8)
        .wrapping_add(arrangement << 1 | touch_first as u64);
    frame
}

/// The pixel re-pack behind [`present_frame`]: rows re-sliced from the
/// canonical side-by-side composition into the presented order and
/// axis, or down to the primary screen alone.
fn rearrange_screens(
    frame: &crate::platform::video::framebuffer::Frame,
    layout: &tango_match::ScreenLayout,
    stacking: crate::config::DsScreenStacking,
    touch_first: bool,
) -> crate::platform::video::framebuffer::Frame {
    const BPP: usize = 4;
    // Each screen's column offset in the canonical composition.
    let mut x0 = vec![0usize; layout.screens.len()];
    for i in 1..x0.len() {
        x0[i] = x0[i - 1] + layout.screens[i - 1].width as usize;
    }
    let order = presented_order(layout, touch_first);
    let src_stride = frame.width as usize * BPP;
    // A screen's row slice in the canonical frame, or `None` past its
    // height (screens shorter than the composite pad with opaque
    // black — never hit on a DS, whose screens match).
    let row_of = |i: usize, row: usize| -> Option<&[u8]> {
        let screen = &layout.screens[i];
        (row < screen.height as usize).then(|| {
            let start = row * src_stride + x0[i] * BPP;
            &frame.pixels[start..start + screen.width as usize * BPP]
        })
    };
    let pad = |pixels: &mut Vec<u8>, px: usize| {
        for _ in 0..px {
            pixels.extend_from_slice(&[0, 0, 0, 0xff]);
        }
    };
    let (out_w, out_h, pixels) = match stacking {
        crate::config::DsScreenStacking::Vertical => {
            let out_w = layout.screens.iter().map(|s| s.width).max().unwrap_or(0) as usize;
            let out_h = layout.screens.iter().map(|s| s.height).sum::<u32>() as usize;
            let mut pixels = Vec::with_capacity(out_w * out_h * BPP);
            for &i in &order {
                for row in 0..layout.screens[i].height as usize {
                    pixels.extend_from_slice(row_of(i, row).unwrap());
                    pad(&mut pixels, out_w - layout.screens[i].width as usize);
                }
            }
            (out_w, out_h, pixels)
        }
        crate::config::DsScreenStacking::PrimaryOnly => {
            let i = order[0];
            let (out_w, out_h) = (layout.screens[i].width as usize, layout.screens[i].height as usize);
            let mut pixels = Vec::with_capacity(out_w * out_h * BPP);
            for row in 0..out_h {
                pixels.extend_from_slice(row_of(i, row).unwrap());
            }
            (out_w, out_h, pixels)
        }
        crate::config::DsScreenStacking::Horizontal => {
            // Same dimensions as the canonical frame, columns reordered
            // within each row.
            let mut pixels = Vec::with_capacity(frame.pixels.len());
            for row in 0..frame.height as usize {
                for &i in &order {
                    match row_of(i, row) {
                        Some(slice) => pixels.extend_from_slice(slice),
                        None => pad(&mut pixels, layout.screens[i].width as usize),
                    }
                }
            }
            (frame.width as usize, frame.height as usize, pixels)
        }
    };
    crate::platform::video::framebuffer::Frame {
        pixels: std::sync::Arc::new(pixels),
        width: out_w as u32,
        height: out_h as u32,
        revision: frame.revision,
        effect: frame.effect,
    }
}

/// Radius of a displayed touch, in native touch-screen pixels — about
/// a stylus tip. Scales with the pane like everything else drawn in it.
const TOUCH_SPOT_R: f32 = 6.0;

/// A recorded stylus touch, drawn over the framebuffer at the spot the
/// touch landed: a translucent accent fill under a solid ring, so it
/// reads over any game art without hiding what it points at. Position
/// and radius are fractions of the pane so the canvas needs no resize
/// handling of its own.
struct TouchSpot {
    fx: f32,
    fy: f32,
    rf: f32,
}

impl<M> canvas::Program<M> for TouchSpot {
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
        let center = Point::new(self.fx * bounds.width, self.fy * bounds.height);
        // Floor of 4px so the spot stays visible however small the
        // pane gets.
        let radius = (self.rf * bounds.width).max(4.0);
        let accent = theme.palette().primary;
        frame.fill(&Path::circle(center, radius), iced::Color { a: 0.35, ..accent });
        frame.stroke(
            &Path::circle(center, radius),
            Stroke::default().with_width(2.0).with_color(accent),
        );
        vec![frame.into_geometry()]
    }
}

/// Body: framebuffer + optional setup panes layered over the game's
/// BNLC background art (cover-fit, crops as needed) or a pure-black
/// backdrop when BNLC isn't installed. The backdrop spans the full
/// body width so the setup panes float on top of the same bezel art.
/// `slots` are the PvP setup-drawer slots (`[left, right]`), each
/// `Some(width)` while that drawer holds the row open — see the
/// comment on `drawer_slot` below; always `[None, None]` outside PvP.
fn emulator_body<'a>(
    game: &'static crate::library::game::Game,
    frame: Element<'a, Message>,
    hide_emulator_border: bool,
    slots: [Option<f32>; 2],
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
    // (`setup_handles_overlay`). A slot's width is its drawer's, so a
    // resize drag moves the emulator's edge in step with the pane's.
    let drawer_slot = |w: f32| iced::widget::Space::new().width(iced::Length::Fixed(w)).height(Fill);
    let mut content_row = row![].spacing(0).height(Fill).width(Fill);
    if let Some(w) = slots[0] {
        content_row = content_row.push(drawer_slot(w));
    }
    content_row = content_row.push(container(frame_container).width(Fill).height(Fill));
    if let Some(w) = slots[1] {
        content_row = content_row.push(drawer_slot(w));
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

/// Picture-in-picture inset, top-right below the corner commands: the
/// other side's screen. Both cores render anyway (replay re-simulates the
/// opponent, training runs a live pair); this just insets the extra one.
/// Drawn through its own shader surface ([`PipProgram`]) because the main
/// framebuffer's pipeline owns a single resident texture. Reads the
/// host's captured [`State::pip_frame`], so it's pure presentation with no
/// message of its own — every session kind can push it directly.
///
/// [`PipProgram`]: crate::platform::video::framebuffer::PipProgram
pub(crate) fn pip_overlay<'a>(
    ctx: Ctx<'a>,
    // The PiP side's recorded touch to draw on its touch screen (the
    // replay input display); `None` everywhere else.
    touch_spot: Option<(u16, u16)>,
) -> Option<Element<'a, Message>> {
    let state = ctx.state;
    let mut frame = state.pip_frame.clone()?;
    // The PiP mirrors the main pane's arrangement — it's the same
    // console shape, just the other side's screens.
    let layout = state.active.as_ref().map(|s| s.screen_layout());
    let mut spot = None;
    if let Some(layout) = layout.as_ref().filter(|layout| layout.screens.len() > 1) {
        let touch_first = ctx.ds_primary_screen == crate::config::DsPrimaryScreen::Touch;
        frame = present_frame(frame, layout, ctx.ds_screen_stacking, ctx.ds_primary_screen);
        // This side's touch as a fraction of the inset — the same
        // mapping the main pane runs in `framebuffer_view`, against
        // the presented frame's dimensions.
        spot = touch_spot.and_then(|(tx, ty)| {
            touch_screen_placement(layout, ctx.ds_screen_stacking, touch_first).map(|((origin_x, origin_y), _)| {
                (
                    (origin_x + tx as f32 + 0.5) / frame.width as f32,
                    (origin_y + ty as f32 + 0.5) / frame.height as f32,
                )
            })
        });
    }
    // 1.5x native: readable without dominating the main view.
    let (w, h) = (frame.width as f32 * 1.5, frame.height as f32 * 1.5);
    let native_w = frame.width;
    let fb = iced::widget::shader::Shader::new(crate::platform::video::framebuffer::PipProgram::new(frame))
        .width(Length::Fixed(w))
        .height(Length::Fixed(h));
    let mut fb: Element<'a, Message> = fb.into();
    // The touch over the inset, same treatment as the main pane's:
    // an event-less canvas the pointer passes straight through.
    if let Some((fx, fy)) = spot {
        let overlay = Canvas::new(TouchSpot {
            fx,
            fy,
            rf: TOUCH_SPOT_R / native_w as f32,
        })
        .width(Length::Fixed(w))
        .height(Length::Fixed(h));
        fb = stack![fb, overlay].into();
    }
    let plate = container(fb).padding(3).style(hud_chip_plate);
    Some(
        container(plate)
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Top)
            .padding(iced::Padding {
                // Clear the corner commands' resting spot.
                top: 56.0,
                right: 12.0,
                bottom: 0.0,
                left: 0.0,
            })
            .into(),
    )
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

/// What a session says over its own screen while a priming walk — the
/// run from power-on to the games' link battle — is what it's doing.
/// Every case shows a black screen and no sound for as long as the walk
/// takes, which on a DS-class game is seconds rather than the instant a
/// GBA one takes; without this the session simply looks hung.
///
/// Resolved once per frame by [`framebuffer_view`], which lays it over
/// the rendered frame itself rather than the pane around it — see
/// [`priming_notice`].
struct PrimingCopy {
    title: String,
    detail: String,
    /// How long the wait has run, or `None` once it's over — a failure
    /// has no clock to run.
    clock: Option<String>,
    /// The failure's dismissal, which is the session's own teardown:
    /// nothing is coming, so the only move left is leaving.
    dismiss: Option<String>,
}

fn priming_copy(lang: &LanguageIdentifier, state: &State) -> Option<PrimingCopy> {
    let (title, detail) = match state.prime_wait()? {
        PrimeWait::Match => (
            t!(lang, "playback-priming-match"),
            t!(lang, "playback-priming-match-detail"),
        ),
        PrimeWait::Peer => (
            t!(lang, "playback-priming-peer"),
            t!(lang, "playback-priming-peer-detail"),
        ),
        PrimeWait::Playback => (
            t!(lang, "playback-priming-replay"),
            t!(lang, "playback-priming-replay-detail"),
        ),
        // The engine's own reason, verbatim under a plain title: the
        // walk fails on things the user can act on (a save with no
        // NetBattle unlocked, a game that never reached its battle),
        // and paraphrasing them into one generic line would throw away
        // the part that says which.
        PrimeWait::Failed(error) => {
            return Some(PrimingCopy {
                title: t!(lang, "playback-priming-failed"),
                detail: error,
                clock: None,
                dismiss: Some(t!(lang, "playback-close")),
            })
        }
    };
    // Counts from the frame the wait was first seen (see
    // [`State::prime_wait_since`]) — shown from zero rather than
    // appearing once the wait gets long, so nothing moves under the
    // user partway through.
    let elapsed = state.prime_wait_since.as_ref().map_or(0, |(_, at)| at.elapsed().as_secs());
    Some(PrimingCopy {
        title,
        detail,
        clock: Some(t!(lang, "playback-priming-elapsed", secs = elapsed as i64)),
        dismiss: None,
    })
}

/// The priming notice as it's drawn: centered copy laid straight over
/// the frame, sized to exactly the rendered frame rect (`w` × `h`) so
/// it can't spill into the letterbox or the pane border around it. No
/// panel and no dim wash — the thing underneath is black anyway, and a
/// modal over it would be chrome around nothing.
///
/// While the walk runs, the title breathes on the shared
/// [`pulse`](crate::ui::anim::pulse) the lobby's in-flight status lines
/// use — the app's existing "still working" cue — over a plain seconds
/// counter, which is the only honest readout available: a walk ends
/// when the games' own traps say it has, not at a knowable fraction. A
/// failure stops breathing and takes a dismissal instead.
fn priming_notice<'a>(copy: &PrimingCopy, w: f32, h: f32) -> Element<'a, Message> {
    let failed = copy.dismiss.is_some();
    let pulse = crate::ui::anim::pulse();
    let title = text(copy.title.clone())
        .size(TEXT_BODY)
        .align_x(iced::alignment::Horizontal::Center)
        .style(move |theme: &iced::Theme| iced::widget::text::Style {
            color: Some(if failed {
                theme.palette().danger
            } else {
                widgets::mix(
                    widgets::muted_color(theme),
                    theme.palette().primary,
                    0.45 + 0.55 * pulse,
                )
            }),
        });
    let mut copy_col = column![title].spacing(6).align_x(Alignment::Center);
    copy_col = copy_col.push(
        text(copy.detail.clone())
            .size(TEXT_CAPTION)
            .align_x(iced::alignment::Horizontal::Center)
            .style(widgets::muted_text_style),
    );
    if let Some(clock) = copy.clock.as_ref() {
        copy_col = copy_col.push(
            text(clock.clone())
                .size(TEXT_CAPTION)
                .align_x(iced::alignment::Horizontal::Center)
                .style(widgets::muted_text_style),
        );
    }
    if let Some(dismiss) = copy.dismiss.as_ref() {
        copy_col = copy_col.push(widgets::labeled_icon_button(
            Icon::X,
            dismiss.clone(),
            Message::Close,
            [6.0, 12.0],
            widgets::neutral,
        ));
    }
    // Padded so the copy wraps inside the frame instead of running to
    // its edges — a GBA frame at 1× is only 240 px wide, and the text
    // has to live within whatever the pane gave us.
    container(copy_col)
        .width(Length::Fixed(w))
        .height(Length::Fixed(h))
        .padding(12)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .into()
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

#[cfg(test)]
mod tests {
    use crate::config::DsScreenStacking::{Horizontal, PrimaryOnly, Vertical};

    fn screen() -> tango_match::Screen {
        tango_match::Screen {
            width: 256,
            height: 192,
        }
    }

    /// A DS composing its whole console: upper screen, then the touch
    /// screen it points at.
    fn both() -> tango_match::ScreenLayout {
        tango_match::ScreenLayout::new([screen(), screen()]).with_touch(1)
    }

    /// The stylus area follows the touch screen through every
    /// arrangement. Pinned because a wrong origin doesn't look wrong —
    /// the pane draws fine and every press just lands somewhere else.
    #[test]
    fn the_stylus_area_follows_the_touch_screen() {
        let place = |stacking, touch_first| super::touch_screen_placement(&both(), stacking, touch_first);
        // Trailing the upper screen, along whichever axis stacks.
        assert_eq!(place(Horizontal, false).unwrap().0, (256.0, 0.0));
        assert_eq!(place(Vertical, false).unwrap().0, (0.0, 192.0));
        // Leading, when it's the primary screen.
        assert_eq!(place(Horizontal, true).unwrap().0, (0.0, 0.0));
        assert_eq!(place(Vertical, true).unwrap().0, (0.0, 0.0));
        assert_eq!(place(PrimaryOnly, true).unwrap().0, (0.0, 0.0));
        // Primary-only led by the upper screen leaves it off the pane.
        assert!(place(PrimaryOnly, false).is_none());
    }

    /// A session composing without its touch screen — a game whose
    /// netbattle never leaves the upper one — has nothing to point at,
    /// so no arrangement produces a stylus area.
    #[test]
    fn a_composition_without_the_touch_screen_has_no_stylus_area() {
        let upper = tango_match::ScreenLayout::new([screen()]);
        for stacking in [Horizontal, Vertical, PrimaryOnly] {
            for touch_first in [false, true] {
                assert!(super::touch_screen_placement(&upper, stacking, touch_first).is_none());
            }
        }
    }

    /// The mirror case, which is the whole reason the layout names its
    /// touch screen rather than the pane assuming the second of two:
    /// a composition of the touch screen alone is all stylus.
    #[test]
    fn a_touch_only_composition_is_all_stylus() {
        let touch = tango_match::ScreenLayout::new([screen()]).with_touch(0);
        for stacking in [Horizontal, Vertical, PrimaryOnly] {
            for touch_first in [false, true] {
                let (origin, size) = super::touch_screen_placement(&touch, stacking, touch_first).unwrap();
                assert_eq!(origin, (0.0, 0.0));
                assert_eq!(size, screen());
            }
        }
    }
}

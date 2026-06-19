use super::*;
// Explicit so these win over iced's prelude `column!`/`row!` macros, which
// would otherwise clash with the sweeten ones re-exported via `super::*`.
use sweeten::widget::{column, mouse_area, row};

mod telemetry;
use telemetry::telemetry_overlay;

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
    let drawer_moving = state.self_panel.is_animating(now) || state.opponent_panel.is_animating(now);
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
        if s.local_loaded.is_some() && state.self_panel.shown() {
            content_row = content_row.push(drawer_slot());
        }
    }
    content_row = content_row.push(container(frame_container).width(Fill).height(Fill));
    if let ActiveSession::PvP(s) = session {
        if s.opponent_loaded.is_some() && state.opponent_panel.shown() {
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
    if s.local_loaded.is_some() && (state.self_panel.shown() || state.self_panel.is_animating(now)) {
        let me = s.local_loaded.as_ref().unwrap();
        let panel = save_view::view(lang, me, &s.local_save_view, true, None, false, false)
            .map(Message::SelfSaveViewAction);
        let pane = setup_pane(panel, -SETUP_DRAWER_TRAVEL, state.self_panel.progress(now));
        panes.push(
            container(pane)
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Left)
                .into(),
        );
    }
    if s.opponent_loaded.is_some() && (state.opponent_panel.shown() || state.opponent_panel.is_animating(now)) {
        let opponent = s.opponent_loaded.as_ref().unwrap();
        let panel = save_view::view(lang, opponent, &s.opponent_save_view, true, None, false, false)
            .map(Message::OpponentSaveViewAction);
        let pane = setup_pane(panel, SETUP_DRAWER_TRAVEL, state.opponent_panel.progress(now));
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
/// tick readouts) plus the options trigger, at the chunky bar-control
/// sizing. SP/PvP don't use this — their few controls live in compact
/// corner chips ([`corner_chips`]).
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
        ActiveSession::PvP(pvp) => pvp.opponent_loaded.is_some() && state.opponent_panel.shown(),
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
            state.self_panel.shown(),
            state.self_panel.progress(now),
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
            state.opponent_panel.shown(),
            state.opponent_panel.progress(now),
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
        .scrub
        .preview
        .or_else(|| r.pending_seek_target())
        .unwrap_or_else(|| r.current_tick())
        .min(total);
    let prefetched = r.prefetch_progress().min(total);
    // The mgba thread is paused for the duration of a scrub drag and
    // the seek chase that follows it, but when playback resumes on
    // landing the session is logically still *playing* — flipping the
    // button to "Play" mid-scrub reads as a stuck pause.
    let logically_playing = (state.scrub.preview.is_some() && state.scrub.resume) || r.seek_will_resume();
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
    let h = state.scrub.hover?;
    let (_, handle) = state.scrub.thumb.as_ref()?;
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
    if !(state.disconnect.visible(now) && matches!(session, ActiveSession::PvP(_))) {
        return None;
    }
    let progress = state.disconnect.progress(now);
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
    if state.disconnect.shown() {
        backdrop = backdrop.on_press(|_| Message::CloseDisconnectConfirm);
    }
    Some(iced::widget::stack![Element::from(backdrop), Element::from(placement)].into())
}


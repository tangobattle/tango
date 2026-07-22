//! Live-PvP session view: the setup drawers and their edge handles,
//! the telemetry deck (see [`telemetry`]), and the disconnect /
//! reconnect modals — plus the [`Message`]s those controls emit and
//! their [`update`] handler.

use super::*;
use crate::session::pvp::PvpSession;
use crate::session::Message as SessionMessage;
// Explicit so these win over iced's prelude `column!`/`row!` macros (see mod.rs).
use sweeten::widget::{column, row};

mod telemetry;
use telemetry::telemetry_overlay;

/// Messages the PvP view emits. Wrapped as [`SessionMessage::Pvp`] on
/// the way out; inert unless a PvP session is active.
#[derive(Debug, Clone)]
pub enum Message {
    /// The match-settings frame-delay slider moved. Live-sets this
    /// side's local frame delay on the running session; the App also
    /// persists it to config. No peer coordination — it's purely a
    /// local display lag.
    SetFrameDelay(u32),
    /// Open/close the match-settings popover anchored on the
    /// telemetry plate (instrument panel).
    ToggleMatchSettings,
    /// Show the "really disconnect?" modal — the corner tear-down
    /// button while the link is live. Disconnect tears the session
    /// down mid-match (same as Close), so the confirm keeps a stray
    /// click from costing the user a real game.
    OpenDisconnectConfirm,
    /// Dismiss the disconnect confirm without disconnecting (the
    /// Cancel button + the modal backdrop both fire this).
    CloseDisconnectConfirm,
    /// Show/hide the opponent's setup side panel.
    ToggleOpponentPanel,
    /// Show/hide the local player's save-view panel.
    ToggleSelfPanel,
    /// User interacted with the opponent's save-view (tab swap,
    /// folder-group toggle, hover, …).
    OpponentSaveView(save_view::Action),
    /// Mirror of [`OpponentSaveView`](Self::OpponentSaveView) for the
    /// local panel.
    SelfSaveView(save_view::Action),
}

/// Apply a PvP-view message. Takes the whole session [`State`]: the
/// panel/popover overlays live there, beside the session slot.
pub(crate) fn update(state: &mut State, msg: Message) -> iced::Task<Message> {
    match msg {
        Message::SetFrameDelay(d) => {
            // Purely local frame delay — apply straight to the running
            // session. Config persistence happens in the App's wrapper
            // (it owns config).
            if let Some(s) = state.active_as::<PvpSession>() {
                s.set_frame_delay(d);
            }
        }
        Message::ToggleMatchSettings => {
            if state.active_as::<PvpSession>().is_some() {
                state.match_settings.toggle();
            }
        }
        Message::OpenDisconnectConfirm => {
            state.disconnect.open();
        }
        Message::CloseDisconnectConfirm => {
            state.disconnect.close();
        }
        Message::ToggleOpponentPanel => {
            state.opponent_panel.toggle();
        }
        Message::ToggleSelfPanel => {
            state.self_panel.toggle();
        }
        Message::OpponentSaveView(action) => {
            if let Some(panes) = state.pvp_panes.as_mut() {
                return panes.opponent_save_view.fold(&action).map(Message::OpponentSaveView);
            }
        }
        Message::SelfSaveView(action) => {
            if let Some(panes) = state.pvp_panes.as_mut() {
                return panes.local_save_view.fold(&action).map(Message::SelfSaveView);
            }
        }
    }
    iced::Task::none()
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

/// Total travel of a setup drawer — what the sidebar slides
/// through on open/close and how far its edge handle rides
/// inward. Equal to the pane width: the sidebar docks flush with
/// its screen edge.
const SETUP_DRAWER_TRAVEL: f32 = SETUP_PANE_WIDTH;

/// Live PvP: emulator with setup-drawer slots, the drawer edge
/// handles, the drawers themselves, telemetry, and the
/// disconnect / reconnect modals.
pub(crate) fn view<'a>(p: &'a PvpSession, ctx: Ctx<'a>) -> Element<'a, SessionMessage> {
    let Ctx { lang, state, .. } = ctx;
    let now = iced::time::Instant::now();
    let frame = framebuffer_view(state, ctx.fractional_scaling, ctx.effect);
    let panes = state.pvp_panes.as_ref();
    let slots = [
        panes.is_some_and(|panes| panes.local_loaded.is_some()) && state.self_panel.shown(),
        panes.is_some_and(|panes| panes.opponent_loaded.is_some()) && state.opponent_panel.shown(),
    ];
    let body = emulator_body(p.local_game(), frame, ctx.hide_emulator_border, slots);
    let mut stacked = stack![body];
    // A drawer pane mid-animation draws in iced's floating layer,
    // above every base stack layer — so for those moments the
    // telemetry plate is hoisted into the floating layer too, where
    // tree order puts it back on top of the moving pane. See
    // `keep_above_drawers` for why it isn't hoisted permanently.
    // The top-right commands stay un-hoisted on purpose: the
    // drawers are supposed to cover them.
    let drawer_moving = state.self_panel.is_animating(now) || state.opponent_panel.is_animating(now);
    if state.controls_anim.visible(now) {
        // The setup toggles ride the screen edges as drawer handles
        // (the replay transport's slot in the layer order).
        stacked = stacked.push(setup_handles_overlay(lang, state, state.controls_anim.progress(now)));
        // Settings + tear-down, top-right; a live link routes the
        // tear-down through the disconnect confirm (whose copy
        // carries the unplug framing); once the link is already gone
        // (`latency()` = None ⇒ remote dropped) there's nothing left
        // to protect, so it closes directly. Pushed BEFORE the setup
        // drawers so an open drawer layers over them rather than the
        // buttons intruding on the pane.
        let tear_down_msg = if p.latency().is_some() {
            SessionMessage::Pvp(Message::OpenDisconnectConfirm)
        } else {
            SessionMessage::Close
        };
        stacked = stacked.push(corner_commands_overlay(lang, state, tear_down_msg, slots[1]));
    }
    // Setup drawers — above the corner commands, below the telemetry
    // plate (see `setup_drawers_overlay`).
    for pane in setup_drawers_overlay(lang, state) {
        stacked = stacked.push(pane.map(SessionMessage::Pvp));
    }
    // Signal indicator / expanded telemetry graph, bottom-right.
    // Deliberately outside the floating-controls gate — connection
    // health stays glanceable even when the controls tuck away.
    if let Some(o) = telemetry_overlay(lang, p, state) {
        stacked = stacked.push(keep_above_drawers(o.map(SessionMessage::Pvp), drawer_moving));
    }
    if let Some(o) = disconnect_overlay(lang, state) {
        stacked = stacked.push(o);
    }
    // The auto-reconnect modal. Above the disconnect-confirm so that if
    // the link drops while that prompt is open, "Reconnecting…" reads over it.
    if let Some(o) = reconnecting_overlay(lang, p) {
        stacked = stacked.push(o);
    }
    finish_session_stack(lang, state, stacked)
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
fn setup_drawers_overlay<'a>(lang: &'a LanguageIdentifier, state: &'a State) -> Vec<Element<'a, Message>> {
    let now = iced::time::Instant::now();
    let Some(s) = state.pvp_panes.as_ref() else {
        return Vec::new();
    };
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
    if let Some(me) = s
        .local_loaded
        .as_ref()
        .filter(|_| state.self_panel.shown() || state.self_panel.is_animating(now))
    {
        let panel = save_view::view(lang, me, &s.local_save_view, true, None, false, false).map(Message::SelfSaveView);
        let pane = setup_pane(panel, -SETUP_DRAWER_TRAVEL, state.self_panel.progress(now));
        panes.push(
            container(pane)
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Left)
                .into(),
        );
    }
    if let Some(opponent) = s
        .opponent_loaded
        .as_ref()
        .filter(|_| state.opponent_panel.shown() || state.opponent_panel.is_animating(now))
    {
        let panel = save_view::view(lang, opponent, &s.opponent_save_view, true, None, false, false)
            .map(Message::OpponentSaveView);
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

/// Hoist a persistent chrome layer into iced's floating layer —
/// an invisible sub-pixel translation — so it keeps drawing above
/// a drawer pane mid-animation (which floats, and floats render
/// above every base stack layer; among floats, tree order wins,
/// and these layers come after the drawer). Used by the telemetry
/// plate, the one piece of chrome that outranks the drawers — the
/// top-right commands deliberately layer UNDER them instead. Only
/// applied while a drawer is actually moving: hoisted permanently,
/// the chrome would also paint over the settings/disconnect modals.
fn keep_above_drawers(el: Element<'_, SessionMessage>, drawer_moving: bool) -> Element<'_, SessionMessage> {
    if drawer_moving {
        iced::widget::float(el)
            .translate(|_bounds, _viewport| iced::Vector::new(0.0, 0.001))
            .into()
    } else {
        el
    }
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
    state: &'a State,
    hide_progress: f32,
) -> Element<'a, SessionMessage> {
    let now = iced::time::Instant::now();
    let panes = state.pvp_panes.as_ref();

    // `on_left`: which screen edge the tab grows out of.
    // `drawer_progress`: how far the tab's drawer is out (0..1) —
    // the tab rides the drawer's moving inner edge.
    let handle = |on_left: bool,
                  open: bool,
                  drawer_progress: f32,
                  accent: Color,
                  label: String,
                  msg: Option<SessionMessage>|
     -> Element<'a, SessionMessage> {
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
            .on_enter(SessionMessage::ControlsHovered(true))
            .on_exit(SessionMessage::ControlsHovered(false));
        // Riding the drawer's inner edge is LAYOUT (edge padding),
        // not a Float translation: a floating element draws in
        // iced's overlay layer above everything, so a permanently
        // translated handle would sit on top of the settings /
        // disconnect modals whenever a drawer is open.
        let ride = drawer_progress * SETUP_DRAWER_TRAVEL;
        let positioned: Element<'a, SessionMessage> = container(pinned)
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
    if panes.is_some_and(|panes| panes.local_loaded.is_some()) {
        edges = edges.push(handle(
            true,
            state.self_panel.shown(),
            state.self_panel.progress(now),
            FIELD_RED,
            t!(lang, "session-self"),
            Some(SessionMessage::Pvp(Message::ToggleSelfPanel)),
        ));
    }
    edges = edges.push(horizontal_space());
    // No tab at all when the peer blinded their setup — a
    // permanently dead handle is just clutter on the edge.
    if panes.is_some_and(|panes| panes.opponent_loaded.is_some()) {
        edges = edges.push(handle(
            false,
            state.opponent_panel.shown(),
            state.opponent_panel.progress(now),
            FIELD_BLUE,
            t!(lang, "session-opponent"),
            Some(SessionMessage::Pvp(Message::ToggleOpponentPanel)),
        ));
    }

    container(edges)
        .width(Fill)
        .height(Fill)
        .align_y(iced::alignment::Vertical::Center)
        .into()
}

/// Disconnect confirmation modal (PvP-only). Centered panel with a
/// dimmed click-to-dismiss backdrop — same shape as app.rs's
/// in-session Settings modal so the two read as the same family
/// of "this interrupts what you're doing" dialogs. Sits above
/// the options popover in the stack so it covers the menu if
/// the user somehow re-opened it.
fn disconnect_overlay<'a>(lang: &'a LanguageIdentifier, state: &'a State) -> Option<Element<'a, SessionMessage>> {
    let now = iced::time::Instant::now();
    if !state.disconnect.visible(now) {
        return None;
    }
    let progress = state.disconnect.progress(now);
    let title = text(t!(lang, "playback-disconnect-prompt")).size(TEXT_BODY + 4.0);
    let body_text = text(t!(lang, "playback-disconnect-detail")).style(widgets::muted_text_style);
    let cancel_btn = widgets::labeled_icon_button(
        Icon::X,
        t!(lang, "playback-cancel"),
        SessionMessage::Pvp(Message::CloseDisconnectConfirm),
        [8.0, 14.0],
        widgets::neutral,
    );
    let disconnect_btn = widgets::labeled_icon_button(
        Icon::Unplug,
        t!(lang, "playback-disconnect"),
        SessionMessage::Close,
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
    // Dim + click-swallow + centering come from the shared
    // scaffolding; the dismiss handler is only armed while the
    // modal is actually open so a click during the fade-out can't
    // re-fire the close.
    Some(widgets::modal_layer(
        anim::pop(panel, progress, 8.0),
        0.55 * progress,
        SessionMessage::NoOp,
        state
            .disconnect
            .shown()
            .then_some(SessionMessage::Pvp(Message::CloseDisconnectConfirm)),
    ))
}

/// The automatic mid-match reconnect modal: shown while a dropped direct link is
/// being transparently rebuilt ([`PvpSession::is_reconnecting`]). The emulator
/// is paused underneath, so this is a static notice rather than an animated
/// one — plus a Disconnect escape hatch (routes through [`Message::Close`], same
/// as the confirm dialog's) so the user can abandon the wait. Pushed last in
/// [`view`] so it sits above every other layer.
fn reconnecting_overlay<'a>(lang: &'a LanguageIdentifier, pvp: &'a PvpSession) -> Option<Element<'a, SessionMessage>> {
    if !pvp.is_reconnecting() {
        return None;
    }
    let title = text(t!(lang, "playback-reconnecting")).size(TEXT_BODY + 4.0);
    let body_text = text(t!(lang, "playback-reconnecting-detail")).style(widgets::muted_text_style);
    // Depleting bar for the time left before give-up, in place of a text
    // countdown: it fills the panel width (no wrap/min-width pitfall) and the
    // coordinator ticks the session redraw ~30 fps while paused so it eases down.
    let time_left = pvp.reconnect_progress().unwrap_or(0.0);
    let progress = iced::widget::progress_bar(0.0..=1.0, time_left)
        .length(Fill)
        .girth(6.0)
        .style(|theme: &iced::Theme| iced::widget::progress_bar::Style {
            background: iced::Background::Color(iced::Color {
                a: 0.12,
                ..iced::Color::WHITE
            }),
            bar: iced::Background::Color(theme.extended_palette().primary.base.color),
            border: iced::Border {
                radius: 3.0.into(),
                ..Default::default()
            },
        });
    let disconnect_btn = widgets::labeled_icon_button(
        Icon::Unplug,
        t!(lang, "playback-disconnect"),
        SessionMessage::Close,
        [8.0, 14.0],
        widgets::danger_button,
    );
    let buttons = row![horizontal_space(), disconnect_btn]
        .spacing(8)
        .align_y(Alignment::Center);
    let panel = container(column![title, body_text, progress, buttons].spacing(14).width(Fill))
        .width(iced::Length::Fixed(420.0))
        .padding(20)
        .style(widgets::panel);
    // Solid dim, no dismiss-on-press: the user leaves only via Disconnect (or
    // the link returning, which clears the overlay on its own).
    Some(widgets::modal_layer(panel.into(), 0.55, SessionMessage::NoOp, None))
}

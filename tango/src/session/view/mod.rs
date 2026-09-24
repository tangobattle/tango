//! Session screens. Each session kind composes its own from the shared
//! pieces here: the emulator body over its backdrop, the framebuffer
//! presentation ([`frame`]), the floating HUD ([`hud`]), and the
//! priming notice ([`priming`]).

use super::*;
// Explicit so this wins over iced's prelude `row!` macro, which
// would otherwise clash with the sweeten ones re-exported via `super::*`.
use sweeten::widget::row;

mod frame;
mod hud;
mod priming;
pub mod pvp;
pub mod replay;
pub mod results;
pub mod singleplayer;
pub mod training;
use frame::{framebuffer_view, pip_overlay, stacked_framebuffers};
use hud::{
    corner_commands_overlay, exit_hold_overlay, hud_chip_plate, lit_plate_button, telemetry_plate_button,
    CONTROLS_SLIDE,
};
pub use results::results_view;

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
    pub result: Option<Result<(), &'a crate::tabs::replays::ExportError>>,
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

/// The active session's screen, or nothing while none is running.
pub fn view(ctx: Ctx<'_>) -> Element<'_, Message> {
    let Some(session) = ctx.state.active.as_deref() else {
        return iced::widget::Space::new().width(Fill).height(Fill).into();
    };
    // Each session kind assembles its own screen. The engine-side
    // trait deliberately knows nothing about rendering, so this is the
    // one place a session's concrete kind picks its view.
    if let Some(s) = session.downcast_ref::<crate::session::replay::ReplaySession>() {
        replay::view(s, ctx)
    } else if let Some(s) = session.downcast_ref::<crate::session::pvp::PvpSession>() {
        pvp::view(s, ctx)
    } else if session
        .downcast_ref::<crate::session::singleplayer::SinglePlayerSession>()
        .is_some()
    {
        singleplayer::view(ctx)
    } else if let Some(s) = session.downcast_ref::<crate::session::training::TrainingSession>() {
        training::view(s, ctx)
    } else {
        // Unreachable today — the four kinds above are the only
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

/// Platform-native chord for one replay opponent-view preset.
fn opponent_view_shortcut(view: crate::config::OpponentView) -> String {
    let digit = match view {
        crate::config::OpponentView::Off => "1",
        crate::config::OpponentView::PictureInPicture => "2",
        crate::config::OpponentView::StackHorizontally => "3",
        crate::config::OpponentView::StackVertically => "4",
    };
    if cfg!(target_os = "macos") {
        format!("⌥{digit}")
    } else {
        format!("Alt+{digit}")
    }
}

/// The four checked rows used by each opponent-view dropdown. Replay
/// menus show their direct preset shortcuts; training shares the choices
/// but not the replay-only keyboard handler, so its rows omit them.
fn opponent_view_items<M>(
    lang: &LanguageIdentifier,
    selected: crate::config::OpponentView,
    message: impl Fn(crate::config::OpponentView) -> M,
    show_shortcuts: bool,
) -> Vec<widgets::MenuItem<M>> {
    use crate::config::OpponentView::{Off, PictureInPicture, StackHorizontally, StackVertically};
    [Off, PictureInPicture, StackHorizontally, StackVertically]
        .into_iter()
        .map(|view| {
            let item = widgets::MenuItem::toggle(opponent_view_label(lang, view), message(view), view == selected);
            if show_shortcuts {
                item.shortcut(opponent_view_shortcut(view))
            } else {
                item
            }
        })
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
fn main_frame_alignment(view: crate::config::OpponentView) -> (iced::alignment::Horizontal, iced::alignment::Vertical) {
    match view {
        crate::config::OpponentView::StackHorizontally => {
            (iced::alignment::Horizontal::Right, iced::alignment::Vertical::Center)
        }
        crate::config::OpponentView::StackVertically => {
            (iced::alignment::Horizontal::Center, iced::alignment::Vertical::Bottom)
        }
        _ => (iced::alignment::Horizontal::Center, iced::alignment::Vertical::Center),
    }
}
/// Body: framebuffer + optional setup panes layered over the game's
/// BNLC background art (cover-fit, crops as needed) or a pure-black
/// backdrop when BNLC isn't installed. The backdrop spans the full
/// body width so the setup panes float on top of the same bezel art.
/// `slots` are the PvP setup-drawer slots (`[left, right]`), each
/// `Some(width)` while that drawer holds the row open — see the
/// comment on `drawer_slot` below; always `[None, None]` outside PvP.
fn emulator_body<'a>(ctx: Ctx<'a>, frame: Element<'a, Message>, slots: [Option<f32>; 2]) -> Element<'a, Message> {
    let frame_container = container(frame).center(Fill);
    let bnlc_bg = if ctx.hide_emulator_border {
        None
    } else {
        ctx.state.backdrop.clone()
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

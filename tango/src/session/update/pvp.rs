//! Live-PvP controls: the frame-delay, drawer and modal messages the PvP
//! view emits and what they do to the session state.

use super::Effect;
use crate::session::pvp::PvpSession;
use crate::session::{view, Message as SessionMessage, State};

/// Messages the PvP view emits. Wrapped as [`SessionMessage::Pvp`] on
/// the way out; inert unless a PvP session is active.
#[derive(Debug, Clone)]
pub enum Message {
    /// The telemetry panel's frame-delay slider moved. Live-sets this
    /// side's local frame delay on the running session and persists it
    /// to config. No peer coordination — it's purely a
    /// local display lag.
    SetFrameDelay(u32),
    /// Open/close the compact frame-delay popover above the persistent
    /// telemetry bar. The charts themselves never collapse.
    ToggleFrameDelayControl,
    /// Show the "really disconnect?" modal — the corner tear-down
    /// button while the link is live. Disconnect tears the session
    /// down mid-match (same as Close), so the confirm keeps a stray
    /// click from costing the user a real game.
    OpenDisconnectConfirm,
    /// Dismiss the disconnect confirm without disconnecting (the
    /// Cancel button + the modal backdrop both fire this).
    CloseDisconnectConfirm,
    /// Dismiss the advisory describing invalid committed builds. The match
    /// continues either way; this only removes the warning card.
    DismissBuildWarning,
    /// Explicitly open/close the opponent build's violation list.
    ToggleBuildWarningViolations,
    /// Show/hide the opponent's setup side panel.
    ToggleOpponentPanel,
    /// Show/hide the local player's save-view panel.
    ToggleSelfPanel,
    /// User interacted with the opponent's save-view (tab swap,
    /// folder-group toggle, hover, …).
    OpponentSaveView(std::sync::Arc<dyn tango_gamesupport::SaveEditorMessage>),
    /// Mirror of [`OpponentSaveView`](Self::OpponentSaveView) for the
    /// local panel.
    SelfSaveView(std::sync::Arc<dyn tango_gamesupport::SaveEditorMessage>),
    /// A setup drawer's inner edge was grabbed to resize it. Carries
    /// the side (0 = self, 1 = opponent) and that drawer's width at
    /// the grab, which the drag then works in deltas off.
    StartPaneResize(usize, f32),
    /// Cursor moved during a drawer resize — window x, from the
    /// full-window capture layer that's up for the drag's duration.
    PaneResizeMoved(f32),
    /// Drawer resize finished: button released, or the cursor left the
    /// window mid-drag. The new widths are persisted here.
    EndPaneResize,
}

/// Apply a PvP-view message. Takes the whole session [`State`] because the
/// setup drawers and disconnect/build-warning overlays live beside the slot.
pub(crate) fn update(state: &mut State, msg: Message, lang: &unic_langid::LanguageIdentifier) -> Option<Effect> {
    match msg {
        Message::SetFrameDelay(d) => {
            // Purely local frame delay — apply straight to the running
            // session, and remember it for the next match.
            if let Some(s) = state.active_as::<PvpSession>() {
                s.set_frame_delay(d);
            }
            return Some(Effect::PersistFrameDelay(d));
        }
        Message::ToggleFrameDelayControl => {
            if state.active_as::<PvpSession>().is_some() {
                state.frame_delay_control.toggle();
            }
        }
        Message::OpenDisconnectConfirm => {
            state.disconnect.open();
        }
        Message::CloseDisconnectConfirm => {
            state.disconnect.close();
        }
        Message::DismissBuildWarning => {
            if let Some(panes) = state.pvp_panes.as_mut() {
                panes.build_warning_dismissed = true;
            }
        }
        Message::ToggleBuildWarningViolations => {
            if let Some(panes) = state.pvp_panes.as_mut() {
                panes.build_warning_violations_expanded = !panes.build_warning_violations_expanded;
            }
        }
        Message::ToggleOpponentPanel => {
            state.opponent_panel.toggle();
        }
        Message::ToggleSelfPanel => {
            state.self_panel.toggle();
        }
        Message::OpponentSaveView(msg) => {
            // View-local folds only in practice (tab swaps, hovers):
            // the in-match drawers render read-only, so the editor can't
            // mint an edit to stage into this side's loaded save.
            if let Some(panes) = state.pvp_panes.as_mut() {
                if let Some(data) = panes.opponent_loaded.as_mut() {
                    let (task, _) = data.editor.update(lang, data, &*msg);
                    return Some(Effect::Task(
                        task.map(|m| SessionMessage::Pvp(Message::OpponentSaveView(m))),
                    ));
                }
            }
        }
        Message::SelfSaveView(msg) => {
            if let Some(panes) = state.pvp_panes.as_mut() {
                if let Some(data) = panes.local_loaded.as_mut() {
                    let (task, _) = data.editor.update(lang, data, &*msg);
                    return Some(Effect::Task(
                        task.map(|m| SessionMessage::Pvp(Message::SelfSaveView(m))),
                    ));
                }
            }
        }
        Message::StartPaneResize(side, width) => {
            if let Some(panes) = state.pvp_panes.as_mut() {
                panes.pane_drag = Some(crate::session::PaneDrag {
                    side,
                    start_width: width,
                    anchor_x: None,
                });
            }
        }
        Message::PaneResizeMoved(x) => {
            if let Some(panes) = state.pvp_panes.as_mut() {
                if let Some(drag) = panes.pane_drag.as_mut() {
                    let anchor = *drag.anchor_x.get_or_insert(x);
                    // Each drawer widens as the cursor pulls its inner
                    // edge toward the middle of the window: rightward
                    // for the left drawer, leftward for the right one.
                    let delta = if drag.side == 0 { x - anchor } else { anchor - x };
                    panes.pane_widths[drag.side] =
                        (drag.start_width + delta).clamp(view::SETUP_PANE_MIN_WIDTH, view::SETUP_PANE_MAX_WIDTH);
                }
            }
        }
        Message::EndPaneResize => {
            let panes = state.pvp_panes.as_mut()?;
            panes.pane_drag = None;
            return Some(Effect::PersistPaneWidths(panes.pane_widths));
        }
    }
    None
}

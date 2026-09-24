//! Live-PvP controls: the frame-delay, drawer and modal messages the PvP
//! view emits, what they do to the session state, and the presentation
//! state (setup drawers, telemetry history) they act on.

use super::Effect;
use crate::session::pvp::PvpSession;
use crate::session::{Message as SessionMessage, State};

/// One per-frame snapshot of the live PvP telemetry, retained in a short ring
/// buffer ([`State::metric_history`]) so the persistent PvP panel can draw a
/// sparkline per metric. `round` is `None` between rounds, when no
/// skew/lead/depth reading exists; when present it is `(skew, depth, lead)`.
#[derive(Clone, Copy)]
pub struct MetricSample {
    pub tps: f32,
    pub fps_target: f32,
    /// Latest raw RTT, absent while the link is still coming up or is
    /// temporarily down. Keeping absence distinct from `0 ms` lets the
    /// persistent chart show an honest gap instead of a perfect-looking sample
    /// before the first pong.
    pub ping_ms: Option<u128>,
    pub round: Option<(i32, u32, i32)>,
}

impl MetricSample {
    /// Read the current telemetry off a live PvP session. Called once per
    /// emulator frame by the [`SessionMessage::UpdateFramebuffer`] handler.
    pub(in crate::session) fn capture(pvp: &PvpSession) -> Self {
        Self {
            tps: pvp.tps(),
            fps_target: pvp.fps_target(),
            // Raw latest ping (not the median) — the sparkline is a live
            // display, so it should track the true per-frame reading and
            // show spikes. The median feeds only the frame-delay suggestion.
            ping_ms: pvp.latency_raw().map(|d| d.as_millis()),
            round: pvp.round_stats().map(|s| (s.skew, s.depth, s.lead)),
        }
    }
}

/// How many frames of telemetry the sparklines retain (~3 s at 60 fps).
pub(in crate::session) const METRIC_HISTORY_LEN: usize = 180;

/// PvP-only presentation state riding alongside the session engine:
/// both sides' fully-loaded selections (rom + parsed save + derived
/// assets) for the in-match setup drawers, plus each drawer's
/// save-view tab/grouping state. Built by
/// [`spawn_pvp`](crate::session::spawn_pvp) and installed into
/// [`State::pvp_panes`] with the session; the loadeds also feed the
/// post-match results card.
pub struct PvpPanes {
    /// Local side's loaded selection — the "my setup" drawer.
    pub local_loaded: Option<crate::selection::LoadedSave>,
    /// Opponent's loaded selection, unless they blinded their setup.
    pub opponent_loaded: Option<crate::selection::LoadedSave>,
    /// Local validation warnings for the opponent's committed save. `None`
    /// means legal. This opaque report is computed from the bytes the match
    /// actually runs, never trusted from a peer flag.
    pub opponent_build_warnings: Option<tango_gamesupport::OpaqueBuildWarnings>,
    /// A build warning remains visible until explicitly dismissed. Replacing
    /// `PvpPanes` for the next match naturally resets it.
    pub build_warning_dismissed: bool,
    /// Whether the warning's exact violation list has been explicitly opened.
    /// Starts collapsed so opponent build details are never shown implicitly.
    pub build_warning_violations_expanded: bool,
    /// Current width of each setup drawer (`[self, opponent]`), seeded
    /// from `config.pvp_setup_pane_widths` at match start and moved by
    /// dragging a drawer's inner edge. The App mirrors it back into
    /// config when a drag ends.
    pub pane_widths: [f32; 2],
    /// The drawer edge currently being dragged, `None` at rest.
    pub pane_drag: Option<PaneDrag>,
}

/// A setup drawer being sized by its inner edge. `anchor_x` is latched
/// on the drag's FIRST move rather than at the press: iced's
/// `mouse_area` press carries no cursor position, so the first move
/// establishes the origin and every move after it is a delta off
/// `start_width` — which keeps the pane from jumping to sit centered
/// under the cursor when the grab lands off the edge's exact pixel.
#[derive(Clone, Copy)]
pub struct PaneDrag {
    /// Which drawer, indexing [`PvpPanes::pane_widths`]: 0 = self
    /// (left edge), 1 = opponent (right).
    pub side: usize,
    /// The drawer's width when the grab started.
    pub start_width: f32,
    /// Window x the drag measures from, `None` until the first move.
    pub anchor_x: Option<f32>,
}

// A LoadedSave is a whole parsed rom + save; a placeholder keeps the
// enclosing app Message (which carries a `Slot<(PvpSession, PvpPanes)>`)
// derivable, same as PreMatchData.
impl std::fmt::Debug for PvpPanes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PvpPanes { .. }")
    }
}

impl PvpPanes {
    /// Fresh presentation state for a match: nothing dismissed or
    /// expanded, and the drawers at their remembered `widths`. Clamped on
    /// the way in: the persisted pair predates the current bounds on an
    /// older config, or the window it was sized against is gone.
    pub(in crate::session) fn new(
        local_loaded: crate::selection::LoadedSave,
        opponent_loaded: Option<crate::selection::LoadedSave>,
        opponent_build_warnings: Option<tango_gamesupport::OpaqueBuildWarnings>,
        widths: [f32; 2],
    ) -> Self {
        Self {
            local_loaded: Some(local_loaded),
            opponent_loaded,
            opponent_build_warnings,
            build_warning_dismissed: false,
            build_warning_violations_expanded: false,
            pane_widths: widths.map(|w| w.clamp(SETUP_PANE_MIN_WIDTH, SETUP_PANE_MAX_WIDTH)),
            pane_drag: None,
        }
    }
}

/// How wide a PvP setup side pane is allowed to get by dragging its
/// inner edge. The floor keeps the save view's tab strip legible; the
/// ceiling keeps the emulator from being squeezed off a modest window
/// with both drawers out. The resting width is the user's — persisted
/// as `config.pvp_setup_pane_widths` and carried on [`PvpPanes`].
pub(crate) const SETUP_PANE_MIN_WIDTH: f32 = 300.0;
pub(crate) const SETUP_PANE_MAX_WIDTH: f32 = 720.0;

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
                panes.pane_drag = Some(PaneDrag {
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
                        (drag.start_width + delta).clamp(SETUP_PANE_MIN_WIDTH, SETUP_PANE_MAX_WIDTH);
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

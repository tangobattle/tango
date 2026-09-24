//! Training controls: the opponent-view and side-swap messages the
//! training view emits.

use super::Effect;
use crate::session::training::TrainingSession;
use crate::session::State;

/// Training-view messages. Wrapped in [`SessionMessage::Training`] on the
/// way out; inert unless a training session is active.
#[derive(Debug, Clone)]
pub enum Message {
    /// Select how the opponent's screen is presented, and persist the
    /// choice.
    SetOpponentView(crate::config::OpponentView),
    /// Swap which side (core) the player controls.
    ToggleSwap,
    /// Pin the floating bar while the opponent-view dropdown is open.
    BarMenuToggled(bool),
}

/// Apply a training-view message.
pub(crate) fn update(state: &mut State, msg: Message) -> Option<Effect> {
    match msg {
        Message::SetOpponentView(view) => {
            if let Some(s) = state.active_as::<TrainingSession>() {
                s.set_opponent_visible(view != crate::config::OpponentView::Off);
            }
            return Some(Effect::PersistOpponentView(view));
        }
        Message::ToggleSwap => {
            if let Some(s) = state.active_as::<TrainingSession>() {
                s.toggle_swap();
            }
        }
        Message::BarMenuToggled(open) => state.bar_menu_open = open,
    }
    None
}

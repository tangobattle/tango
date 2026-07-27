//! App-side handles for the embedded save views. Everything here is
//! deliberately opaque: the app renders a save view through
//! `data.ui.view(..)` and folds its messages through `data.ui.update(..)`
//! (see `tango_gamesupport::SaveUi`), and the only save-view knowledge
//! it keeps is this file's constructors and the [`Outcome`]s an update
//! hands back.

/// What the app must act on after folding a save-view message.
pub use tango_gamesupport::SaveUiOutcome as Outcome;

/// An opaque message minted inside an embedded save view, en route
/// back to that view's `update`.
pub type Msg = std::sync::Arc<dyn tango_gamesupport::SaveUiMessage>;

/// One embedded save view's opaque state (active tab, edit session,
/// scroll + animation bookkeeping). Game-independent — it outlives
/// game and save switches, which is why the play tab holds one
/// persistently instead of rebuilding it per selection.
pub struct State(Box<dyn tango_gamesupport::SaveViewState>);

impl State {
    pub fn new() -> Self {
        Self(tango_gamesupport_common::save_ui::new_save_view_state())
    }

    /// Leave any in-progress edit session. The App calls this when the
    /// loaded save is rebuilt out from under the view, where staged
    /// edits (which lived in the previous in-memory save) are already
    /// gone.
    pub fn clear_editing(&mut self) {
        tango_gamesupport_common::save_ui::clear_editing(&mut *self.0);
    }

    /// Play the save-switch entrance: just the panes under the save
    /// view's sub-tab strip rise, leaving the strip planted.
    pub fn animate_save_switch(&mut self, now: iced::time::Instant) {
        tango_gamesupport_common::save_ui::animate_save_switch(&mut *self.0, now);
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for State {
    type Target = dyn tango_gamesupport::SaveViewState;
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl std::ops::DerefMut for State {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.0
    }
}

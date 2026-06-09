//! BN2 implementation of the generic [`crate::custom_screen`] deliberation timer.
//!
//! BN2's chip-select close is the simplest of the series: writing the closing
//! sub-state alone runs the game's teardown, so [`pin_confirm`] is the whole
//! mechanism and there's no confirm button to inject. See
//! [`super::munger::Munger::force_close_custom_screen`].

use crate::custom_screen::CustomScreenHooks;

use super::munger::Munger;

/// `custom_screen_scene` value meaning the chip-select screen is up. Held for
/// the entire phase including teardown, so the timer keeps counting even if a
/// player camps in a sub-dialog.
const SCENE_CUSTOM: u8 = 2;

/// `custom_screen_substate` value the teardown writes (and that we write to
/// force it); past this we stop pinning and let the close animate.
const SUBSTATE_CLOSING: u8 = 8;

impl CustomScreenHooks for Munger {
    fn in_custom_screen(&self, core: mgba::core::CoreMutRef) -> bool {
        self.custom_screen_scene(core) == SCENE_CUSTOM
    }

    fn close_started(&self, core: mgba::core::CoreMutRef) -> bool {
        self.custom_screen_substate(core) == SUBSTATE_CLOSING
    }

    fn pin_confirm(&self, core: mgba::core::CoreMutRef) {
        self.force_close_custom_screen(core);
    }

    fn confirm_joyflags(&self) -> u16 {
        // BN2 closes purely on the written sub-state; no button to inject.
        0
    }
}

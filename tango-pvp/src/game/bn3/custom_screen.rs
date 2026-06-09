//! BN3 implementation of the generic [`crate::custom_screen`] deliberation timer.
//!
//! BN3 differs from BN5/BN6 in two ways: the in-custom scene value is 8 (not 4),
//! and the confirm button is START (which ignores the grid cursor) rather than A
//! over OK. The pin still forces the sub-state to selecting so the injected
//! Start reaches the chip-select handler. See
//! [`super::munger::Munger::force_close_custom_screen`].

use crate::custom_screen::CustomScreenHooks;

use super::munger::Munger;

/// `custom_screen_scene` value meaning the chip-select screen is up. Held for
/// the entire phase including teardown, so the timer keeps counting even if a
/// player camps in a sub-dialog.
const SCENE_CUSTOM: u8 = 8;

/// `custom_screen_substate` value once the teardown has begun; past this we stop
/// pinning and let the close animate.
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
        // BN3 confirms chip selection with START (cursor-independent).
        0x0008
    }
}

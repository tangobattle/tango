//! BN3 implementation of the generic [`crate::custom_screen`] deliberation timer.
//!
//! BN3 differs from BN5/BN6: the in-custom scene value is 8 (not 4), and the
//! confirm is reached not by an A-over-OK cursor but through the chip-select
//! menu's "OK confirmed" state. Rather than inject the natural Start press
//! (whose effect on the menu animation is awkward to hold), we drive the menu's
//! confirm directly as a pure state write — see
//! [`super::munger::Munger::force_close_custom_screen`] — so no button is
//! injected (`confirm_joyflags` is 0).

use crate::custom_screen::CustomScreenHooks;

use super::munger::Munger;

/// `custom_screen_scene` value meaning the chip-select screen is up. Held for
/// the entire phase including teardown, so the timer keeps counting even if a
/// player camps in a sub-dialog.
const SCENE_CUSTOM: u8 = 8;

/// `custom_screen_substate` value once the teardown has begun (the stable late
/// state of the close).
const SUBSTATE_CLOSING: u8 = 8;

/// `custom_menu_confirm` (menu+1) value ≥ this means OK has been confirmed (12,
/// then animating down through 8). We treat that as "close started" so the
/// timer's latch stops re-writing the confirm the very next tick — otherwise
/// holding `menu+1 = 12` would clobber the 12→8 commit animation and stall.
const MENU_CONFIRMED: u8 = 8;

impl CustomScreenHooks for Munger {
    fn in_custom_screen(&self, core: mgba::core::CoreMutRef) -> bool {
        self.custom_screen_scene(core) == SCENE_CUSTOM
    }

    fn close_started(&self, core: mgba::core::CoreMutRef) -> bool {
        // Either the menu's confirm has fired (prompt: stops the one-shot write)
        // or the sub-state has reached the late teardown value.
        self.custom_menu_confirm(core) >= MENU_CONFIRMED || self.custom_screen_substate(core) == SUBSTATE_CLOSING
    }

    fn pin_confirm(&self, core: mgba::core::CoreMutRef) {
        self.force_close_custom_screen(core);
    }

    fn confirm_joyflags(&self) -> u16 {
        // BN3's close is driven entirely by the state write; no button needed.
        0
    }

    fn probe(&self, core: mgba::core::CoreMutRef) -> u32 {
        // byte0 = scene (0x02006ca1, ==8 in custom), byte1 = substate
        // (0x02006ca2), byte2 = menu confirm (0x0200f7f1).
        (self.custom_screen_scene(core) as u32)
            | ((self.custom_screen_substate(core) as u32) << 8)
            | ((self.custom_menu_confirm(core) as u32) << 16)
    }
}

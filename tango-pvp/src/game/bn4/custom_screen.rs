//! BN4 implementation of the generic [`crate::custom_screen`] deliberation timer.
//!
//! BN4 shares BN6's sub-scene shape (`+0` phase = 4, `+1` close = 8) but reaches
//! the confirm differently: rather than placing a grid cursor on OK, it opens
//! the OK sub-menu (`+2 := 8`, the state Start produces) and confirms with A.
//! See [`super::munger::Munger::force_close_custom_screen`].

use crate::custom_screen::CustomScreenHooks;

use super::munger::Munger;

/// `battle_subscene` value meaning the chip-select screen is up. Held for the
/// entire phase including teardown, so the timer keeps counting even if a player
/// camps in a sub-dialog.
const SUBSCENE_CUSTOM: u8 = 4;

/// Custom-screen sub-state the teardown writes once the close has begun
/// (`battle_subscene+1`); past this we stop pinning and let it animate.
const SUBPHASE_CLOSING: u8 = 8;

impl CustomScreenHooks for Munger {
    fn in_custom_screen(&self, core: mgba::core::CoreMutRef) -> bool {
        self.battle_subscene(core) == SUBSCENE_CUSTOM
    }

    fn close_started(&self, core: mgba::core::CoreMutRef) -> bool {
        self.custom_subphase(core) == SUBPHASE_CLOSING
    }

    fn pin_confirm(&self, core: mgba::core::CoreMutRef) {
        self.force_close_custom_screen(core);
    }

    fn confirm_joyflags(&self) -> u16 {
        // BN4 confirms from the opened OK sub-menu with A.
        0x0001
    }
}

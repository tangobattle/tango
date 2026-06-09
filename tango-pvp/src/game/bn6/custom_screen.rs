//! BN6 implementation of the generic [`crate::custom_screen`] deliberation timer.
//!
//! All the timing/latch logic lives in the root module; this just maps its
//! [`CustomScreen`](crate::custom_screen::CustomScreen) trait onto BN6's battle
//! RAM. The cursor/sub-state writes that drive the real teardown — and the full
//! watchpoint-RE writeup of *why* it has to be done this way — are in
//! [`super::munger::Munger::force_close_custom_screen`].

use crate::custom_screen::CustomScreenHooks;

use super::munger::Munger;

/// `battle_subscene` value meaning the custom (chip-select) screen is up. It
/// stays this for the *entire* chip-select phase, including every sub-state, so
/// the timer keeps counting even if a player camps in a sub-dialog.
const SUBSCENE_CUSTOM: u8 = 4;

/// Custom-screen sub-state value the teardown routine writes once the close has
/// begun (`battle_subscene+1`); past this we stop pinning and let it animate.
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
        // BN6 confirms chip selection with A; the pinned cursor sits on OK.
        0x0001
    }
}

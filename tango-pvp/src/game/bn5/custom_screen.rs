//! BN5 implementation of the generic [`crate::custom_screen`] deliberation timer.
//!
//! BN5's chip-select screen is structurally identical to BN6 — the same
//! sub-scene struct (`+0` phase, `+1` close sub-state, `+7` cursor) with the
//! same magic values, just relocated. The timing/latch logic lives in the root
//! module; this maps its trait onto BN5's RAM. See
//! [`super::munger::Munger::force_close_custom_screen`] for the close RE.

use crate::custom_screen::CustomScreenHooks;

use super::munger::Munger;

/// `battle_subscene` value meaning the custom (chip-select) screen is up. Stays
/// this for the entire phase, including every sub-state, so the timer keeps
/// counting even if a player camps in a sub-dialog.
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
        // BN5 confirms chip selection with A; the pinned cursor sits on OK.
        0x0001
    }
}

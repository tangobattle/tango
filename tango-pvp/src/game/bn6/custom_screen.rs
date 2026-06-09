//! PROTOTYPE: a hard deliberation cap on the custom (chip-select) screen.
//!
//! Once the screen has been up longer than [`TICK_LIMIT`], we drive the game's
//! *own* close (the only path that cleanly commits chips + inits combat). Per
//! the watchpoint RE in [`super::munger::Munger::force_close_custom_screen`], the
//! close is irreducibly coupled to the input handler, so we run it the real way:
//! each tick we pin the custom-screen sub-state back to "selecting" (popping out
//! of any sub-dialog) and the cursor onto the OK button, and OR the A button into
//! the local input. The game's selecting handler then runs its genuine teardown.
//! We stop pinning the instant the teardown starts so its animation can finish.
//!
//! Determinism: the cursor/sub-state writes are a pure function of synced game
//! state (`battle_subscene` + Tango's battle tick), applied on every core type
//! (primary, shadow, stepper) for the player that core simulates. The A confirm
//! rides the normal synced joyflags channel (the primary ORs it into the
//! recorded local input; shadow/stepper read it back from the input pairs). So
//! all simulations reach the same close on the same tick.
//!
//! Caveats to validate live (two clients): the `closing` latch isn't snapshotted,
//! so a rollback re-simming across the close could mistime it (fine for the
//! common opening-screen case with shallow rollback); the OK cursor index (10)
//! and `SUBPHASE_CLOSING` were RE'd on one BR6E layout; and forcing out of a
//! sub-dialog skips that dialog's own cleanup — confirm it doesn't corrupt.

use std::cell::Cell;
use std::sync::atomic::{AtomicI32, Ordering};

use super::munger::Munger;

/// Chip-select deliberation cap, in battle ticks (60/s). 600 ≈ 10s. A real
/// build would thread this from the agreed match settings rather than a const.
pub(super) const TICK_LIMIT: u32 = 600;

/// Tango joyflags bit for the A button (the chip-select confirm), OR'd into the
/// local input while the timer is enforcing.
pub(super) const CONFIRM_JOYFLAG: u16 = 0x0001;

/// PROTOTYPE debug channel for the on-screen countdown: battle ticks left in the
/// custom screen, or -1 when not in it. Written by the *primary* core only (the
/// live local view) and read by the GUI overlay. A real build would surface this
/// through the session/telemetry path instead of a process global.
pub static DEBUG_REMAINING: AtomicI32 = AtomicI32::new(-1);

/// Status returned by [`CustomScreenTimer::enforce`] so the live primary can
/// publish a countdown to the GUI.
pub(super) enum Status {
    NotInCustom,
    InCustom { remaining: u32, expired: bool },
}

/// `battle_subscene` value meaning "the custom (chip-select) screen is up". It
/// stays this for the *entire* chip-select phase, including every sub-state, so
/// the timer keeps counting even if a player camps in a sub-dialog.
const SUBSCENE_CUSTOM: u8 = 4;

/// Per-core scratch tracking when the current custom screen opened.
///
/// `open_tick` is `u32::MAX` while not in the custom screen, and is latched to
/// Tango's battle tick on the rising edge of `battle_subscene == 4`. Every core
/// walks the identical synced subscene timeline, so each independently derives
/// the same open tick — no shared/game-side anchor needed. (Replay *seeking*
/// straight into a mid-custom snapshot would skip the edge; hardening that means
/// persisting `open_tick` in the stepper checkpoint. Live play always crosses
/// the edge forward, so it's exact there.)
/// `0x364c1` value the teardown routine writes once the close has begun — past
/// this point we must stop pinning the sub-state and let the close animation run.
const SUBPHASE_CLOSING: u8 = 8;

pub(super) struct CustomScreenTimer {
    open_tick: Cell<u32>,
    /// Set once the close has started (sub-state reached `SUBPHASE_CLOSING`), so
    /// the per-tick pinning stops and the game's close animation can finish.
    closing: Cell<bool>,
}

impl CustomScreenTimer {
    pub(super) fn new() -> Self {
        Self {
            open_tick: Cell::new(u32::MAX),
            closing: Cell::new(false),
        }
    }

    /// Run once per tick from a core's `main_read_joyflags` hook. While the
    /// chip-select screen is over `limit`, pins the cursor + sub-state every tick
    /// (forcing out of any sub-dialog onto the OK button) so the game's own
    /// selecting handler runs the confirm. Returns whether the caller should also
    /// inject the confirm (A) input this tick.
    pub(super) fn enforce(&self, munger: &Munger, core: mgba::core::CoreMutRef, limit: u32, player_index: u8) -> Status {
        let subscene = munger.battle_subscene(core);
        let tick = munger.current_tick(core);
        let expired = self.should_force_close(subscene, tick, limit);
        if subscene != SUBSCENE_CUSTOM {
            self.closing.set(false);
            return Status::NotInCustom;
        }
        // Once the game's teardown has begun, stop pinning and let it animate to
        // combat — otherwise re-pinning the sub-state stalls the close.
        if munger.custom_subphase(core) == SUBPHASE_CLOSING {
            self.closing.set(true);
        }
        let confirm = expired && !self.closing.get();
        if confirm {
            munger.force_close_custom_screen(core, player_index);
        }
        Status::InCustom {
            remaining: limit.saturating_sub(tick.saturating_sub(self.open_tick.get())),
            expired: confirm,
        }
    }

    /// Primary-side wrapper: runs [`enforce`], publishes the GUI countdown, and
    /// reports whether to inject the confirm input.
    pub(super) fn enforce_and_report(
        &self,
        munger: &Munger,
        core: mgba::core::CoreMutRef,
        limit: u32,
        player_index: u8,
    ) -> bool {
        let (remaining, expired) = match self.enforce(munger, core, limit, player_index) {
            Status::NotInCustom => (-1, false),
            Status::InCustom { remaining, expired } => (remaining as i32, expired),
        };
        DEBUG_REMAINING.store(remaining, Ordering::Relaxed);
        expired
    }

    /// Pure decision split out so it can be tested against captured battle
    /// timelines without a core. Also advances/resets the `open_tick` anchor.
    fn should_force_close(&self, subscene: u8, tick: u32, limit: u32) -> bool {
        if subscene != SUBSCENE_CUSTOM {
            self.open_tick.set(u32::MAX);
            return false;
        }
        let open = if self.open_tick.get() == u32::MAX {
            self.open_tick.set(tick);
            tick
        } else {
            self.open_tick.get()
        };
        tick.saturating_sub(open) >= limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real per-tick `battle_subscene` timeline (ticks 0..=800) captured from a
    /// BR5E golden replay: 0 (intro) for 0..205, 4 (custom screen) for
    /// 205..539, 8 (combat) thereafter. Exercises the timer against genuine
    /// game state rather than a synthetic curve.
    const SUBSCENE_TIMELINE: &str = include_str!("testdata_subscene_timeline.txt");

    fn timeline() -> Vec<u8> {
        SUBSCENE_TIMELINE.trim().split(',').map(|s| s.parse().unwrap()).collect()
    }

    #[test]
    fn timeline_has_one_custom_window() {
        let t = timeline();
        assert_eq!(t.len(), 801);
        assert_eq!((t[204], t[205], t[538], t[539]), (0, SUBSCENE_CUSTOM, SUBSCENE_CUSTOM, 8));
    }

    /// Replaying the real subscene timeline with a short limit, the close should
    /// fire on exactly the ticks inside the custom window past the budget — and
    /// nowhere else.
    #[test]
    fn force_closes_only_inside_custom_after_limit() {
        const LIMIT: u32 = 120;
        const OPEN: u32 = 205; // subscene -> 4 in the captured data
        let timer = CustomScreenTimer::new();
        let mut first = None;
        for (tick, &subscene) in timeline().iter().enumerate() {
            let tick = tick as u32;
            let close = timer.should_force_close(subscene, tick, LIMIT);
            let in_custom = subscene == SUBSCENE_CUSTOM;
            let expected = in_custom && tick - OPEN >= LIMIT;
            assert_eq!(close, expected, "tick {tick}");
            if close && first.is_none() {
                first = Some(tick);
            }
            // Anchor releases the instant we leave the custom screen.
            if !in_custom {
                assert_eq!(timer.open_tick.get(), u32::MAX, "anchor reset @tick {tick}");
            }
        }
        assert_eq!(first, Some(OPEN + LIMIT), "fires exactly when the budget runs out");
    }
}

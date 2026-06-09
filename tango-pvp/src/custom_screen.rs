//! PROTOTYPE: a generic hard deliberation cap on the chip-select ("custom")
//! screen, shared by every game.
//!
//! Once the screen has been up longer than the configured limit, we drive the
//! game's *own* close (the only path that cleanly commits chips + inits combat).
//! On the games where this has been reverse-engineered, the close is coupled to
//! the input handler, so we run it the real way: each tick we pin the screen
//! onto the confirm path ([`CustomScreenHooks::pin_confirm`] — e.g. cursor on
//! OK, sub-state back to "selecting", which also pops out of any sub-dialog) and
//! OR the game's confirm button ([`CustomScreenHooks::confirm_joyflags`]) into
//! the local input. The game's own handler then runs its genuine teardown. We
//! stop pinning the instant the teardown starts
//! ([`CustomScreenHooks::close_started`]) so its animation can finish.
//!
//! What "pin" and "confirm" mean is per-game and lives behind the trait: BN5/BN6
//! pin a grid cursor onto OK and confirm with A; BN4 opens the OK sub-menu and
//! confirms with A; BN2 and BN3 need no button at all — writing their close /
//! menu-confirm sub-state runs the teardown standalone.
//!
//! Determinism: the pin writes are a pure function of synced game state (the
//! game's "in custom screen" flag + Tango's synced battle tick, passed in by
//! each core), applied on every core type (primary, shadow, stepper) for the
//! player that core simulates. The confirm press rides the normal synced
//! joyflags channel (the primary ORs it into the recorded local input;
//! shadow/stepper read it back from the input pairs). So all simulations reach
//! the same close on the same tick.
//!
//! Ownership: the live primary's per-round state ([`Round`](crate::battle::Round))
//! owns a [`CustomScreenTimer`] directly (plain mutable state, no interior
//! mutability). The shadow and stepper recreate/clone their per-round state, so
//! there the timer lives in the `main_read_joyflags` trap closure behind a
//! `RefCell`. Enablement + limit come in through the `Match` and are threaded to
//! each. Per-game specifics live behind the [`CustomScreenHooks`] trait,
//! implemented by a small adapter the game arms its timer with.

/// Per-game operations the timer needs. Each game implements this with a small
/// adapter over its battle-RAM layout. All methods take the live core by value
/// (`CoreMutRef` is `Copy`) and must be pure reads except
/// [`pin_confirm`](CustomScreen::pin_confirm).
pub trait CustomScreenHooks {
    /// True while the chip-select screen is up — including *every* sub-state /
    /// sub-dialog, so the timer keeps counting even if a player camps in one.
    fn in_custom_screen(&self, core: mgba::core::CoreMutRef) -> bool;

    /// True once the game's own close/teardown has begun — past this point the
    /// timer stops pinning so the close animation isn't stalled.
    fn close_started(&self, core: mgba::core::CoreMutRef) -> bool;

    /// Pin the screen onto the confirm path (e.g. cursor on the OK button,
    /// sub-state forced to "selecting"). The primary then injects
    /// [`confirm_joyflags`](Self::confirm_joyflags) and the game's own handler
    /// runs its real teardown. Called every tick while over the limit until the
    /// close starts. May be a no-op for games that close on the button alone.
    fn pin_confirm(&self, core: mgba::core::CoreMutRef);

    /// Joyflags the primary ORs into the local input to drive the confirm while
    /// enforcing (`0` for games that close purely on the pinned state). This is
    /// per-game: A (`0x0001`) for BN4/5/6; BN2 and BN3 close on their pinned
    /// state alone (`0`). Rides the synced input channel so every core sees it.
    fn confirm_joyflags(&self) -> u16;
}

/// Default chip-select deliberation cap, in battle ticks (60/s). 600 ≈ 10s.
/// Used when a match enables the timer without a specific value.
pub const DEFAULT_TICK_LIMIT: u32 = 600;

/// Per-round deliberation timer. Embeds the game's [`CustomScreenHooks`] adapter
/// (boxed, since the per-core round state that owns it is game-agnostic) and a
/// configurable `limit`. Owned mutably by that state — so it's plain mutable
/// fields, no interior mutability.
///
/// `open_tick` is `None` while not in the custom screen, and is latched to the
/// battle tick on the rising edge of entering it. Every core walks the identical
/// synced state, so each independently derives the same open tick — no
/// shared/game-side anchor needed. (Replay *seeking* straight into a mid-custom
/// snapshot would skip the edge; hardening that means persisting `open_tick` in
/// the stepper checkpoint. Live play always crosses the edge forward.)
pub struct CustomScreenTimer {
    game: Box<dyn CustomScreenHooks + Send>,
    limit: u32,
    open_tick: Option<u32>,
    /// Set once the close has started, so the per-tick pinning stops and the
    /// game's close animation can finish.
    closing: bool,
    /// Cached ticks-remaining for [`remaining`](Self::remaining), updated each
    /// [`enforce`](Self::enforce); `None` while not in the custom screen.
    remaining: Option<u32>,
}

impl CustomScreenTimer {
    pub fn new(game: Box<dyn CustomScreenHooks + Send>, limit: u32) -> Self {
        Self {
            game,
            limit,
            open_tick: None,
            closing: false,
            remaining: None,
        }
    }

    /// Run once per tick from a core's `main_read_joyflags` hook, passing that
    /// core's synced Tango tick (`last_loaded_tick+1` on the primary,
    /// `current_tick()` on the stepper/shadow — all identical at a given logical
    /// tick). Pins the screen onto the confirm path while over the limit and
    /// returns the joyflags the primary should OR into the local input this tick
    /// (`0` when not confirming, or for games that close on the pinned state
    /// alone). The stepper/shadow ignore the return — the confirm press is
    /// already in the recorded input stream.
    pub fn enforce(&mut self, core: mgba::core::CoreMutRef, tick: u32) -> u16 {
        let confirm = if self.game.in_custom_screen(core) {
            self.advance(tick, self.game.close_started(core))
        } else {
            self.advance_idle()
        };
        if confirm {
            self.game.pin_confirm(core);
            self.game.confirm_joyflags()
        } else {
            0
        }
    }

    /// Ticks left in the custom screen, or `None` when not in it. For the GUI
    /// countdown; updated by [`enforce`](Self::enforce).
    pub fn remaining(&self) -> Option<u32> {
        self.remaining
    }

    /// Timing core, split from the core reads so it can be unit-tested against a
    /// captured timeline. Returns whether to drive the confirm.
    fn advance(&mut self, tick: u32, close_started: bool) -> bool {
        let open = *self.open_tick.get_or_insert(tick);
        let elapsed = tick.saturating_sub(open);
        if close_started {
            self.closing = true;
        }
        self.remaining = Some(self.limit.saturating_sub(elapsed));
        elapsed >= self.limit && !self.closing
    }

    fn advance_idle(&mut self) -> bool {
        self.open_tick = None;
        self.closing = false;
        self.remaining = None;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stand-in adapter: the timing tests drive [`CustomScreenTimer::advance`]
    /// directly, which never touches the game, so these can be unreachable.
    struct NullGame;
    impl CustomScreenHooks for NullGame {
        fn in_custom_screen(&self, _: mgba::core::CoreMutRef) -> bool {
            unreachable!()
        }
        fn close_started(&self, _: mgba::core::CoreMutRef) -> bool {
            unreachable!()
        }
        fn pin_confirm(&self, _: mgba::core::CoreMutRef) {
            unreachable!()
        }
        fn confirm_joyflags(&self) -> u16 {
            unreachable!()
        }
    }

    /// Real per-tick `battle_subscene` timeline (ticks 0..=800) captured from a
    /// BR5E golden replay: 0 (intro) for 0..205, 4 (custom screen) for 205..539,
    /// 8 (combat) thereafter. Exercises the generic timer against genuine game
    /// state; `subscene == 4` is BN6's "in custom screen".
    const SUBSCENE_TIMELINE: &str = include_str!("game/bn6/testdata_subscene_timeline.txt");
    const BN6_CUSTOM: u8 = 4;

    fn timeline() -> Vec<u8> {
        SUBSCENE_TIMELINE
            .trim()
            .split(',')
            .map(|s| s.parse().unwrap())
            .collect()
    }

    #[test]
    fn timeline_has_one_custom_window() {
        let t = timeline();
        assert_eq!(t.len(), 801);
        assert_eq!((t[204], t[205], t[538], t[539]), (0, BN6_CUSTOM, BN6_CUSTOM, 8));
    }

    /// Replaying the real subscene timeline with a short limit, the confirm
    /// should fire on exactly the ticks inside the custom window past the budget
    /// — and nowhere else — and `remaining` should track the countdown.
    #[test]
    fn fires_only_inside_custom_after_limit() {
        const LIMIT: u32 = 120;
        const OPEN: u32 = 205; // enters custom here in the captured data
        let mut timer = CustomScreenTimer::new(Box::new(NullGame), LIMIT);
        let mut first = None;
        for (tick, &subscene) in timeline().iter().enumerate() {
            let tick = tick as u32;
            let in_custom = subscene == BN6_CUSTOM;
            let confirm = if in_custom {
                timer.advance(tick, false)
            } else {
                timer.advance_idle()
            };
            let expected = in_custom && tick - OPEN >= LIMIT;
            assert_eq!(confirm, expected, "tick {tick}");
            if confirm && first.is_none() {
                first = Some(tick);
            }
            if !in_custom {
                assert_eq!(timer.remaining(), None, "no countdown outside custom @tick {tick}");
            } else {
                assert_eq!(timer.remaining(), Some(LIMIT.saturating_sub(tick - OPEN)));
            }
        }
        assert_eq!(first, Some(OPEN + LIMIT), "fires exactly when the budget runs out");
    }
}

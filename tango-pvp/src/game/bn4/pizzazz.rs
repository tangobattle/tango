//! BN4 "pizzazz mode" gating.
//!
//! Six traps in the battle effects path each gate on `match_type.1 == 1`
//! (the bn4 link battle subtype) and OR a fixed bit into a register. The
//! check + body is identical for primary/shadow/stepper; only the state
//! source differs. Build all six in one helper per mode.

use crate::hooks::{match_trap, shadow_trap, stepper_trap, MatchHandle, Trap};

/// `(rom address, gpr index)` pairs for the six pizzazz mov sites.
fn entries(hooks: &super::Hooks) -> [(u32, usize); 6] {
    let r = &hooks.offsets.rom;
    [
        (r.battle_pizzazz_init_mov, 0),
        (r.battle_pizzazz_bg_mov, 1),
        (r.battle_pizzazz_self_mov, 1),
        (r.battle_pizzazz_opponent_mov, 0),
        (r.battle_pizzazz_silhouette_mov, 0),
        (r.battle_pizzazz_final_mov, 1),
    ]
}

/// OR the pizzazz bit into the specified GPR. The trap body is the same in
/// every mode; this is the inner action.
fn apply(mut core: mgba::core::CoreMutRef, gpr: usize) {
    let v = core.as_ref().gba().cpu().gpr(gpr) | 0x20;
    core.gba_mut().cpu_mut().set_gpr(gpr, v);
}

pub(super) fn primary_traps(hooks: &super::Hooks, match_: &MatchHandle) -> Vec<Trap> {
    entries(hooks)
        .into_iter()
        .map(|(addr, gpr)| {
            match_trap(addr, match_, move |match_, core| {
                if match_.match_type().1 != 1 {
                    return;
                }
                apply(core, gpr);
            })
        })
        .collect()
}

pub(super) fn shadow_traps(hooks: &super::Hooks, shadow_state: &crate::shadow::State) -> Vec<Trap> {
    entries(hooks)
        .into_iter()
        .map(|(addr, gpr)| {
            shadow_trap(addr, shadow_state, move |shadow_state, core| {
                if shadow_state.match_type().1 != 1 {
                    return;
                }
                apply(core, gpr);
            })
        })
        .collect()
}

pub(super) fn stepper_traps(hooks: &super::Hooks, stepper_state: &crate::stepper::State) -> Vec<Trap> {
    entries(hooks)
        .into_iter()
        .map(|(addr, gpr)| {
            stepper_trap(addr, stepper_state, move |state, core| {
                if state.match_type().1 != 1 {
                    return;
                }
                apply(core, gpr);
            })
        })
        .collect()
}

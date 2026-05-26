//! Display-core traps for the presentation-buffer model.
//!
//! The display core renders the live core's published `present_state` frames
//! `frame_delay` behind the network frontier. The actual state load happens in
//! the PvP session's display frame_callback (it owns the core each frame and
//! the published buffer); these traps only have to keep the core able to *run*
//! a battle frame from a loaded state without blocking on the link cable.
//!
//! bn3's in-battle comm goes through the `send_and_receive` calls plus a SIO
//! status check that otherwise drives the "Communication Error" UI, so the
//! neutering mirrors the stepper's SIO bypass (skip the calls, report linking
//! success + no error, stage the canned init rx) but drops all the
//! `Match`/round/netcode logic — the loaded state already carries the per-tick
//! state we want to render.

use crate::battle::DisplayHandle;
use crate::hooks::Trap;

use super::INIT_RX;

pub(super) fn traps(hooks: &super::Hooks, handle: DisplayHandle) -> Vec<Trap> {
    let make_send_and_receive_call_hook = || {
        Box::new(move |mut core: mgba::core::CoreMutRef| {
            let pc = core.as_ref().gba().cpu().thumb_pc();
            core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
            core.gba_mut().cpu_mut().set_gpr(0, 3);
        })
    };

    vec![
        (
            hooks.offsets.rom.handle_input_init_send_and_receive_call,
            make_send_and_receive_call_hook(),
        ),
        (
            hooks.offsets.rom.handle_input_update_send_and_receive_call,
            make_send_and_receive_call_hook(),
        ),
        (
            hooks.offsets.rom.handle_input_deinit_send_and_receive_call,
            make_send_and_receive_call_hook(),
        ),
        {
            let munger = hooks.munger();
            (
                hooks.offsets.rom.comm_menu_send_and_receive_call,
                Box::new(move |mut core: mgba::core::CoreMutRef| {
                    let pc = core.as_ref().gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                    core.gba_mut().cpu_mut().set_gpr(0, 3);
                    munger.set_rx_packet(core, 0, &INIT_RX);
                    munger.set_rx_packet(core, 1, &INIT_RX);
                }),
            )
        },
        (
            hooks.offsets.rom.process_battle_input_ret,
            Box::new(move |mut core: mgba::core::CoreMutRef| {
                core.gba_mut().cpu_mut().set_gpr(0, 0);
            }),
        ),
        (
            hooks.offsets.rom.init_sio_call,
            Box::new(|mut core: mgba::core::CoreMutRef| {
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
            }),
        ),
        (
            hooks.offsets.rom.sio_teardown_clear_entry,
            Box::new(|mut core: mgba::core::CoreMutRef| {
                // Skip the 3-instruction clear block plus the trailing
                // SIO-register-cleanup BL, landing on the function's pop {pc}.
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 0xc);
            }),
        ),
        (
            hooks.offsets.rom.comm_status_check_entry,
            Box::new(|mut core: mgba::core::CoreMutRef| {
                // Force return value 0 (no error) and PC-redirect to the
                // function's epilogue, defeating the state-1 → state-3
                // transition that displays the "Communication Error" UI.
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_gpr(0, 0);
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 0x22);
            }),
        ),
        (hooks.offsets.rom.main_read_joyflags, {
            let buffer = handle.clone();
            Box::new(move |mut core: mgba::core::CoreMutRef| {
                let _ = buffer.advance_blocking(|state| {
                    core.load_state(state).expect("load present state");
                });
            })
        }),
    ]
}

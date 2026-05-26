//! Display-core traps for the presentation-buffer model.
//!
//! The display core renders the live core's published `present_state` frames
//! `frame_delay` behind the network frontier. The actual state load happens in
//! the PvP session's display frame_callback (it owns the core each frame and
//! the published buffer); these traps only have to keep the core able to *run*
//! a battle frame from a loaded state without blocking on the link cable —
//! exactly the in-battle neutering the primary core relies on, minus all the
//! `Match`/netcode logic.
//!
//! No comm-menu / RNG / `main_read_joyflags` traps: the core may sit at the
//! (hidden) comm screen until the first frame is published, at which point the
//! callback load drops it straight into the battle loop.

use crate::battle::DisplayHandle;
use crate::hooks::Trap;

pub(super) fn traps(hooks: &super::Hooks, handle: DisplayHandle) -> Vec<Trap> {
    vec![
        (
            hooks
                .offsets
                .rom
                .comm_menu_in_battle_call_comm_menu_handle_link_cable_input,
            {
                let munger = hooks.munger();
                Box::new(move |mut core| {
                    let pc = core.as_ref().gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 6);
                    munger.set_copy_data_input_state(core, 2);
                })
            },
        ),
        (hooks.offsets.rom.main_read_joyflags, {
            let buffer = handle.clone();
            Box::new(move |mut core| {
                let _ = buffer.advance_blocking(|state| {
                    core.load_state(state).expect("load present state");
                });
            })
        }),
    ]
}

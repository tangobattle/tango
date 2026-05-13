use crate::hooks::Trap;

pub(super) fn traps(hooks: &super::Hooks) -> Vec<Trap> {
    vec![
        (hooks.offsets.rom.start_screen_jump_table_entry, {
            let munger = hooks.munger();
            Box::new(move |core| {
                munger.skip_logo(core);
            })
        }),
        (hooks.offsets.rom.intro_jump_table_entry, {
            let munger = hooks.munger();
            Box::new(move |core| {
                munger.skip_intro(core);
            })
        }),
        (hooks.offsets.rom.start_screen_sram_unmask_ret, {
            let munger = hooks.munger();
            Box::new(move |core| {
                munger.continue_from_title_menu(core);
            })
        }),
        (hooks.offsets.rom.game_load_ret, {
            let munger = hooks.munger();
            Box::new(move |core| {
                munger.open_comm_menu_from_overworld(core);
            })
        }),
        (
            hooks.offsets.rom.comm_menu_handle_link_cable_input,
            Box::new(move |mut core| {
                //Skip call
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                //return r0 = 0, r1 = 0
                core.gba_mut().cpu_mut().set_gpr(0, 0);
                core.gba_mut().cpu_mut().set_gpr(1, 0);
            }),
        ),
        (hooks.offsets.rom.copy_input_data_r0_tst, {
            Box::new(move |mut core: mgba::core::CoreMutRef| {
                // Skip giant section of code. This section checks if r0 == 8 and a bunch of input state checks that aren't present in BN4.
                // It's probably fine! Maybe!
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 0x1C);
            })
        }),
    ]
}

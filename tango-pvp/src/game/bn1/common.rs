use crate::hooks::Trap;

pub(super) fn traps(hooks: &super::Hooks) -> Vec<Trap> {
    vec![
        (hooks.offsets.rom.start_screen_jump_table_entry, {
            let munger = hooks.munger();
            Box::new(move |core| {
                munger.skip_logo(core);
            })
        }),
        (
            hooks.offsets.rom.start_screen_play_music_call,
            Box::new(move |mut core| {
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
            }),
        ),
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
    ]
}

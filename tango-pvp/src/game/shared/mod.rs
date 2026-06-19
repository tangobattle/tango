//! Code shared verbatim by a subset of the game modules. Only patterns that
//! are byte-for-byte identical across the games that use them live here; the
//! games remain otherwise self-contained.

pub(crate) mod rng;

/// The three start-up traps shared verbatim by BN3, BN5 and BN6: skip the boot
/// logo, continue past the title menu, then jump from the overworld into the
/// comm menu. Each of those games' `common::traps` is just an invocation of
/// this macro — a macro rather than a function because the body resolves
/// `Hooks`/`Munger`/offset fields against the invoking game's own types.
macro_rules! start_screen_common_traps {
    ($hooks:expr) => {{
        let hooks = $hooks;
        vec![
            (hooks.offsets.rom.start_screen_jump_table_entry, {
                let munger = hooks.munger();
                Box::new(move |core| {
                    munger.skip_logo(core);
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
        ]
    }};
}

pub(crate) use start_screen_common_traps;

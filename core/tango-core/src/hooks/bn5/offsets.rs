#[derive(Clone, Copy)]
pub(super) struct EWRAMOffsets {
    // Outgoing packet.
    pub(super) tx_packet: u32,

    // Incoming packet.
    pub(super) rx_packet_arr: u32,

    /// Location of the battle state struct in memory.
    pub(super) battle_state: u32,

    /// Start screen jump table control.
    pub(super) start_screen_control: u32,

    /// Title menu jump table control.
    pub(super) title_menu_control: u32,

    /// START menu jump table control.
    pub(super) menu_control: u32,

    /// START menu submenu (e.g. comm menu) jump table control.
    pub(super) submenu_control: u32,

    /// Local RNG state. Doesn't need to be synced.
    pub(super) rng1_state: u32,

    /// Shared RNG state. Must be synced.
    pub(super) rng2_state: u32,

    pub(super) rng3_state: u32,

    /// The state of copying input data, usually returned by get_copy_data_input_state_ret.
    pub(super) copy_data_input_state: u32,
}

#[derive(Clone, Copy)]
pub(super) struct ROMOffsets {
    /// This is the entry point for the start screen, i.e. when the CAPCOM logo is displayed.
    ///
    /// It is expected that at this point, you may write to the start_screen_control EWRAM address to skip to the title screen.
    pub(super) start_screen_jump_table_entry: u32,

    /// This is immediately after SRAM is copied to EWRAM and unmasked.
    ///
    /// At this point, it is safe to do the equivalent of selecting the CONTINUE on the START menu.
    pub(super) start_screen_sram_unmask_ret: u32,

    /// This is immediately after game initialization is complete: that is, the internal state is set correctly.
    ///
    /// At this point, it is safe to jump into the link battle menu.
    pub(super) game_load_ret: u32,

    /// This is directly after where KEYINPUT is read into r4 and then processed.
    ///
    /// Input is injected here directly by Tango into r4 from client. We avoid doing it via the usual input interrupt handling mechanism because this is more precise.
    pub(super) main_read_joyflags: u32,

    /// This hooks the return from the function that is called to determine the current state of copying input data.
    ///
    /// Expected values are: 2 if input is ready, 4 if remote has disconnected.

    /// This hooks the entry into the function that will copy received input data from rx_packet_arr into game state, as well as copies the next game state into tx_packet.
    ///
    /// Received packets should be injected here into rx_packet_arr.
    pub(super) copy_input_data_entry: u32,

    /// This hooks the exit into the function that will copy received input data from rx_packet_arr into game state, as well as copies the next game state into tx_packet.
    ///
    /// Packets to transmit should be injected here into tx_packet.
    pub(super) copy_input_data_ret: u32,

    pub(super) round_end_set_win: u32,
    pub(super) round_end_set_loss: u32,
    pub(super) round_end_damage_judge_set_win: u32,
    pub(super) round_end_damage_judge_set_loss: u32,
    pub(super) round_end_damage_judge_set_draw: u32,

    /// This hooks the point after the battle start routine is complete.
    ///
    /// Tango initializes its own battle tracking state at this point.
    pub(super) round_start_ret: u32,

    /// This hooks the point when the round is ending and the game will process no further input.
    ///
    /// At this point, Tango will clean up its round state and commit the replay.
    pub(super) round_set_ending: u32,

    /// This hooks the point after the battle end routine is complete.
    pub(super) round_end_entry: u32,

    /// This hooks the point where the internal round timer is incremented.
    pub(super) round_post_increment_tick: u32,

    /// This hooks the point determining if the player is player 2 or not.
    ///
    /// r0 should be set to the local player index.
    pub(super) battle_is_p2_tst: u32,

    /// This hooks another point determining if the player is player 2 or not.
    ///
    /// r0 should be set to the local player index.
    pub(super) link_is_p2_ret: u32,

    /// This is the entry point to the comm menu.
    ///
    /// Here, Tango jumps directly into link battle.
    pub(super) comm_menu_init_ret: u32,

    /// This is the entry point to link battle in the comm menu: that is, the first match has started.
    ///
    /// We need to perform some initialization we skipped here, such as setting stage and background.
    pub(super) comm_menu_init_battle_entry: u32,

    /// This handles underlying link cable SIO in the comm menu.
    ///
    /// This should never be called.
    pub(super) handle_sio_entry: u32,

    /// This handles in-battle link cable SIO in the comm menu.
    ///
    /// This should be skipped.
    pub(super) in_battle_call_handle_link_cable_input: u32,

    /// This hooks the entrypoint to the function that is called when a match ends.
    ///
    /// Tango ends its match here.
    pub(super) comm_menu_end_battle_entry: u32,
}

#[rustfmt::skip]
static EWRAM_OFFSETS: EWRAMOffsets = EWRAMOffsets {
    tx_packet:              0x02036df0,
    rx_packet_arr:          0x0203a090,
    battle_state:           0x02034a90,
    start_screen_control:   0x02013000,
    title_menu_control:     0x0200b980,
    menu_control:           0x0200e950,
    submenu_control:        0x0200ab20,
    rng1_state:             0x02001c94,
    rng2_state:             0x02001d40,
    rng3_state:             0x0200269c,
    copy_data_input_state:  0x0203f245,
};

#[derive(Clone, Copy)]
pub struct Offsets {
    pub(super) rom: ROMOffsets,
    pub(super) ewram: EWRAMOffsets,
}

#[rustfmt::skip]
pub static MEGAMAN5_TP_BRBE_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_jump_table_entry:          0x0803c492,
        start_screen_sram_unmask_ret:           0x0803008a,
        game_load_ret:                          0x08004a74,
        main_read_joyflags:                     0x080003ea,
        copy_input_data_entry:                  0x0801c800,
        copy_input_data_ret:                    0x0801c8c8,
        round_end_set_win:                      0x08007474,
        round_end_set_loss:                     0x08007488,
        round_end_damage_judge_set_win:         0x080076f6,
        round_end_damage_judge_set_loss:        0x0800770a,
        round_end_damage_judge_set_draw:        0x08007710,
        round_start_ret:                        0x0800673e,
        round_set_ending:                       0x0800812c,
        round_end_entry:                        0x0800702e,
        round_post_increment_tick:              0x08006c2e,
        battle_is_p2_tst:                       0x0803d020,
        link_is_p2_ret:                         0x0803d03e,
        comm_menu_init_ret:                     0x08134c34,
        comm_menu_init_battle_entry:            0x08135928,
        handle_sio_entry:                       0x0803d11c,
        in_battle_call_handle_link_cable_input: 0x08006c12,
        comm_menu_end_battle_entry:             0x08134b50,
    },
};

#[rustfmt::skip]
pub static MEGAMAN5_TC_BRKE_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_jump_table_entry:          0x0803c496,
        start_screen_sram_unmask_ret:           0x0803008e,
        game_load_ret:                          0x08004a74,
        main_read_joyflags:                     0x080003ea,
        copy_input_data_entry:                  0x0801c7fc,
        copy_input_data_ret:                    0x0801c8c4,
        round_end_set_win:                      0x08007474,
        round_end_set_loss:                     0x08007488,
        round_end_damage_judge_set_win:         0x080076f6,
        round_end_damage_judge_set_loss:        0x0800770a,
        round_end_damage_judge_set_draw:        0x08007710,
        round_start_ret:                        0x0800673e,
        round_set_ending:                       0x0800812c,
        round_end_entry:                        0x0800702e,
        round_post_increment_tick:              0x08006c2e,
        battle_is_p2_tst:                       0x0803d024,
        link_is_p2_ret:                         0x0803d042,
        comm_menu_init_ret:                     0x08134d1c,
        comm_menu_init_battle_entry:            0x08135a10,
        handle_sio_entry:                       0x0803d120,
        in_battle_call_handle_link_cable_input: 0x08006c12,
        comm_menu_end_battle_entry:             0x08134c38,
    },
};

#[rustfmt::skip]
pub static ROCKEXE5_TOBBRBJ_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_jump_table_entry:          0x0803c3f6,
        start_screen_sram_unmask_ret:           0x08030026,
        game_load_ret:                          0x08004a74,
        main_read_joyflags:                     0x080003ea,
        copy_input_data_entry:                  0x0801c7bc,
        copy_input_data_ret:                    0x0801c884,
        round_end_set_win:                      0x08007474,
        round_end_set_loss:                     0x08007488,
        round_end_damage_judge_set_win:         0x080076f6,
        round_end_damage_judge_set_loss:        0x0800770a,
        round_end_damage_judge_set_draw:        0x08007710,
        round_start_ret:                        0x0800673e,
        round_set_ending:                       0x0800812c,
        round_end_entry:                        0x0800702e,
        round_post_increment_tick:              0x08006c2e,
        battle_is_p2_tst:                       0x0803cf34,
        link_is_p2_ret:                         0x0803cf52,
        comm_menu_init_ret:                     0x081347ec,
        comm_menu_init_battle_entry:            0x081354e0,
        handle_sio_entry:                       0x0803d030,
        in_battle_call_handle_link_cable_input: 0x08006c12,
        comm_menu_end_battle_entry:             0x08134708,
    },
};

#[rustfmt::skip]
pub static ROCKEXE5_TOCBRKJ_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_jump_table_entry:          0x0803c3fa,
        start_screen_sram_unmask_ret:           0x0803002a,
        game_load_ret:                          0x08004a74,
        main_read_joyflags:                     0x080003ea,
        copy_input_data_entry:                  0x0801c7b8,
        copy_input_data_ret:                    0x0801c880,
        round_end_set_win:                      0x08007474,
        round_end_set_loss:                     0x08007488,
        round_end_damage_judge_set_win:         0x080076f6,
        round_end_damage_judge_set_loss:        0x0800770a,
        round_end_damage_judge_set_draw:        0x08007710,
        round_start_ret:                        0x0800673e,
        round_set_ending:                       0x0800812c,
        round_end_entry:                        0x0800702e,
        round_post_increment_tick:              0x08006c2e,
        battle_is_p2_tst:                       0x0803cf38,
        link_is_p2_ret:                         0x0803cf56,
        comm_menu_init_ret:                     0x081348d4,
        comm_menu_init_battle_entry:            0x081355c8,
        handle_sio_entry:                       0x0803d034,
        in_battle_call_handle_link_cable_input: 0x08006c12,
        comm_menu_end_battle_entry:             0x081347f0,
    },
};

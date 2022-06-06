#[derive(Clone, Copy)]
pub(super) struct EWRAMOffsets {
    // Outgoing packet.
    pub(super) tx_packet: u32,

    // Incoming packet.
    pub(super) rx_packet_arr: u32,

    /// Start screen jump table control.
    pub(super) start_screen_control: u32,

    /// Title menu jump table control.
    pub(super) title_menu_control: u32,

    /// Subsystem control.
    pub(super) subsystem_control: u32,

    /// START menu submenu (e.g. comm menu) jump table control.
    pub(super) submenu_control: u32,

    /// Local RNG state. Doesn't need to be synced.
    pub(super) rng1_state: u32,

    /// Shared RNG state. Must be synced.
    pub(super) rng2_state: u32,

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

    /// This is immediately after SRAM is copied to EWRAM and unmasked.
    ///
    /// At this point, it is safe to do the equivalent of selecting the From save point option on the NG+ menu.
    pub(super) ngplus_menu_init_ret: u32,

    /// This is immediately after game initialization is complete: that is, the internal state is set correctly.
    ///
    /// At this point, it is safe to jump into the link battle menu.
    pub(super) game_load_ret: u32,

    /// This is directly after where KEYINPUT is read into r4 and then processed.
    ///
    /// Input is injected here directly by Tango into r4 from client. We avoid doing it via the usual input interrupt handling mechanism because this is more precise.
    pub(super) main_read_joyflags: u32,

    /// This hooks the entry into the function that will copy received input data from rx_packet_arr into game state, as well as copies the next game state into tx_packet.
    ///
    /// Received packets should be injected here into rx_packet_arr.
    pub(super) copy_input_data_entry: u32,

    /// This hooks the exit into the function that will copy received input data from rx_packet_arr into game state, as well as copies the next game state into tx_packet.
    ///
    /// Packets to transmit should be injected here into tx_packet.
    pub(super) copy_input_data_ret: u32,

    /// This hooks the point after the game determines who the winner is, returned in r0.
    ///
    /// If r0 = 1, the local player won the last round.
    /// If r0 = 2, the remote player won the last round.
    /// Otherwise, the battle hasn't ended.
    pub(super) round_run_unpaused_step_cmp_retval: u32,

    /// This hooks the point after the battle start routine is complete.
    ///
    /// Tango initializes its own battle tracking state at this point.
    pub(super) round_start_ret: u32,

    /// This hooks the point when the round is ending and the game will process no further input.
    ///
    /// At this point, Tango will clean up its round state and commit the replay.
    pub(super) round_ending_ret: u32,

    /// This hooks the point after the battle end routine is complete.
    pub(super) round_end_entry: u32,

    pub(super) round_call_jump_table_ret: u32,

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
    tx_packet:              0x02037bc0,
    rx_packet_arr:          0x0203ac10,
    start_screen_control:   0x0200b220,
    title_menu_control:     0x0200b220,
    subsystem_control:      0x0200a7e0,
    submenu_control:        0x0200a450,
    rng1_state:             0x020015d4,
    rng2_state:             0x02001790,
    copy_data_input_state:  0x0203f6d5,
};

#[derive(Clone, Copy)]
pub struct Offsets {
    pub(super) rom: ROMOffsets,
    pub(super) ewram: EWRAMOffsets,
}

#[rustfmt::skip]
pub static MEGAMANBN4BM: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_jump_table_entry:          0x0802d786,
        start_screen_sram_unmask_ret:           0x080253ca,
        ngplus_menu_init_ret:                   0x080255aa,
        game_load_ret:                          0x08004996,
        main_read_joyflags:                     0x080003c6,
        copy_input_data_entry:                  0x08017b8e,
        copy_input_data_ret:                    0x08017c56,
        round_run_unpaused_step_cmp_retval:     0x08007120,
        round_start_ret:                        0x08006710,
        round_ending_ret:                       0x080077da,
        round_end_entry:                        0x08006e1e,
        round_call_jump_table_ret:              0x08006b28,
        battle_is_p2_tst:                       0x08048204,
        link_is_p2_ret:                         0x08048222,
        comm_menu_init_ret:                     0x0803956a,
        handle_sio_entry:                       0x080482f8,
        in_battle_call_handle_link_cable_input: 0x08006b16,
        comm_menu_end_battle_entry:             0x0803a794,
    },
};

#[rustfmt::skip]
pub static MEGAMANBN4RS: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_jump_table_entry:          0x0802d786,
        start_screen_sram_unmask_ret:           0x080253c6,
        ngplus_menu_init_ret:                   0x080255a6,
        game_load_ret:                          0x08004996,
        main_read_joyflags:                     0x080003c6,
        copy_input_data_entry:                  0x08017b8e,
        copy_input_data_ret:                    0x08017c56,
        round_run_unpaused_step_cmp_retval:     0x08007120,
        round_start_ret:                        0x08006710,
        round_ending_ret:                       0x080077da,
        round_end_entry:                        0x08006e1e,
        round_call_jump_table_ret:              0x08006b28,
        battle_is_p2_tst:                       0x080481fc,
        link_is_p2_ret:                         0x0804821a,
        comm_menu_init_ret:                     0x08039562,
        handle_sio_entry:                       0x080482f0,
        in_battle_call_handle_link_cable_input: 0x08006b16,
        comm_menu_end_battle_entry:             0x0803a78c,
    },
};

#[rustfmt::skip]
pub static ROCK_EXE4_BM_10: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_jump_table_entry:          0x0802d69a,
        start_screen_sram_unmask_ret:           0x080252d2,
        ngplus_menu_init_ret:                   0x080254b2,
        game_load_ret:                          0x08004976,
        main_read_joyflags:                     0x080003c6,
        copy_input_data_entry:                  0x08017a9a,
        copy_input_data_ret:                    0x08017b62,
        round_run_unpaused_step_cmp_retval:     0x080070f4,
        round_start_ret:                        0x080066ec,
        round_ending_ret:                       0x080077ae,
        round_end_entry:                        0x08006dfa,
        round_call_jump_table_ret:              0x08006b04,
        battle_is_p2_tst:                       0x080480c4,
        link_is_p2_ret:                         0x080480e2,
        comm_menu_init_ret:                     0x08039442,
        handle_sio_entry:                       0x080481b8,
        in_battle_call_handle_link_cable_input: 0x08006af2,
        comm_menu_end_battle_entry:             0x0803a66c,
    },
};

#[allow(dead_code)]
#[rustfmt::skip]
pub static ROCK_EXE4_BM_11: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_jump_table_entry:          0x0802d6d6,
        start_screen_sram_unmask_ret:           0x0802530e,
        ngplus_menu_init_ret:                   0x080254ee,
        game_load_ret:                          0x08004976,
        main_read_joyflags:                     0x080003c6,
        copy_input_data_entry:                  0x08017ace,
        copy_input_data_ret:                    0x08017b96,
        round_run_unpaused_step_cmp_retval:     0x080070f8,
        round_start_ret:                        0x080066f0,
        round_ending_ret:                       0x080077b2,
        round_end_entry:                        0x08006dfe,
        round_call_jump_table_ret:              0x08006b08,
        battle_is_p2_tst:                       0x08048100,
        link_is_p2_ret:                         0x0804811e,
        comm_menu_init_ret:                     0x0803947e,
        handle_sio_entry:                       0x080481f4,
        in_battle_call_handle_link_cable_input: 0x08006af6,
        comm_menu_end_battle_entry:             0x0803a6a8,
    },
};

#[allow(dead_code)]
#[rustfmt::skip]
pub static ROCK_EXE4_RS_10: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_jump_table_entry:          0x0802d696,
        start_screen_sram_unmask_ret:           0x080252ce,
        ngplus_menu_init_ret:                   0x080254ae,
        game_load_ret:                          0x08004976,
        main_read_joyflags:                     0x080003c6,
        copy_input_data_entry:                  0x08017a9a,
        copy_input_data_ret:                    0x08017b62,
        round_run_unpaused_step_cmp_retval:     0x080070f4,
        round_start_ret:                        0x080066ec,
        round_ending_ret:                       0x080077ae,
        round_end_entry:                        0x08006dfa,
        round_call_jump_table_ret:              0x08006b04,
        battle_is_p2_tst:                       0x080480bc,
        link_is_p2_ret:                         0x080480da,
        comm_menu_init_ret:                     0x0803943a,
        handle_sio_entry:                       0x080481b0,
        in_battle_call_handle_link_cable_input: 0x08006af2,
        comm_menu_end_battle_entry:             0x0803a664,
    },
};

#[rustfmt::skip]
pub static ROCK_EXE4_RS_11: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_jump_table_entry:          0x0802d6d2,
        start_screen_sram_unmask_ret:           0x0802530a,
        ngplus_menu_init_ret:                   0x080254ea,
        game_load_ret:                          0x08004976,
        main_read_joyflags:                     0x080003c6,
        copy_input_data_entry:                  0x08017ace,
        copy_input_data_ret:                    0x08017b96,
        round_run_unpaused_step_cmp_retval:     0x080070f8,
        round_start_ret:                        0x080066f0,
        round_ending_ret:                       0x080077b2,
        round_end_entry:                        0x08006dfe,
        round_call_jump_table_ret:              0x08006b08,
        battle_is_p2_tst:                       0x080480f8,
        link_is_p2_ret:                         0x08048116,
        comm_menu_init_ret:                     0x08039476,
        handle_sio_entry:                       0x080481ec,
        in_battle_call_handle_link_cable_input: 0x08006af6,
        comm_menu_end_battle_entry:             0x0803a6a0,
    },
};

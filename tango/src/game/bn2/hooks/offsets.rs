#[derive(Clone, Copy)]
pub(super) struct EWRAMOffsets {
    // Outgoing packet.
    pub(super) tx_packet: u32,

    // Incoming packet.
    pub(super) rx_packet_arr: u32,

    /// Title menu jump table control.
    pub(super) title_menu_control: u32,

    /// Subsystem control.
    pub(super) subsystem_control: u32,

    /// START menu submenu (e.g. comm menu) jump table control.
    pub(super) submenu_control: u32,

    /// Shared RNG state. Must be synced.
    pub(super) rng_state: u32,

    pub(super) is_linking: u32,
    pub(super) packet_seqnum: u32,
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

    pub(super) start_screen_play_music_call: u32,

    /// This is immediately after game initialization is complete: that is, the internal state is set correctly.
    ///
    /// At this point, it is safe to jump into the link battle menu.
    pub(super) game_load_ret: u32,

    /// This is directly after where KEYINPUT is read into r4 and then processed.
    ///
    /// Input is injected here directly by Tango into r4 from client. We avoid doing it via the usual input interrupt handling mechanism because this is more precise.
    pub(super) main_read_joyflags: u32,

    pub(super) init_sio_call: u32,
    pub(super) comm_menu_send_and_receive_call: u32,
    pub(super) handle_input_custom_send_and_receive_call: u32,
    pub(super) handle_input_in_turn_send_and_receive_call: u32,

    pub(super) round_call_jump_table_ret: u32,

    /// This hooks the point after the battle start routine is complete.
    ///
    /// Tango initializes its own battle tracking state at this point.
    pub(super) round_start_ret: u32,

    pub(super) round_end_set_win: u32,
    pub(super) round_end_set_loss: u32,
    pub(super) round_end_damage_judge_set_win: u32,
    pub(super) round_end_damage_judge_set_loss: u32,
    pub(super) round_end_damage_judge_set_draw: u32,

    pub(super) round_ending_entry1: u32,
    pub(super) round_ending_entry2: u32,

    /// This hooks the point after the battle end routine is complete.
    pub(super) round_end_entry: u32,

    /// This hooks another point determining if the player is player 2 or not.
    ///
    /// r0 should be set to the local player index.
    pub(super) link_is_p2_ret: u32,

    /// This is the entry point to the comm menu.
    ///
    /// Here, Tango jumps directly into link battle.
    pub(super) comm_menu_init_ret: u32,

    /// This hooks the exit from the function that is called when a match ends.
    ///
    /// Tango ends its match here.
    pub(super) match_end_ret: u32,

    pub(super) battle_start_play_music_call: u32,
}

#[rustfmt::skip]
static EWRAM_OFFSETS: EWRAMOffsets = EWRAMOffsets {
    tx_packet:              0x02004f80,
    rx_packet_arr:          0x02009ba0,
    title_menu_control:     0x02009b80,
    subsystem_control:      0x02009078,
    submenu_control:        0x02007ea0,
    rng_state:              0x02009080,
    is_linking:             0x0200eae0,
    packet_seqnum:          0x0200ea9c,
};

#[derive(Clone, Copy)]
pub struct Offsets {
    pub(super) rom: ROMOffsets,
    pub(super) ewram: EWRAMOffsets,
}

#[rustfmt::skip]
pub static AE2E_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_jump_table_entry:              0x08024a54,
        start_screen_sram_unmask_ret:               0x0801c1f8,
        start_screen_play_music_call:               0x0801c174,
        game_load_ret:                              0x08003ccc,
        main_read_joyflags:                         0x08000376,
        init_sio_call:                              0x08006612,
        comm_menu_send_and_receive_call:            0x0802b07e,
        handle_input_custom_send_and_receive_call:  0x08006666,
        handle_input_in_turn_send_and_receive_call: 0x08006956,
        round_call_jump_table_ret:                  0x08005834,
        round_start_ret:                            0x08004e34,
        round_end_set_win:                          0x08006ec8,
        round_end_set_loss:                         0x08006ed0,
        round_end_damage_judge_set_win:             0x08005fd8,
        round_end_damage_judge_set_loss:            0x08005fc8,
        round_end_damage_judge_set_draw:            0x08005fbe,
        round_ending_entry1:                        0x08005c3a,
        round_ending_entry2:                        0x08005de6,
        round_end_entry:                            0x08006114,
        link_is_p2_ret:                             0x08002b28,
        comm_menu_init_ret:                         0x0802b2a0,
        match_end_ret:                              0x080061a2,
        battle_start_play_music_call:               0x08006ce0,
    },
};

#[rustfmt::skip]
pub static AE2J_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_jump_table_entry:              0x0802495c,
        start_screen_sram_unmask_ret:               0x0801c084,
        start_screen_play_music_call:               0x0801c000,
        game_load_ret:                              0x08003ccc,
        main_read_joyflags:                         0x08000376,
        init_sio_call:                              0x08006602,
        comm_menu_send_and_receive_call:            0x0802aefa,
        handle_input_custom_send_and_receive_call:  0x08006656,
        handle_input_in_turn_send_and_receive_call: 0x0800683e,
        round_call_jump_table_ret:                  0x08005830,
        round_start_ret:                            0x08004e30,
        round_end_set_win:                          0x08006d88,
        round_end_set_loss:                         0x08006d90,
        round_end_damage_judge_set_win:             0x08005fc8,
        round_end_damage_judge_set_loss:            0x08005fb8,
        round_end_damage_judge_set_draw:            0x08005fae,
        round_ending_entry1:                        0x08005c2a,
        round_ending_entry2:                        0x08005dd6,
        round_end_entry:                            0x08006104,
        link_is_p2_ret:                             0x08002b28,
        comm_menu_init_ret:                         0x0802b11c,
        match_end_ret:                              0x08006192,
        battle_start_play_music_call:               0x08006b9c,
    },
};

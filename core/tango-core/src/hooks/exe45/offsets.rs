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

    /// This is the entry point for the intro, i.e. when the PET appears and zooms in
    ///
    /// It is expected that at this point, you may write to the start_screen_control EWRAM address to skip to the intro.
    pub(super) intro_jump_table_entry: u32,

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

    /// This routine has to return 0 for r0 and r1 for battle to start
    ///
    /// Here, Tango sets r0 and r1 to 0
    pub(super) comm_menu_connection_check_ret: u32,

    /// This handles underlying link cable SIO in the comm menu.
    ///
    /// This should never be called.
    pub(super) handle_sio_entry: u32,

    /// This handles in-battle link cable SIO in the comm menu.
    ///
    /// This should be skipped.
    pub(super) in_battle_call_handle_link_cable_input: u32,

    /// This hooks the exit from the function that is called when a match ends.
    ///
    /// Tango ends its match here.
    pub(super) match_end_ret: u32,
}

#[rustfmt::skip]
static EWRAM_OFFSETS: EWRAMOffsets = EWRAMOffsets {
    tx_packet:              0x02035640,
    rx_packet_arr:          0x02038690,
    title_menu_control:     0x02010810,
    subsystem_control:      0x0200FD50,
    submenu_control:        0x0200F970,
    rng1_state:             0x02003D58,
    rng2_state:             0x02003F6C,
    copy_data_input_state:  0x0203DBBD,
};

#[derive(Clone, Copy)]
pub struct Offsets {
    pub(super) rom: ROMOffsets,
    pub(super) ewram: EWRAMOffsets,
}

#[rustfmt::skip]
pub static ROCKEXE45ROBR4J_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_jump_table_entry:          0x080305EE,
        intro_jump_table_entry:                 0x08045AEC,
        start_screen_sram_unmask_ret:           0x08028D3E,
        game_load_ret:                          0x08004266,
        main_read_joyflags:                     0x0800039E,
        copy_input_data_entry:                  0x08019262,//Routine slightly different from BN4
        copy_input_data_ret:                    0x08019364,
        round_end_set_win:                      0x080075D8,
        round_end_set_loss:                     0x080075EC,
        round_end_damage_judge_set_win:         0x08007882,
        round_end_damage_judge_set_loss:        0x08007896,
        round_end_damage_judge_set_draw:        0x0800789C,
        round_start_ret:                        0x08006B2E,
        round_set_ending:                       0x08007CC4,
        round_end_entry:                        0x080071EE,
        round_call_jump_table_ret:              0x08006E50,
        battle_is_p2_tst:                       0x0804A3A8,
        link_is_p2_ret:                         0x0804A3C6,
        comm_menu_init_ret:                     0x080440D2,//Routine different from BN4
        comm_menu_connection_check_ret:         0x08044BF6,
        handle_sio_entry:                       0x0804A49C,
        in_battle_call_handle_link_cable_input: 0x08006E3E,
        match_end_ret:                          0x08004746,
    },
};

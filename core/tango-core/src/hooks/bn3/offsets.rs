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

    pub(super) is_linking: u32,
}

#[derive(Clone, Copy)]
pub(super) struct ROMOffsets {
    // This sets the IE.
    pub(super) ie_toggle_hblank: u32,

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

    pub(super) init_sio_call: u32,
    pub(super) comm_menu_send_and_receive_call: u32,
    pub(super) handle_input_init_send_and_receive_call: u32,
    pub(super) handle_input_update_send_and_receive_call: u32,
    pub(super) handle_input_deinit_send_and_receive_call: u32,

    pub(super) handle_input_post_call: u32,

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

    /// This hooks the point determining if the player is player 2 or not.
    ///
    /// r0 should be set to the local player index.
    pub(super) battle_is_p2_ret: u32,

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
}

#[rustfmt::skip]
static EWRAM_OFFSETS: EWRAMOffsets = EWRAMOffsets {
    tx_packet:              0x02006d50,
    rx_packet_arr:          0x0200a330,
    title_menu_control:     0x0200a300,
    subsystem_control:      0x020097f8,
    submenu_control:        0x020093d0,
    rng1_state:             0x02009730,
    rng2_state:             0x02009800,
    is_linking:             0x0203b36e,
};

#[derive(Clone, Copy)]
pub struct Offsets {
    pub(super) rom: ROMOffsets,
    pub(super) ewram: EWRAMOffsets,
}

#[rustfmt::skip]
pub static MEGA_EXE3_BLA3XE: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        ie_toggle_hblank:                           0x08000174,
        start_screen_jump_table_entry:              0x0802b32c,
        start_screen_sram_unmask_ret:               0x08022016,
        game_load_ret:                              0x08004510,
        main_read_joyflags:                         0x08000392,
        init_sio_call:                              0x0803e976,
        comm_menu_send_and_receive_call:            0x0803e996,
        handle_input_init_send_and_receive_call:    0x080085d2,
        handle_input_update_send_and_receive_call:  0x080086a8,
        handle_input_deinit_send_and_receive_call:  0x0800877e,
        handle_input_post_call:                     0x0800643e,
        round_run_unpaused_step_cmp_retval:         0, // TODO
        round_start_ret:                            0x080059a8,
        round_ending_ret:                           0, // TODO
        round_end_entry:                            0, // TODO
        battle_is_p2_ret:                           0x08008c6a,
        link_is_p2_ret:                             0x0800354c,
        comm_menu_init_ret:                         0x0803e08a,
        match_end_ret:                              0, // TODO

    },
};

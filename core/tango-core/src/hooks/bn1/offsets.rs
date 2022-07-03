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
}

#[rustfmt::skip]
static EWRAM_OFFSETS: EWRAMOffsets = EWRAMOffsets {
    tx_packet:              0x020037d0,
    rx_packet_arr:          0x020075a0,
    title_menu_control:     0x02007590,
    subsystem_control:      0x02006cb8,
    submenu_control:        0x020062e0,
    rng_state:              0x02006cc0,
    packet_seqnum:          0x0200c1dc,
};

#[derive(Clone, Copy)]
pub struct Offsets {
    pub(super) rom: ROMOffsets,
    pub(super) ewram: EWRAMOffsets,
}

#[rustfmt::skip]
pub static MEGAMAN_BNAREE_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_jump_table_entry:              0x08018ca4,
        start_screen_sram_unmask_ret:               0x080104d2,
        game_load_ret:                              0x0800407e,
        main_read_joyflags:                         0x08000356,
        init_sio_call:                              0x0801ccee,
        comm_menu_send_and_receive_call:            0x0801cca4,
        handle_input_custom_send_and_receive_call:  0x08007842,
        handle_input_in_turn_send_and_receive_call: 0x08007aea,
        round_call_jump_table_ret:                  0x0800589a,
        round_start_ret:                            0x0800527a,
        round_end_set_win:                          0x08006d18,
        round_end_set_loss:                         0x08006d20,
        round_ending_entry1:                        0x08005bb4,
        round_ending_entry2:                        0x08005c2a,
        round_end_entry:                            0x08005cd0,
        link_is_p2_ret:                             0x08002c58,
        comm_menu_init_ret:                         0x0801ce94,
        match_end_ret:                              0x08005cd0,
    },
};

#[rustfmt::skip]
pub static ROCKMAN_EXEAREJ_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_jump_table_entry:              0x08018c18,
        start_screen_sram_unmask_ret:               0x0801048e,
        game_load_ret:                              0x0800406e,
        main_read_joyflags:                         0x08000356,
        init_sio_call:                              0x0801cbec,
        comm_menu_send_and_receive_call:            0x0801cbc4,
        handle_input_custom_send_and_receive_call:  0x0800782e,
        handle_input_in_turn_send_and_receive_call: 0x08007aba,
        round_call_jump_table_ret:                  0x0800588a,
        round_start_ret:                            0x0800526a,
        round_end_set_win:                          0x08006d08,
        round_end_set_loss:                         0x08006d10,
        round_ending_entry1:                        0x08005ba4,
        round_ending_entry2:                        0x08005ba6,
        round_end_entry:                            0x08005cc0,
        link_is_p2_ret:                             0x08002c48,
        comm_menu_init_ret:                         0x0801cd90,
        match_end_ret:                              0x08005cc0,
    },
};

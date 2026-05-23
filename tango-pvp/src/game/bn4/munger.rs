#[derive(Clone)]
pub(super) struct Munger {
    pub(super) offsets: &'static super::offsets::Offsets,
}

impl Munger {
    pub(super) fn skip_logo(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x00, -1, 0x10);
    }

    pub(super) fn continue_from_title_menu(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x00, -1, 0x08);
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x01, -1, 0x0c);
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x02, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x03, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x04, -1, 0xff);
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x08, -1, 0x01);
    }

    pub(super) fn continue_from_ngplus_menu(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x0, -1, 0x10);
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x1, -1, 0x00);
    }

    pub(super) fn open_comm_menu_from_overworld(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.subsystem_control, -1, 0x1c);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x0, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x2, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x3, -1, 0x00);
    }

    pub(super) fn select_battle_init_substate(&self, mut core: mgba::core::CoreMutRef) {
        // Advance the comm-menu state to Trill's original "skip to
        // battle init" target: outer dispatcher entry 7 (= [1]=0x1c),
        // sub-state 4, sub-sub-state 0x20. The settings-handler trap
        // calls this after pre-seeding rng so that once the handler
        // returns and the state machine ticks again, the outer
        // dispatcher lands at the battle-init path (which reads the
        // [0x11]/[0x2c] the handler just wrote).
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, 0x1c);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x3, -1, 0x20);
    }

    pub(super) fn start_battle_from_comm_menu(&self, mut core: mgba::core::CoreMutRef, match_type: u8) {
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x0, -1, 0x18);
        // Route through the in-game settings-handler function. Chain:
        //   [1]=0x0c -> outer dispatcher entry 3 (inner dispatcher
        //               that reads [r5, #2])
        //   [2]=4    -> 1st settings handler function (BL to generator
        //               at e.g. 0x803aa74 in B4BE_00).
        // After the function runs (under the comm_menu_settings_entry
        // trap), it sets [2]=0xc itself and the comm-menu state
        // machine continues naturally toward battle init.
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, 0x0c);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x2, -1, 0x04);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x3, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0xf, -1, 0x47 + match_type);
        // [0x14] is the halfword the settings handler uses for
        // various draws/helpers; the natural game state has it = 2.
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x14, -1, 0x02);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x15, -1, 0x00);
        // [0x2a] is the halfword the settings handler loads as the
        // generator's match_type argument (table-indexed for the
        // settings range).
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x2a, -1, match_type);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x2b, -1, 0x00);
    }

    pub(super) fn set_rng1_state(&self, mut core: mgba::core::CoreMutRef, state: u32) {
        core.raw_write_32(self.offsets.ewram.rng1_state, -1, state);
    }

    pub(super) fn set_rng2_state(&self, mut core: mgba::core::CoreMutRef, state: u32) {
        core.raw_write_32(self.offsets.ewram.rng2_state, -1, state);
    }

    pub(super) fn rng1_state(&self, mut core: mgba::core::CoreMutRef) -> u32 {
        core.raw_read_32(self.offsets.ewram.rng1_state, -1)
    }

    pub(super) fn rng2_state(&self, mut core: mgba::core::CoreMutRef) -> u32 {
        core.raw_read_32(self.offsets.ewram.rng2_state, -1)
    }

    pub(super) fn set_rx_packet(&self, mut core: mgba::core::CoreMutRef, index: u32, packet: &[u8; 0x10]) {
        core.raw_write_range(self.offsets.ewram.rx_packet_arr + index * 0x10, -1, packet)
    }

    pub(super) fn tx_packet(&self, mut core: mgba::core::CoreMutRef) -> [u8; 0x10] {
        let mut buf = [0u8; 0x10];
        core.raw_read_range(self.offsets.ewram.tx_packet, -1, &mut buf[..]);
        buf
    }

    pub(super) fn set_copy_data_input_state(&self, mut core: mgba::core::CoreMutRef, v: u8) {
        core.raw_write_8(self.offsets.ewram.copy_data_input_state, -1, v);
    }
}

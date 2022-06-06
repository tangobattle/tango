#[derive(Clone)]
pub(super) struct Munger {
    pub(super) offsets: super::offsets::Offsets,
}

impl Munger {
    pub(super) fn skip_logo(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.start_screen_control + 0x00, -1, 0x10);
    }

    pub(super) fn continue_from_title_menu(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x00, -1, 0x08);
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x01, -1, 0x10);
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x02, -1, 0x01);
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x03, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x04, -1, 0xff);
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x08, -1, 0x01);
    }

    pub(super) fn open_comm_menu_from_overworld(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.menu_control + 0x0, -1, 0x10);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x1, -1, 0x04);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x2, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x3, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x4, -1, 0x06);
        core.raw_write_8(self.offsets.ewram.menu_control + 0x5, -1, 0x01);
    }

    pub(super) fn start_battle_from_comm_menu(
        &self,
        mut core: mgba::core::CoreMutRef,
        match_type: u8,
    ) {
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x0, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, 0x0c);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x2, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x3, -1, match_type * 2);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1c, -1, 0x01);
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

    pub(super) fn set_rx_packet(
        &self,
        mut core: mgba::core::CoreMutRef,
        index: u32,
        packet: &[u8; 0x10],
    ) {
        core.raw_write_range(self.offsets.ewram.rx_packet_arr + index * 0x10, -1, packet)
    }

    pub(super) fn tx_packet(&self, mut core: mgba::core::CoreMutRef) -> [u8; 0x10] {
        core.raw_read_range(self.offsets.ewram.tx_packet, -1)
    }

    pub(super) fn set_battle_settings_and_background(
        &self,
        mut core: mgba::core::CoreMutRef,
        battle_settings: u8,
        background: u8,
    ) {
        core.raw_write_8(
            self.offsets.ewram.submenu_control + 0x16,
            -1,
            battle_settings,
        );
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x17, -1, background);
    }

    pub(super) fn current_tick(&self, mut core: mgba::core::CoreMutRef) -> u32 {
        core.raw_read_32(self.offsets.ewram.battle_state + 0x60, -1)
    }

    pub(super) fn set_current_tick(&self, mut core: mgba::core::CoreMutRef, v: u32) {
        core.raw_write_32(self.offsets.ewram.battle_state + 0x60, -1, v)
    }

    pub(super) fn set_copy_data_input_state(&self, mut core: mgba::core::CoreMutRef, v: u8) {
        core.raw_write_8(self.offsets.ewram.copy_data_input_state, -1, v);
    }
}

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
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x18, -1, 0x01);
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
        match_type: u16,
    ) {
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x0, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x2, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x3, -1, 0x00);
        core.raw_write_16(self.offsets.ewram.submenu_control + 0x12, -1, match_type);
    }

    pub(super) fn set_rng1_state(&self, mut core: mgba::core::CoreMutRef, state: u32) {
        core.raw_write_32(self.offsets.ewram.rng1_state, -1, state);
    }

    pub(super) fn set_rng2_state(&self, mut core: mgba::core::CoreMutRef, state: u32) {
        core.raw_write_32(self.offsets.ewram.rng2_state, -1, state);
    }

    pub(super) fn local_custom_screen_state(&self, mut core: mgba::core::CoreMutRef) -> u8 {
        core.raw_read_8(self.offsets.ewram.battle_state + 0x11, -1)
    }

    pub(super) fn tx_buf(&self, mut core: mgba::core::CoreMutRef) -> Vec<u8> {
        core.raw_read_range::<0x100>(self.offsets.ewram.tx_buf, -1)
            .to_vec()
    }

    pub(super) fn set_player_input_state(
        &self,
        mut core: mgba::core::CoreMutRef,
        index: u32,
        keys_pressed: u16,
        custom_screen_state: u8,
    ) {
        let a_player_input = self.offsets.ewram.player_input_data_arr + index * 0x08;
        let keys_held = core.raw_read_16(a_player_input + 0x02, -1) | 0xfc00;
        core.raw_write_16(a_player_input + 0x02, -1, keys_pressed);
        core.raw_write_16(a_player_input + 0x04, -1, !keys_held & keys_pressed);
        core.raw_write_16(a_player_input + 0x06, -1, keys_held & !keys_pressed);
        core.raw_write_8(
            self.offsets.ewram.battle_state + 0x14 + index,
            -1,
            custom_screen_state,
        )
    }

    pub(super) fn set_rx_buf(
        &self,
        mut core: mgba::core::CoreMutRef,
        index: u32,
        marshaled: &[u8],
    ) {
        core.raw_write_range(self.offsets.ewram.rx_buf_arr + index * 0x100, -1, marshaled)
    }

    pub(super) fn set_link_battle_settings_and_background(
        &self,
        mut core: mgba::core::CoreMutRef,
        v: u16,
    ) {
        core.raw_write_16(self.offsets.ewram.submenu_control + 0x2a, -1, v)
    }

    pub(super) fn current_tick(&self, mut core: mgba::core::CoreMutRef) -> u32 {
        core.raw_read_32(self.offsets.ewram.battle_state + 0x60, -1)
    }
}

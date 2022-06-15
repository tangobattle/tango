#[derive(Clone)]
pub(super) struct Munger {
    pub(super) offsets: super::offsets::Offsets,
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

    pub(super) fn open_comm_menu_from_overworld(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.subsystem_control, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x0, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x2, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x3, -1, 0x00);
    }

    pub(super) fn start_battle_from_comm_menu(
        &self,
        mut core: mgba::core::CoreMutRef,
        match_type: u8,
        background: u8,
    ) {
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x0, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, 0x30);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x2, -1, 0x00);
        core.raw_write_range(
            self.offsets.ewram.tx_packet,
            -1,
            &[
                0x01, 0x00, 0x00, 0xff, background, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00,
            ],
        );
        core.raw_write_8(
            self.offsets.ewram.submenu_control + 0x1c,
            -1,
            // 0 = lightweight, 1 = mediumweight, 2 = heavyweight, 3 = tri-battle
            match match_type {
                0 => 0,
                1 => 3,
                _ => 0,
            },
        );
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

    pub(super) fn is_linking(&self, mut core: mgba::core::CoreMutRef) -> bool {
        core.raw_read_8(self.offsets.ewram.is_linking, -1) == 1
    }
}

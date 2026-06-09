#[derive(Clone)]
pub(super) struct Munger {
    pub(super) offsets: &'static super::offsets::Offsets,
}

impl Munger {
    pub(super) fn skip_logo(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x00, -1, 0x10);
    }

    pub(super) fn continue_from_title_menu(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.title_menu_control + 0x00, -1, 0x01);
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

    pub(super) fn start_battle_from_comm_menu(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x0, -1, 0x18);
        // [1]=0x28 routes the inner dispatcher to the state handler
        // that contains the inline bg-generator block (vs the old
        // [1]=0x2c which jumped to the post-handshake state). The
        // comm_menu_settings_entry trap PC-redirects past the SIO
        // checks so the bg gen runs and writes tx_packet[2] itself.
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, 0x28);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x2, -1, 0x00);
        // Tx-packet template; [2] is the bg byte (left 0 — the ROM
        // handler overwrites it via `strb r0, [r7, #2]`).
        core.raw_write_range(
            self.offsets.ewram.tx_packet,
            -1,
            &[
                0x00, 0x04, 0x00, 0xff, 0xff, 0xff, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04,
            ],
        );
    }

    pub(super) fn select_battle_init_substate(&self, mut core: mgba::core::CoreMutRef, v: u8) {
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, v)
    }

    pub(super) fn set_rng_state(&self, mut core: mgba::core::CoreMutRef, state: u32) {
        core.raw_write_32(self.offsets.ewram.rng_state, -1, state);
    }

    pub(super) fn rng_state(&self, mut core: mgba::core::CoreMutRef) -> u32 {
        core.raw_read_32(self.offsets.ewram.rng_state, -1)
    }

    pub(super) fn set_rx_packet(&self, mut core: mgba::core::CoreMutRef, index: u32, packet: &[u8; 0x10]) {
        core.raw_write_range(self.offsets.ewram.rx_packet_arr + index * 0x10, -1, packet)
    }

    pub(super) fn tx_packet(&self, mut core: mgba::core::CoreMutRef) -> [u8; 0x10] {
        let mut buf = [0u8; 0x10];
        core.raw_read_range(self.offsets.ewram.tx_packet, -1, &mut buf[..]);
        buf
    }

    pub(super) fn is_linking(&self, mut core: mgba::core::CoreMutRef) -> bool {
        core.raw_read_8(self.offsets.ewram.is_linking, -1) == 1
    }

    pub(super) fn packet_seqnum(&self, mut core: mgba::core::CoreMutRef) -> u32 {
        core.raw_read_32(self.offsets.ewram.packet_seqnum, -1)
    }

    /// Custom (chip-select) screen scene phase. 2 == the screen is up (stays so
    /// through teardown, so the timer keeps counting in any sub-dialog).
    pub(super) fn custom_screen_scene(&self, mut core: mgba::core::CoreMutRef) -> u8 {
        core.raw_read_8(self.offsets.ewram.custom_screen_scene, -1) as u8
    }

    /// Custom-screen close sub-state. 8 == teardown has begun.
    pub(super) fn custom_screen_substate(&self, mut core: mgba::core::CoreMutRef) -> u8 {
        core.raw_read_8(self.offsets.ewram.custom_screen_substate, -1) as u8
    }

    /// Force the chip-select screen closed. Unlike BN5/BN6, BN2's close needs no
    /// cursor pin or injected button: writing the closing sub-state (8) makes the
    /// game's own closing-state handler run the teardown standalone (commit chips
    /// → animation → combat). Validated against the AE2E golden replay.
    pub(super) fn force_close_custom_screen(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.custom_screen_substate, -1, 8);
    }
}

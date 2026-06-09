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

    pub(super) fn open_comm_menu_from_overworld(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.subsystem_control, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x0, -1, 0x18);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x2, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x3, -1, 0x00);
    }

    pub(super) fn start_battle_from_comm_menu(&self, mut core: mgba::core::CoreMutRef, match_type: u8) {
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x0, -1, 0x18);
        // [1]=0x2c routes the inner dispatcher to the state handler
        // whose generator-path branch calls the ROM bg generator (vs
        // [1]=0x30 which jumps straight to the post-handshake state).
        // The comm_menu_settings_entry trap pre-seeds rng then PC-
        // redirects past the function's SIO checks so the generator
        // runs and writes the bg byte into the tx_packet itself.
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, 0x2c);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x2, -1, 0x00);
        // Tx-packet template; [4] is the bg byte (left 0 — the ROM
        // handler overwrites it via `strb r4, [r7, #4]`).
        core.raw_write_range(
            self.offsets.ewram.tx_packet,
            -1,
            &[
                0x01, 0x00, 0x00, 0xff, 0x00, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
        );
        // 0 = lightweight, 1 = midweight, 2 = heavyweight, 3 = tri-battle
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1c, -1, match_type);
    }

    pub(super) fn select_battle_init_substate(&self, mut core: mgba::core::CoreMutRef, v: u8) {
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, v)
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

    pub(super) fn is_linking(&self, mut core: mgba::core::CoreMutRef) -> bool {
        core.raw_read_8(self.offsets.ewram.is_linking, -1) == 1
    }

    /// Custom (chip-select) screen scene phase. 8 == the screen is up (stays so
    /// through teardown, so the timer keeps counting in any sub-dialog).
    pub(super) fn custom_screen_scene(&self, mut core: mgba::core::CoreMutRef) -> u8 {
        core.raw_read_8(self.offsets.ewram.custom_screen_scene, -1) as u8
    }

    /// Custom-screen sub-state machine. 4 == selecting; 8 == teardown begun.
    pub(super) fn custom_screen_substate(&self, mut core: mgba::core::CoreMutRef) -> u8 {
        core.raw_read_8(self.offsets.ewram.custom_screen_substate, -1) as u8
    }

    /// Pin the custom screen onto the confirm path. BN3 confirms with START
    /// (which ignores the grid cursor), so there's no cursor to place; we just
    /// force the sub-state to selecting (4) to pop out of any sub-dialog so the
    /// injected Start lands on the chip-select handler, which runs the genuine
    /// teardown (commit chips → animation → combat). Validated against the A6BE
    /// golden replay.
    pub(super) fn force_close_custom_screen(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.custom_screen_substate, -1, 4);
    }
}

#[derive(Clone)]
pub(super) struct Munger {
    pub(super) offsets: &'static super::offsets::Offsets,
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

    pub(super) fn start_battle_from_comm_menu(&self, mut core: mgba::core::CoreMutRef, match_type: u8) {
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x0, -1, 0x18);
        // Route the comm-menu state machine through the in-game
        // settings-handler function so the ROM generator writes
        // submenu_control[0x16]/[0x17] itself. Chain:
        //   [1]=4   -> outer dispatcher entry 1 (0x08134c40 in BRBE)
        //   [2]=0x14-> middle dispatcher
        //   [3]=0   -> settings-handler dispatcher
        //   [0x15]=0-> settings-handler function (BL to generator)
        // The comm_menu_settings_entry trap pre-seeds rng then advances
        // [1]=0x0c, [2]=0 so the *next* outer-dispatcher tick lands at
        // init_battle_entry, which consumes the just-written settings.
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, 0x04);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x2, -1, 0x14);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x3, -1, match_type * 2);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x15, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1c, -1, 0x01);
        // [0x3e:0x40] is the halfword the in-game settings generator
        // reads (LDRH at comm_menu_settings_entry+0x32 — 0x08134f20 in
        // BRBE) as its match_type / range argument: 0 = normal stage
        // range (0..0x44), 1 = extended (0..0x60).
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x3e, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x3f, -1, 0x00);
    }

    pub(super) fn select_init_battle_substate(&self, mut core: mgba::core::CoreMutRef) {
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, 0x0c);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x2, -1, 0x00);
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

    pub(super) fn current_tick(&self, mut core: mgba::core::CoreMutRef) -> u32 {
        core.raw_read_32(self.offsets.ewram.battle_state + 0x60, -1)
    }

    pub(super) fn set_current_tick(&self, mut core: mgba::core::CoreMutRef, v: u32) {
        core.raw_write_32(self.offsets.ewram.battle_state + 0x60, -1, v)
    }

    pub(super) fn set_copy_data_input_state(&self, mut core: mgba::core::CoreMutRef, v: u8) {
        core.raw_write_8(self.offsets.ewram.copy_data_input_state, -1, v);
    }

    /// Custom (chip-select) screen scene phase (`battle_subscene+0`). 4 == the
    /// chip-select screen is up for the whole phase including teardown.
    pub(super) fn battle_subscene(&self, mut core: mgba::core::CoreMutRef) -> u8 {
        core.raw_read_8(self.offsets.ewram.battle_subscene, -1) as u8
    }

    /// Custom-screen state-machine sub-state (`battle_subscene+1`). 4 ==
    /// selecting; 8 == the teardown/close has begun.
    pub(super) fn custom_subphase(&self, mut core: mgba::core::CoreMutRef) -> u8 {
        core.raw_read_8(self.offsets.ewram.battle_subscene + 1, -1) as u8
    }

    /// Pin the custom-screen state machine onto the natural confirm path so the
    /// game runs its own teardown (commit chips → close animation → combat).
    /// BN5 is structurally identical to BN6 (validated against BRKE): the
    /// selecting handler reads the grid cursor at `struct+7`; on A over the OK
    /// cell it dispatches the teardown (`0x08024960`) which sets `struct+1 := 8`.
    /// So force sub-state to selecting (4, pops out of any sub-dialog) and the
    /// cursor onto OK (10); the caller injects A. NOTE: cursor index 10 = OK was
    /// observed on BRKE — verify for the other BN5 variants.
    pub(super) fn force_close_custom_screen(&self, mut core: mgba::core::CoreMutRef) {
        let sub = self.offsets.ewram.battle_subscene;
        core.raw_write_8(sub + 1, -1, 4); // 0x02036b11 = selecting state (exit dialogs)
        core.raw_write_8(sub + 7, -1, 10); // 0x02036b17 = cursor on OK button
    }
}

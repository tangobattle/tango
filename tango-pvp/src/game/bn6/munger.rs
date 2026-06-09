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

    pub(super) fn start_battle_from_comm_menu(&self, mut core: mgba::core::CoreMutRef, match_type: u8) {
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x0, -1, 0x18);
        // submenu_control[1] = 0x10 lands the inner dispatcher on the
        // settings-handler state. Tango's comm_menu_settings_trap fires
        // there, pre-seeds RNG, and lets the game's own generator run.
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, 0x10);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x2, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x3, -1, 0x00);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x12, -1, match_type);
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x13, -1, 0);
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

    pub(super) fn select_battle_init_substate(&self, mut core: mgba::core::CoreMutRef, v: u8) {
        core.raw_write_8(self.offsets.ewram.submenu_control + 0x1, -1, v)
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

    /// Top-level battle sub-scene index. 4 == the custom (chip-select) screen.
    pub(super) fn battle_subscene(&self, mut core: mgba::core::CoreMutRef) -> u8 {
        core.raw_read_8(self.offsets.ewram.battle_subscene, -1) as u8
    }

    /// Custom-screen state-machine sub-state (`battle_subscene+1`). 4 ==
    /// selecting; 8 == the teardown/close has begun.
    pub(super) fn custom_subphase(&self, mut core: mgba::core::CoreMutRef) -> u8 {
        core.raw_read_8(self.offsets.ewram.battle_subscene + 1, -1) as u8
    }

    /// Latch one player's chip-select "ready" flag to the value the game writes
    /// when that player presses OK: `battle_state + 0x14 + player_index`. Doing
    /// Pin the custom-screen state machine onto the natural confirm path so the
    /// game runs its *own* teardown (commit chips → close animation → combat).
    ///
    /// Watchpoint RE (BR6E) traced the close completely: the custom screen is a
    /// nested jump-table state machine whose outer sub-state is `battle_subscene+1`
    /// (`0x020364c1`); the selecting handler (`0x08028b74`) reads the menu cell
    /// under the cursor (index at `struct+7`) and, on an A-press over the OK cell,
    /// dispatches to the teardown (`0x08028d3a`) — which commits the selection and
    /// sets `0x364c1 := 8` (close animation, ~60 frames → combat). There is no
    /// decoupled close entry: setting output state (the `battle_state +0x14/+0x15`
    /// "ready" flags, written by `copy_input_data` from the rx packets) never
    /// closes, and PC-hijacking the teardown corrupts it (it needs the live
    /// selecting-handler register context).
    ///
    /// So we drive the *real* confirm: force the sub-state to selecting (4) — which
    /// pops out of any sub-dialog — and the cursor (`struct+7`) onto OK (10); the
    /// caller injects A. The game's selecting handler then runs the genuine
    /// teardown with proper context. Verified end-to-end in the harness (closes to
    /// combat with real HP). NOTE: cursor index 10 = OK was observed on one BR6E
    /// layout — confirm it's layout-independent; JP/other ROMs need their own value.
    pub(super) fn force_close_custom_screen(&self, mut core: mgba::core::CoreMutRef) {
        let sub = self.offsets.ewram.battle_subscene;
        core.raw_write_8(sub + 1, -1, 4); // 0x020364c1 = selecting state (exit dialogs)
        core.raw_write_8(sub + 7, -1, 10); // 0x020364c7 = cursor on OK button
    }
}

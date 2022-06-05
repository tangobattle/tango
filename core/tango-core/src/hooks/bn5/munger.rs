#[derive(Clone)]
pub(super) struct Munger {
    pub(super) offsets: super::offsets::Offsets,
}

impl Munger {
    pub(super) fn skip_logo(&self, mut core: mgba::core::CoreMutRef) {}

    pub(super) fn continue_from_title_menu(&self, mut core: mgba::core::CoreMutRef) {}

    pub(super) fn open_comm_menu_from_overworld(&self, mut core: mgba::core::CoreMutRef) {}

    pub(super) fn start_battle_from_comm_menu(
        &self,
        mut core: mgba::core::CoreMutRef,
        match_type: u8,
    ) {
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

    pub(super) fn current_tick(&self, mut core: mgba::core::CoreMutRef) -> u32 {
        core.raw_read_32(self.offsets.ewram.battle_state + 0x60, -1)
    }

    pub(super) fn set_current_tick(&self, mut core: mgba::core::CoreMutRef, v: u32) {
        core.raw_write_32(self.offsets.ewram.battle_state + 0x60, -1, v)
    }
}

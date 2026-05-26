use byteorder::ByteOrder;

mod common;
mod display;
mod munger;
mod offsets;
mod primary;
mod rng;
mod shadow;
mod stepper;

pub struct Hooks {
    offsets: &'static offsets::Offsets,
}

impl Hooks {
    fn munger(&self) -> munger::Munger {
        munger::Munger { offsets: self.offsets }
    }
}

pub static AE2E_00: Hooks = Hooks {
    offsets: &offsets::AE2E_00,
};

pub static AE2J_00_AC: Hooks = Hooks {
    offsets: &offsets::AE2J_00_AC,
};

const INIT_RX: [u8; 16] = [
    0x00, 0x04, 0x00, 0xff, 0xff, 0xff, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04,
];

impl crate::hooks::Hooks for Hooks {
    fn common_traps(&self) -> Vec<crate::hooks::Trap> {
        common::traps(self)
    }

    fn primary_traps(
        &self,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        match_: crate::hooks::MatchHandle,
        completion_token: crate::hooks::CompletionToken,
    ) -> Vec<crate::hooks::Trap> {
        primary::traps(self, joyflags, match_, completion_token)
    }

    fn shadow_traps(&self, shadow_state: crate::shadow::State) -> Vec<crate::hooks::Trap> {
        shadow::traps(self, shadow_state)
    }

    fn display_traps(&self, handle: crate::battle::DisplayHandle) -> Vec<crate::hooks::Trap> {
        display::traps(self, handle)
    }

    fn stepper_traps(&self, stepper_state: crate::stepper::State) -> Vec<crate::hooks::Trap> {
        stepper::traps(self, stepper_state)
    }

    fn predict_rx(&self, rx: &mut Vec<u8>) {
        if rx[0] == 0x05 {
            let tick = byteorder::LittleEndian::read_u32(&rx[0xc..0x10]);
            byteorder::LittleEndian::write_u32(&mut rx[0xc..0x10], tick + 1);
        }
    }

    fn prepare_for_fastforward(&self, mut core: mgba::core::CoreMutRef) {
        core.gba_mut()
            .cpu_mut()
            .set_thumb_pc(self.offsets.rom.main_read_joyflags);
    }
}

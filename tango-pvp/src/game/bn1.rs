use byteorder::ByteOrder;

mod common;
mod munger;
mod offsets;
mod primary;
mod rng;
mod shadow;
mod stepper;

pub static AREE_00: Hooks = Hooks {
    offsets: &offsets::AREE_00,
};

pub static AREJ_00: Hooks = Hooks {
    offsets: &offsets::AREJ_00,
};

pub struct Hooks {
    pub offsets: &'static offsets::Offsets,
}

impl Hooks {
    fn munger(&self) -> munger::Munger {
        munger::Munger { offsets: self.offsets }
    }
}

const INIT_RX: [u8; 16] = [
    0x40, 0xff, 0xff, 0xff, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
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

    fn stepper_traps(&self, stepper_state: crate::stepper::State) -> Vec<crate::hooks::Trap> {
        stepper::traps(self, stepper_state)
    }

    fn predict_rx(&self, rx: &mut Vec<u8>) {
        if rx[0] == 0x80 {
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

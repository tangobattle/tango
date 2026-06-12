mod common;
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
        disable_bgm: bool,
    ) -> Vec<crate::hooks::Trap> {
        primary::traps(self, joyflags, match_, completion_token, disable_bgm)
    }

    fn shadow_traps(&self, shadow_state: crate::shadow::State) -> Vec<crate::hooks::Trap> {
        shadow::traps(self, shadow_state)
    }

    fn stepper_traps(&self, stepper_state: crate::stepper::State) -> Vec<crate::hooks::Trap> {
        stepper::traps(self, stepper_state)
    }

    fn prepare_for_fastforward(&self, mut core: mgba::core::CoreMutRef) {
        core.gba_mut()
            .cpu_mut()
            .set_thumb_pc(self.offsets.rom.main_read_joyflags);
    }

    fn inject_joyflags_on_primary_snapshot(&self, mut core: mgba::core::CoreMutRef, joyflags: u16) {
        core.gba_mut().cpu_mut().set_gpr(4, (joyflags | 0xfc00) as i32);
    }
}

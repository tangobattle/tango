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

pub static A3XE_00: Hooks = Hooks {
    offsets: &offsets::A3XE_00,
};
pub static A6BE_00: Hooks = Hooks {
    offsets: &offsets::A6BE_00,
};
pub static A3XJ_01: Hooks = Hooks {
    offsets: &offsets::A3XJ_01,
};
pub static A6BJ_01: Hooks = Hooks {
    offsets: &offsets::A6BJ_01,
};

const INIT_RX: [u8; 16] = [
    0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
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

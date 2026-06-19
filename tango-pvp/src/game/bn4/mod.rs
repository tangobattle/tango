mod common;
mod munger;
mod offsets;
mod pizzazz;
mod primary;
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

pub static B4BE_00: Hooks = Hooks {
    offsets: &offsets::B4BE_00,
};
pub static B4WE_00: Hooks = Hooks {
    offsets: &offsets::B4WE_00,
};
pub static B4BJ_01: Hooks = Hooks {
    offsets: &offsets::B4BJ_01,
};
pub static B4WJ_01: Hooks = Hooks {
    offsets: &offsets::B4WJ_01,
};

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
        core.gba_mut().cpu_mut().set_gpr(4, (joyflags | !crate::input::JOYFLAGS_MASK) as i32);
    }
}

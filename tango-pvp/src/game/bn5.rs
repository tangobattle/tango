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

pub static BRBE_00: Hooks = Hooks {
    offsets: &offsets::BRBE_00,
};
pub static BRKE_00: Hooks = Hooks {
    offsets: &offsets::BRKE_00,
};
pub static BRBJ_00: Hooks = Hooks {
    offsets: &offsets::BRBJ_00,
};
pub static BRKJ_00: Hooks = Hooks {
    offsets: &offsets::BRKJ_00,
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

    fn prepare_for_fastforward(&self, mut core: mgba::core::CoreMutRef) {
        core.gba_mut()
            .cpu_mut()
            .set_thumb_pc(self.offsets.rom.main_read_joyflags);
    }
}

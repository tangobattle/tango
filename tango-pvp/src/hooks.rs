pub struct CompletionToken {
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl CompletionToken {
    pub fn new(flag: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        Self { flag }
    }

    pub fn complete(&self) {
        self.flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

pub trait Hooks {
    fn patch(&self, _core: mgba::core::CoreMutRef) {}

    fn common_traps(&self) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)>;

    fn stepper_traps(&self, stepper_state: crate::stepper::State) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)>;

    fn shadow_traps(&self, shadow_state: crate::shadow::State) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)>;

    fn stepper_replay_traps(&self) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)> {
        vec![]
    }

    fn primary_traps(
        &self,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        match_: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<crate::battle::Match>>>>,
        completion_token: CompletionToken,
    ) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)>;

    fn packet_size(&self) -> usize {
        return 0x10;
    }

    fn prepare_for_fastforward(&self, core: mgba::core::CoreMutRef);

    fn predict_rx(&self, _rx: &mut Vec<u8>) {}
}

pub fn hooks_for_gamedb_entry(entry: &tango_gamedb::Game) -> Option<&'static (dyn Hooks + Send + Sync)> {
    Some(match entry.rom_code_and_revision {
        (b"AREJ", 0x00) => &crate::game::bn1::AREJ_00,
        (b"AREE", 0x00) => &crate::game::bn1::AREE_00,

        (b"AE2J", 0x00) => &crate::game::bn2::AE2J_00_AC,
        (b"AE2E", 0x00) => &crate::game::bn2::AE2E_00,

        (b"A6BJ", 0x01) => &crate::game::bn3::A6BJ_01,
        (b"A3XJ", 0x01) => &crate::game::bn3::A3XJ_01,
        (b"A6BE", 0x00) => &crate::game::bn3::A6BE_00,
        (b"A3XE", 0x00) => &crate::game::bn3::A3XE_00,

        (b"B4WJ", 0x01) => &crate::game::bn4::B4WJ_01,
        (b"B4BJ", 0x01) => &crate::game::bn4::B4BJ_01,
        (b"B4WE", 0x00) => &crate::game::bn4::B4WE_00,
        (b"B4BE", 0x00) => &crate::game::bn4::B4BE_00,

        (b"BRBJ", 0x00) => &crate::game::bn5::BRBJ_00,
        (b"BRKJ", 0x00) => &crate::game::bn5::BRKJ_00,
        (b"BRBE", 0x00) => &crate::game::bn5::BRBE_00,
        (b"BRKE", 0x00) => &crate::game::bn5::BRKE_00,

        (b"BR5J", 0x00) => &crate::game::bn6::BR5J_00,
        (b"BR6J", 0x00) => &crate::game::bn6::BR6J_00,
        (b"BR5E", 0x00) => &crate::game::bn6::BR5E_00,
        (b"BR6E", 0x00) => &crate::game::bn6::BR6E_00,

        (b"BR4J", 0x00) => &crate::game::exe45::BR4J_00,

        _ => {
            return None;
        }
    })
}

use crate::{battle, replayer, session, shadow};

mod bn1;
mod bn2;
mod bn3;
mod bn4;
mod bn5;
mod bn6;
mod exe45;

pub fn find_hook(
    mut core: mgba::core::CoreMutRef,
) -> Option<&'static Box<dyn Hooks + Send + Sync>> {
    Some(match (&core.full_rom_name(), core.rom_revision()) {
        (b"MEGAMAN6_FXXBR6E", 0x00) => &bn6::MEGAMAN6_FXXBR6E_00,
        (b"MEGAMAN6_GXXBR5E", 0x00) => &bn6::MEGAMAN6_GXXBR5E_00,
        (b"ROCKEXE6_RXXBR6J", 0x00) => &bn6::ROCKEXE6_RXXBR6J_00,
        (b"ROCKEXE6_GXXBR5J", 0x00) => &bn6::ROCKEXE6_GXXBR5J_00,
        (b"MEGAMAN5_TP_BRBE", 0x00) => &bn5::MEGAMAN5_TP_BRBE_00,
        (b"MEGAMAN5_TC_BRKE", 0x00) => &bn5::MEGAMAN5_TC_BRKE_00,
        (b"ROCKEXE5_TOBBRBJ", 0x00) => &bn5::ROCKEXE5_TOBBRBJ_00,
        (b"ROCKEXE5_TOCBRKJ", 0x00) => &bn5::ROCKEXE5_TOCBRKJ_00,
        (b"ROCKEXE4.5ROBR4J", 0x00) => &exe45::ROCKEXE45ROBR4J_00,
        (b"MEGAMANBN4BMB4BE", 0x00) => &bn4::MEGAMANBN4BMB4BE_00,
        (b"MEGAMANBN4RSB4WE", 0x00) => &bn4::MEGAMANBN4RSB4WE_00,
        (b"ROCK_EXE4_BMB4BJ", 0x00) => &bn4::ROCK_EXE4_BMB4BJ_00,
        (b"ROCK_EXE4_BMB4BJ", 0x01) => &bn4::ROCK_EXE4_BMB4BJ_01,
        (b"ROCK_EXE4_RSB4WJ", 0x00) => &bn4::ROCK_EXE4_RSB4WJ_00,
        (b"ROCK_EXE4_RSB4WJ", 0x01) => &bn4::ROCK_EXE4_RSB4WJ_01,
        (b"MEGA_EXE3_BLA3XE", 0x00) => &bn3::MEGA_EXE3_BLA3XE_00,
        (b"MEGA_EXE3_WHA6BE", 0x00) => &bn3::MEGA_EXE3_WHA6BE_00,
        (b"ROCK_EXE3_BKA3XJ", 0x01) => &bn3::ROCK_EXE3_BKA3XJ_01,
        (b"ROCKMAN_EXE3A6BJ", 0x01) => &bn3::ROCKMAN_EXE3A6BJ_01,
        (b"MEGAMAN_EXE2AE2E", 0x00) => &bn2::MEGAMAN_EXE2AE2E_00,
        (b"ROCKMAN_EXE2AE2J", 0x01) => &bn2::ROCKMAN_EXE2AE2J_01,
        _ => {
            return None;
        }
    })
}

pub fn find(code: &str, revision: u8) -> Option<Box<dyn Game>> {
    Some(match (code, revision) {
        ("AREJ", 0x00) => Box::new(bn1::EXE1 {}),
        ("AREE", 0x00) => Box::new(bn1::BN1 {}),
        // ("AE2J", 0x01) => Box::new(bn2::EXE2 {}),
        // ("AE2E", 0x00) => Box::new(bn2::BN2 {}),
        // ("A6BJ", 0x01) => Box::new(bn3::EXE3W{}),
        // ("A3XJ", 0x01) => Box::new(bn3::EXE3B{}),
        // ("A6BE", 0x00) => Box::new(bn3::BN3W{}),
        // ("A3XE", 0x00) => Box::new(bn3::BN3B{}),
        // ("B4BJ", 0x00) => Box::new(bn4::EXE4BM{}),
        // ("B4WJ", 0x01) => Box::new(bn4::EXE4RS{}),
        // ("B4BE", 0x00) => Box::new(bn4::BN4BM{}),
        // ("B4WE", 0x00) => Box::new(bn4::BN4RS{}),
        // ("BR4J", 0x00) => Box::new(exe45::EXE45{}),
        // ("BRBJ", 0x00) => Box::new(bn5::EXE5B{}),
        // ("BRKJ", 0x00) => Box::new(bn5::EXE5C{}),
        // ("BRBE", 0x00) => Box::new(bn5::EXE5P{}),
        // ("BRKE", 0x00) => Box::new(bn5::EXE5C{}),
        // ("BR5J", 0x00) => Box::new(bn6::EXE6G{}),
        // ("BR6J", 0x00) => Box::new(bn6::EXE6F{}),
        // ("BR5E", 0x00) => Box::new(bn6::BN6G{}),
        // ("BR6E", 0x00) => Box::new(bn6::BN6F{}),
        _ => {
            return None;
        }
    })
}

pub trait Game {
    fn family_name(&self) -> &str;
    fn version_name(&self) -> Option<&str>;
    fn hooks(&self) -> Box<dyn Hooks + Send + Sync + 'static>;
}

pub trait Hooks {
    fn patch(&self, _core: mgba::core::CoreMutRef) {}

    fn common_traps(&self) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)>;

    fn replayer_traps(
        &self,
        replayer_state: replayer::State,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)>;

    fn shadow_traps(
        &self,
        shadow_state: shadow::State,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)>;

    fn primary_traps(
        &self,
        handle: tokio::runtime::Handle,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        match_: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<battle::Match>>>>,
        completion_token: session::CompletionToken,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)>;

    fn packet_size(&self) -> usize {
        return 0x10;
    }

    fn prepare_for_fastforward(&self, core: mgba::core::CoreMutRef);

    fn predict_rx(&self, _rx: &mut Vec<u8>) {}
}

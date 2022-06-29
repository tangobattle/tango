use crate::{facade, replayer, shadow};

mod bn2;
mod bn3;
mod bn4;
mod bn5;
mod bn6;
mod exe45;

pub fn get(mut core: mgba::core::CoreMutRef) -> Option<&'static Box<dyn Hooks + Send + Sync>> {
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
        _ => {
            return None;
        }
    })
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
        facade: facade::Facade,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)>;

    fn rx_size(&self) -> usize {
        return 0x10;
    }

    fn prepare_for_fastforward(&self, core: mgba::core::CoreMutRef);

    fn predict_rx(&self, _rx: &mut Vec<u8>) {}

    fn replace_opponent_name(&self, _core: mgba::core::CoreMutRef, _name: &str) {}
}

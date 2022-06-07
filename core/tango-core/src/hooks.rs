use crate::{facade, fastforwarder, shadow};

mod bn3;
mod bn4;
mod bn5;
mod bn6;

pub fn get(mut core: mgba::core::CoreMutRef) -> Option<&'static Box<dyn Hooks + Send + Sync>> {
    match &core.full_rom_name() {
        b"MEGAMAN6_FXXBR6E" => Some(&bn6::MEGAMAN6_FXXBR6E),
        b"MEGAMAN6_GXXBR5E" => Some(&bn6::MEGAMAN6_GXXBR5E),
        b"ROCKEXE6_RXXBR6J" => Some(&bn6::ROCKEXE6_RXXBR6J),
        b"ROCKEXE6_GXXBR5J" => Some(&bn6::ROCKEXE6_GXXBR5J),
        b"MEGAMAN5_TP_BRBE" => Some(&bn5::MEGAMAN5_TP_BRBE),
        b"MEGAMAN5_TC_BRKE" => Some(&bn5::MEGAMAN5_TC_BRKE),
        b"ROCKEXE5_TOBBRBJ" => Some(&bn5::ROCKEXE5_TOBBRBJ),
        b"ROCKEXE5_TOCBRKJ" => Some(&bn5::ROCKEXE5_TOCBRKJ),
        b"MEGAMANBN4BMB4BE" => Some(&bn4::MEGAMANBN4BMB4BE),
        b"MEGAMANBN4RSB4WE" => Some(&bn4::MEGAMANBN4RSB4WE),
        b"ROCK_EXE4_BMB4BJ" => match core.rom_revision() {
            0x00 => Some(&bn4::ROCK_EXE4_BMB4BJ_00),
            0x01 => Some(&bn4::ROCK_EXE4_BMB4BJ_01),
            _ => None,
        },
        b"ROCK_EXE4_RSB4WJ" => match core.rom_revision() {
            0x00 => Some(&bn4::ROCK_EXE4_RSB4WJ_00),
            0x01 => Some(&bn4::ROCK_EXE4_RSB4WJ_01),
            _ => None,
        },
        b"MEGA_EXE3_BLA3XE" => Some(&bn3::MEGA_EXE3_BLA3XE),
        _ => None,
    }
}

pub trait Hooks {
    fn common_traps(&self) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)>;

    fn fastforwarder_traps(
        &self,
        ff_state: fastforwarder::State,
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

    fn placeholder_rx(&self) -> Vec<u8>;

    fn prepare_for_fastforward(&self, core: mgba::core::CoreMutRef);

    fn replace_opponent_name(&self, core: mgba::core::CoreMutRef, name: &str);
}

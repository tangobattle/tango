use crate::{facade, fastforwarder, shadow};

mod bn4;
mod bn5;
mod bn6;

pub fn get(mut core: mgba::core::CoreMutRef) -> Option<&'static Box<dyn Hooks + Send + Sync>> {
    match core.as_ref().game_title().as_str() {
        "MEGAMAN6_FXX" => Some(&bn6::MEGAMAN6_FXX),
        "MEGAMAN6_GXX" => Some(&bn6::MEGAMAN6_GXX),
        "ROCKEXE6_RXX" => Some(&bn6::ROCKEXE6_RXX),
        "ROCKEXE6_GXX" => Some(&bn6::ROCKEXE6_GXX),
        "MEGAMAN5_TP_" => Some(&bn5::MEGAMAN5_TP_),
        "MEGAMAN5_TC_" => Some(&bn5::MEGAMAN5_TC_),
        "ROCKEXE5_TOB" => Some(&bn5::ROCKEXE5_TOB),
        "ROCKEXE5_TOC" => Some(&bn5::ROCKEXE5_TOC),
        "MEGAMANBN4BM" => Some(&bn4::MEGAMANBN4BM),
        "MEGAMANBN4RS" => Some(&bn4::MEGAMANBN4RS),
        "ROCK_EXE4_BM" => {
            if &core.raw_read_range(0x08113abc, -1) == b"Nov 11 2003 22:00:07" {
                log::info!("this is blue moon 1.0");
                Some(&bn4::ROCK_EXE4_BM_10)
            } else if &core.raw_read_range(0x08113b38, -1) == b"Jan 27 2004 21:09:17" {
                log::info!("this is blue moon 1.1");
                Some(&bn4::ROCK_EXE4_BM_11)
            } else {
                None
            }
        }
        "ROCK_EXE4_RS" => {
            if &core.raw_read_range(0x08113aa8, -1) == b"Nov 11 2003 21:54:47" {
                log::info!("this is red sun 1.0");
                Some(&bn4::ROCK_EXE4_RS_10)
            } else if &core.raw_read_range(0x08113b24, -1) == b"Jan 27 2004 21:00:28" {
                log::info!("this is red sun 1.1");
                Some(&bn4::ROCK_EXE4_RS_11)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub trait Hooks {
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

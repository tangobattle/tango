use crate::{facade, fastforwarder, hooks, input, shadow};

#[derive(Clone)]
pub struct BN4 {}

lazy_static! {
    pub static ref MEGAMANBN4BM: Box<dyn hooks::Hooks + Send + Sync> = BN4::new();
    pub static ref MEGAMANBN4RS: Box<dyn hooks::Hooks + Send + Sync> = BN4::new();
    pub static ref ROCK_EXE4_BM: Box<dyn hooks::Hooks + Send + Sync> = BN4::new();
    pub static ref ROCK_EXE4_RS: Box<dyn hooks::Hooks + Send + Sync> = BN4::new();
}

impl BN4 {
    pub fn new() -> Box<dyn hooks::Hooks + Send + Sync> {
        Box::new(BN4 {})
    }
}

impl hooks::Hooks for BN4 {
    fn primary_traps(
        &self,
        handle: tokio::runtime::Handle,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        facade: facade::Facade,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)> {
        vec![]
    }

    fn shadow_traps(
        &self,
        shadow_state: shadow::State,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)> {
        vec![]
    }

    fn fastforwarder_traps(
        &self,
        ff_state: fastforwarder::State,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)> {
        vec![]
    }

    fn placeholder_rx(&self) -> Vec<u8> {
        vec![0; 0x10]
    }

    fn prepare_for_fastforward(&self, mut core: mgba::core::CoreMutRef) {}

    fn replace_opponent_name(&self, mut core: mgba::core::CoreMutRef, name: &str) {}

    fn current_tick(&self, core: mgba::core::CoreMutRef) -> u32 {
        0
    }
}

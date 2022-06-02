use crate::{facade, fastforwarder, shadow};

mod bn4;
mod bn6;

lazy_static! {
    pub static ref HOOKS: std::collections::HashMap<String, &'static Box<dyn Hooks + Send + Sync>> = {
        let mut hooks =
            std::collections::HashMap::<String, &'static Box<dyn Hooks + Send + Sync>>::new();
        hooks.insert("MEGAMAN6_FXX".to_string(), &bn6::MEGAMAN6_FXX);
        hooks.insert("MEGAMAN6_GXX".to_string(), &bn6::MEGAMAN6_GXX);
        hooks.insert("ROCKEXE6_RXX".to_string(), &bn6::ROCKEXE6_RXX);
        hooks.insert("ROCKEXE6_GXX".to_string(), &bn6::ROCKEXE6_GXX);
        hooks.insert("MEGAMANBN4BM".to_string(), &bn4::MEGAMANBN4BM);
        hooks.insert("MEGAMANBN4RS".to_string(), &bn4::MEGAMANBN4RS);
        hooks.insert("ROCK_EXE4_BM".to_string(), &bn4::ROCK_EXE4_BM);
        hooks.insert("ROCK_EXE4_RS".to_string(), &bn4::ROCK_EXE4_RS);
        hooks
    };
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

    fn current_tick(&self, core: mgba::core::CoreMutRef) -> u32;
}

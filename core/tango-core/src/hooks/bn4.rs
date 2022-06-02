mod munger;
mod offsets;

use crate::{facade, fastforwarder, hooks, input, shadow};

#[derive(Clone)]
pub struct BN4 {
    offsets: offsets::Offsets,
    munger: munger::Munger,
}

lazy_static! {
    pub static ref MEGAMANBN4BM: Box<dyn hooks::Hooks + Send + Sync> =
        BN4::new(offsets::MEGAMANBN4BM);
    // pub static ref MEGAMANBN4RS: Box<dyn hooks::Hooks + Send + Sync> =
    //     BN4::new(offsets::MEGAMANBN4RS);
    // pub static ref ROCK_EXE4_BM: Box<dyn hooks::Hooks + Send + Sync> = BN4::new();
    // pub static ref ROCK_EXE4_RS: Box<dyn hooks::Hooks + Send + Sync> = BN4::new();
}

impl BN4 {
    pub fn new(offsets: offsets::Offsets) -> Box<dyn hooks::Hooks + Send + Sync> {
        Box::new(BN4 {
            offsets,
            munger: munger::Munger { offsets },
        })
    }
}

fn step_rng(seed: u32) -> u32 {
    let seed = std::num::Wrapping(seed);
    (((seed * std::num::Wrapping(2)) - (seed >> 0x1f) + std::num::Wrapping(1))
        ^ std::num::Wrapping(0x873ca9e5))
    .0
}

fn generate_rng1_state(rng: &mut impl rand::Rng) -> u32 {
    let mut rng1_state = 0;
    for _ in 0..rng.gen_range(0..=0xffffusize) {
        rng1_state = step_rng(rng1_state);
    }
    rng1_state
}

fn generate_rng2_state(rng: &mut impl rand::Rng) -> u32 {
    let mut rng2_state = 0xa338244f;
    for _ in 0..rng.gen_range(0..=0xffffusize) {
        rng2_state = step_rng(rng2_state);
    }
    rng2_state
}

impl hooks::Hooks for BN4 {
    fn primary_traps(
        &self,
        handle: tokio::runtime::Handle,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        facade: facade::Facade,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)> {
        vec![
            {
                let munger = self.munger.clone();
                (
                    self.offsets.rom.start_screen_jump_table_entry,
                    Box::new(move |core| {
                        munger.skip_logo(core);
                    }),
                )
            },
            {
                let munger = self.munger.clone();
                (
                    self.offsets.rom.start_screen_sram_unmask_ret,
                    Box::new(move |core| {
                        munger.continue_from_title_menu(core);
                    }),
                )
            },
        ]
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

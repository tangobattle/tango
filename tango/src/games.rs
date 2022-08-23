use crate::{battle, replayer, session, shadow};

mod bn1;
mod bn2;
mod bn3;
mod bn4;
mod bn5;
mod bn6;
mod exe45;

lazy_static! {
    static ref GAMES: std::collections::HashMap<(&'static str, u8), &'static (dyn Game + Send + Sync)> =
        std::collections::HashMap::from([
            (("AREJ", 0x00), bn1::EXE1),
            (("AREE", 0x00), bn1::BN1),
            (("AE2J", 0x01), bn2::EXE2),
            (("AE2E", 0x00), bn2::BN2),
            (("A6BJ", 0x01), bn3::EXE3W),
            (("A3XJ", 0x01), bn3::EXE3B),
            (("A6BE", 0x00), bn3::BN3W),
            (("A3XE", 0x00), bn3::BN3B),
            (("B4WJ", 0x01), bn4::EXE4RS),
            (("B4BJ", 0x00), bn4::EXE4BM),
            (("B4WE", 0x00), bn4::BN4RS),
            (("B4BE", 0x00), bn4::BN4BM),
            (("BR4J", 0x00), exe45::EXE45),
            (("BRBJ", 0x00), bn5::EXE5B),
            (("BRKJ", 0x00), bn5::EXE5C),
            (("BRBE", 0x00), bn5::BN5P),
            (("BRKE", 0x00), bn5::BN5C),
            (("BR5J", 0x00), bn6::EXE6G),
            (("BR6J", 0x00), bn6::EXE6F),
            (("BR5E", 0x00), bn6::BN6G),
            (("BR6E", 0x00), bn6::BN6F),
        ]);
}

pub fn find(code: &str, revision: u8) -> Option<&'static (dyn Game + Send + Sync)> {
    GAMES.get(&(code, revision)).map(|game| *game)
}

// pub fn detect(rom: &[u8]) -> Option<&'static (dyn Game)> {}

pub trait Save {}

pub trait Game {
    fn family(&self) -> &str;
    fn variant(&self) -> u32;
    fn language(&self) -> unic_langid::LanguageIdentifier;
    fn expected_crc32(&self) -> u32;
    fn hooks(&self) -> &'static (dyn Hooks + Send + Sync);
    fn parse_save(&self, data: Vec<u8>) -> Result<Box<dyn Save>, anyhow::Error> {
        anyhow::bail!("not implemented");
    }
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

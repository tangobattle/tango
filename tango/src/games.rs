use crate::{battle, replayer, session, shadow};

mod bn1;
mod bn2;
mod bn3;
mod bn4;
mod bn5;
mod bn6;
mod exe45;

lazy_static! {
    static ref GAMES: std::collections::HashMap<(&'static str, u8), Box<dyn Game + Send + Sync + 'static>> = {
        let mut hm = std::collections::HashMap::<
            (&'static str, u8),
            Box<dyn Game + Send + Sync + 'static>,
        >::new();
        hm.insert(("AREJ", 0x00), Box::new(bn1::EXE1 {}));
        hm.insert(("AREE", 0x00), Box::new(bn1::BN1 {}));
        hm.insert(("AE2J", 0x01), Box::new(bn2::EXE2 {}));
        hm.insert(("AE2E", 0x00), Box::new(bn2::BN2 {}));
        hm.insert(("A6BJ", 0x01), Box::new(bn3::EXE3W {}));
        hm.insert(("A3XJ", 0x01), Box::new(bn3::EXE3B {}));
        hm.insert(("A6BE", 0x00), Box::new(bn3::BN3W {}));
        hm.insert(("A3XE", 0x00), Box::new(bn3::BN3B {}));
        hm.insert(("B4WJ", 0x01), Box::new(bn4::EXE4RS {}));
        hm.insert(("B4BJ", 0x00), Box::new(bn4::EXE4BM {}));
        hm.insert(("B4WE", 0x00), Box::new(bn4::BN4RS {}));
        hm.insert(("B4BE", 0x00), Box::new(bn4::BN4BM {}));
        hm.insert(("BR4J", 0x00), Box::new(exe45::EXE45 {}));
        hm.insert(("BRBJ", 0x00), Box::new(bn5::EXE5B {}));
        hm.insert(("BRKJ", 0x00), Box::new(bn5::EXE5C {}));
        hm.insert(("BRBE", 0x00), Box::new(bn5::BN5P {}));
        hm.insert(("BRKE", 0x00), Box::new(bn5::BN5C {}));
        hm.insert(("BR5J", 0x00), Box::new(bn6::EXE6G {}));
        hm.insert(("BR6J", 0x00), Box::new(bn6::EXE6F {}));
        hm.insert(("BR5E", 0x00), Box::new(bn6::BN6G {}));
        hm.insert(("BR6E", 0x00), Box::new(bn6::BN6F {}));
        hm
    };
}

pub fn find(code: &str, revision: u8) -> Option<Box<dyn Game + Send + Sync + 'static>> {
    GAMES.get(&(code, revision)).cloned()
}

pub trait Save {}

pub trait Game
where
    Self: GameClone,
{
    fn family(&self) -> &str;
    fn variant(&self) -> u32;
    fn language(&self) -> unic_langid::LanguageIdentifier;
    fn expected_crc32(&self) -> u32;
    fn hooks(&self) -> Box<dyn Hooks + Send + Sync + 'static>;
    fn parse_save(&self, data: Vec<u8>) -> Result<Box<dyn Save>, anyhow::Error> {
        anyhow::bail!("not implemented");
    }
}

pub trait GameClone {
    fn clone_box(&self) -> Box<dyn Game + Sync + Send + 'static>;
}

impl<T: Game + Sync + Send + Clone + 'static> GameClone for T {
    fn clone_box(&self) -> Box<dyn Game + Sync + Send + 'static> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn Game + Sync + Send + 'static> {
    fn clone(&self) -> Box<dyn Game + Sync + Send + 'static> {
        self.clone_box()
    }
}

pub trait Hooks
where
    Self: HooksClone,
{
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

pub trait HooksClone {
    fn clone_box(&self) -> Box<dyn Hooks + Sync + Send + 'static>;
}

impl<T: Hooks + Sync + Send + Clone + 'static> HooksClone for T {
    fn clone_box(&self) -> Box<dyn Hooks + Sync + Send + 'static> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn Hooks + Sync + Send + 'static> {
    fn clone(&self) -> Box<dyn Hooks + Sync + Send + 'static> {
        self.clone_box()
    }
}

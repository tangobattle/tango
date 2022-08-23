mod hooks;

use crate::games;

#[derive(Clone)]
pub struct EXE2;
impl games::Game for EXE2 {
    fn family(&self) -> &str {
        "exe2"
    }

    fn variant(&self) -> u32 {
        0
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0x41576087
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::AE2J_01.clone())
    }
}

#[derive(Clone)]
pub struct BN2;
impl games::Game for BN2 {
    fn family(&self) -> &str {
        "bn2"
    }

    fn variant(&self) -> u32 {
        0
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0x6d961f82
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::AE2E_00.clone())
    }
}

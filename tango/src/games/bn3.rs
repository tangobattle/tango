mod hooks;

use crate::games;

#[derive(Clone)]
pub struct EXE3W;
impl games::Game for EXE3W {
    fn family(&self) -> &str {
        "exe3"
    }

    fn variant(&self) -> u32 {
        0
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xe48e6bc9
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::A6BJ_01.clone())
    }
}

#[derive(Clone)]
pub struct EXE3B;
impl games::Game for EXE3B {
    fn family(&self) -> &str {
        "exe3"
    }

    fn variant(&self) -> u32 {
        1
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xfd57493b
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::A3XJ_01.clone())
    }
}

#[derive(Clone)]
pub struct BN3W;
impl games::Game for BN3W {
    fn family(&self) -> &str {
        "bn3"
    }

    fn variant(&self) -> u32 {
        0
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0x0be4410a
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::A6BE_00.clone())
    }
}

#[derive(Clone)]
pub struct BN3B;
impl games::Game for BN3B {
    fn family(&self) -> &str {
        "bn3"
    }

    fn variant(&self) -> u32 {
        1
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0xc0c780f9
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::A3XE_00.clone())
    }
}

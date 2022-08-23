mod hooks;

use crate::games;

#[derive(Clone)]

pub struct EXE4RS;
impl games::Game for EXE4RS {
    fn family(&self) -> &str {
        "exe4"
    }

    fn variant(&self) -> u32 {
        0
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xcf0e8b05
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::B4WJ_01.clone())
    }
}

#[derive(Clone)]

pub struct EXE4BM;
impl games::Game for EXE4BM {
    fn family(&self) -> &str {
        "exe4"
    }

    fn variant(&self) -> u32 {
        1
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xed7c5b50
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::B4BJ_01.clone())
    }
}

#[derive(Clone)]

pub struct BN4RS;
impl games::Game for BN4RS {
    fn family(&self) -> &str {
        "bn4"
    }

    fn variant(&self) -> u32 {
        0
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0x2120695c
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::B4WE_00.clone())
    }
}

#[derive(Clone)]

pub struct BN4BM;
impl games::Game for BN4BM {
    fn family(&self) -> &str {
        "bn4"
    }

    fn variant(&self) -> u32 {
        1
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0x758a46e9
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::B4BE_00.clone())
    }
}

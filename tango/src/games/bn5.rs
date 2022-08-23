mod hooks;

use crate::games;

#[derive(Clone)]
pub struct EXE5B;
impl games::Game for EXE5B {
    fn family(&self) -> &str {
        "exe5"
    }

    fn variant(&self) -> u32 {
        0
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xc73f23c0
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::BRBJ_00.clone())
    }
}

#[derive(Clone)]
pub struct EXE5C;
impl games::Game for EXE5C {
    fn family(&self) -> &str {
        "exe5"
    }

    fn variant(&self) -> u32 {
        1
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0x16842635
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::BRKJ_00.clone())
    }
}

#[derive(Clone)]
pub struct BN5P;
impl games::Game for BN5P {
    fn family(&self) -> &str {
        "bn5"
    }

    fn variant(&self) -> u32 {
        0
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0xa73e83a4
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::BRBE_00.clone())
    }
}

#[derive(Clone)]
pub struct BN5C;
impl games::Game for BN5C {
    fn family(&self) -> &str {
        "bn5"
    }

    fn variant(&self) -> u32 {
        1
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0xa552f683
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::BRKE_00.clone())
    }
}

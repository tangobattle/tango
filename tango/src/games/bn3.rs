mod hooks;

use crate::games;

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
        todo!()
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::A6BJ_01.clone())
    }
}

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
        todo!()
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::A3XJ_01.clone())
    }
}

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
        todo!()
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::A6BE_00.clone())
    }
}

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
        todo!()
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::A3XE_00.clone())
    }
}

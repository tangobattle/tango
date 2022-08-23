mod hooks;

use crate::games;

pub struct EXE6G;
impl games::Game for EXE6G {
    fn family(&self) -> &str {
        "exe6"
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
        Box::new(hooks::BR5J_00.clone())
    }
}

pub struct EXE6F;
impl games::Game for EXE6F {
    fn family(&self) -> &str {
        "exe6"
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
        Box::new(hooks::BR6J_00.clone())
    }
}

pub struct BN6G;
impl games::Game for BN6G {
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
        todo!()
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::BR5E_00.clone())
    }
}

pub struct BN6F;
impl games::Game for BN6F {
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
        todo!()
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::BR6E_00.clone())
    }
}

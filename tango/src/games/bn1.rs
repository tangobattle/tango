mod hooks;

use crate::games;

pub struct EXE1;
impl games::Game for EXE1 {
    fn family(&self) -> &str {
        "exe1"
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn variant(&self) -> u32 {
        0
    }

    fn expected_crc32(&self) -> u32 {
        0xd9516e50
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::AREJ_00.clone())
    }
}

pub struct BN1;
impl games::Game for BN1 {
    fn family(&self) -> &str {
        "bn1"
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn variant(&self) -> u32 {
        0
    }

    fn expected_crc32(&self) -> u32 {
        0x1d347971
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::AREE_00.clone())
    }
}

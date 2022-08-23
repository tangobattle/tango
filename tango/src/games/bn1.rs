mod hooks;

use crate::games;

struct EXE1Impl;
pub const EXE1: &'static (dyn games::Game + Send + Sync) = &EXE1Impl {};

impl games::Game for EXE1Impl {
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

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::AREJ_00
    }
}

struct BN1Impl;
pub const BN1: &'static (dyn games::Game + Send + Sync) = &BN1Impl {};

impl games::Game for BN1Impl {
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

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::AREE_00
    }
}

mod hooks;

use crate::games;

struct EXE45Impl;
pub const EXE45: &'static (dyn games::Game + Send + Sync) = &EXE45Impl {};

impl games::Game for EXE45Impl {
    fn family(&self) -> &str {
        "exe45"
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn variant(&self) -> u32 {
        0
    }

    fn expected_crc32(&self) -> u32 {
        0xa646601b
    }

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::BR4J_00
    }
}

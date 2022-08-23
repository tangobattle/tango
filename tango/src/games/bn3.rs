mod hooks;

use crate::games;

struct EXE3WImpl;
pub const EXE3W: &'static (dyn games::Game + Send + Sync) = &EXE3WImpl {};

impl games::Game for EXE3WImpl {
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

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::A6BJ_01
    }
}

struct EXE3BImpl;
pub const EXE3B: &'static (dyn games::Game + Send + Sync) = &EXE3BImpl {};

impl games::Game for EXE3BImpl {
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

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::A3XJ_01
    }
}

struct BN3WImpl;
pub const BN3W: &'static (dyn games::Game + Send + Sync) = &BN3WImpl {};

impl games::Game for BN3WImpl {
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

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::A6BE_00
    }
}

struct BN3BImpl;
pub const BN3B: &'static (dyn games::Game + Send + Sync) = &BN3BImpl {};

impl games::Game for BN3BImpl {
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

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::A3XE_00
    }
}

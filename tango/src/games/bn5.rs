mod hooks;

use crate::games;

struct EXE5BImpl;
pub const EXE5B: &'static (dyn games::Game + Send + Sync) = &EXE5BImpl {};

impl games::Game for EXE5BImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BRBJ", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u32) {
        ("exe5", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xc73f23c0
    }

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::BRBJ_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn games::Save>, anyhow::Error> {
        anyhow::bail!("not implemented");
    }
}

struct EXE5CImpl;
pub const EXE5C: &'static (dyn games::Game + Send + Sync) = &EXE5CImpl {};

impl games::Game for EXE5CImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BRKJ", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u32) {
        ("exe5", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0x16842635
    }

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::BRKJ_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn games::Save>, anyhow::Error> {
        anyhow::bail!("not implemented");
    }
}

struct BN5PImpl;
pub const BN5P: &'static (dyn games::Game + Send + Sync) = &BN5PImpl {};

impl games::Game for BN5PImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BRBE", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u32) {
        ("bn5", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0xa73e83a4
    }

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::BRBE_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn games::Save>, anyhow::Error> {
        anyhow::bail!("not implemented");
    }
}

struct BN5CImpl;
pub const BN5C: &'static (dyn games::Game + Send + Sync) = &BN5CImpl {};

impl games::Game for BN5CImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BRKE", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u32) {
        ("bn5", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0xa552f683
    }

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::BRKE_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn games::Save>, anyhow::Error> {
        anyhow::bail!("not implemented");
    }
}

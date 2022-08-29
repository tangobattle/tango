mod hooks;
mod save;

use crate::game;

const MATCH_TYPES: &[usize] = &[1];

struct EXE2Impl;
pub const EXE2: &'static (dyn game::Game + Send + Sync) = &EXE2Impl {};

impl game::Game for EXE2Impl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"AE2J", 0x01)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe2", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0x41576087
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::AE2J_01
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save>, anyhow::Error> {
        let save = save::Save::new(data)?;
        Ok(Box::new(save))
    }
}

pub struct BN2Impl;
pub const BN2: &'static (dyn game::Game + Send + Sync) = &BN2Impl {};

impl game::Game for BN2Impl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"AE2E", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn2", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0x6d961f82
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::AE2E_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save>, anyhow::Error> {
        let save = save::Save::new(data)?;
        Ok(Box::new(save))
    }
}

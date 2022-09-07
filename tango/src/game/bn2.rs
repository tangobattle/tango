mod hooks;
mod rom;
mod save;

use crate::{game, patch};

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

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::new(data)?))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::from_wram(data)?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        save: &[u8],
        overrides: &patch::ROMOverrides,
    ) -> Result<Box<dyn crate::rom::Assets + Send + Sync>, anyhow::Error> {
        let override_charset = overrides
            .charset
            .as_ref()
            .map(|charset| charset.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        Ok(Box::new(rom::Assets::new(
            &rom::AE2J_00,
            override_charset
                .as_ref()
                .map(|cs| cs.as_slice())
                .unwrap_or(&rom::JA_CHARSET),
            rom,
            save,
        )))
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

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::new(data)?))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::from_wram(data)?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        save: &[u8],
        overrides: &patch::ROMOverrides,
    ) -> Result<Box<dyn crate::rom::Assets + Send + Sync>, anyhow::Error> {
        let override_charset = overrides
            .charset
            .as_ref()
            .map(|charset| charset.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        Ok(Box::new(rom::Assets::new(
            &rom::AE2E_00,
            override_charset
                .as_ref()
                .map(|cs| cs.as_slice())
                .unwrap_or(&rom::EN_CHARSET),
            rom,
            save,
        )))
    }
}

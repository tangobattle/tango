mod hooks;
mod rom;
mod save;

use crate::{game, patch};

const MATCH_TYPES: &[usize] = &[4, 1];

struct EXE3WImpl;
pub const EXE3W: &'static (dyn game::Game + Send + Sync) = &EXE3WImpl {};

impl game::Game for EXE3WImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"A6BJ", 0x01)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe3", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xe48e6bc9
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::A6BJ_01
    }

    fn save_from_wram(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::from_wram(
            data,
            save::GameInfo {
                variant: save::Variant::White,
            },
        )?))
    }

    fn parse_save(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        let save = save::Save::new(data)?;
        if save.game_info()
            != &(save::GameInfo {
                variant: save::Variant::White,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &patch::SaveeditOverrides,
    ) -> Result<Box<dyn crate::rom::Assets + Send + Sync>, anyhow::Error> {
        let override_charset = overrides
            .charset
            .as_ref()
            .map(|charset| charset.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        Ok(Box::new(rom::Assets::new(
            &rom::A6BJ_01,
            override_charset
                .as_ref()
                .map(|cs| cs.as_slice())
                .unwrap_or(&rom::JA_CHARSET),
            rom,
            wram,
        )))
    }
}

struct EXE3BImpl;
pub const EXE3B: &'static (dyn game::Game + Send + Sync) = &EXE3BImpl {};

impl game::Game for EXE3BImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"A3XJ", 0x01)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe3", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xfd57493b
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::A3XJ_01
    }

    fn save_from_wram(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::from_wram(
            data,
            save::GameInfo {
                variant: save::Variant::Blue,
            },
        )?))
    }

    fn parse_save(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        let save = save::Save::new(data)?;
        if save.game_info()
            != &(save::GameInfo {
                variant: save::Variant::Blue,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &patch::SaveeditOverrides,
    ) -> Result<Box<dyn crate::rom::Assets + Send + Sync>, anyhow::Error> {
        let override_charset = overrides
            .charset
            .as_ref()
            .map(|charset| charset.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        Ok(Box::new(rom::Assets::new(
            &rom::A3XJ_01,
            override_charset
                .as_ref()
                .map(|cs| cs.as_slice())
                .unwrap_or(&rom::JA_CHARSET),
            rom,
            wram,
        )))
    }
}

struct BN3WImpl;
pub const BN3W: &'static (dyn game::Game + Send + Sync) = &BN3WImpl {};

impl game::Game for BN3WImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"A6BE", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn3", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0x0be4410a
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::A6BE_00
    }

    fn save_from_wram(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::from_wram(
            data,
            save::GameInfo {
                variant: save::Variant::White,
            },
        )?))
    }

    fn parse_save(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        let save = save::Save::new(data)?;
        if save.game_info()
            != &(save::GameInfo {
                variant: save::Variant::White,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &patch::SaveeditOverrides,
    ) -> Result<Box<dyn crate::rom::Assets + Send + Sync>, anyhow::Error> {
        let override_charset = overrides
            .charset
            .as_ref()
            .map(|charset| charset.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        Ok(Box::new(rom::Assets::new(
            &rom::A6BE_00,
            override_charset
                .as_ref()
                .map(|cs| cs.as_slice())
                .unwrap_or(&rom::EN_CHARSET),
            rom,
            wram,
        )))
    }
}

struct BN3BImpl;
pub const BN3B: &'static (dyn game::Game + Send + Sync) = &BN3BImpl {};

impl game::Game for BN3BImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"A3XE", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn3", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0xc0c780f9
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::A3XE_00
    }

    fn save_from_wram(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::from_wram(
            data,
            save::GameInfo {
                variant: save::Variant::Blue,
            },
        )?))
    }

    fn parse_save(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        let save = save::Save::new(data)?;
        if save.game_info()
            != &(save::GameInfo {
                variant: save::Variant::Blue,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &patch::SaveeditOverrides,
    ) -> Result<Box<dyn crate::rom::Assets + Send + Sync>, anyhow::Error> {
        let override_charset = overrides
            .charset
            .as_ref()
            .map(|charset| charset.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        Ok(Box::new(rom::Assets::new(
            &rom::A3XE_00,
            override_charset
                .as_ref()
                .map(|cs| cs.as_slice())
                .unwrap_or(&rom::EN_CHARSET),
            rom,
            wram,
        )))
    }
}

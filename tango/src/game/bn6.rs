mod hooks;
mod rom;
mod save;

use crate::{game, patch};

const MATCH_TYPES: &[usize] = &[1, 1];

struct EXE6GImpl;
pub const EXE6G: &'static (dyn game::Game + Send + Sync) = &EXE6GImpl {};

impl game::Game for EXE6GImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BR5J", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe6", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0x6285918a
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::BR5J_00
    }

    fn parse_save(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        let save = save::Save::new(data)?;
        if save.game_info()
            != &(save::GameInfo {
                region: save::Region::JP,
                variant: save::Variant::Gregar,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::from_wram(
            data,
            save::GameInfo {
                region: save::Region::JP,
                variant: save::Variant::Gregar,
            },
        )?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &patch::ROMOverrides,
    ) -> Result<Box<dyn crate::rom::Assets + Send + Sync>, anyhow::Error> {
        let override_charset = overrides
            .charset
            .as_ref()
            .map(|charset| charset.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        Ok(Box::new(rom::Assets::new(
            &rom::BR5J_00,
            rom,
            wram,
            rom::AssetLoadOptions {
                charset: override_charset
                    .as_ref()
                    .map(|cs| cs.as_slice())
                    .unwrap_or(&rom::JA_CHARSET),
                chip_names: &overrides.chip_names,
                navicust_part_names: &overrides.navicust_part_names,
                modcard56_names: &overrides.modcard56_names,
                modcard56_effect_names: &overrides.modcard56_effect_names,
            },
        )))
    }
}

struct EXE6FImpl;
pub const EXE6F: &'static (dyn game::Game + Send + Sync) = &EXE6FImpl {};

impl game::Game for EXE6FImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BR6J", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe6", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0x2dfb603e
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::BR6J_00
    }

    fn parse_save(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        let save = save::Save::new(data)?;
        if save.game_info()
            != &(save::GameInfo {
                region: save::Region::JP,
                variant: save::Variant::Falzar,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::from_wram(
            data,
            save::GameInfo {
                region: save::Region::JP,
                variant: save::Variant::Falzar,
            },
        )?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &patch::ROMOverrides,
    ) -> Result<Box<dyn crate::rom::Assets + Send + Sync>, anyhow::Error> {
        let override_charset = overrides
            .charset
            .as_ref()
            .map(|charset| charset.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        Ok(Box::new(rom::Assets::new(
            &rom::BR6J_00,
            rom,
            wram,
            rom::AssetLoadOptions {
                charset: override_charset
                    .as_ref()
                    .map(|cs| cs.as_slice())
                    .unwrap_or(&rom::JA_CHARSET),
                chip_names: &overrides.chip_names,
                navicust_part_names: &overrides.navicust_part_names,
                modcard56_names: &overrides.modcard56_names,
                modcard56_effect_names: &overrides.modcard56_effect_names,
            },
        )))
    }
}

struct BN6GImpl;
pub const BN6G: &'static (dyn game::Game + Send + Sync) = &BN6GImpl {};

impl game::Game for BN6GImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BR5E", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn6", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0x79452182
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::BR5E_00
    }

    fn parse_save(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        let save = save::Save::new(data)?;
        if save.game_info()
            != &(save::GameInfo {
                region: save::Region::US,
                variant: save::Variant::Gregar,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::from_wram(
            data,
            save::GameInfo {
                region: save::Region::US,
                variant: save::Variant::Gregar,
            },
        )?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &patch::ROMOverrides,
    ) -> Result<Box<dyn crate::rom::Assets + Send + Sync>, anyhow::Error> {
        let override_charset = overrides
            .charset
            .as_ref()
            .map(|charset| charset.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        Ok(Box::new(rom::Assets::new(
            &rom::BR5E_00,
            rom,
            wram,
            rom::AssetLoadOptions {
                charset: override_charset
                    .as_ref()
                    .map(|cs| cs.as_slice())
                    .unwrap_or(&rom::EN_CHARSET),
                chip_names: &overrides.chip_names,
                navicust_part_names: &overrides.navicust_part_names,
                modcard56_names: &overrides.modcard56_names,
                modcard56_effect_names: &overrides.modcard56_effect_names,
            },
        )))
    }
}

struct BN6FImpl;
pub const BN6F: &'static (dyn game::Game + Send + Sync) = &BN6FImpl {};

impl game::Game for BN6FImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BR6E", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn6", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0xdee6f2a9
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::BR6E_00
    }

    fn parse_save(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        let save = save::Save::new(data)?;
        if save.game_info()
            != &(save::GameInfo {
                region: save::Region::US,
                variant: save::Variant::Falzar,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(
        &self,
        data: &[u8],
    ) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::from_wram(
            data,
            save::GameInfo {
                region: save::Region::US,
                variant: save::Variant::Falzar,
            },
        )?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &patch::ROMOverrides,
    ) -> Result<Box<dyn crate::rom::Assets + Send + Sync>, anyhow::Error> {
        let override_charset = overrides
            .charset
            .as_ref()
            .map(|charset| charset.iter().map(|s| s.as_str()).collect::<Vec<_>>());

        Ok(Box::new(rom::Assets::new(
            &rom::BR6E_00,
            rom,
            wram,
            rom::AssetLoadOptions {
                charset: override_charset
                    .as_ref()
                    .map(|cs| cs.as_slice())
                    .unwrap_or(&rom::EN_CHARSET),
                chip_names: &overrides.chip_names,
                navicust_part_names: &overrides.navicust_part_names,
                modcard56_names: &overrides.modcard56_names,
                modcard56_effect_names: &overrides.modcard56_effect_names,
            },
        )))
    }
}

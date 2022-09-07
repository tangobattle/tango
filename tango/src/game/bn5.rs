mod hooks;
mod rom;
mod save;

use crate::{game, patch};

const MATCH_TYPES: &[usize] = &[2, 2];

struct EXE5BImpl;
pub const EXE5B: &'static (dyn game::Game + Send + Sync) = &EXE5BImpl {};

impl game::Game for EXE5BImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BRBJ", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe5", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xc73f23c0
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::BRBJ_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        let save = save::Save::new(data)?;
        if save.game_info()
            != &(save::GameInfo {
                region: save::Region::JP,
                variant: save::Variant::Protoman,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::from_wram(
            data,
            save::GameInfo {
                region: save::Region::JP,
                variant: save::Variant::Protoman,
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
            &rom::BRBJ_00,
            override_charset
                .as_ref()
                .map(|cs| cs.as_slice())
                .unwrap_or(&rom::JA_CHARSET),
            rom,
            wram,
        )))
    }
}

struct EXE5CImpl;
pub const EXE5C: &'static (dyn game::Game + Send + Sync) = &EXE5CImpl {};

impl game::Game for EXE5CImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BRKJ", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe5", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0x16842635
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::BRKJ_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        let save = save::Save::new(data)?;
        if save.game_info()
            != &(save::GameInfo {
                region: save::Region::JP,
                variant: save::Variant::Colonel,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::from_wram(
            data,
            save::GameInfo {
                region: save::Region::JP,
                variant: save::Variant::Colonel,
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
            &rom::BRKJ_00,
            override_charset
                .as_ref()
                .map(|cs| cs.as_slice())
                .unwrap_or(&rom::JA_CHARSET),
            rom,
            wram,
        )))
    }
}

struct BN5PImpl;
pub const BN5P: &'static (dyn game::Game + Send + Sync) = &BN5PImpl {};

impl game::Game for BN5PImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BRBE", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn5", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0xa73e83a4
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::BRBE_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        let save = save::Save::new(data)?;
        if save.game_info()
            != &(save::GameInfo {
                region: save::Region::US,
                variant: save::Variant::Protoman,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::from_wram(
            data,
            save::GameInfo {
                region: save::Region::US,
                variant: save::Variant::Protoman,
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
            &rom::BRBE_00,
            override_charset
                .as_ref()
                .map(|cs| cs.as_slice())
                .unwrap_or(&rom::EN_CHARSET),
            rom,
            wram,
        )))
    }
}

struct BN5CImpl;
pub const BN5C: &'static (dyn game::Game + Send + Sync) = &BN5CImpl {};

impl game::Game for BN5CImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BRKE", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn5", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0xa552f683
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::BRKE_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        let save = save::Save::new(data)?;
        if save.game_info()
            != &(save::GameInfo {
                region: save::Region::US,
                variant: save::Variant::Colonel,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::from_wram(
            data,
            save::GameInfo {
                region: save::Region::US,
                variant: save::Variant::Colonel,
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
            &rom::BRKE_00,
            override_charset
                .as_ref()
                .map(|cs| cs.as_slice())
                .unwrap_or(&rom::EN_CHARSET),
            rom,
            wram,
        )))
    }
}

mod hooks;
mod rom;
mod save;

use crate::game;

const MATCH_TYPES: &[usize] = &[1];

struct EXE1Impl;
pub const EXE1: &'static (dyn game::Game + Send + Sync) = &EXE1Impl {};

impl game::Game for EXE1Impl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"AREJ", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe1", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xd9516e50
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::AREJ_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        let save = save::Save::new(data)?;
        if save.game_info()
            != &(save::GameInfo {
                region: save::Region::JP,
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
            },
        )?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn crate::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(rom::Assets::new(
            &rom::AREJ_00,
            overrides
                .charset
                .as_ref()
                .cloned()
                .unwrap_or_else(|| rom::JA_CHARSET.iter().map(|s| s.to_string()).collect()),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn crate::save::Save + Send + Sync))] {
        lazy_static! {
            static ref SAVE: save::Save = save::Save::from_raw(
                include_bytes!("bn1/save/jp.raw").clone(),
                save::GameInfo {
                    region: save::Region::JP,
                }
            );
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn crate::save::Save + Send + Sync))> =
                vec![("", &*SAVE as &(dyn crate::save::Save + Send + Sync),)];
        }
        TEMPLATES.as_slice()
    }
}

struct BN1Impl;
pub const BN1: &'static (dyn game::Game + Send + Sync) = &BN1Impl {};

impl game::Game for BN1Impl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"AREE", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn1", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0x1d347971
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::AREE_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        let save = save::Save::new(data)?;
        if save.game_info()
            != &(save::GameInfo {
                region: save::Region::US,
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
            },
        )?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn crate::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(rom::Assets::new(
            &rom::AREE_00,
            overrides
                .charset
                .as_ref()
                .cloned()
                .unwrap_or_else(|| rom::EN_CHARSET.iter().map(|s| s.to_string()).collect()),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn crate::save::Save + Send + Sync))] {
        lazy_static! {
            static ref SAVE: save::Save = save::Save::from_raw(
                include_bytes!("bn1/save/us.raw").clone(),
                save::GameInfo {
                    region: save::Region::JP,
                }
            );
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn crate::save::Save + Send + Sync))> =
                vec![("", &*SAVE as &(dyn crate::save::Save + Send + Sync),)];
        }
        TEMPLATES.as_slice()
    }
}

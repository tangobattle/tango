mod hooks;
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

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save>, anyhow::Error> {
        let save = save::Save::new(data)?;
        let game_info = save.game_info().unwrap();
        if game_info
            != (save::GameInfo {
                region: save::Region::JP,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
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

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save>, anyhow::Error> {
        let save = save::Save::new(data)?;
        let game_info = save.game_info().unwrap();
        if game_info
            != (save::GameInfo {
                region: save::Region::US,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
    }
}

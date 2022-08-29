mod hooks;
mod save;

use crate::game;

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
        let game_info = save.game_info().unwrap();
        if game_info
            != (save::GameInfo {
                region: save::Region::JP,
                variant: save::Variant::Gregar,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
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
        let game_info = save.game_info().unwrap();
        if game_info
            != (save::GameInfo {
                region: save::Region::JP,
                variant: save::Variant::Falzar,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
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
        let game_info = save.game_info().unwrap();
        if game_info
            != (save::GameInfo {
                region: save::Region::US,
                variant: save::Variant::Gregar,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
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
        let game_info = save.game_info().unwrap();
        if game_info
            != (save::GameInfo {
                region: save::Region::US,
                variant: save::Variant::Falzar,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
    }
}

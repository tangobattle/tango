mod hooks;
mod save;

use crate::games;

struct EXE3WImpl;
pub const EXE3W: &'static (dyn games::Game + Send + Sync) = &EXE3WImpl {};

impl games::Game for EXE3WImpl {
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

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::A6BJ_01
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn games::Save>, anyhow::Error> {
        let save = save::Save::new(data)?;
        let game_info = save.game_info();
        if *game_info
            != (save::GameInfo {
                variant: save::Variant::White,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
    }
}

struct EXE3BImpl;
pub const EXE3B: &'static (dyn games::Game + Send + Sync) = &EXE3BImpl {};

impl games::Game for EXE3BImpl {
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

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::A3XJ_01
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn games::Save>, anyhow::Error> {
        let save = save::Save::new(data)?;
        let game_info = save.game_info();
        if *game_info
            != (save::GameInfo {
                variant: save::Variant::Blue,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
    }
}

struct BN3WImpl;
pub const BN3W: &'static (dyn games::Game + Send + Sync) = &BN3WImpl {};

impl games::Game for BN3WImpl {
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

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::A6BE_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn games::Save>, anyhow::Error> {
        let save = save::Save::new(data)?;
        let game_info = save.game_info();
        if *game_info
            != (save::GameInfo {
                variant: save::Variant::White,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
    }
}

struct BN3BImpl;
pub const BN3B: &'static (dyn games::Game + Send + Sync) = &BN3BImpl {};

impl games::Game for BN3BImpl {
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

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::A3XE_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn games::Save>, anyhow::Error> {
        let save = save::Save::new(data)?;
        let game_info = save.game_info();
        if *game_info
            != (save::GameInfo {
                variant: save::Variant::Blue,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
    }
}

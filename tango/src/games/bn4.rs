mod hooks;
mod save;

use crate::games;

struct EXE4RSImpl;
pub const EXE4RS: &'static (dyn games::Game + Send + Sync) = &EXE4RSImpl {};

impl games::Game for EXE4RSImpl {
    fn family(&self) -> &str {
        "exe4"
    }

    fn variant(&self) -> u32 {
        0
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xcf0e8b05
    }

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::B4WJ_01
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn games::Save>, anyhow::Error> {
        let save = save::Save::new(data)?;
        let game_info = save.game_info();
        if game_info.variant != save::Variant::RedSun
            || (game_info.region != save::Region::JP && game_info.region != save::Region::Any)
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
    }
}

struct EXE4BMImpl;
pub const EXE4BM: &'static (dyn games::Game + Send + Sync) = &EXE4BMImpl {};

impl games::Game for EXE4BMImpl {
    fn family(&self) -> &str {
        "exe4"
    }

    fn variant(&self) -> u32 {
        1
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xed7c5b50
    }

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::B4BJ_01
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn games::Save>, anyhow::Error> {
        let save = save::Save::new(data)?;
        let game_info = save.game_info();
        if game_info.variant != save::Variant::BlueMoon
            || (game_info.region != save::Region::JP && game_info.region != save::Region::Any)
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
    }
}

struct BN4RSImpl;
pub const BN4RS: &'static (dyn games::Game + Send + Sync) = &BN4RSImpl {};

impl games::Game for BN4RSImpl {
    fn family(&self) -> &str {
        "bn4"
    }

    fn variant(&self) -> u32 {
        0
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0x2120695c
    }

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::B4WE_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn games::Save>, anyhow::Error> {
        let save = save::Save::new(data)?;
        let game_info = save.game_info();
        if game_info.variant != save::Variant::RedSun
            || (game_info.region != save::Region::US && game_info.region != save::Region::Any)
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
    }
}

struct BN4BMImpl;
pub const BN4BM: &'static (dyn games::Game + Send + Sync) = &BN4BMImpl {};

impl games::Game for BN4BMImpl {
    fn family(&self) -> &str {
        "bn4"
    }

    fn variant(&self) -> u32 {
        1
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0x758a46e9
    }

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::B4BE_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn games::Save>, anyhow::Error> {
        let save = save::Save::new(data)?;
        let game_info = save.game_info();
        if game_info.variant != save::Variant::BlueMoon
            || (game_info.region != save::Region::US && game_info.region != save::Region::Any)
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
    }
}

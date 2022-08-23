mod hooks;
mod save;

use crate::games;

struct EXE6GImpl;
pub const EXE6G: &'static (dyn games::Game + Send + Sync) = &EXE6GImpl {};

impl games::Game for EXE6GImpl {
    fn family(&self) -> &str {
        "exe6"
    }

    fn variant(&self) -> u32 {
        0
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0x6285918a
    }

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::BR5J_00
    }

    fn parse_save(&self, data: Vec<u8>) -> Result<Box<dyn games::Save>, anyhow::Error> {
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
pub const EXE6F: &'static (dyn games::Game + Send + Sync) = &EXE6FImpl {};

impl games::Game for EXE6FImpl {
    fn family(&self) -> &str {
        "exe6"
    }

    fn variant(&self) -> u32 {
        1
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0x2dfb603e
    }

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::BR6J_00
    }

    fn parse_save(&self, data: Vec<u8>) -> Result<Box<dyn games::Save>, anyhow::Error> {
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
pub const BN6G: &'static (dyn games::Game + Send + Sync) = &BN6GImpl {};

impl games::Game for BN6GImpl {
    fn family(&self) -> &str {
        "bn6"
    }

    fn variant(&self) -> u32 {
        0
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0x79452182
    }

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::BR5E_00
    }

    fn parse_save(&self, data: Vec<u8>) -> Result<Box<dyn games::Save>, anyhow::Error> {
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
pub const BN6F: &'static (dyn games::Game + Send + Sync) = &BN6FImpl {};

impl games::Game for BN6FImpl {
    fn family(&self) -> &str {
        "bn6"
    }

    fn variant(&self) -> u32 {
        1
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0xdee6f2a9
    }

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::BR6E_00
    }

    fn parse_save(&self, data: Vec<u8>) -> Result<Box<dyn games::Save>, anyhow::Error> {
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

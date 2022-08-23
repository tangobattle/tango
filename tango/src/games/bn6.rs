mod hooks;
mod save;

use crate::games;

#[derive(Clone)]
pub struct EXE6G;
impl games::Game for EXE6G {
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

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::BR5J_00.clone())
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

#[derive(Clone)]
pub struct EXE6F;
impl games::Game for EXE6F {
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

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::BR6J_00.clone())
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

#[derive(Clone)]
pub struct BN6G;
impl games::Game for BN6G {
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

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::BR5E_00.clone())
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

#[derive(Clone)]
pub struct BN6F;
impl games::Game for BN6F {
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

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::BR6E_00.clone())
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

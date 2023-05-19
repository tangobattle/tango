mod hooks;

use crate::game;

const MATCH_TYPES: &[usize] = &[2, 2];

struct EXE4RSImpl;
pub const EXE4RS: &'static (dyn game::Game + Send + Sync) = &EXE4RSImpl {};

impl game::Game for EXE4RSImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"B4WJ", 0x01)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe4", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xcf0e8b05
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::B4WJ_01
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn4::save::Save::new(data)?;
        let game_info = save.game_info();
        if game_info.variant != tango_dataview::game::bn4::save::Variant::RedSun
            || (game_info.region != tango_dataview::game::bn4::save::Region::JP
                && game_info.region != tango_dataview::game::bn4::save::Region::Any)
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn4::save::Save::from_wram(
            data,
            tango_dataview::game::bn4::save::GameInfo {
                region: tango_dataview::game::bn4::save::Region::JP,
                variant: tango_dataview::game::bn4::save::Variant::RedSun,
            },
        )?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn tango_dataview::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(crate::rom::OverridenAssets::new(
            tango_dataview::game::bn4::rom::Assets::new(
                &tango_dataview::game::bn4::rom::B4WJ_01,
                overrides
                    .language
                    .as_ref()
                    .and_then(|lang| tango_dataview::game::bn4::rom::patch_cards::for_language(lang))
                    .unwrap_or(&tango_dataview::game::bn4::rom::patch_cards::JA_PATCH_CARDS),
                overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn4::rom::JA_CHARSET
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                }),
                rom.to_vec(),
                wram.to_vec(),
            ),
            overrides,
        )))
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_HP_997_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/dark_hp_997_rs_jp.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region::JP,
                        variant: tango_dataview::game::bn4::save::Variant::RedSun
                    }
                )
                .unwrap();
            static ref LIGHT_HP_999_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_999_rs_jp.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region::JP,
                        variant: tango_dataview::game::bn4::save::Variant::RedSun
                    }
                )
                .unwrap();
            static ref LIGHT_HP_1000_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_1000_rs_jp.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region::JP,
                        variant: tango_dataview::game::bn4::save::Variant::RedSun
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                (
                    "dark-hp-997",
                    &*DARK_HP_997_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-999",
                    &*LIGHT_HP_999_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-1000",
                    &*LIGHT_HP_1000_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
            ];
        }
        TEMPLATES.as_slice()
    }
}

struct EXE4BMImpl;
pub const EXE4BM: &'static (dyn game::Game + Send + Sync) = &EXE4BMImpl {};

impl game::Game for EXE4BMImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"B4BJ", 0x01)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe4", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0x709bbf07
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::B4BJ_01
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn4::save::Save::new(data)?;
        let game_info = save.game_info();
        if game_info.variant != tango_dataview::game::bn4::save::Variant::BlueMoon
            || (game_info.region != tango_dataview::game::bn4::save::Region::JP
                && game_info.region != tango_dataview::game::bn4::save::Region::Any)
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn4::save::Save::from_wram(
            data,
            tango_dataview::game::bn4::save::GameInfo {
                region: tango_dataview::game::bn4::save::Region::JP,
                variant: tango_dataview::game::bn4::save::Variant::BlueMoon,
            },
        )?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn tango_dataview::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(crate::rom::OverridenAssets::new(
            tango_dataview::game::bn4::rom::Assets::new(
                &tango_dataview::game::bn4::rom::B4BJ_01,
                overrides
                    .language
                    .as_ref()
                    .and_then(|lang| tango_dataview::game::bn4::rom::patch_cards::for_language(lang))
                    .unwrap_or(&tango_dataview::game::bn4::rom::patch_cards::JA_PATCH_CARDS),
                overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn4::rom::JA_CHARSET
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                }),
                rom.to_vec(),
                wram.to_vec(),
            ),
            overrides,
        )))
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_HP_997_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/dark_hp_997_bm_jp.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region::JP,
                        variant: tango_dataview::game::bn4::save::Variant::RedSun
                    }
                )
                .unwrap();
            static ref LIGHT_HP_999_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_999_bm_jp.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region::JP,
                        variant: tango_dataview::game::bn4::save::Variant::RedSun
                    }
                )
                .unwrap();
            static ref LIGHT_HP_1000_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_1000_bm_jp.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region::JP,
                        variant: tango_dataview::game::bn4::save::Variant::RedSun
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                (
                    "dark-hp-997",
                    &*DARK_HP_997_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-999",
                    &*LIGHT_HP_999_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-1000",
                    &*LIGHT_HP_1000_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
            ];
        }
        TEMPLATES.as_slice()
    }
}

struct BN4RSImpl;
pub const BN4RS: &'static (dyn game::Game + Send + Sync) = &BN4RSImpl {};

impl game::Game for BN4RSImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"B4WE", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn4", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0x2120695c
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::B4WE_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn4::save::Save::new(data)?;
        let game_info = save.game_info();
        if game_info.variant != tango_dataview::game::bn4::save::Variant::RedSun
            || (game_info.region != tango_dataview::game::bn4::save::Region::US
                && game_info.region != tango_dataview::game::bn4::save::Region::Any)
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn4::save::Save::from_wram(
            data,
            tango_dataview::game::bn4::save::GameInfo {
                region: tango_dataview::game::bn4::save::Region::US,
                variant: tango_dataview::game::bn4::save::Variant::RedSun,
            },
        )?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn tango_dataview::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(crate::rom::OverridenAssets::new(
            tango_dataview::game::bn4::rom::Assets::new(
                &tango_dataview::game::bn4::rom::B4WE_00,
                overrides
                    .language
                    .as_ref()
                    .and_then(|lang| tango_dataview::game::bn4::rom::patch_cards::for_language(lang))
                    .unwrap_or(&tango_dataview::game::bn4::rom::patch_cards::EN_PATCH_CARDS),
                overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn4::rom::EN_CHARSET
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                }),
                rom.to_vec(),
                wram.to_vec(),
            ),
            overrides,
        )))
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_HP_997_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/dark_hp_997_rs_us.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region::US,
                        variant: tango_dataview::game::bn4::save::Variant::RedSun
                    }
                )
                .unwrap();
            static ref LIGHT_HP_999_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_999_rs_us.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region::US,
                        variant: tango_dataview::game::bn4::save::Variant::RedSun
                    }
                )
                .unwrap();
            static ref LIGHT_HP_1000_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_1000_rs_us.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region::US,
                        variant: tango_dataview::game::bn4::save::Variant::RedSun
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                (
                    "dark-hp-997",
                    &*DARK_HP_997_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-999",
                    &*LIGHT_HP_999_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-1000",
                    &*LIGHT_HP_1000_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
            ];
        }
        TEMPLATES.as_slice()
    }
}

struct BN4BMImpl;
pub const BN4BM: &'static (dyn game::Game + Send + Sync) = &BN4BMImpl {};

impl game::Game for BN4BMImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"B4BE", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn4", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0x758a46e9
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::B4BE_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn4::save::Save::new(data)?;
        let game_info = save.game_info();
        if game_info.variant != tango_dataview::game::bn4::save::Variant::BlueMoon
            || (game_info.region != tango_dataview::game::bn4::save::Region::US
                && game_info.region != tango_dataview::game::bn4::save::Region::Any)
        {
            anyhow::bail!("save is not compatible: got {:?}", game_info);
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn4::save::Save::from_wram(
            data,
            tango_dataview::game::bn4::save::GameInfo {
                region: tango_dataview::game::bn4::save::Region::US,
                variant: tango_dataview::game::bn4::save::Variant::BlueMoon,
            },
        )?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn tango_dataview::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(crate::rom::OverridenAssets::new(
            tango_dataview::game::bn4::rom::Assets::new(
                &tango_dataview::game::bn4::rom::B4BE_00,
                overrides
                    .language
                    .as_ref()
                    .and_then(|lang| tango_dataview::game::bn4::rom::patch_cards::for_language(lang))
                    .unwrap_or(&tango_dataview::game::bn4::rom::patch_cards::EN_PATCH_CARDS),
                overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn4::rom::EN_CHARSET
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                }),
                rom.to_vec(),
                wram.to_vec(),
            ),
            overrides,
        )))
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_HP_997_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/dark_hp_997_bm_us.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region::US,
                        variant: tango_dataview::game::bn4::save::Variant::BlueMoon
                    }
                )
                .unwrap();
            static ref LIGHT_HP_999_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_999_bm_us.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region::US,
                        variant: tango_dataview::game::bn4::save::Variant::BlueMoon
                    }
                )
                .unwrap();
            static ref LIGHT_HP_1000_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_1000_bm_us.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region::US,
                        variant: tango_dataview::game::bn4::save::Variant::BlueMoon
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                (
                    "dark-hp-997",
                    &*DARK_HP_997_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-999",
                    &*LIGHT_HP_999_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-1000",
                    &*LIGHT_HP_1000_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
            ];
        }
        TEMPLATES.as_slice()
    }
}

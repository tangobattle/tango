mod hooks;

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

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn1::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn1::save::GameInfo {
                region: tango_dataview::game::bn1::save::Region::JP,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn1::save::Save::from_wram(
            data,
            tango_dataview::game::bn1::save::GameInfo {
                region: tango_dataview::game::bn1::save::Region::JP,
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
            tango_dataview::game::bn1::rom::Assets::new(
                &tango_dataview::game::bn1::rom::AREJ_00,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn1::rom::JA_CHARSET
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
            static ref SAVE: tango_dataview::game::bn1::save::Save = tango_dataview::game::bn1::save::Save::from_wram(
                include_bytes!("bn1/save/jp.raw"),
                tango_dataview::game::bn1::save::GameInfo {
                    region: tango_dataview::game::bn1::save::Region::JP,
                }
            )
            .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> =
                vec![("", &*SAVE as &(dyn tango_dataview::save::Save + Send + Sync))];
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

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn1::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn1::save::GameInfo {
                region: tango_dataview::game::bn1::save::Region::US,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn1::save::Save::from_wram(
            data,
            tango_dataview::game::bn1::save::GameInfo {
                region: tango_dataview::game::bn1::save::Region::US,
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
            tango_dataview::game::bn1::rom::Assets::new(
                &tango_dataview::game::bn1::rom::AREE_00,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn1::rom::EN_CHARSET
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
            static ref SAVE: tango_dataview::game::bn1::save::Save = tango_dataview::game::bn1::save::Save::from_wram(
                include_bytes!("bn1/save/us.raw"),
                tango_dataview::game::bn1::save::GameInfo {
                    region: tango_dataview::game::bn1::save::Region::US,
                }
            )
            .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> =
                vec![("", &*SAVE as &(dyn tango_dataview::save::Save + Send + Sync))];
        }
        TEMPLATES.as_slice()
    }
}

mod hooks;
mod rom;
mod save;

use crate::game;

const MATCH_TYPES: &[usize] = &[1, 1];

struct EXE45Impl;
pub const EXE45: &'static (dyn game::Game + Send + Sync) = &EXE45Impl {};

impl game::Game for EXE45Impl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BR4J", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe45", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xa646601b
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::BR4J_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::new(data)?))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn crate::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(save::Save::from_wram(data)?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn crate::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(rom::Assets::new(
            &rom::BR4J_00,
            overrides
                .charset
                .as_ref()
                .cloned()
                .unwrap_or_else(|| rom::CHARSET.iter().map(|s| s.to_string()).collect()),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn crate::save::Save + Send + Sync))] {
        lazy_static! {
            static ref SAVE: save::Save = save::Save::from_wram(include_bytes!("exe45/save/any.raw")).unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn crate::save::Save + Send + Sync))> =
                vec![("", &*SAVE as &(dyn crate::save::Save + Send + Sync))];
        }
        TEMPLATES.as_slice()
    }
}

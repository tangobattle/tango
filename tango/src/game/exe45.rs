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

    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::exe45::BR4J_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::exe45::save::Save::new(data)?))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::exe45::save::Save::from_wram(data)?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn tango_dataview::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(crate::rom::OverridenAssets::new(
            tango_dataview::game::exe45::rom::Assets::new(
                &tango_dataview::game::exe45::rom::BR4J_00,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::exe45::rom::CHARSET
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
            static ref SAVE: tango_dataview::game::exe45::save::Save =
                tango_dataview::game::exe45::save::Save::from_wram(include_bytes!("exe45/save/any.raw")).unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> =
                vec![("", &*SAVE as &(dyn tango_dataview::save::Save + Send + Sync))];
        }
        TEMPLATES.as_slice()
    }
}

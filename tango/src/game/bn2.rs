use crate::game;

const MATCH_TYPES: &[usize] = &[1];

struct EXE2Impl;
pub const EXE2: &'static (dyn game::Game + Send + Sync) = &EXE2Impl {};

lazy_static! {
    static ref HUB_ANY_SAVE: tango_dataview::game::bn2::save::Save =
        tango_dataview::game::bn2::save::Save::from_wram(include_bytes!("bn2/save/hub_any.raw")).unwrap();
    static ref GUTS_ANY_SAVE: tango_dataview::game::bn2::save::Save =
        tango_dataview::game::bn2::save::Save::from_wram(include_bytes!("bn2/save/guts_any.raw")).unwrap();
    static ref CUSTOM_ANY_SAVE: tango_dataview::game::bn2::save::Save =
        tango_dataview::game::bn2::save::Save::from_wram(include_bytes!("bn2/save/custom_any.raw")).unwrap();
    static ref TEAM_ANY_SAVE: tango_dataview::game::bn2::save::Save =
        tango_dataview::game::bn2::save::Save::from_wram(include_bytes!("bn2/save/team_any.raw")).unwrap();
    static ref SHIELD_ANY_SAVE: tango_dataview::game::bn2::save::Save =
        tango_dataview::game::bn2::save::Save::from_wram(include_bytes!("bn2/save/shield_any.raw")).unwrap();
    static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
        ("hub", &*HUB_ANY_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
        (
            "guts",
            &*GUTS_ANY_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
        ),
        (
            "custom",
            &*CUSTOM_ANY_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
        ),
        (
            "team",
            &*TEAM_ANY_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
        ),
        (
            "shield",
            &*SHIELD_ANY_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
        ),
    ];
}

impl game::Game for EXE2Impl {
    fn gamedb_entry(&self) -> &tango_gamedb::Game {
        &tango_gamedb::AE2J_00_AC
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn2::AE2J_00_AC
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn2::save::Save::new(data)?))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn2::save::Save::from_wram(data)?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        save: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn tango_dataview::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(crate::rom::OverridenAssets::new(
            tango_dataview::game::bn2::rom::Assets::new(
                &tango_dataview::game::bn2::rom::AE2J_00_AC,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn2::rom::JA_CHARSET
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                }),
                rom.to_vec(),
                save.to_vec(),
            ),
            overrides,
        )))
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        TEMPLATES.as_slice()
    }
}

pub struct BN2Impl;
pub const BN2: &'static (dyn game::Game + Send + Sync) = &BN2Impl {};

impl game::Game for BN2Impl {
    fn gamedb_entry(&self) -> &tango_gamedb::Game {
        &tango_gamedb::AE2E_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn2::AE2E_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn2::save::Save::new(data)?))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn2::save::Save::from_wram(data)?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        save: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn tango_dataview::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(crate::rom::OverridenAssets::new(
            tango_dataview::game::bn2::rom::Assets::new(
                &tango_dataview::game::bn2::rom::AE2E_00,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn2::rom::EN_CHARSET
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                }),
                rom.to_vec(),
                save.to_vec(),
            ),
            overrides,
        )))
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        TEMPLATES.as_slice()
    }
}

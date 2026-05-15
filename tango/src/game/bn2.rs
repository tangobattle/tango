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
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::AE2J_00_AC
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        TEMPLATES.as_slice()
    }
}

pub struct BN2Impl;
pub const BN2: &'static (dyn game::Game + Send + Sync) = &BN2Impl {};

impl game::Game for BN2Impl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::AE2E_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        TEMPLATES.as_slice()
    }
}

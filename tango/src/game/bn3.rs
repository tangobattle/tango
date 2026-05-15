use crate::game;

const MATCH_TYPES: &[usize] = &[4, 1];

lazy_static! {
    static ref HEAT_GUTS_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_guts_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref AQUA_GUTS_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_guts_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref ELEC_GUTS_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_guts_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref WOOD_GUTS_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_guts_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref HEAT_CUSTOM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_custom_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref AQUA_CUSTOM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_custom_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref ELEC_CUSTOM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_custom_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref WOOD_CUSTOM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_custom_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref HEAT_SHIELD_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_shield_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref AQUA_SHIELD_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_shield_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref ELEC_SHIELD_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_shield_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref WOOD_SHIELD_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_shield_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref HEAT_TEAM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_team_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref AQUA_TEAM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_team_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref ELEC_TEAM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_team_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref WOOD_TEAM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_team_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref HEAT_GROUND_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_ground_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref AQUA_GROUND_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_ground_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref ELEC_GROUND_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_ground_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref WOOD_GROUND_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_ground_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref HEAT_BUG_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_bug_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref AQUA_BUG_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_bug_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref ELEC_BUG_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_bug_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref WOOD_BUG_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_bug_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref WHITE_TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
        ("heat-guts", &*HEAT_GUTS_WHITE_SAVE),
        ("aqua-guts", &*AQUA_GUTS_WHITE_SAVE),
        ("elec-guts", &*ELEC_GUTS_WHITE_SAVE),
        ("wood-guts", &*WOOD_GUTS_WHITE_SAVE),
        ("heat-custom", &*HEAT_CUSTOM_WHITE_SAVE),
        ("aqua-custom", &*AQUA_CUSTOM_WHITE_SAVE),
        ("elec-custom", &*ELEC_CUSTOM_WHITE_SAVE),
        ("wood-custom", &*WOOD_CUSTOM_WHITE_SAVE),
        ("heat-shield", &*HEAT_SHIELD_WHITE_SAVE),
        ("aqua-shield", &*AQUA_SHIELD_WHITE_SAVE),
        ("elec-shield", &*ELEC_SHIELD_WHITE_SAVE),
        ("wood-shield", &*WOOD_SHIELD_WHITE_SAVE),
        ("heat-team", &*HEAT_TEAM_WHITE_SAVE),
        ("aqua-team", &*AQUA_TEAM_WHITE_SAVE),
        ("elec-team", &*ELEC_TEAM_WHITE_SAVE),
        ("wood-team", &*WOOD_TEAM_WHITE_SAVE),
        ("heat-ground", &*HEAT_GROUND_WHITE_SAVE),
        ("aqua-ground", &*AQUA_GROUND_WHITE_SAVE),
        ("elec-ground", &*ELEC_GROUND_WHITE_SAVE),
        ("wood-ground", &*WOOD_GROUND_WHITE_SAVE),
        ("heat-bug", &*HEAT_BUG_WHITE_SAVE),
        ("aqua-bug", &*AQUA_BUG_WHITE_SAVE),
        ("elec-bug", &*ELEC_BUG_WHITE_SAVE),
        ("wood-bug", &*WOOD_BUG_WHITE_SAVE),
    ];
}

lazy_static! {
    static ref HEAT_GUTS_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_guts_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref AQUA_GUTS_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_guts_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref ELEC_GUTS_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_guts_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref WOOD_GUTS_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_guts_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref HEAT_CUSTOM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_custom_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref AQUA_CUSTOM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_custom_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref ELEC_CUSTOM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_custom_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref WOOD_CUSTOM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_custom_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref HEAT_SHIELD_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_shield_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref AQUA_SHIELD_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_shield_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref ELEC_SHIELD_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_shield_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref WOOD_SHIELD_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_shield_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref HEAT_TEAM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_team_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref AQUA_TEAM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_team_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref ELEC_TEAM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_team_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref WOOD_TEAM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_team_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref HEAT_SHADOW_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_shadow_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref AQUA_SHADOW_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_shadow_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref ELEC_SHADOW_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_shadow_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref WOOD_SHADOW_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_shadow_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref HEAT_BUG_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_bug_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref AQUA_BUG_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_bug_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref ELEC_BUG_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_bug_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref WOOD_BUG_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_bug_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref BLUE_TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
        ("heat-guts", &*HEAT_GUTS_BLUE_SAVE),
        ("aqua-guts", &*AQUA_GUTS_BLUE_SAVE),
        ("elec-guts", &*ELEC_GUTS_BLUE_SAVE),
        ("wood-guts", &*WOOD_GUTS_BLUE_SAVE),
        ("heat-custom", &*HEAT_CUSTOM_BLUE_SAVE),
        ("aqua-custom", &*AQUA_CUSTOM_BLUE_SAVE),
        ("elec-custom", &*ELEC_CUSTOM_BLUE_SAVE),
        ("wood-custom", &*WOOD_CUSTOM_BLUE_SAVE),
        ("heat-shield", &*HEAT_SHIELD_BLUE_SAVE),
        ("aqua-shield", &*AQUA_SHIELD_BLUE_SAVE),
        ("elec-shield", &*ELEC_SHIELD_BLUE_SAVE),
        ("wood-shield", &*WOOD_SHIELD_BLUE_SAVE),
        ("heat-team", &*HEAT_TEAM_BLUE_SAVE),
        ("aqua-team", &*AQUA_TEAM_BLUE_SAVE),
        ("elec-team", &*ELEC_TEAM_BLUE_SAVE),
        ("wood-team", &*WOOD_TEAM_BLUE_SAVE),
        ("heat-shadow", &*HEAT_SHADOW_BLUE_SAVE),
        ("aqua-shadow", &*AQUA_SHADOW_BLUE_SAVE),
        ("elec-shadow", &*ELEC_SHADOW_BLUE_SAVE),
        ("wood-shadow", &*WOOD_SHADOW_BLUE_SAVE),
        ("heat-bug", &*HEAT_BUG_BLUE_SAVE),
        ("aqua-bug", &*AQUA_BUG_BLUE_SAVE),
        ("elec-bug", &*ELEC_BUG_BLUE_SAVE),
        ("wood-bug", &*WOOD_BUG_BLUE_SAVE),
    ];
}

struct EXE3WImpl;
pub const EXE3W: &'static (dyn game::Game + Send + Sync) = &EXE3WImpl {};

impl game::Game for EXE3WImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::A6BJ_01
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        WHITE_TEMPLATES.as_slice()
    }
}

struct EXE3BImpl;
pub const EXE3B: &'static (dyn game::Game + Send + Sync) = &EXE3BImpl {};

impl game::Game for EXE3BImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::A3XJ_01
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        BLUE_TEMPLATES.as_slice()
    }
}

struct BN3WImpl;
pub const BN3W: &'static (dyn game::Game + Send + Sync) = &BN3WImpl {};

impl game::Game for BN3WImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::A6BE_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        WHITE_TEMPLATES.as_slice()
    }
}

struct BN3BImpl;
pub const BN3B: &'static (dyn game::Game + Send + Sync) = &BN3BImpl {};

impl game::Game for BN3BImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::A3XE_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        BLUE_TEMPLATES.as_slice()
    }
}

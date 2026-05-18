use super::Game;
use std::sync::LazyLock;
use tango_dataview::save::Save as SaveTrait;

const MATCH_TYPES: &[usize] = &[4, 1];

macro_rules! bn3_save {
    ($file:expr, $variant:ident) => {
        LazyLock::new(|| {
            tango_dataview::game::bn3::save::Save::from_wram(
                include_bytes!($file),
                tango_dataview::game::bn3::save::GameInfo {
                    variant: tango_dataview::game::bn3::save::Variant::$variant,
                },
            )
            .unwrap()
        })
    };
}

// ---------------- WHITE (variant 0) ----------------
static HEAT_GUTS_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/heat_guts_white_any.raw", White);
static AQUA_GUTS_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/aqua_guts_white_any.raw", White);
static ELEC_GUTS_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/elec_guts_white_any.raw", White);
static WOOD_GUTS_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/wood_guts_white_any.raw", White);
static HEAT_CUSTOM_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/heat_custom_white_any.raw", White);
static AQUA_CUSTOM_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/aqua_custom_white_any.raw", White);
static ELEC_CUSTOM_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/elec_custom_white_any.raw", White);
static WOOD_CUSTOM_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/wood_custom_white_any.raw", White);
static HEAT_SHIELD_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/heat_shield_white_any.raw", White);
static AQUA_SHIELD_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/aqua_shield_white_any.raw", White);
static ELEC_SHIELD_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/elec_shield_white_any.raw", White);
static WOOD_SHIELD_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/wood_shield_white_any.raw", White);
static HEAT_TEAM_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/heat_team_white_any.raw", White);
static AQUA_TEAM_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/aqua_team_white_any.raw", White);
static ELEC_TEAM_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/elec_team_white_any.raw", White);
static WOOD_TEAM_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/wood_team_white_any.raw", White);
static HEAT_GROUND_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/heat_ground_white_any.raw", White);
static AQUA_GROUND_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/aqua_ground_white_any.raw", White);
static ELEC_GROUND_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/elec_ground_white_any.raw", White);
static WOOD_GROUND_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/wood_ground_white_any.raw", White);
static HEAT_BUG_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/heat_bug_white_any.raw", White);
static AQUA_BUG_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/aqua_bug_white_any.raw", White);
static ELEC_BUG_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/elec_bug_white_any.raw", White);
static WOOD_BUG_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/wood_bug_white_any.raw", White);
static WHITE_T: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> =
    LazyLock::new(|| {
        vec![
            ("heat-guts", &*HEAT_GUTS_W as &(dyn SaveTrait + Send + Sync)),
            ("aqua-guts", &*AQUA_GUTS_W), ("elec-guts", &*ELEC_GUTS_W), ("wood-guts", &*WOOD_GUTS_W),
            ("heat-custom", &*HEAT_CUSTOM_W), ("aqua-custom", &*AQUA_CUSTOM_W), ("elec-custom", &*ELEC_CUSTOM_W), ("wood-custom", &*WOOD_CUSTOM_W),
            ("heat-shield", &*HEAT_SHIELD_W), ("aqua-shield", &*AQUA_SHIELD_W), ("elec-shield", &*ELEC_SHIELD_W), ("wood-shield", &*WOOD_SHIELD_W),
            ("heat-team", &*HEAT_TEAM_W), ("aqua-team", &*AQUA_TEAM_W), ("elec-team", &*ELEC_TEAM_W), ("wood-team", &*WOOD_TEAM_W),
            ("heat-ground", &*HEAT_GROUND_W), ("aqua-ground", &*AQUA_GROUND_W), ("elec-ground", &*ELEC_GROUND_W), ("wood-ground", &*WOOD_GROUND_W),
            ("heat-bug", &*HEAT_BUG_W), ("aqua-bug", &*AQUA_BUG_W), ("elec-bug", &*ELEC_BUG_W), ("wood-bug", &*WOOD_BUG_W),
        ]
    });

// ---------------- BLUE (variant 1) ----------------
static HEAT_GUTS_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/heat_guts_blue_any.raw", Blue);
static AQUA_GUTS_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/aqua_guts_blue_any.raw", Blue);
static ELEC_GUTS_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/elec_guts_blue_any.raw", Blue);
static WOOD_GUTS_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/wood_guts_blue_any.raw", Blue);
static HEAT_CUSTOM_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/heat_custom_blue_any.raw", Blue);
static AQUA_CUSTOM_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/aqua_custom_blue_any.raw", Blue);
static ELEC_CUSTOM_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/elec_custom_blue_any.raw", Blue);
static WOOD_CUSTOM_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/wood_custom_blue_any.raw", Blue);
static HEAT_SHIELD_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/heat_shield_blue_any.raw", Blue);
static AQUA_SHIELD_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/aqua_shield_blue_any.raw", Blue);
static ELEC_SHIELD_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/elec_shield_blue_any.raw", Blue);
static WOOD_SHIELD_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/wood_shield_blue_any.raw", Blue);
static HEAT_TEAM_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/heat_team_blue_any.raw", Blue);
static AQUA_TEAM_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/aqua_team_blue_any.raw", Blue);
static ELEC_TEAM_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/elec_team_blue_any.raw", Blue);
static WOOD_TEAM_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/wood_team_blue_any.raw", Blue);
static HEAT_SHADOW_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/heat_shadow_blue_any.raw", Blue);
static AQUA_SHADOW_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/aqua_shadow_blue_any.raw", Blue);
static ELEC_SHADOW_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/elec_shadow_blue_any.raw", Blue);
static WOOD_SHADOW_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/wood_shadow_blue_any.raw", Blue);
static HEAT_BUG_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/heat_bug_blue_any.raw", Blue);
static AQUA_BUG_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/aqua_bug_blue_any.raw", Blue);
static ELEC_BUG_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/elec_bug_blue_any.raw", Blue);
static WOOD_BUG_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("../../../tango/src/game/bn3/save/wood_bug_blue_any.raw", Blue);
static BLUE_T: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> =
    LazyLock::new(|| {
        vec![
            ("heat-guts", &*HEAT_GUTS_B as &(dyn SaveTrait + Send + Sync)),
            ("aqua-guts", &*AQUA_GUTS_B), ("elec-guts", &*ELEC_GUTS_B), ("wood-guts", &*WOOD_GUTS_B),
            ("heat-custom", &*HEAT_CUSTOM_B), ("aqua-custom", &*AQUA_CUSTOM_B), ("elec-custom", &*ELEC_CUSTOM_B), ("wood-custom", &*WOOD_CUSTOM_B),
            ("heat-shield", &*HEAT_SHIELD_B), ("aqua-shield", &*AQUA_SHIELD_B), ("elec-shield", &*ELEC_SHIELD_B), ("wood-shield", &*WOOD_SHIELD_B),
            ("heat-team", &*HEAT_TEAM_B), ("aqua-team", &*AQUA_TEAM_B), ("elec-team", &*ELEC_TEAM_B), ("wood-team", &*WOOD_TEAM_B),
            ("heat-shadow", &*HEAT_SHADOW_B), ("aqua-shadow", &*AQUA_SHADOW_B), ("elec-shadow", &*ELEC_SHADOW_B), ("wood-shadow", &*WOOD_SHADOW_B),
            ("heat-bug", &*HEAT_BUG_B), ("aqua-bug", &*AQUA_BUG_B), ("elec-bug", &*ELEC_BUG_B), ("wood-bug", &*WOOD_BUG_B),
        ]
    });

struct EXE3WImpl;
pub const EXE3W: &'static (dyn Game + Send + Sync) = &EXE3WImpl;
impl Game for EXE3WImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) { &tango_gamedb::A6BJ_01 }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) { &tango_pvp::game::bn3::A6BJ_01 }
    fn match_types(&self) -> &'static [usize] { MATCH_TYPES }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] { WHITE_T.as_slice() }
}

struct EXE3BImpl;
pub const EXE3B: &'static (dyn Game + Send + Sync) = &EXE3BImpl;
impl Game for EXE3BImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) { &tango_gamedb::A3XJ_01 }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) { &tango_pvp::game::bn3::A3XJ_01 }
    fn match_types(&self) -> &'static [usize] { MATCH_TYPES }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] { BLUE_T.as_slice() }
}

struct BN3WImpl;
pub const BN3W: &'static (dyn Game + Send + Sync) = &BN3WImpl;
impl Game for BN3WImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) { &tango_gamedb::A6BE_00 }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) { &tango_pvp::game::bn3::A6BE_00 }
    fn match_types(&self) -> &'static [usize] { MATCH_TYPES }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] { WHITE_T.as_slice() }
}

struct BN3BImpl;
pub const BN3B: &'static (dyn Game + Send + Sync) = &BN3BImpl;
impl Game for BN3BImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) { &tango_gamedb::A3XE_00 }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) { &tango_pvp::game::bn3::A3XE_00 }
    fn match_types(&self) -> &'static [usize] { MATCH_TYPES }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] { BLUE_T.as_slice() }
}

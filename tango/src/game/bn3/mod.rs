use super::{Game, LazyImage, SaveTemplates};
use std::sync::LazyLock;
use tango_dataview::save::Save as SaveTrait;

const MATCH_TYPES: &[usize] = &[4, 1];
static BACKGROUND_W: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../backgrounds/3-0.png")).unwrap());
static BACKGROUND_B: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../backgrounds/3-1.png")).unwrap());
static EXE3W_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/exe3-0.png")).unwrap());
static EXE3B_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/exe3-1.png")).unwrap());
static BN3W_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/bn3-0.png")).unwrap());
static BN3B_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/bn3-1.png")).unwrap());

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
static HEAT_GUTS_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/heat_guts_white_any.raw", White);
static AQUA_GUTS_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/aqua_guts_white_any.raw", White);
static ELEC_GUTS_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/elec_guts_white_any.raw", White);
static WOOD_GUTS_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/wood_guts_white_any.raw", White);
static HEAT_CUSTOM_W: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/heat_custom_white_any.raw", White);
static AQUA_CUSTOM_W: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/aqua_custom_white_any.raw", White);
static ELEC_CUSTOM_W: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/elec_custom_white_any.raw", White);
static WOOD_CUSTOM_W: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/wood_custom_white_any.raw", White);
static HEAT_SHIELD_W: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/heat_shield_white_any.raw", White);
static AQUA_SHIELD_W: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/aqua_shield_white_any.raw", White);
static ELEC_SHIELD_W: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/elec_shield_white_any.raw", White);
static WOOD_SHIELD_W: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/wood_shield_white_any.raw", White);
static HEAT_TEAM_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/heat_team_white_any.raw", White);
static AQUA_TEAM_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/aqua_team_white_any.raw", White);
static ELEC_TEAM_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/elec_team_white_any.raw", White);
static WOOD_TEAM_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/wood_team_white_any.raw", White);
static HEAT_GROUND_W: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/heat_ground_white_any.raw", White);
static AQUA_GROUND_W: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/aqua_ground_white_any.raw", White);
static ELEC_GROUND_W: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/elec_ground_white_any.raw", White);
static WOOD_GROUND_W: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/wood_ground_white_any.raw", White);
static HEAT_BUG_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/heat_bug_white_any.raw", White);
static AQUA_BUG_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/aqua_bug_white_any.raw", White);
static ELEC_BUG_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/elec_bug_white_any.raw", White);
static WOOD_BUG_W: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/wood_bug_white_any.raw", White);
static WHITE_T: SaveTemplates = LazyLock::new(|| {
    vec![
        ("heat-guts", &*HEAT_GUTS_W as &(dyn SaveTrait + Send + Sync)),
        ("aqua-guts", &*AQUA_GUTS_W),
        ("elec-guts", &*ELEC_GUTS_W),
        ("wood-guts", &*WOOD_GUTS_W),
        ("heat-custom", &*HEAT_CUSTOM_W),
        ("aqua-custom", &*AQUA_CUSTOM_W),
        ("elec-custom", &*ELEC_CUSTOM_W),
        ("wood-custom", &*WOOD_CUSTOM_W),
        ("heat-shield", &*HEAT_SHIELD_W),
        ("aqua-shield", &*AQUA_SHIELD_W),
        ("elec-shield", &*ELEC_SHIELD_W),
        ("wood-shield", &*WOOD_SHIELD_W),
        ("heat-team", &*HEAT_TEAM_W),
        ("aqua-team", &*AQUA_TEAM_W),
        ("elec-team", &*ELEC_TEAM_W),
        ("wood-team", &*WOOD_TEAM_W),
        ("heat-ground", &*HEAT_GROUND_W),
        ("aqua-ground", &*AQUA_GROUND_W),
        ("elec-ground", &*ELEC_GROUND_W),
        ("wood-ground", &*WOOD_GROUND_W),
        ("heat-bug", &*HEAT_BUG_W),
        ("aqua-bug", &*AQUA_BUG_W),
        ("elec-bug", &*ELEC_BUG_W),
        ("wood-bug", &*WOOD_BUG_W),
    ]
});

// ---------------- BLUE (variant 1) ----------------
static HEAT_GUTS_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/heat_guts_blue_any.raw", Blue);
static AQUA_GUTS_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/aqua_guts_blue_any.raw", Blue);
static ELEC_GUTS_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/elec_guts_blue_any.raw", Blue);
static WOOD_GUTS_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/wood_guts_blue_any.raw", Blue);
static HEAT_CUSTOM_B: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/heat_custom_blue_any.raw", Blue);
static AQUA_CUSTOM_B: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/aqua_custom_blue_any.raw", Blue);
static ELEC_CUSTOM_B: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/elec_custom_blue_any.raw", Blue);
static WOOD_CUSTOM_B: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/wood_custom_blue_any.raw", Blue);
static HEAT_SHIELD_B: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/heat_shield_blue_any.raw", Blue);
static AQUA_SHIELD_B: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/aqua_shield_blue_any.raw", Blue);
static ELEC_SHIELD_B: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/elec_shield_blue_any.raw", Blue);
static WOOD_SHIELD_B: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/wood_shield_blue_any.raw", Blue);
static HEAT_TEAM_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/heat_team_blue_any.raw", Blue);
static AQUA_TEAM_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/aqua_team_blue_any.raw", Blue);
static ELEC_TEAM_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/elec_team_blue_any.raw", Blue);
static WOOD_TEAM_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/wood_team_blue_any.raw", Blue);
static HEAT_SHADOW_B: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/heat_shadow_blue_any.raw", Blue);
static AQUA_SHADOW_B: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/aqua_shadow_blue_any.raw", Blue);
static ELEC_SHADOW_B: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/elec_shadow_blue_any.raw", Blue);
static WOOD_SHADOW_B: LazyLock<tango_dataview::game::bn3::save::Save> =
    bn3_save!("save/wood_shadow_blue_any.raw", Blue);
static HEAT_BUG_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/heat_bug_blue_any.raw", Blue);
static AQUA_BUG_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/aqua_bug_blue_any.raw", Blue);
static ELEC_BUG_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/elec_bug_blue_any.raw", Blue);
static WOOD_BUG_B: LazyLock<tango_dataview::game::bn3::save::Save> = bn3_save!("save/wood_bug_blue_any.raw", Blue);
static BLUE_T: SaveTemplates = LazyLock::new(|| {
    vec![
        ("heat-guts", &*HEAT_GUTS_B as &(dyn SaveTrait + Send + Sync)),
        ("aqua-guts", &*AQUA_GUTS_B),
        ("elec-guts", &*ELEC_GUTS_B),
        ("wood-guts", &*WOOD_GUTS_B),
        ("heat-custom", &*HEAT_CUSTOM_B),
        ("aqua-custom", &*AQUA_CUSTOM_B),
        ("elec-custom", &*ELEC_CUSTOM_B),
        ("wood-custom", &*WOOD_CUSTOM_B),
        ("heat-shield", &*HEAT_SHIELD_B),
        ("aqua-shield", &*AQUA_SHIELD_B),
        ("elec-shield", &*ELEC_SHIELD_B),
        ("wood-shield", &*WOOD_SHIELD_B),
        ("heat-team", &*HEAT_TEAM_B),
        ("aqua-team", &*AQUA_TEAM_B),
        ("elec-team", &*ELEC_TEAM_B),
        ("wood-team", &*WOOD_TEAM_B),
        ("heat-shadow", &*HEAT_SHADOW_B),
        ("aqua-shadow", &*AQUA_SHADOW_B),
        ("elec-shadow", &*ELEC_SHADOW_B),
        ("wood-shadow", &*WOOD_SHADOW_B),
        ("heat-bug", &*HEAT_BUG_B),
        ("aqua-bug", &*AQUA_BUG_B),
        ("elec-bug", &*ELEC_BUG_B),
        ("wood-bug", &*WOOD_BUG_B),
    ]
});

pub static EXE3W: Game = Game {
    gamedb_entry: &tango_gamedb::A6BJ_01,
    hooks: &tango_pvp::game::bn3::A6BJ_01,
    match_types: MATCH_TYPES,
    save_templates: &WHITE_T,
    logo_image: &EXE3W_LOGO,
    background_image: &BACKGROUND_W,
};

pub static EXE3B: Game = Game {
    gamedb_entry: &tango_gamedb::A3XJ_01,
    hooks: &tango_pvp::game::bn3::A3XJ_01,
    match_types: MATCH_TYPES,
    save_templates: &BLUE_T,
    logo_image: &EXE3B_LOGO,
    background_image: &BACKGROUND_B,
};

pub static BN3W: Game = Game {
    gamedb_entry: &tango_gamedb::A6BE_00,
    hooks: &tango_pvp::game::bn3::A6BE_00,
    match_types: MATCH_TYPES,
    save_templates: &WHITE_T,
    logo_image: &BN3W_LOGO,
    background_image: &BACKGROUND_W,
};

pub static BN3B: Game = Game {
    gamedb_entry: &tango_gamedb::A3XE_00,
    hooks: &tango_pvp::game::bn3::A3XE_00,
    match_types: MATCH_TYPES,
    save_templates: &BLUE_T,
    logo_image: &BN3B_LOGO,
    background_image: &BACKGROUND_B,
};

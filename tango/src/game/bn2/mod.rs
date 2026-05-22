use super::{Game, LazyImage, SaveTemplates};
use std::sync::LazyLock;
use tango_dataview::save::Save as SaveTrait;

const MATCH_TYPES: &[usize] = &[1];

macro_rules! bn2_save {
    ($file:expr) => {
        LazyLock::new(|| tango_dataview::game::bn2::save::Save::from_wram(include_bytes!($file)).unwrap())
    };
}

static HUB_ANY: LazyLock<tango_dataview::game::bn2::save::Save> = bn2_save!("save/hub_any.raw");
static GUTS_ANY: LazyLock<tango_dataview::game::bn2::save::Save> = bn2_save!("save/guts_any.raw");
static CUSTOM_ANY: LazyLock<tango_dataview::game::bn2::save::Save> = bn2_save!("save/custom_any.raw");
static TEAM_ANY: LazyLock<tango_dataview::game::bn2::save::Save> = bn2_save!("save/team_any.raw");
static SHIELD_ANY: LazyLock<tango_dataview::game::bn2::save::Save> = bn2_save!("save/shield_any.raw");

static BACKGROUND: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../backgrounds/2.png")).unwrap());

static TEMPLATES: SaveTemplates = LazyLock::new(|| {
    vec![
        ("hub", &*HUB_ANY as &(dyn SaveTrait + Send + Sync)),
        ("guts", &*GUTS_ANY as &(dyn SaveTrait + Send + Sync)),
        ("custom", &*CUSTOM_ANY as &(dyn SaveTrait + Send + Sync)),
        ("team", &*TEAM_ANY as &(dyn SaveTrait + Send + Sync)),
        ("shield", &*SHIELD_ANY as &(dyn SaveTrait + Send + Sync)),
    ]
});

static EXE2_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/exe2-0.png")).unwrap());
static BN2_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/bn2-0.png")).unwrap());

pub static EXE2: Game = Game {
    gamedb_entry: &tango_gamedb::AE2J_00_AC,
    hooks: &tango_pvp::game::bn2::AE2J_00_AC,
    match_types: MATCH_TYPES,
    save_templates: &TEMPLATES,
    logo_image: &EXE2_LOGO,
    background_image: &BACKGROUND,
};

pub static BN2: Game = Game {
    gamedb_entry: &tango_gamedb::AE2E_00,
    hooks: &tango_pvp::game::bn2::AE2E_00,
    match_types: MATCH_TYPES,
    save_templates: &TEMPLATES,
    logo_image: &BN2_LOGO,
    background_image: &BACKGROUND,
};

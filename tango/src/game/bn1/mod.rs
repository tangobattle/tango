use super::{Game, LazyImage, SaveTemplates};
use std::sync::LazyLock;
use tango_dataview::save::Save as SaveTrait;

const MATCH_TYPES: &[usize] = &[1];

static BACKGROUND: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../backgrounds/1.png")).unwrap());

// ---------------- EXE1 JP ----------------
static EXE1_SAVE: LazyLock<tango_dataview::game::bn1::save::Save> = LazyLock::new(|| {
    tango_dataview::game::bn1::save::Save::from_wram(
        include_bytes!("save/jp.raw"),
        tango_dataview::game::bn1::save::GameInfo {
            region: tango_dataview::game::bn1::save::Region::JP,
        },
    )
    .unwrap()
});
static EXE1_T: SaveTemplates = LazyLock::new(|| vec![("", &*EXE1_SAVE as &(dyn SaveTrait + Send + Sync))]);
static EXE1_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/exe1-0.png")).unwrap());

pub static EXE1: Game = Game {
    gamedb_entry: &tango_gamedb::AREJ_00,
    hooks: &tango_pvp::game::bn1::AREJ_00,
    match_types: MATCH_TYPES,
    save_templates: &EXE1_T,
    logo_image: &EXE1_LOGO,
    background_image: &BACKGROUND,
};

// ---------------- BN1 US ----------------
static BN1_SAVE: LazyLock<tango_dataview::game::bn1::save::Save> = LazyLock::new(|| {
    tango_dataview::game::bn1::save::Save::from_wram(
        include_bytes!("save/us.raw"),
        tango_dataview::game::bn1::save::GameInfo {
            region: tango_dataview::game::bn1::save::Region::US,
        },
    )
    .unwrap()
});
static BN1_T: SaveTemplates = LazyLock::new(|| vec![("", &*BN1_SAVE as &(dyn SaveTrait + Send + Sync))]);
static BN1_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/bn1-0.png")).unwrap());

pub static BN1: Game = Game {
    gamedb_entry: &tango_gamedb::AREE_00,
    hooks: &tango_pvp::game::bn1::AREE_00,
    match_types: MATCH_TYPES,
    save_templates: &BN1_T,
    logo_image: &BN1_LOGO,
    background_image: &BACKGROUND,
};

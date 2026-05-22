use super::{BackgroundRef, Game, LazyImage, SaveTemplates};
use crate::bnlc;
use std::sync::LazyLock;
use tango_dataview::save::Save as SaveTrait;

const MATCH_TYPES: &[usize] = &[1, 1];
const BACKGROUND: BackgroundRef = BackgroundRef {
    volume: bnlc::Volume::Vol2,
    tga: "16.tga",
};
static EXE5B_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/exe5-0.png")).unwrap());
static EXE5C_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/exe5-1.png")).unwrap());
static BN5P_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/bn5-0.png")).unwrap());
static BN5C_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/bn5-1.png")).unwrap());

macro_rules! bn5_save {
    ($file:expr, $region:ident, $variant:ident) => {
        LazyLock::new(|| {
            tango_dataview::game::bn5::save::Save::from_wram(
                include_bytes!($file),
                tango_dataview::game::bn5::save::GameInfo {
                    region: tango_dataview::game::bn5::save::Region::$region,
                    variant: tango_dataview::game::bn5::save::Variant::$variant,
                },
            )
            .unwrap()
        })
    };
}

// ---------------- EXE5 Blues (Protoman) JP ----------------
static EXE5B_DARK: LazyLock<tango_dataview::game::bn5::save::Save> =
    bn5_save!("save/dark_protoman_jp.raw", JP, Protoman);
static EXE5B_LIGHT: LazyLock<tango_dataview::game::bn5::save::Save> =
    bn5_save!("save/light_protoman_jp.raw", JP, Protoman);
static EXE5B_T: SaveTemplates = LazyLock::new(|| {
    vec![
        ("dark", &*EXE5B_DARK as &(dyn SaveTrait + Send + Sync)),
        ("light", &*EXE5B_LIGHT),
    ]
});

pub static EXE5B: Game = Game {
    gamedb_entry: &tango_gamedb::BRBJ_00,
    hooks: &tango_pvp::game::bn5::BRBJ_00,
    match_types: MATCH_TYPES,
    save_templates: &EXE5B_T,
    logo_image: &EXE5B_LOGO,
    background: BACKGROUND,
};

// ---------------- EXE5 Colonel JP ----------------
static EXE5C_DARK: LazyLock<tango_dataview::game::bn5::save::Save> = bn5_save!("save/dark_colonel_jp.raw", JP, Colonel);
static EXE5C_LIGHT: LazyLock<tango_dataview::game::bn5::save::Save> =
    bn5_save!("save/light_colonel_jp.raw", JP, Colonel);
static EXE5C_T: SaveTemplates = LazyLock::new(|| {
    vec![
        ("dark", &*EXE5C_DARK as &(dyn SaveTrait + Send + Sync)),
        ("light", &*EXE5C_LIGHT),
    ]
});

pub static EXE5C: Game = Game {
    gamedb_entry: &tango_gamedb::BRKJ_00,
    hooks: &tango_pvp::game::bn5::BRKJ_00,
    match_types: MATCH_TYPES,
    save_templates: &EXE5C_T,
    logo_image: &EXE5C_LOGO,
    background: BACKGROUND,
};

// ---------------- BN5 Protoman US ----------------
static BN5P_DARK: LazyLock<tango_dataview::game::bn5::save::Save> =
    bn5_save!("save/dark_protoman_us.raw", US, Protoman);
static BN5P_LIGHT: LazyLock<tango_dataview::game::bn5::save::Save> =
    bn5_save!("save/light_protoman_us.raw", US, Protoman);
static BN5P_T: SaveTemplates = LazyLock::new(|| {
    vec![
        ("dark", &*BN5P_DARK as &(dyn SaveTrait + Send + Sync)),
        ("light", &*BN5P_LIGHT),
    ]
});

pub static BN5P: Game = Game {
    gamedb_entry: &tango_gamedb::BRBE_00,
    hooks: &tango_pvp::game::bn5::BRBE_00,
    match_types: MATCH_TYPES,
    save_templates: &BN5P_T,
    logo_image: &BN5P_LOGO,
    background: BACKGROUND,
};

// ---------------- BN5 Colonel US ----------------
static BN5C_DARK: LazyLock<tango_dataview::game::bn5::save::Save> = bn5_save!("save/dark_colonel_us.raw", US, Colonel);
static BN5C_LIGHT: LazyLock<tango_dataview::game::bn5::save::Save> =
    bn5_save!("save/light_colonel_us.raw", US, Colonel);
static BN5C_T: SaveTemplates = LazyLock::new(|| {
    vec![
        ("dark", &*BN5C_DARK as &(dyn SaveTrait + Send + Sync)),
        ("light", &*BN5C_LIGHT),
    ]
});

pub static BN5C: Game = Game {
    gamedb_entry: &tango_gamedb::BRKE_00,
    hooks: &tango_pvp::game::bn5::BRKE_00,
    match_types: MATCH_TYPES,
    save_templates: &BN5C_T,
    logo_image: &BN5C_LOGO,
    background: BACKGROUND,
};

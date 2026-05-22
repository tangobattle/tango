use super::{BackgroundRef, Game, LazyImage, SaveTemplates};
use crate::bnlc;
use std::sync::LazyLock;
use tango_dataview::save::Save as SaveTrait;

const MATCH_TYPES: &[usize] = &[2, 2];
const BACKGROUND: BackgroundRef = BackgroundRef {
    volume: bnlc::Volume::Vol2,
    tga: "13.tga",
};
static EXE4RS_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/exe4-0.png")).unwrap());
static EXE4BM_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/exe4-1.png")).unwrap());
static BN4RS_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/bn4-0.png")).unwrap());
static BN4BM_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/bn4-1.png")).unwrap());

macro_rules! bn4_save {
    ($file:expr, $jp:expr, $us:expr, $variant:ident) => {
        LazyLock::new(|| {
            tango_dataview::game::bn4::save::Save::from_wram(
                include_bytes!($file),
                tango_dataview::game::bn4::save::GameInfo {
                    region: tango_dataview::game::bn4::save::Region { jp: $jp, us: $us },
                    variant: tango_dataview::game::bn4::save::Variant::$variant,
                },
            )
            .unwrap()
        })
    };
}

// ---------------- EXE4 Red Sun (JP) ----------------
static EXE4RS_DARK997: LazyLock<tango_dataview::game::bn4::save::Save> =
    bn4_save!("save/dark_hp_997_rs_jp.raw", true, false, RedSun);
static EXE4RS_LIGHT999: LazyLock<tango_dataview::game::bn4::save::Save> =
    bn4_save!("save/light_hp_999_rs_jp.raw", true, false, RedSun);
static EXE4RS_LIGHT1000: LazyLock<tango_dataview::game::bn4::save::Save> =
    bn4_save!("save/light_hp_1000_rs_jp.raw", true, false, RedSun);
static EXE4RS_T: SaveTemplates = LazyLock::new(|| {
    vec![
        ("dark-hp-997", &*EXE4RS_DARK997 as &(dyn SaveTrait + Send + Sync)),
        ("light-hp-999", &*EXE4RS_LIGHT999),
        ("light-hp-1000", &*EXE4RS_LIGHT1000),
    ]
});

pub static EXE4RS: Game = Game {
    gamedb_entry: &tango_gamedb::B4WJ_01,
    hooks: &tango_pvp::game::bn4::B4WJ_01,
    match_types: MATCH_TYPES,
    save_templates: &EXE4RS_T,
    logo_image: &EXE4RS_LOGO,
    background: BACKGROUND,
};

// ---------------- EXE4 Blue Moon (JP) ----------------
static EXE4BM_DARK997: LazyLock<tango_dataview::game::bn4::save::Save> =
    bn4_save!("save/dark_hp_997_bm_jp.raw", true, false, BlueMoon);
static EXE4BM_LIGHT999: LazyLock<tango_dataview::game::bn4::save::Save> =
    bn4_save!("save/light_hp_999_bm_jp.raw", true, false, BlueMoon);
static EXE4BM_LIGHT1000: LazyLock<tango_dataview::game::bn4::save::Save> =
    bn4_save!("save/light_hp_1000_bm_jp.raw", true, false, BlueMoon);
static EXE4BM_T: SaveTemplates = LazyLock::new(|| {
    vec![
        ("dark-hp-997", &*EXE4BM_DARK997 as &(dyn SaveTrait + Send + Sync)),
        ("light-hp-999", &*EXE4BM_LIGHT999),
        ("light-hp-1000", &*EXE4BM_LIGHT1000),
    ]
});

pub static EXE4BM: Game = Game {
    gamedb_entry: &tango_gamedb::B4BJ_01,
    hooks: &tango_pvp::game::bn4::B4BJ_01,
    match_types: MATCH_TYPES,
    save_templates: &EXE4BM_T,
    logo_image: &EXE4BM_LOGO,
    background: BACKGROUND,
};

// ---------------- BN4 Red Sun (US) ----------------
static BN4RS_DARK997: LazyLock<tango_dataview::game::bn4::save::Save> =
    bn4_save!("save/dark_hp_997_rs_us.raw", false, true, RedSun);
static BN4RS_LIGHT999: LazyLock<tango_dataview::game::bn4::save::Save> =
    bn4_save!("save/light_hp_999_rs_us.raw", false, true, RedSun);
static BN4RS_LIGHT1000: LazyLock<tango_dataview::game::bn4::save::Save> =
    bn4_save!("save/light_hp_1000_rs_us.raw", false, true, RedSun);
static BN4RS_T: SaveTemplates = LazyLock::new(|| {
    vec![
        ("dark-hp-997", &*BN4RS_DARK997 as &(dyn SaveTrait + Send + Sync)),
        ("light-hp-999", &*BN4RS_LIGHT999),
        ("light-hp-1000", &*BN4RS_LIGHT1000),
    ]
});

pub static BN4RS: Game = Game {
    gamedb_entry: &tango_gamedb::B4WE_00,
    hooks: &tango_pvp::game::bn4::B4WE_00,
    match_types: MATCH_TYPES,
    save_templates: &BN4RS_T,
    logo_image: &BN4RS_LOGO,
    background: BACKGROUND,
};

// ---------------- BN4 Blue Moon (US) ----------------
static BN4BM_DARK997: LazyLock<tango_dataview::game::bn4::save::Save> =
    bn4_save!("save/dark_hp_997_bm_us.raw", false, true, BlueMoon);
static BN4BM_LIGHT999: LazyLock<tango_dataview::game::bn4::save::Save> =
    bn4_save!("save/light_hp_999_bm_us.raw", false, true, BlueMoon);
static BN4BM_LIGHT1000: LazyLock<tango_dataview::game::bn4::save::Save> =
    bn4_save!("save/light_hp_1000_bm_us.raw", false, true, BlueMoon);
static BN4BM_T: SaveTemplates = LazyLock::new(|| {
    vec![
        ("dark-hp-997", &*BN4BM_DARK997 as &(dyn SaveTrait + Send + Sync)),
        ("light-hp-999", &*BN4BM_LIGHT999),
        ("light-hp-1000", &*BN4BM_LIGHT1000),
    ]
});

pub static BN4BM: Game = Game {
    gamedb_entry: &tango_gamedb::B4BE_00,
    hooks: &tango_pvp::game::bn4::B4BE_00,
    match_types: MATCH_TYPES,
    save_templates: &BN4BM_T,
    logo_image: &BN4BM_LOGO,
    background: BACKGROUND,
};

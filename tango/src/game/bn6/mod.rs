use super::{BackgroundRef, Game, LazyImage, SaveTemplates};
use crate::bnlc;
use std::sync::LazyLock;
use tango_dataview::save::Save as SaveTrait;

const MATCH_TYPES: &[usize] = &[1, 1];
const BACKGROUND: BackgroundRef = BackgroundRef {
    volume: bnlc::Volume::Vol2,
    tga: "19.tga",
};
static EXE6G_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/exe6-0.png")).unwrap());
static EXE6F_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/exe6-1.png")).unwrap());
static BN6G_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/bn6-0.png")).unwrap());
static BN6F_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/bn6-1.png")).unwrap());

macro_rules! bn6_save {
    ($file:expr, $region:ident, $variant:ident) => {
        LazyLock::new(|| {
            tango_dataview::game::bn6::save::Save::from_wram(
                include_bytes!($file),
                tango_dataview::game::bn6::save::GameInfo {
                    region: tango_dataview::game::bn6::save::Region::$region,
                    variant: tango_dataview::game::bn6::save::Variant::$variant,
                },
            )
            .unwrap()
        })
    };
}

// ---------------- EXE6 Gregar JP (BR5J_00) ----------------
static EXE6G_MEGA: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/g_jp.raw", JP, Gregar);
static EXE6G_HEAT: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/heatman_g_jp.raw", JP, Gregar);
static EXE6G_ELEC: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/elecman_g_jp.raw", JP, Gregar);
static EXE6G_SLASH: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/slashman_g_jp.raw", JP, Gregar);
static EXE6G_ERASE: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/eraseman_g_jp.raw", JP, Gregar);
static EXE6G_CHARGE: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/chargeman_g_jp.raw", JP, Gregar);
static EXE6G_PROTO: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/protoman_g_jp.raw", JP, Gregar);
static EXE6G_T: SaveTemplates = LazyLock::new(|| {
    vec![
        ("megaman", &*EXE6G_MEGA as &(dyn SaveTrait + Send + Sync)),
        ("heatman", &*EXE6G_HEAT),
        ("elecman", &*EXE6G_ELEC),
        ("slashman", &*EXE6G_SLASH),
        ("eraseman", &*EXE6G_ERASE),
        ("chargeman", &*EXE6G_CHARGE),
        ("protoman", &*EXE6G_PROTO),
    ]
});

pub static EXE6G: Game = Game {
    gamedb_entry: &tango_gamedb::BR5J_00,
    hooks: &tango_pvp::game::bn6::BR5J_00,
    match_types: MATCH_TYPES,
    save_templates: &EXE6G_T,
    logo_image: &EXE6G_LOGO,
    background: BACKGROUND,
};

// ---------------- EXE6 Falzar JP (BR6J_00) ----------------
static EXE6F_MEGA: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/f_jp.raw", JP, Falzar);
static EXE6F_SPOUT: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/spoutman_f_jp.raw", JP, Falzar);
static EXE6F_TOMA: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/tomahawkman_f_jp.raw", JP, Falzar);
static EXE6F_TENGU: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/tenguman_f_jp.raw", JP, Falzar);
static EXE6F_GROUND: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/groundman_f_jp.raw", JP, Falzar);
static EXE6F_DUST: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/dustman_f_jp.raw", JP, Falzar);
static EXE6F_PROTO: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/protoman_f_jp.raw", JP, Falzar);
static EXE6F_T: SaveTemplates = LazyLock::new(|| {
    vec![
        ("megaman", &*EXE6F_MEGA as &(dyn SaveTrait + Send + Sync)),
        ("spoutman", &*EXE6F_SPOUT),
        ("tomahawkman", &*EXE6F_TOMA),
        ("tenguman", &*EXE6F_TENGU),
        ("groundman", &*EXE6F_GROUND),
        ("dustman", &*EXE6F_DUST),
        ("protoman", &*EXE6F_PROTO),
    ]
});

pub static EXE6F: Game = Game {
    gamedb_entry: &tango_gamedb::BR6J_00,
    hooks: &tango_pvp::game::bn6::BR6J_00,
    match_types: MATCH_TYPES,
    save_templates: &EXE6F_T,
    logo_image: &EXE6F_LOGO,
    background: BACKGROUND,
};

// ---------------- BN6 Gregar US (BR5E_00) ----------------
static BN6G_MEGA: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/g_us.raw", US, Gregar);
static BN6G_HEAT: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/heatman_g_us.raw", US, Gregar);
static BN6G_ELEC: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/elecman_g_us.raw", US, Gregar);
static BN6G_SLASH: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/slashman_g_us.raw", US, Gregar);
static BN6G_ERASE: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/eraseman_g_us.raw", US, Gregar);
static BN6G_CHARGE: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/chargeman_g_us.raw", US, Gregar);
static BN6G_PROTO: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/protoman_g_us.raw", US, Gregar);
static BN6G_T: SaveTemplates = LazyLock::new(|| {
    vec![
        ("megaman", &*BN6G_MEGA as &(dyn SaveTrait + Send + Sync)),
        ("heatman", &*BN6G_HEAT),
        ("elecman", &*BN6G_ELEC),
        ("slashman", &*BN6G_SLASH),
        ("eraseman", &*BN6G_ERASE),
        ("chargeman", &*BN6G_CHARGE),
        ("protoman", &*BN6G_PROTO),
    ]
});

pub static BN6G: Game = Game {
    gamedb_entry: &tango_gamedb::BR5E_00,
    hooks: &tango_pvp::game::bn6::BR5E_00,
    match_types: MATCH_TYPES,
    save_templates: &BN6G_T,
    logo_image: &BN6G_LOGO,
    background: BACKGROUND,
};

// ---------------- BN6 Falzar US (BR6E_00) ----------------
static BN6F_MEGA: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/f_us.raw", US, Falzar);
static BN6F_SPOUT: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/spoutman_f_us.raw", US, Falzar);
static BN6F_TOMA: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/tomahawkman_f_us.raw", US, Falzar);
static BN6F_TENGU: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/tenguman_f_us.raw", US, Falzar);
static BN6F_GROUND: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/groundman_f_us.raw", US, Falzar);
static BN6F_DUST: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/dustman_f_us.raw", US, Falzar);
static BN6F_PROTO: LazyLock<tango_dataview::game::bn6::save::Save> = bn6_save!("save/protoman_f_us.raw", US, Falzar);
static BN6F_T: SaveTemplates = LazyLock::new(|| {
    vec![
        ("megaman", &*BN6F_MEGA as &(dyn SaveTrait + Send + Sync)),
        ("spoutman", &*BN6F_SPOUT),
        ("tomahawkman", &*BN6F_TOMA),
        ("tenguman", &*BN6F_TENGU),
        ("groundman", &*BN6F_GROUND),
        ("dustman", &*BN6F_DUST),
        ("protoman", &*BN6F_PROTO),
    ]
});

pub static BN6F: Game = Game {
    gamedb_entry: &tango_gamedb::BR6E_00,
    hooks: &tango_pvp::game::bn6::BR6E_00,
    match_types: MATCH_TYPES,
    save_templates: &BN6F_T,
    logo_image: &BN6F_LOGO,
    background: BACKGROUND,
};

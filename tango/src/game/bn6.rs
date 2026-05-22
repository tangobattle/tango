use super::Game;
use std::sync::LazyLock;
use tango_dataview::save::Save as SaveTrait;

const MATCH_TYPES: &[usize] = &[1, 1];

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
static EXE6G_MEGA: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/g_jp.raw", JP, Gregar);
static EXE6G_HEAT: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/heatman_g_jp.raw", JP, Gregar);
static EXE6G_ELEC: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/elecman_g_jp.raw", JP, Gregar);
static EXE6G_SLASH: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/slashman_g_jp.raw", JP, Gregar);
static EXE6G_ERASE: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/eraseman_g_jp.raw", JP, Gregar);
static EXE6G_CHARGE: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/chargeman_g_jp.raw", JP, Gregar);
static EXE6G_PROTO: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/protoman_g_jp.raw", JP, Gregar);
static EXE6G_T: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> = LazyLock::new(|| {
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

struct EXE6GImpl;
pub const EXE6G: &'static (dyn Game + Send + Sync) = &EXE6GImpl;
impl Game for EXE6GImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::BR5J_00
    }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn6::BR5J_00
    }
    fn match_types(&self) -> &'static [usize] {
        MATCH_TYPES
    }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] {
        EXE6G_T.as_slice()
    }
}

// ---------------- EXE6 Falzar JP (BR6J_00) ----------------
static EXE6F_MEGA: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/f_jp.raw", JP, Falzar);
static EXE6F_SPOUT: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/spoutman_f_jp.raw", JP, Falzar);
static EXE6F_TOMA: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/tomahawkman_f_jp.raw", JP, Falzar);
static EXE6F_TENGU: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/tenguman_f_jp.raw", JP, Falzar);
static EXE6F_GROUND: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/groundman_f_jp.raw", JP, Falzar);
static EXE6F_DUST: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/dustman_f_jp.raw", JP, Falzar);
static EXE6F_PROTO: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/protoman_f_jp.raw", JP, Falzar);
static EXE6F_T: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> = LazyLock::new(|| {
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

struct EXE6FImpl;
pub const EXE6F: &'static (dyn Game + Send + Sync) = &EXE6FImpl;
impl Game for EXE6FImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::BR6J_00
    }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn6::BR6J_00
    }
    fn match_types(&self) -> &'static [usize] {
        MATCH_TYPES
    }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] {
        EXE6F_T.as_slice()
    }
}

// ---------------- BN6 Gregar US (BR5E_00) ----------------
static BN6G_MEGA: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/g_us.raw", US, Gregar);
static BN6G_HEAT: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/heatman_g_us.raw", US, Gregar);
static BN6G_ELEC: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/elecman_g_us.raw", US, Gregar);
static BN6G_SLASH: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/slashman_g_us.raw", US, Gregar);
static BN6G_ERASE: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/eraseman_g_us.raw", US, Gregar);
static BN6G_CHARGE: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/chargeman_g_us.raw", US, Gregar);
static BN6G_PROTO: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/protoman_g_us.raw", US, Gregar);
static BN6G_T: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> = LazyLock::new(|| {
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

struct BN6GImpl;
pub const BN6G: &'static (dyn Game + Send + Sync) = &BN6GImpl;
impl Game for BN6GImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::BR5E_00
    }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn6::BR5E_00
    }
    fn match_types(&self) -> &'static [usize] {
        MATCH_TYPES
    }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] {
        BN6G_T.as_slice()
    }
}

// ---------------- BN6 Falzar US (BR6E_00) ----------------
static BN6F_MEGA: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/f_us.raw", US, Falzar);
static BN6F_SPOUT: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/spoutman_f_us.raw", US, Falzar);
static BN6F_TOMA: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/tomahawkman_f_us.raw", US, Falzar);
static BN6F_TENGU: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/tenguman_f_us.raw", US, Falzar);
static BN6F_GROUND: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/groundman_f_us.raw", US, Falzar);
static BN6F_DUST: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/dustman_f_us.raw", US, Falzar);
static BN6F_PROTO: LazyLock<tango_dataview::game::bn6::save::Save> =
    bn6_save!("../../../tango/src/game/bn6/save/protoman_f_us.raw", US, Falzar);
static BN6F_T: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> = LazyLock::new(|| {
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

struct BN6FImpl;
pub const BN6F: &'static (dyn Game + Send + Sync) = &BN6FImpl;
impl Game for BN6FImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::BR6E_00
    }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn6::BR6E_00
    }
    fn match_types(&self) -> &'static [usize] {
        MATCH_TYPES
    }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] {
        BN6F_T.as_slice()
    }
}

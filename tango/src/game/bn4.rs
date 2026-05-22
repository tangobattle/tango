use super::Game;
use std::sync::LazyLock;
use tango_dataview::save::Save as SaveTrait;

const MATCH_TYPES: &[usize] = &[2, 2];

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
static EXE4RS_DARK997: LazyLock<tango_dataview::game::bn4::save::Save> = bn4_save!(
    "../../../tango/src/game/bn4/save/dark_hp_997_rs_jp.raw",
    true,
    false,
    RedSun
);
static EXE4RS_LIGHT999: LazyLock<tango_dataview::game::bn4::save::Save> = bn4_save!(
    "../../../tango/src/game/bn4/save/light_hp_999_rs_jp.raw",
    true,
    false,
    RedSun
);
static EXE4RS_LIGHT1000: LazyLock<tango_dataview::game::bn4::save::Save> = bn4_save!(
    "../../../tango/src/game/bn4/save/light_hp_1000_rs_jp.raw",
    true,
    false,
    RedSun
);
static EXE4RS_T: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> = LazyLock::new(|| {
    vec![
        ("dark-hp-997", &*EXE4RS_DARK997 as &(dyn SaveTrait + Send + Sync)),
        ("light-hp-999", &*EXE4RS_LIGHT999),
        ("light-hp-1000", &*EXE4RS_LIGHT1000),
    ]
});

struct EXE4RSImpl;
pub const EXE4RS: &'static (dyn Game + Send + Sync) = &EXE4RSImpl;
impl Game for EXE4RSImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::B4WJ_01
    }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn4::B4WJ_01
    }
    fn match_types(&self) -> &'static [usize] {
        MATCH_TYPES
    }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] {
        EXE4RS_T.as_slice()
    }
}

// ---------------- EXE4 Blue Moon (JP) ----------------
static EXE4BM_DARK997: LazyLock<tango_dataview::game::bn4::save::Save> = bn4_save!(
    "../../../tango/src/game/bn4/save/dark_hp_997_bm_jp.raw",
    true,
    false,
    BlueMoon
);
static EXE4BM_LIGHT999: LazyLock<tango_dataview::game::bn4::save::Save> = bn4_save!(
    "../../../tango/src/game/bn4/save/light_hp_999_bm_jp.raw",
    true,
    false,
    BlueMoon
);
static EXE4BM_LIGHT1000: LazyLock<tango_dataview::game::bn4::save::Save> = bn4_save!(
    "../../../tango/src/game/bn4/save/light_hp_1000_bm_jp.raw",
    true,
    false,
    BlueMoon
);
static EXE4BM_T: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> = LazyLock::new(|| {
    vec![
        ("dark-hp-997", &*EXE4BM_DARK997 as &(dyn SaveTrait + Send + Sync)),
        ("light-hp-999", &*EXE4BM_LIGHT999),
        ("light-hp-1000", &*EXE4BM_LIGHT1000),
    ]
});

struct EXE4BMImpl;
pub const EXE4BM: &'static (dyn Game + Send + Sync) = &EXE4BMImpl;
impl Game for EXE4BMImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::B4BJ_01
    }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn4::B4BJ_01
    }
    fn match_types(&self) -> &'static [usize] {
        MATCH_TYPES
    }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] {
        EXE4BM_T.as_slice()
    }
}

// ---------------- BN4 Red Sun (US) ----------------
static BN4RS_DARK997: LazyLock<tango_dataview::game::bn4::save::Save> = bn4_save!(
    "../../../tango/src/game/bn4/save/dark_hp_997_rs_us.raw",
    false,
    true,
    RedSun
);
static BN4RS_LIGHT999: LazyLock<tango_dataview::game::bn4::save::Save> = bn4_save!(
    "../../../tango/src/game/bn4/save/light_hp_999_rs_us.raw",
    false,
    true,
    RedSun
);
static BN4RS_LIGHT1000: LazyLock<tango_dataview::game::bn4::save::Save> = bn4_save!(
    "../../../tango/src/game/bn4/save/light_hp_1000_rs_us.raw",
    false,
    true,
    RedSun
);
static BN4RS_T: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> = LazyLock::new(|| {
    vec![
        ("dark-hp-997", &*BN4RS_DARK997 as &(dyn SaveTrait + Send + Sync)),
        ("light-hp-999", &*BN4RS_LIGHT999),
        ("light-hp-1000", &*BN4RS_LIGHT1000),
    ]
});

struct BN4RSImpl;
pub const BN4RS: &'static (dyn Game + Send + Sync) = &BN4RSImpl;
impl Game for BN4RSImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::B4WE_00
    }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn4::B4WE_00
    }
    fn match_types(&self) -> &'static [usize] {
        MATCH_TYPES
    }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] {
        BN4RS_T.as_slice()
    }
}

// ---------------- BN4 Blue Moon (US) ----------------
static BN4BM_DARK997: LazyLock<tango_dataview::game::bn4::save::Save> = bn4_save!(
    "../../../tango/src/game/bn4/save/dark_hp_997_bm_us.raw",
    false,
    true,
    BlueMoon
);
static BN4BM_LIGHT999: LazyLock<tango_dataview::game::bn4::save::Save> = bn4_save!(
    "../../../tango/src/game/bn4/save/light_hp_999_bm_us.raw",
    false,
    true,
    BlueMoon
);
static BN4BM_LIGHT1000: LazyLock<tango_dataview::game::bn4::save::Save> = bn4_save!(
    "../../../tango/src/game/bn4/save/light_hp_1000_bm_us.raw",
    false,
    true,
    BlueMoon
);
static BN4BM_T: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> = LazyLock::new(|| {
    vec![
        ("dark-hp-997", &*BN4BM_DARK997 as &(dyn SaveTrait + Send + Sync)),
        ("light-hp-999", &*BN4BM_LIGHT999),
        ("light-hp-1000", &*BN4BM_LIGHT1000),
    ]
});

struct BN4BMImpl;
pub const BN4BM: &'static (dyn Game + Send + Sync) = &BN4BMImpl;
impl Game for BN4BMImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::B4BE_00
    }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn4::B4BE_00
    }
    fn match_types(&self) -> &'static [usize] {
        MATCH_TYPES
    }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] {
        BN4BM_T.as_slice()
    }
}

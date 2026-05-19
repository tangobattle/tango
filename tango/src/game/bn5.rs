use super::Game;
use std::sync::LazyLock;
use tango_dataview::save::Save as SaveTrait;

const MATCH_TYPES: &[usize] = &[1, 1];

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
    bn5_save!("../../../tango/src/game/bn5/save/dark_protoman_jp.raw", JP, Protoman);
static EXE5B_LIGHT: LazyLock<tango_dataview::game::bn5::save::Save> =
    bn5_save!("../../../tango/src/game/bn5/save/light_protoman_jp.raw", JP, Protoman);
static EXE5B_T: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> = LazyLock::new(|| {
    vec![
        ("dark", &*EXE5B_DARK as &(dyn SaveTrait + Send + Sync)),
        ("light", &*EXE5B_LIGHT),
    ]
});

struct EXE5BImpl;
pub const EXE5B: &'static (dyn Game + Send + Sync) = &EXE5BImpl;
impl Game for EXE5BImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) { &tango_gamedb::BRBJ_00 }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) { &tango_pvp::game::bn5::BRBJ_00 }
    fn match_types(&self) -> &'static [usize] { MATCH_TYPES }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] { EXE5B_T.as_slice() }
}

// ---------------- EXE5 Colonel JP ----------------
static EXE5C_DARK: LazyLock<tango_dataview::game::bn5::save::Save> =
    bn5_save!("../../../tango/src/game/bn5/save/dark_colonel_jp.raw", JP, Colonel);
static EXE5C_LIGHT: LazyLock<tango_dataview::game::bn5::save::Save> =
    bn5_save!("../../../tango/src/game/bn5/save/light_colonel_jp.raw", JP, Colonel);
static EXE5C_T: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> = LazyLock::new(|| {
    vec![
        ("dark", &*EXE5C_DARK as &(dyn SaveTrait + Send + Sync)),
        ("light", &*EXE5C_LIGHT),
    ]
});

struct EXE5CImpl;
pub const EXE5C: &'static (dyn Game + Send + Sync) = &EXE5CImpl;
impl Game for EXE5CImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) { &tango_gamedb::BRKJ_00 }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) { &tango_pvp::game::bn5::BRKJ_00 }
    fn match_types(&self) -> &'static [usize] { MATCH_TYPES }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] { EXE5C_T.as_slice() }
}

// ---------------- BN5 Protoman US ----------------
static BN5P_DARK: LazyLock<tango_dataview::game::bn5::save::Save> =
    bn5_save!("../../../tango/src/game/bn5/save/dark_protoman_us.raw", US, Protoman);
static BN5P_LIGHT: LazyLock<tango_dataview::game::bn5::save::Save> =
    bn5_save!("../../../tango/src/game/bn5/save/light_protoman_us.raw", US, Protoman);
static BN5P_T: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> = LazyLock::new(|| {
    vec![
        ("dark", &*BN5P_DARK as &(dyn SaveTrait + Send + Sync)),
        ("light", &*BN5P_LIGHT),
    ]
});

struct BN5PImpl;
pub const BN5P: &'static (dyn Game + Send + Sync) = &BN5PImpl;
impl Game for BN5PImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) { &tango_gamedb::BRBE_00 }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) { &tango_pvp::game::bn5::BRBE_00 }
    fn match_types(&self) -> &'static [usize] { MATCH_TYPES }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] { BN5P_T.as_slice() }
}

// ---------------- BN5 Colonel US ----------------
static BN5C_DARK: LazyLock<tango_dataview::game::bn5::save::Save> =
    bn5_save!("../../../tango/src/game/bn5/save/dark_colonel_us.raw", US, Colonel);
static BN5C_LIGHT: LazyLock<tango_dataview::game::bn5::save::Save> =
    bn5_save!("../../../tango/src/game/bn5/save/light_colonel_us.raw", US, Colonel);
static BN5C_T: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> = LazyLock::new(|| {
    vec![
        ("dark", &*BN5C_DARK as &(dyn SaveTrait + Send + Sync)),
        ("light", &*BN5C_LIGHT),
    ]
});

struct BN5CImpl;
pub const BN5C: &'static (dyn Game + Send + Sync) = &BN5CImpl;
impl Game for BN5CImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) { &tango_gamedb::BRKE_00 }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) { &tango_pvp::game::bn5::BRKE_00 }
    fn match_types(&self) -> &'static [usize] { MATCH_TYPES }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] { BN5C_T.as_slice() }
}

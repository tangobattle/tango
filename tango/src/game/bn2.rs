use super::Game;
use std::sync::LazyLock;
use tango_dataview::save::Save as SaveTrait;

const MATCH_TYPES: &[usize] = &[1];

macro_rules! bn2_save {
    ($file:expr) => {
        LazyLock::new(|| tango_dataview::game::bn2::save::Save::from_wram(include_bytes!($file)).unwrap())
    };
}

static HUB_ANY: LazyLock<tango_dataview::game::bn2::save::Save> =
    bn2_save!("../../../tango/src/game/bn2/save/hub_any.raw");
static GUTS_ANY: LazyLock<tango_dataview::game::bn2::save::Save> =
    bn2_save!("../../../tango/src/game/bn2/save/guts_any.raw");
static CUSTOM_ANY: LazyLock<tango_dataview::game::bn2::save::Save> =
    bn2_save!("../../../tango/src/game/bn2/save/custom_any.raw");
static TEAM_ANY: LazyLock<tango_dataview::game::bn2::save::Save> =
    bn2_save!("../../../tango/src/game/bn2/save/team_any.raw");
static SHIELD_ANY: LazyLock<tango_dataview::game::bn2::save::Save> =
    bn2_save!("../../../tango/src/game/bn2/save/shield_any.raw");

static TEMPLATES: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> = LazyLock::new(|| {
    vec![
        ("hub", &*HUB_ANY as &(dyn SaveTrait + Send + Sync)),
        ("guts", &*GUTS_ANY as &(dyn SaveTrait + Send + Sync)),
        ("custom", &*CUSTOM_ANY as &(dyn SaveTrait + Send + Sync)),
        ("team", &*TEAM_ANY as &(dyn SaveTrait + Send + Sync)),
        ("shield", &*SHIELD_ANY as &(dyn SaveTrait + Send + Sync)),
    ]
});

struct EXE2Impl;
pub const EXE2: &'static (dyn Game + Send + Sync) = &EXE2Impl;

impl Game for EXE2Impl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::AE2J_00_AC
    }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn2::AE2J_00_AC
    }
    fn match_types(&self) -> &'static [usize] {
        MATCH_TYPES
    }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] {
        TEMPLATES.as_slice()
    }
}

struct BN2Impl;
pub const BN2: &'static (dyn Game + Send + Sync) = &BN2Impl;

impl Game for BN2Impl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::AE2E_00
    }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn2::AE2E_00
    }
    fn match_types(&self) -> &'static [usize] {
        MATCH_TYPES
    }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] {
        TEMPLATES.as_slice()
    }
}

use crate::game;

const MATCH_TYPES: &[usize] = &[2, 2];

struct EXE4RSImpl;
pub const EXE4RS: &'static (dyn game::Game + Send + Sync) = &EXE4RSImpl {};

impl game::Game for EXE4RSImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::B4WJ_01
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_HP_997_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/dark_hp_997_rs_jp.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region { jp: true, us: false },
                        variant: tango_dataview::game::bn4::save::Variant::RedSun
                    }
                )
                .unwrap();
            static ref LIGHT_HP_999_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_999_rs_jp.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region { jp: true, us: false },
                        variant: tango_dataview::game::bn4::save::Variant::RedSun
                    }
                )
                .unwrap();
            static ref LIGHT_HP_1000_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_1000_rs_jp.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region { jp: true, us: false },
                        variant: tango_dataview::game::bn4::save::Variant::RedSun
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                (
                    "dark-hp-997",
                    &*DARK_HP_997_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-999",
                    &*LIGHT_HP_999_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-1000",
                    &*LIGHT_HP_1000_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
            ];
        }
        TEMPLATES.as_slice()
    }
}

struct EXE4BMImpl;
pub const EXE4BM: &'static (dyn game::Game + Send + Sync) = &EXE4BMImpl {};

impl game::Game for EXE4BMImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::B4BJ_01
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_HP_997_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/dark_hp_997_bm_jp.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region { jp: true, us: false },
                        variant: tango_dataview::game::bn4::save::Variant::BlueMoon
                    }
                )
                .unwrap();
            static ref LIGHT_HP_999_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_999_bm_jp.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region { jp: true, us: false },
                        variant: tango_dataview::game::bn4::save::Variant::BlueMoon
                    }
                )
                .unwrap();
            static ref LIGHT_HP_1000_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_1000_bm_jp.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region { jp: true, us: false },
                        variant: tango_dataview::game::bn4::save::Variant::BlueMoon
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                (
                    "dark-hp-997",
                    &*DARK_HP_997_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-999",
                    &*LIGHT_HP_999_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-1000",
                    &*LIGHT_HP_1000_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
            ];
        }
        TEMPLATES.as_slice()
    }
}

struct BN4RSImpl;
pub const BN4RS: &'static (dyn game::Game + Send + Sync) = &BN4RSImpl {};

impl game::Game for BN4RSImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::B4WE_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_HP_997_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/dark_hp_997_rs_us.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region { jp: false, us: true },
                        variant: tango_dataview::game::bn4::save::Variant::RedSun
                    }
                )
                .unwrap();
            static ref LIGHT_HP_999_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_999_rs_us.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region { jp: false, us: true },
                        variant: tango_dataview::game::bn4::save::Variant::RedSun
                    }
                )
                .unwrap();
            static ref LIGHT_HP_1000_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_1000_rs_us.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region { jp: false, us: true },
                        variant: tango_dataview::game::bn4::save::Variant::RedSun
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                (
                    "dark-hp-997",
                    &*DARK_HP_997_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-999",
                    &*LIGHT_HP_999_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-1000",
                    &*LIGHT_HP_1000_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
            ];
        }
        TEMPLATES.as_slice()
    }
}

struct BN4BMImpl;
pub const BN4BM: &'static (dyn game::Game + Send + Sync) = &BN4BMImpl {};

impl game::Game for BN4BMImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::B4BE_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_HP_997_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/dark_hp_997_bm_us.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region { jp: false, us: true },
                        variant: tango_dataview::game::bn4::save::Variant::BlueMoon
                    }
                )
                .unwrap();
            static ref LIGHT_HP_999_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_999_bm_us.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region { jp: false, us: true },
                        variant: tango_dataview::game::bn4::save::Variant::BlueMoon
                    }
                )
                .unwrap();
            static ref LIGHT_HP_1000_SAVE: tango_dataview::game::bn4::save::Save =
                tango_dataview::game::bn4::save::Save::from_wram(
                    include_bytes!("bn4/save/light_hp_1000_bm_us.raw"),
                    tango_dataview::game::bn4::save::GameInfo {
                        region: tango_dataview::game::bn4::save::Region { jp: false, us: true },
                        variant: tango_dataview::game::bn4::save::Variant::BlueMoon
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                (
                    "dark-hp-997",
                    &*DARK_HP_997_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-999",
                    &*LIGHT_HP_999_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "light-hp-1000",
                    &*LIGHT_HP_1000_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
            ];
        }
        TEMPLATES.as_slice()
    }
}

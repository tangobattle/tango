use crate::game;

const MATCH_TYPES: &[usize] = &[2, 2];

struct EXE5BImpl;
pub const EXE5B: &'static (dyn game::Game + Send + Sync) = &EXE5BImpl {};

impl game::Game for EXE5BImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::BRBJ_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/dark_protoman_jp.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::JP,
                        variant: tango_dataview::game::bn5::save::Variant::Protoman
                    }
                )
                .unwrap();
            static ref LIGHT_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/light_protoman_jp.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::JP,
                        variant: tango_dataview::game::bn5::save::Variant::Protoman
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                ("dark", &*DARK_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
                ("light", &*LIGHT_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
            ];
        }
        TEMPLATES.as_slice()
    }
}

struct EXE5CImpl;
pub const EXE5C: &'static (dyn game::Game + Send + Sync) = &EXE5CImpl {};

impl game::Game for EXE5CImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::BRKJ_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/dark_colonel_jp.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::JP,
                        variant: tango_dataview::game::bn5::save::Variant::Colonel
                    }
                )
                .unwrap();
            static ref LIGHT_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/light_colonel_jp.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::JP,
                        variant: tango_dataview::game::bn5::save::Variant::Colonel
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                ("dark", &*DARK_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
                ("light", &*LIGHT_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
            ];
        }
        TEMPLATES.as_slice()
    }
}

struct BN5PImpl;
pub const BN5P: &'static (dyn game::Game + Send + Sync) = &BN5PImpl {};

impl game::Game for BN5PImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::BRBE_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/dark_protoman_us.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::US,
                        variant: tango_dataview::game::bn5::save::Variant::Protoman
                    }
                )
                .unwrap();
            static ref LIGHT_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/light_protoman_us.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::US,
                        variant: tango_dataview::game::bn5::save::Variant::Protoman
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                ("dark", &*DARK_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
                ("light", &*LIGHT_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
            ];
        }
        TEMPLATES.as_slice()
    }
}

struct BN5CImpl;
pub const BN5C: &'static (dyn game::Game + Send + Sync) = &BN5CImpl {};

impl game::Game for BN5CImpl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::BRKE_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/dark_colonel_us.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::US,
                        variant: tango_dataview::game::bn5::save::Variant::Colonel
                    }
                )
                .unwrap();
            static ref LIGHT_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/light_colonel_us.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::US,
                        variant: tango_dataview::game::bn5::save::Variant::Colonel
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                ("dark", &*DARK_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
                ("light", &*LIGHT_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
            ];
        }
        TEMPLATES.as_slice()
    }
}

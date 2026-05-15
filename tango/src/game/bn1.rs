use crate::game;

const MATCH_TYPES: &[usize] = &[1];

struct EXE1Impl;
pub const EXE1: &'static (dyn game::Game + Send + Sync) = &EXE1Impl {};

impl game::Game for EXE1Impl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::AREJ_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref SAVE: tango_dataview::game::bn1::save::Save = tango_dataview::game::bn1::save::Save::from_wram(
                include_bytes!("bn1/save/jp.raw"),
                tango_dataview::game::bn1::save::GameInfo {
                    region: tango_dataview::game::bn1::save::Region::JP,
                }
            )
            .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> =
                vec![("", &*SAVE as &(dyn tango_dataview::save::Save + Send + Sync))];
        }
        TEMPLATES.as_slice()
    }
}

struct BN1Impl;
pub const BN1: &'static (dyn game::Game + Send + Sync) = &BN1Impl {};

impl game::Game for BN1Impl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::AREE_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref SAVE: tango_dataview::game::bn1::save::Save = tango_dataview::game::bn1::save::Save::from_wram(
                include_bytes!("bn1/save/us.raw"),
                tango_dataview::game::bn1::save::GameInfo {
                    region: tango_dataview::game::bn1::save::Region::US,
                }
            )
            .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> =
                vec![("", &*SAVE as &(dyn tango_dataview::save::Save + Send + Sync))];
        }
        TEMPLATES.as_slice()
    }
}

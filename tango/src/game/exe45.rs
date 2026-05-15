use crate::game;

const MATCH_TYPES: &[usize] = &[1, 1];

struct EXE45Impl;
pub const EXE45: &'static (dyn game::Game + Send + Sync) = &EXE45Impl {};

impl game::Game for EXE45Impl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::BR4J_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref SAVE: tango_dataview::game::exe45::save::Save =
                tango_dataview::game::exe45::save::Save::from_wram(include_bytes!("exe45/save/any.raw")).unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> =
                vec![("", &*SAVE as &(dyn tango_dataview::save::Save + Send + Sync))];
        }
        TEMPLATES.as_slice()
    }
}

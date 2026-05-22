use super::Game;
use std::sync::LazyLock;
use tango_dataview::save::Save as SaveTrait;

const MATCH_TYPES: &[usize] = &[1];

struct EXE45Impl;
pub const EXE45: &'static (dyn Game + Send + Sync) = &EXE45Impl;

impl Game for EXE45Impl {
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync) {
        &tango_gamedb::BR4J_00
    }
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::exe45::BR4J_00
    }
    fn match_types(&self) -> &'static [usize] {
        MATCH_TYPES
    }
    fn save_templates(&self) -> &'static [(&'static str, &'static (dyn SaveTrait + Send + Sync))] {
        static SAVE: LazyLock<tango_dataview::game::exe45::save::Save> = LazyLock::new(|| {
            tango_dataview::game::exe45::save::Save::from_wram(include_bytes!(
                "../../../tango/src/game/exe45/save/any.raw"
            ))
            .unwrap()
        });
        static T: LazyLock<Vec<(&'static str, &'static (dyn SaveTrait + Send + Sync))>> =
            LazyLock::new(|| vec![("", &*SAVE as &(dyn SaveTrait + Send + Sync))]);
        T.as_slice()
    }
}

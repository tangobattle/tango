use super::{BackgroundRef, Game, LazyImage, SaveTemplates};
use crate::bnlc;
use std::sync::LazyLock;
use tango_dataview::save::Save as SaveTrait;

const MATCH_TYPES: &[usize] = &[1];
const BACKGROUND: BackgroundRef = BackgroundRef {
    volume: bnlc::Volume::Vol1,
    tga: "04.tga",
};
static LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("../../logos/exe45.png")).unwrap());

static EXE45_SAVE: LazyLock<tango_dataview::game::exe45::save::Save> =
    LazyLock::new(|| tango_dataview::game::exe45::save::Save::from_wram(include_bytes!("save/any.raw")).unwrap());
static EXE45_T: SaveTemplates = LazyLock::new(|| vec![("", &*EXE45_SAVE as &(dyn SaveTrait + Send + Sync))]);

pub static EXE45: Game = Game {
    gamedb_entry: &tango_gamedb::BR4J_00,
    hooks: &tango_pvp::game::exe45::BR4J_00,
    match_types: MATCH_TYPES,
    save_templates: &EXE45_T,
    logo_image: &LOGO,
    background: BACKGROUND,
};

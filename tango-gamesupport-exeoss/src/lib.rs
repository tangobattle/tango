//! Rockman.EXE Operate Shooting Star.
//!
//! A DS remake of the first Battle Network with a Star Force crossover
//! bolted on, released in Japan only — so one build, one ROM revision,
//! and a family of its own.
//!
//! Netplay here runs on melonDS the way BN5DS's does: two emulated DSes
//! talking over emulated local wireless, with the pair as the rollback
//! unit. The games run their real wireless protocol — discovery,
//! association, the host's command/reply rounds — so a link battle
//! behaves exactly as it does on hardware.

pub use tango_gamesupport_exeoss_dataview as dataview;

pub mod pvp;

use tango_gamesupport::{BackgroundRef, Family, Game, Region, Volume};

/// One mode with one subtype, until the cart's own comm screens say
/// otherwise. BN1 had a single battle mode and this is its remake; what
/// the wireless menus actually offer is a question for the walk.
const MATCH_TYPES: &[usize] = &[1];
const BACKGROUND: BackgroundRef = BackgroundRef {
    volume: Volume::Vol1,
    tga: "01.tga",
};

/// The melonDS engine over the cart's priming walk — what the
/// registration hands Tango as its [`tango_match::Backend`].
static ENGINE_PVP_B6XJ_00: tango_backend_melonds::DsBackend = tango_backend_melonds::DsBackend::new(&pvp::JP);

/// The cart's save, identified so one can be picked before a match. The
/// interior is unmapped — this recognizes the dump and round-trips its
/// bytes, which is what putting it in the save picker needs.
fn parse_save(data: &[u8]) -> Result<tango_gamesupport::BoxedSave, tango_gamesupport::Error> {
    let save = dataview::save::Save::new(data)?;
    Ok(tango_gamesupport_common::dataview::wrap_save(Box::new(save)))
}

/// The only release: Japan, one revision.
pub static EXEOSS: Game = Game {
    family: &EXEOSS_FAMILY,
    variant: 0,
    rom_code: b"B6XJ",
    revision: 0x00,
    crc32: 0x0644_1e8b,
    region: Region::JP,

    parse_save_fn: |data| parse_save(data),
    // No save or ROM model yet, so nothing to decode the cart's assets
    // into.
    load_rom_assets_fn: None,

    pvp: &ENGINE_PVP_B6XJ_00,

    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    save_templates: None,
    logo_image: None,
    // No BNLC release to borrow art from — the remake never shipped in
    // the Legacy Collection.
    background: Some(BACKGROUND),
    #[cfg(feature = "ui")]
    save_editor: &tango_gamesupport_common::editor::EMPTY_SAVE_EDITOR,
};

/// This family's simulation version. See [`Family::sim_version`] for
/// what it gates and what warrants a bump.
///
/// 0: the first shipped walk.
const SIM_VERSION: u32 = 0;

pub static EXEOSS_FAMILY: Family = Family {
    id: "exeoss",
    sim_version: SIM_VERSION,
    games: &[&EXEOSS],
    translations: &[("en-US", include_str!("../locales/en-US/exeoss.ftl"))],
};

/// Every game family this crate provides.
pub static FAMILIES: &[&Family] = &[&EXEOSS_FAMILY];

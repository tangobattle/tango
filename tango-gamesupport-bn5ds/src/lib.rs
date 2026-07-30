//! MegaMan Battle Network 5: Double Team DS.
//!
//! One cart carrying both Team ProtoMan and Team Colonel — the player
//! picks a side per save, so there is a single ROM revision rather than
//! the paired versions the GBA games ship as.
//!
//! Netplay here runs on [`melonds_rollback`]: two emulated DSes talking
//! over emulated local wireless, with the pair as the rollback unit.
//! The games run their real wireless protocol — discovery, association,
//! the host's command/reply rounds — so a link battle behaves exactly
//! as it does on hardware.

pub use tango_gamesupport_bn5ds_dataview as dataview;
#[cfg(feature = "ui")]
pub use tango_gamesupport_bn5ds_ui as ui;

pub mod pvp;

use std::sync::LazyLock;
use tango_gamesupport::{BackgroundRef, Family, Game, LazyImage, Region, Volume};

/// Single Battle and Triple Battle, each with no subtypes. Both run as
/// Practice: Real Thing spends the players' own records on the result,
/// which is not netplay's to spend.
const MATCH_TYPES: &[usize] = &[1, 1];

const BACKGROUND: BackgroundRef = BackgroundRef {
    volume: Volume::Vol2,
    tga: "16.tga",
};

/// The melonDS engine over each build's priming walk — what the
/// registrations hand Tango as their [`tango_match::Backend`]s.
static ENGINE_PVP_A5TE_00: tango_backend_melonds::DsBackend = tango_backend_melonds::DsBackend::new(&pvp::US);
static ENGINE_PVP_A5TJ_00: tango_backend_melonds::DsBackend = tango_backend_melonds::DsBackend::new(&pvp::JP);

static BN5DS_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/bn5ds-0.png")).unwrap());

/// The cartridge's saves, identified so one can be picked before a
/// match — and read as far as the dataview's mapping reaches. A dump
/// holds both of the game's files, so this opens on whichever was saved
/// most recently and the editor's file picker reaches the other. Saves
/// cross regions: the two builds keep one format.
fn parse_save(data: &[u8]) -> Result<tango_gamesupport::BoxedSave, tango_gamesupport::Error> {
    let set = dataview::save::SaveSet::parse(data)?;
    Ok(tango_gamesupport_common::dataview::wrap_save(Box::new(set.current())))
}

/// The US release. One ROM rather than the paired versions the GBA
/// games ship as: a single cart carries both teams and the player picks
/// a side per save.
pub static BN5DS: Game = Game {
    family: &BN5DS_FAMILY,
    variant: 0,
    rom_code: b"A5TE",
    revision: 0x00,
    crc32: 0x16f0_3f13,
    region: Region::US,

    parse_save_fn: |data| parse_save(data),
    // The DS cart has no wram-derived assets — everything comes off
    // the cart image itself.
    load_rom_assets_fn: Some(|rom, _wram, charset| {
        tango_gamesupport_common::dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::A5TE_00,
            charset.unwrap_or(dataview::rom::EN_CHARSET),
            rom.to_vec(),
        )))
    }),

    pvp: &ENGINE_PVP_A5TE_00,

    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    save_templates: None,
    logo_image: Some(&BN5DS_LOGO),
    background: Some(BACKGROUND),
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
};

/// The JP release, Rockman EXE 5 DS: Twin Leaders. Its own family, as
/// with the GBA pairs: netplay compatibility is family-scoped, and the
/// two builds could not play each other anyway — the wireless protocol
/// the games run carries the build's own identity.
pub static EXE5DS: Game = Game {
    family: &EXE5DS_FAMILY,
    variant: 0,
    rom_code: b"A5TJ",
    revision: 0x00,
    crc32: 0x9fdf_2ece,
    region: Region::JP,

    parse_save_fn: |data| parse_save(data),
    load_rom_assets_fn: Some(|rom, _wram, charset| {
        tango_gamesupport_common::dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::A5TJ_00,
            charset.unwrap_or(dataview::rom::JA_CHARSET),
            rom.to_vec(),
        )))
    }),

    pvp: &ENGINE_PVP_A5TJ_00,
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    save_templates: None,
    logo_image: None,
    background: Some(BACKGROUND),
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
};

/// This crate's families' replay compatibility version — one engine
/// serves them all, so a change that invalidates one family's
/// recordings invalidates its siblings' too. See
/// [`Family::replay_version`] for what warrants a bump.
///
/// 1: priming hands off when the battle transition starts (the board
/// module's departure) instead of when the battle module arrives, a
/// few frames earlier — recordings made against the old finish line
/// carry inputs offset by that gap.
const REPLAY_VERSION: u32 = 1;

pub static BN5DS_FAMILY: Family = Family {
    id: "bn5ds",
    replay_version: REPLAY_VERSION,
    games: &[&BN5DS],
    translations: &[("en-US", include_str!("../locales/en-US/bn5ds.ftl"))],
};

pub static EXE5DS_FAMILY: Family = Family {
    id: "exe5ds",
    replay_version: REPLAY_VERSION,
    games: &[&EXE5DS],
    translations: &[("en-US", include_str!("../locales/en-US/exe5ds.ftl"))],
};

/// Every game family this crate provides.
pub static FAMILIES: &[&Family] = &[&EXE5DS_FAMILY, &BN5DS_FAMILY];

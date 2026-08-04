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
use tango_gamesupport::{BackgroundRef, Family, Game, LazyImage, Region, SaveTemplates, Volume};

/// Single Battle and Triple Battle, each in a plain and a Team subtype.
/// The two kinds are separate routes off the Network board rather than
/// separate modes on one screen — Team Battle is its own board button,
/// and the mode chooser past it offers the same two buttons the plain
/// route's does, which is what makes team a subtype of each rather than
/// a third and fourth mode.
///
/// All four run as Practice: Real Thing spends the players' own records
/// on the result, which is not netplay's to spend.
const MATCH_TYPES: &[usize] = &[2, 2];

const BACKGROUND: BackgroundRef = BackgroundRef {
    volume: Volume::Vol2,
    tga: "16.tga",
};

/// The melonDS engine over each build's priming walk — what the
/// registrations hand Tango as their [`tango_match::Backend`]s.
static ENGINE_PVP_A5TE_00: tango_backend_melonds::DsBackend = tango_backend_melonds::DsBackend::new(&pvp::US);
static ENGINE_PVP_A5TJ_00: tango_backend_melonds::DsBackend = tango_backend_melonds::DsBackend::new(&pvp::JP);

static BN5DS_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/bn5ds-0.png")).unwrap());

/// The bundled template: a finished US cartridge, stripped to its two
/// files' live block pairs — one file per team, both stories cleared.
/// A dump's other blocks are only history (a stale generation of one
/// file) and erased flash, and [`dataview::save::SaveSet::parse`]
/// already reconstitutes a short dump by padding it back out, so the
/// pairs are all that needs checking in. What the cart wore rather than
/// wrote is normalized away with them: the generation counters are
/// renumbered down to 1 and 2 (only their order decides which file is
/// current), and the fill outside the image and the footer — flash the
/// game neither reads nor checksums — is zeroed. The template save
/// itself is the cart's current file; the other rides along in the
/// dump, where the editor's file picker reaches it. Both builds share
/// it: saves cross regions (see [`parse_save`]), a US dump included.
static BN5DS_SAVE: LazyLock<dataview::save::Save> =
    LazyLock::new(|| dataview::save::SaveSet::parse(include_bytes!("saves/us.raw")).unwrap().current());
static BN5DS_T: SaveTemplates = LazyLock::new(|| {
    vec![(
        "",
        tango_gamesupport_common::dataview::wrap_save(Box::new(BN5DS_SAVE.clone())),
    )]
});

/// The cartridge's saves, identified so one can be picked before a
/// match — and read as far as the dataview's mapping reaches. A dump
/// holds both of the game's files, so this opens on whichever the game
/// itself calls most recently saved, which is also the one a session
/// plays; the editor's file picker moves that. Saves cross regions: the
/// two builds keep one format.
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
    save_templates: Some(&BN5DS_T),
    logo_image: Some(&BN5DS_LOGO),
    background: Some(BACKGROUND),
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
    save_templates: Some(&BN5DS_T),
    logo_image: None,
    background: Some(BACKGROUND),
};

/// Expands to this crate's per-locale Fluent fragments for `$fam` (bare
/// keys; the family supplies the namespace).
macro_rules! family_translations {
    ($fam:literal) => {
        &[
            ("de-DE", include_str!(concat!("../locales/de-DE/", $fam, ".ftl"))),
            ("en-US", include_str!(concat!("../locales/en-US/", $fam, ".ftl"))),
            ("es-419", include_str!(concat!("../locales/es-419/", $fam, ".ftl"))),
            ("fr-FR", include_str!(concat!("../locales/fr-FR/", $fam, ".ftl"))),
            ("ja-JP", include_str!(concat!("../locales/ja-JP/", $fam, ".ftl"))),
            ("nl-NL", include_str!(concat!("../locales/nl-NL/", $fam, ".ftl"))),
            ("pt-BR", include_str!(concat!("../locales/pt-BR/", $fam, ".ftl"))),
            ("ru-RU", include_str!(concat!("../locales/ru-RU/", $fam, ".ftl"))),
            ("vi-VN", include_str!(concat!("../locales/vi-VN/", $fam, ".ftl"))),
            ("zh-CN", include_str!(concat!("../locales/zh-CN/", $fam, ".ftl"))),
            ("zh-TW", include_str!(concat!("../locales/zh-TW/", $fam, ".ftl"))),
        ]
    };
}

pub static BN5DS_FAMILY: Family = Family {
    id: "bn5ds",
    games: &[&BN5DS],
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
    translations: family_translations!("bn5ds"),
};

pub static EXE5DS_FAMILY: Family = Family {
    id: "exe5ds",
    games: &[&EXE5DS],
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
    translations: family_translations!("exe5ds"),
};

/// Every game family this crate provides.
pub static FAMILIES: &[&Family] = &[&EXE5DS_FAMILY, &BN5DS_FAMILY];

//! Game support for Mega Man Battle Chip Challenge / Rockman EXE Battle
//! Chip GP.
//!
//! [`dataview`] is the save/ROM data layer, [`pvp`] the SIO-engine
//! support. The `&'static` [`Game`] registrations below bundle those
//! with the ROM identity, parsers, and presentation bits.

pub use tango_gamesupport_bcc_dataview as dataview;
#[cfg(feature = "ui")]
pub use tango_gamesupport_bcc_ui as ui;

pub mod pvp;

use std::sync::LazyLock;
use tango_gamesupport::{BackgroundRef, Error, Family, Game, LazyImage, Region, SaveTemplates, Volume};

/// Link battle: mode 0 = Normal, mode 1 = Random. (The menu's third
/// entry, Guest, plays off a deck the other player sends over, so it is
/// not a netplay match type.)
const MATCH_TYPES: &[usize] = &[1, 1];

const BACKGROUND: BackgroundRef = BackgroundRef {
    volume: Volume::Vol1,
    tga: "04.tga",
};

/// The bundled starter save. BCC's payload carries no region marker and
/// both games read each other's saves, so one template serves both.
/// Every cartridge in this family, as its ROM header names it.
///
/// A match needs both seats' engine support and a factory only
/// holds its own, so this is where the peer's gets resolved — the
/// one place that knows what this family's siblings are.
pub static FAMILY: &[tango_backend_mgba::Seat] = &[(b"A89E", 0, &pvp::PVP_A89E_00), (b"A89J", 0, &pvp::PVP_A89J_00)];

static ENGINE_PVP_A89E_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_A89E_00, FAMILY);
static ENGINE_PVP_A89J_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_A89J_00, FAMILY);

static SAVE: LazyLock<dataview::save::Save> =
    LazyLock::new(|| dataview::save::Save::from_wram(include_bytes!("saves/default.raw")).unwrap());
static TEMPLATES: SaveTemplates = LazyLock::new(|| {
    vec![(
        "",
        tango_gamesupport_common_dataview::wrap_save(Box::new(SAVE.clone())),
    )]
});

fn parse_save(data: &[u8]) -> Result<tango_gamesupport::BoxedSave, Error> {
    Ok(tango_gamesupport_common_dataview::wrap_save(Box::new(
        dataview::save::Save::new(data)?,
    )))
}

// ---------------- BCC US (A89E_00) ----------------
static BCC_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/bcc-0.png")).unwrap());

pub static BCC: Game = Game {
    family: &BCC_FAMILY,
    variant: 0,
    rom_code: b"A89E",
    revision: 0x00,
    crc32: 0x26be44fd,
    rom_size: 0x800000,
    region: Region::US,
    parse_save_fn: parse_save,
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common_dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::A89E_00,
            charset.unwrap_or(dataview::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_A89E_00,
    save_templates: Some(&TEMPLATES),
    logo_image: Some(&BCC_LOGO),
    background: Some(BACKGROUND),
};

// ---------------- EXEBCGP JP (A89J_00) ----------------
static EXEBCGP_LOGO: LazyImage =
    LazyLock::new(|| image::load_from_memory(include_bytes!("logos/exebcgp-0.png")).unwrap());

pub static EXEBCGP: Game = Game {
    family: &EXEBCGP_FAMILY,
    variant: 0,
    rom_code: b"A89J",
    revision: 0x00,
    crc32: 0x9217fb18,
    rom_size: 0x800000,
    region: Region::JP,
    parse_save_fn: parse_save,
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common_dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::A89J_00,
            charset.unwrap_or(dataview::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_A89J_00,
    save_templates: Some(&TEMPLATES),
    logo_image: Some(&EXEBCGP_LOGO),
    background: Some(BACKGROUND),
};

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

pub static BCC_FAMILY: Family = Family {
    id: "bcc",
    games: &[&BCC],
    match_types: MATCH_TYPES,
    players_colored_by_seat: true,
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
    translations: family_translations!("bcc"),
};

pub static EXEBCGP_FAMILY: Family = Family {
    id: "exebcgp",
    games: &[&EXEBCGP],
    match_types: MATCH_TYPES,
    players_colored_by_seat: true,
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
    translations: family_translations!("exebcgp"),
};

/// Every game family this crate provides.
pub static FAMILIES: &[&Family] = &[&BCC_FAMILY, &EXEBCGP_FAMILY];

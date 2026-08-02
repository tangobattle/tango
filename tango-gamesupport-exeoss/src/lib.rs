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
#[cfg(feature = "ui")]
pub use tango_gamesupport_exeoss_ui as ui;

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

/// The cart's save, identified so one can be picked before a match and
/// read — and edited — as far as the dataview's mapping reaches, which
/// is the chip folder. A dump holds one in-game file, so there is
/// nothing to pick between: the parse opens on whichever of its two
/// banks the console would, the newest one whose stamps check out.
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
    // The DS cart has no wram-derived assets — everything comes off the
    // cart image itself, and there is one charset, the cart being
    // Japan-only.
    load_rom_assets_fn: Some(|rom, _wram, charset| {
        tango_gamesupport_common::dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::B6XJ_00,
            charset.unwrap_or(dataview::rom::JA_CHARSET),
            rom.to_vec(),
        )))
    }),

    pvp: &ENGINE_PVP_B6XJ_00,

    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    save_templates: None,
    logo_image: None,
    // No BNLC release to borrow art from — the remake never shipped in
    // the Legacy Collection.
    background: Some(BACKGROUND),
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
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

pub static EXEOSS_FAMILY: Family = Family {
    id: "exeoss",
    games: &[&EXEOSS],
    translations: family_translations!("exeoss"),
};

/// Every game family this crate provides.
pub static FAMILIES: &[&Family] = &[&EXEOSS_FAMILY];

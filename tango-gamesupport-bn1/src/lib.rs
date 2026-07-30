//! Game support for Mega Man Battle Network 1 / Rockman EXE 1.
//!
//! [`dataview`] is the save/ROM data layer, [`pvp_hooks`] the PvP rollback
//! hooks. The `&'static` [`Game`] registrations below bundle those with the
//! ROM identity, parsers, and presentation bits; [`GAMES`] lists them.

pub use tango_gamesupport_bn1_dataview as dataview;
#[cfg(feature = "ui")]
pub use tango_gamesupport_bn1_ui as ui;

pub mod pvp;

use std::sync::LazyLock;
use tango_gamesupport::{BackgroundRef, Error, Family, Game, LazyImage, Region, SaveTemplates, Volume};

const MATCH_TYPES: &[usize] = &[1];
const BACKGROUND: BackgroundRef = BackgroundRef {
    volume: Volume::Vol1,
    tga: "01.tga",
};

// ---------------- EXE1 JP (AREJ_00) ----------------
/// Every cartridge in this family, as its ROM header names it.
///
/// A match needs both seats' engine support and a factory only
/// holds its own, so this is where the peer's gets resolved — the
/// one place that knows what this family's siblings are.
pub static FAMILY: &[tango_backend_mgba::Seat] = &[
    (b"AREJ", 0, &pvp::PVP_AREJ_00),
    (b"AREE", 0, &pvp::PVP_AREE_00),
];

static ENGINE_PVP_AREJ_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_AREJ_00, FAMILY);
static ENGINE_PVP_AREE_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_AREE_00, FAMILY);

static EXE1_SAVE: LazyLock<dataview::save::Save> = LazyLock::new(|| {
    dataview::save::Save::from_wram(
        include_bytes!("saves/jp.raw"),
        dataview::save::GameInfo {
            region: dataview::save::Region::JP,
        },
    )
    .unwrap()
});
static EXE1_T: SaveTemplates = LazyLock::new(|| {
    vec![(
        "",
        tango_gamesupport_common::dataview::wrap_save(Box::new(EXE1_SAVE.clone())),
    )]
});
static EXE1_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/exe1-0.png")).unwrap());

pub static EXE1: Game = Game {
    family: &EXE1_FAMILY,
    variant: 0,
    rom_code: b"AREJ",
    revision: 0x00,
    crc32: 0xd9516e50,
    region: Region::JP,
    parse_save_fn: |data| {
        let save = dataview::save::Save::new(data)?;
        if save.game_info()
            != &(dataview::save::GameInfo {
                region: dataview::save::Region::JP,
            })
        {
            return Err(Error::IncompatibleSave);
        }
        Ok(tango_gamesupport_common::dataview::wrap_save(Box::new(save)))
    },
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common::dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::AREJ_00,
            charset.unwrap_or(dataview::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_AREJ_00,
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    save_templates: Some(&EXE1_T),
    logo_image: Some(&EXE1_LOGO),
    background: Some(BACKGROUND),
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
};

// ---------------- BN1 US (AREE_00) ----------------
static BN1_SAVE: LazyLock<dataview::save::Save> = LazyLock::new(|| {
    dataview::save::Save::from_wram(
        include_bytes!("saves/us.raw"),
        dataview::save::GameInfo {
            region: dataview::save::Region::US,
        },
    )
    .unwrap()
});
static BN1_T: SaveTemplates = LazyLock::new(|| {
    vec![(
        "",
        tango_gamesupport_common::dataview::wrap_save(Box::new(BN1_SAVE.clone())),
    )]
});
static BN1_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/bn1-0.png")).unwrap());

pub static BN1: Game = Game {
    family: &BN1_FAMILY,
    variant: 0,
    rom_code: b"AREE",
    revision: 0x00,
    crc32: 0x1d347971,
    region: Region::US,
    parse_save_fn: |data| {
        let save = dataview::save::Save::new(data)?;
        if save.game_info()
            != &(dataview::save::GameInfo {
                region: dataview::save::Region::US,
            })
        {
            return Err(Error::IncompatibleSave);
        }
        Ok(tango_gamesupport_common::dataview::wrap_save(Box::new(save)))
    },
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common::dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::AREE_00,
            charset.unwrap_or(dataview::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_AREE_00,
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    save_templates: Some(&BN1_T),
    logo_image: Some(&BN1_LOGO),
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

/// This crate's families' replay compatibility version — one engine
/// serves them all, so a change that invalidates one family's
/// recordings invalidates its siblings' too. See
/// [`Family::replay_version`] for what warrants a bump.
const REPLAY_VERSION: u32 = 0;

pub static EXE1_FAMILY: Family = Family {
    id: "exe1",
    replay_version: REPLAY_VERSION,
    games: &[&EXE1],
    translations: family_translations!("exe1"),
};

pub static BN1_FAMILY: Family = Family {
    id: "bn1",
    replay_version: REPLAY_VERSION,
    games: &[&BN1],
    translations: family_translations!("bn1"),
};

/// Every game family this crate provides. The app aggregates the
/// `FAMILIES` of the enabled crates into its registry + localizer.
pub static FAMILIES: &[&Family] = &[&EXE1_FAMILY, &BN1_FAMILY];

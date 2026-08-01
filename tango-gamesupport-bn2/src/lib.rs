pub use tango_gamesupport_bn2_dataview as dataview;
#[cfg(feature = "ui")]
pub use tango_gamesupport_bn2_ui as ui;

pub mod pvp;

use std::sync::LazyLock;
use tango_gamesupport::{BackgroundRef, Family, Game, LazyImage, Region, SaveTemplates, Volume};

const MATCH_TYPES: &[usize] = &[1];
const BACKGROUND: BackgroundRef = BackgroundRef {
    volume: Volume::Vol1,
    tga: "05.tga",
};

macro_rules! bn2_save {
    ($file:expr) => {
        LazyLock::new(|| crate::dataview::save::Save::from_wram(include_bytes!($file)).unwrap())
    };
}

/// Every cartridge in this family, as its ROM header names it.
///
/// A match needs both seats' engine support and a factory only
/// holds its own, so this is where the peer's gets resolved — the
/// one place that knows what this family's siblings are.
pub static FAMILY: &[tango_backend_mgba::Seat] = &[(b"AE2J", 0, &pvp::SIO_AE2J_00_AC), (b"AE2E", 0, &pvp::PVP_AE2E_00)];

static ENGINE_SIO_AE2J_00_AC: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::SIO_AE2J_00_AC, FAMILY);
static ENGINE_PVP_AE2E_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_AE2E_00, FAMILY);

static HUB_ANY: LazyLock<crate::dataview::save::Save> = bn2_save!("saves/hub_any.raw");
static GUTS_ANY: LazyLock<crate::dataview::save::Save> = bn2_save!("saves/guts_any.raw");
static CUSTOM_ANY: LazyLock<crate::dataview::save::Save> = bn2_save!("saves/custom_any.raw");
static TEAM_ANY: LazyLock<crate::dataview::save::Save> = bn2_save!("saves/team_any.raw");
static SHIELD_ANY: LazyLock<crate::dataview::save::Save> = bn2_save!("saves/shield_any.raw");

static TEMPLATES: SaveTemplates = LazyLock::new(|| {
    vec![
        (
            "hub",
            tango_gamesupport_common::dataview::wrap_save(Box::new(HUB_ANY.clone())),
        ),
        (
            "guts",
            tango_gamesupport_common::dataview::wrap_save(Box::new(GUTS_ANY.clone())),
        ),
        (
            "custom",
            tango_gamesupport_common::dataview::wrap_save(Box::new(CUSTOM_ANY.clone())),
        ),
        (
            "team",
            tango_gamesupport_common::dataview::wrap_save(Box::new(TEAM_ANY.clone())),
        ),
        (
            "shield",
            tango_gamesupport_common::dataview::wrap_save(Box::new(SHIELD_ANY.clone())),
        ),
    ]
});

static EXE2_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/exe2-0.png")).unwrap());
static BN2_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/bn2-0.png")).unwrap());

pub static EXE2: Game = Game {
    family: &EXE2_FAMILY,
    variant: 0,
    rom_code: b"AE2J",
    revision: 0x00,
    crc32: 0x046eed8d,
    region: Region::JP,
    parse_save_fn: |data| {
        Ok(tango_gamesupport_common::dataview::wrap_save(Box::new(
            dataview::save::Save::new(data)?,
        )))
    },
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common::dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::AE2J_00_AC,
            charset.unwrap_or(dataview::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_SIO_AE2J_00_AC,
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    save_templates: Some(&TEMPLATES),
    logo_image: Some(&EXE2_LOGO),
    background: Some(BACKGROUND),
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
};

pub static BN2: Game = Game {
    family: &BN2_FAMILY,
    variant: 0,
    rom_code: b"AE2E",
    revision: 0x00,
    crc32: 0x6d961f82,
    region: Region::US,
    parse_save_fn: |data| {
        Ok(tango_gamesupport_common::dataview::wrap_save(Box::new(
            dataview::save::Save::new(data)?,
        )))
    },
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common::dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::AE2E_00,
            charset.unwrap_or(dataview::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_AE2E_00,
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    save_templates: Some(&TEMPLATES),
    logo_image: Some(&BN2_LOGO),
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

pub static EXE2_FAMILY: Family = Family {
    id: "exe2",
    games: &[&EXE2],
    translations: family_translations!("exe2"),
};

pub static BN2_FAMILY: Family = Family {
    id: "bn2",
    games: &[&BN2],
    translations: family_translations!("bn2"),
};

/// Every game family this crate provides. The app aggregates the
/// `FAMILIES` of the enabled crates into its registry + localizer.
pub static FAMILIES: &[&Family] = &[&EXE2_FAMILY, &BN2_FAMILY];

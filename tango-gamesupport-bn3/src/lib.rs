pub use tango_gamesupport_bn3_dataview as dataview;
#[cfg(feature = "ui")]
pub use tango_gamesupport_bn3_ui as ui;

pub mod pvp;

use std::sync::LazyLock;
use tango_gamesupport::{BackgroundRef, Error, Family, Game, LazyImage, Region, SaveTemplates, Volume};

const MATCH_TYPES: &[usize] = &[4, 1];
const BACKGROUND: BackgroundRef = BackgroundRef {
    volume: Volume::Vol1,
    tga: "07.tga",
};
/// Every cartridge in this family, as its ROM header names it.
///
/// A match needs both seats' engine support and a factory only
/// holds its own, so this is where the peer's gets resolved — the
/// one place that knows what this family's siblings are.
pub static FAMILY: &[tango_backend_mgba::Seat] = &[
    (b"A6BJ", 1, &pvp::PVP_A6BJ_01),
    (b"A3XJ", 1, &pvp::PVP_A3XJ_01),
    (b"A6BE", 0, &pvp::PVP_A6BE_00),
    (b"A3XE", 0, &pvp::PVP_A3XE_00),
];

static ENGINE_PVP_A6BJ_01: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_A6BJ_01, FAMILY);
static ENGINE_PVP_A3XJ_01: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_A3XJ_01, FAMILY);
static ENGINE_PVP_A6BE_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_A6BE_00, FAMILY);
static ENGINE_PVP_A3XE_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_A3XE_00, FAMILY);

static EXE3W_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/exe3-0.png")).unwrap());
static EXE3B_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/exe3-1.png")).unwrap());
static BN3W_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/bn3-0.png")).unwrap());
static BN3B_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/bn3-1.png")).unwrap());

macro_rules! bn3_save {
    ($file:expr, $variant:ident) => {
        LazyLock::new(|| {
            crate::dataview::save::Save::from_wram(
                include_bytes!($file),
                crate::dataview::save::GameInfo {
                    variant: crate::dataview::save::Variant::$variant,
                },
            )
            .unwrap()
        })
    };
}

// ---------------- WHITE (variant 0) ----------------
static HEAT_GUTS_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/heat_guts_white_any.raw", White);
static AQUA_GUTS_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/aqua_guts_white_any.raw", White);
static ELEC_GUTS_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/elec_guts_white_any.raw", White);
static WOOD_GUTS_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/wood_guts_white_any.raw", White);
static HEAT_CUSTOM_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/heat_custom_white_any.raw", White);
static AQUA_CUSTOM_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/aqua_custom_white_any.raw", White);
static ELEC_CUSTOM_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/elec_custom_white_any.raw", White);
static WOOD_CUSTOM_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/wood_custom_white_any.raw", White);
static HEAT_SHIELD_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/heat_shield_white_any.raw", White);
static AQUA_SHIELD_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/aqua_shield_white_any.raw", White);
static ELEC_SHIELD_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/elec_shield_white_any.raw", White);
static WOOD_SHIELD_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/wood_shield_white_any.raw", White);
static HEAT_TEAM_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/heat_team_white_any.raw", White);
static AQUA_TEAM_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/aqua_team_white_any.raw", White);
static ELEC_TEAM_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/elec_team_white_any.raw", White);
static WOOD_TEAM_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/wood_team_white_any.raw", White);
static HEAT_GROUND_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/heat_ground_white_any.raw", White);
static AQUA_GROUND_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/aqua_ground_white_any.raw", White);
static ELEC_GROUND_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/elec_ground_white_any.raw", White);
static WOOD_GROUND_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/wood_ground_white_any.raw", White);
static HEAT_BUG_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/heat_bug_white_any.raw", White);
static AQUA_BUG_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/aqua_bug_white_any.raw", White);
static ELEC_BUG_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/elec_bug_white_any.raw", White);
static WOOD_BUG_W: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/wood_bug_white_any.raw", White);
static WHITE_T: SaveTemplates = LazyLock::new(|| {
    vec![
        (
            "heat-guts",
            tango_gamesupport_common_dataview::wrap_save(Box::new(HEAT_GUTS_W.clone())),
        ),
        (
            "aqua-guts",
            tango_gamesupport_common_dataview::wrap_save(Box::new(AQUA_GUTS_W.clone())),
        ),
        (
            "elec-guts",
            tango_gamesupport_common_dataview::wrap_save(Box::new(ELEC_GUTS_W.clone())),
        ),
        (
            "wood-guts",
            tango_gamesupport_common_dataview::wrap_save(Box::new(WOOD_GUTS_W.clone())),
        ),
        (
            "heat-custom",
            tango_gamesupport_common_dataview::wrap_save(Box::new(HEAT_CUSTOM_W.clone())),
        ),
        (
            "aqua-custom",
            tango_gamesupport_common_dataview::wrap_save(Box::new(AQUA_CUSTOM_W.clone())),
        ),
        (
            "elec-custom",
            tango_gamesupport_common_dataview::wrap_save(Box::new(ELEC_CUSTOM_W.clone())),
        ),
        (
            "wood-custom",
            tango_gamesupport_common_dataview::wrap_save(Box::new(WOOD_CUSTOM_W.clone())),
        ),
        (
            "heat-shield",
            tango_gamesupport_common_dataview::wrap_save(Box::new(HEAT_SHIELD_W.clone())),
        ),
        (
            "aqua-shield",
            tango_gamesupport_common_dataview::wrap_save(Box::new(AQUA_SHIELD_W.clone())),
        ),
        (
            "elec-shield",
            tango_gamesupport_common_dataview::wrap_save(Box::new(ELEC_SHIELD_W.clone())),
        ),
        (
            "wood-shield",
            tango_gamesupport_common_dataview::wrap_save(Box::new(WOOD_SHIELD_W.clone())),
        ),
        (
            "heat-team",
            tango_gamesupport_common_dataview::wrap_save(Box::new(HEAT_TEAM_W.clone())),
        ),
        (
            "aqua-team",
            tango_gamesupport_common_dataview::wrap_save(Box::new(AQUA_TEAM_W.clone())),
        ),
        (
            "elec-team",
            tango_gamesupport_common_dataview::wrap_save(Box::new(ELEC_TEAM_W.clone())),
        ),
        (
            "wood-team",
            tango_gamesupport_common_dataview::wrap_save(Box::new(WOOD_TEAM_W.clone())),
        ),
        (
            "heat-ground",
            tango_gamesupport_common_dataview::wrap_save(Box::new(HEAT_GROUND_W.clone())),
        ),
        (
            "aqua-ground",
            tango_gamesupport_common_dataview::wrap_save(Box::new(AQUA_GROUND_W.clone())),
        ),
        (
            "elec-ground",
            tango_gamesupport_common_dataview::wrap_save(Box::new(ELEC_GROUND_W.clone())),
        ),
        (
            "wood-ground",
            tango_gamesupport_common_dataview::wrap_save(Box::new(WOOD_GROUND_W.clone())),
        ),
        (
            "heat-bug",
            tango_gamesupport_common_dataview::wrap_save(Box::new(HEAT_BUG_W.clone())),
        ),
        (
            "aqua-bug",
            tango_gamesupport_common_dataview::wrap_save(Box::new(AQUA_BUG_W.clone())),
        ),
        (
            "elec-bug",
            tango_gamesupport_common_dataview::wrap_save(Box::new(ELEC_BUG_W.clone())),
        ),
        (
            "wood-bug",
            tango_gamesupport_common_dataview::wrap_save(Box::new(WOOD_BUG_W.clone())),
        ),
    ]
});

// ---------------- BLUE (variant 1) ----------------
static HEAT_GUTS_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/heat_guts_blue_any.raw", Blue);
static AQUA_GUTS_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/aqua_guts_blue_any.raw", Blue);
static ELEC_GUTS_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/elec_guts_blue_any.raw", Blue);
static WOOD_GUTS_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/wood_guts_blue_any.raw", Blue);
static HEAT_CUSTOM_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/heat_custom_blue_any.raw", Blue);
static AQUA_CUSTOM_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/aqua_custom_blue_any.raw", Blue);
static ELEC_CUSTOM_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/elec_custom_blue_any.raw", Blue);
static WOOD_CUSTOM_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/wood_custom_blue_any.raw", Blue);
static HEAT_SHIELD_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/heat_shield_blue_any.raw", Blue);
static AQUA_SHIELD_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/aqua_shield_blue_any.raw", Blue);
static ELEC_SHIELD_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/elec_shield_blue_any.raw", Blue);
static WOOD_SHIELD_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/wood_shield_blue_any.raw", Blue);
static HEAT_TEAM_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/heat_team_blue_any.raw", Blue);
static AQUA_TEAM_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/aqua_team_blue_any.raw", Blue);
static ELEC_TEAM_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/elec_team_blue_any.raw", Blue);
static WOOD_TEAM_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/wood_team_blue_any.raw", Blue);
static HEAT_SHADOW_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/heat_shadow_blue_any.raw", Blue);
static AQUA_SHADOW_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/aqua_shadow_blue_any.raw", Blue);
static ELEC_SHADOW_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/elec_shadow_blue_any.raw", Blue);
static WOOD_SHADOW_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/wood_shadow_blue_any.raw", Blue);
static HEAT_BUG_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/heat_bug_blue_any.raw", Blue);
static AQUA_BUG_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/aqua_bug_blue_any.raw", Blue);
static ELEC_BUG_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/elec_bug_blue_any.raw", Blue);
static WOOD_BUG_B: LazyLock<crate::dataview::save::Save> = bn3_save!("saves/wood_bug_blue_any.raw", Blue);
static BLUE_T: SaveTemplates = LazyLock::new(|| {
    vec![
        (
            "heat-guts",
            tango_gamesupport_common_dataview::wrap_save(Box::new(HEAT_GUTS_B.clone())),
        ),
        (
            "aqua-guts",
            tango_gamesupport_common_dataview::wrap_save(Box::new(AQUA_GUTS_B.clone())),
        ),
        (
            "elec-guts",
            tango_gamesupport_common_dataview::wrap_save(Box::new(ELEC_GUTS_B.clone())),
        ),
        (
            "wood-guts",
            tango_gamesupport_common_dataview::wrap_save(Box::new(WOOD_GUTS_B.clone())),
        ),
        (
            "heat-custom",
            tango_gamesupport_common_dataview::wrap_save(Box::new(HEAT_CUSTOM_B.clone())),
        ),
        (
            "aqua-custom",
            tango_gamesupport_common_dataview::wrap_save(Box::new(AQUA_CUSTOM_B.clone())),
        ),
        (
            "elec-custom",
            tango_gamesupport_common_dataview::wrap_save(Box::new(ELEC_CUSTOM_B.clone())),
        ),
        (
            "wood-custom",
            tango_gamesupport_common_dataview::wrap_save(Box::new(WOOD_CUSTOM_B.clone())),
        ),
        (
            "heat-shield",
            tango_gamesupport_common_dataview::wrap_save(Box::new(HEAT_SHIELD_B.clone())),
        ),
        (
            "aqua-shield",
            tango_gamesupport_common_dataview::wrap_save(Box::new(AQUA_SHIELD_B.clone())),
        ),
        (
            "elec-shield",
            tango_gamesupport_common_dataview::wrap_save(Box::new(ELEC_SHIELD_B.clone())),
        ),
        (
            "wood-shield",
            tango_gamesupport_common_dataview::wrap_save(Box::new(WOOD_SHIELD_B.clone())),
        ),
        (
            "heat-team",
            tango_gamesupport_common_dataview::wrap_save(Box::new(HEAT_TEAM_B.clone())),
        ),
        (
            "aqua-team",
            tango_gamesupport_common_dataview::wrap_save(Box::new(AQUA_TEAM_B.clone())),
        ),
        (
            "elec-team",
            tango_gamesupport_common_dataview::wrap_save(Box::new(ELEC_TEAM_B.clone())),
        ),
        (
            "wood-team",
            tango_gamesupport_common_dataview::wrap_save(Box::new(WOOD_TEAM_B.clone())),
        ),
        (
            "heat-shadow",
            tango_gamesupport_common_dataview::wrap_save(Box::new(HEAT_SHADOW_B.clone())),
        ),
        (
            "aqua-shadow",
            tango_gamesupport_common_dataview::wrap_save(Box::new(AQUA_SHADOW_B.clone())),
        ),
        (
            "elec-shadow",
            tango_gamesupport_common_dataview::wrap_save(Box::new(ELEC_SHADOW_B.clone())),
        ),
        (
            "wood-shadow",
            tango_gamesupport_common_dataview::wrap_save(Box::new(WOOD_SHADOW_B.clone())),
        ),
        (
            "heat-bug",
            tango_gamesupport_common_dataview::wrap_save(Box::new(HEAT_BUG_B.clone())),
        ),
        (
            "aqua-bug",
            tango_gamesupport_common_dataview::wrap_save(Box::new(AQUA_BUG_B.clone())),
        ),
        (
            "elec-bug",
            tango_gamesupport_common_dataview::wrap_save(Box::new(ELEC_BUG_B.clone())),
        ),
        (
            "wood-bug",
            tango_gamesupport_common_dataview::wrap_save(Box::new(WOOD_BUG_B.clone())),
        ),
    ]
});

fn parse_save(data: &[u8], variant: dataview::save::Variant) -> Result<tango_gamesupport::BoxedSave, Error> {
    let save = dataview::save::Save::new(data)?;
    if save.game_info() != &(dataview::save::GameInfo { variant }) {
        return Err(Error::IncompatibleSave);
    }
    Ok(tango_gamesupport_common_dataview::wrap_save(Box::new(save)))
}

pub static EXE3W: Game = Game {
    family: &EXE3_FAMILY,
    variant: 0,
    rom_code: b"A6BJ",
    revision: 0x01,
    crc32: 0xe48e6bc9,
    region: Region::JP,
    parse_save_fn: |data| parse_save(data, dataview::save::Variant::White),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common_dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::A6BJ_01,
            charset.unwrap_or(dataview::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_A6BJ_01,
    save_templates: Some(&WHITE_T),
    logo_image: Some(&EXE3W_LOGO),
    background: Some(BACKGROUND),
};

pub static EXE3B: Game = Game {
    family: &EXE3_FAMILY,
    variant: 1,
    rom_code: b"A3XJ",
    revision: 0x01,
    crc32: 0xfd57493b,
    region: Region::JP,
    parse_save_fn: |data| parse_save(data, dataview::save::Variant::Blue),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common_dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::A3XJ_01,
            charset.unwrap_or(dataview::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_A3XJ_01,
    save_templates: Some(&BLUE_T),
    logo_image: Some(&EXE3B_LOGO),
    background: Some(BACKGROUND),
};

pub static BN3W: Game = Game {
    family: &BN3_FAMILY,
    variant: 0,
    rom_code: b"A6BE",
    revision: 0x00,
    crc32: 0x0be4410a,
    region: Region::US,
    parse_save_fn: |data| parse_save(data, dataview::save::Variant::White),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common_dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::A6BE_00,
            charset.unwrap_or(dataview::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_A6BE_00,
    save_templates: Some(&WHITE_T),
    logo_image: Some(&BN3W_LOGO),
    background: Some(BACKGROUND),
};

pub static BN3B: Game = Game {
    family: &BN3_FAMILY,
    variant: 1,
    rom_code: b"A3XE",
    revision: 0x00,
    crc32: 0xc0c780f9,
    region: Region::US,
    parse_save_fn: |data| parse_save(data, dataview::save::Variant::Blue),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common_dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::A3XE_00,
            charset.unwrap_or(dataview::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_A3XE_00,
    save_templates: Some(&BLUE_T),
    logo_image: Some(&BN3B_LOGO),
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

pub static EXE3_FAMILY: Family = Family {
    id: "exe3",
    games: &[&EXE3W, &EXE3B],
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
    translations: family_translations!("exe3"),
};

pub static BN3_FAMILY: Family = Family {
    id: "bn3",
    games: &[&BN3W, &BN3B],
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
    translations: family_translations!("bn3"),
};

/// Every game family this crate provides. The app aggregates the
/// `FAMILIES` of the enabled crates into its registry + localizer.
pub static FAMILIES: &[&Family] = &[&EXE3_FAMILY, &BN3_FAMILY];

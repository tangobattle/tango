pub use tango_gamesupport_bn4_dataview as dataview;
#[cfg(feature = "ui")]
pub use tango_gamesupport_bn4_ui as ui;

pub mod pvp;

use std::sync::LazyLock;
use tango_gamesupport::{BackgroundRef, Error, Family, Game, LazyImage, Region, SaveTemplates, Volume};

const MATCH_TYPES: &[usize] = &[1, 1];
const BACKGROUND: BackgroundRef = BackgroundRef {
    volume: Volume::Vol2,
    tga: "13.tga",
};
/// Every cartridge in this family, as its ROM header names it.
///
/// A match needs both seats' engine support and a factory only
/// holds its own, so this is where the peer's gets resolved — the
/// one place that knows what this family's siblings are.
pub static FAMILY: &[tango_backend_mgba::Seat] = &[
    (b"B4WJ", 1, &pvp::PVP_B4WJ_01),
    (b"B4BJ", 1, &pvp::PVP_B4BJ_01),
    (b"B4WE", 0, &pvp::PVP_B4WE_00),
    (b"B4BE", 0, &pvp::PVP_B4BE_00),
];

static ENGINE_PVP_B4WJ_01: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_B4WJ_01, FAMILY);
static ENGINE_PVP_B4BJ_01: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_B4BJ_01, FAMILY);
static ENGINE_PVP_B4WE_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_B4WE_00, FAMILY);
static ENGINE_PVP_B4BE_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_B4BE_00, FAMILY);

static EXE4RS_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/exe4-0.png")).unwrap());
static EXE4BM_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/exe4-1.png")).unwrap());
static BN4RS_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/bn4-0.png")).unwrap());
static BN4BM_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/bn4-1.png")).unwrap());

macro_rules! bn4_save {
    ($file:expr, $jp:expr, $us:expr, $variant:ident) => {
        LazyLock::new(|| {
            crate::dataview::save::Save::from_wram(
                include_bytes!($file),
                crate::dataview::save::GameInfo {
                    region: crate::dataview::save::Region { jp: $jp, us: $us },
                    variant: crate::dataview::save::Variant::$variant,
                },
            )
            .unwrap()
        })
    };
}

// ---------------- EXE4 Red Sun (JP) ----------------
static EXE4RS_DARK997: LazyLock<crate::dataview::save::Save> =
    bn4_save!("saves/dark_hp_997_rs_jp.raw", true, false, RedSun);
static EXE4RS_LIGHT999: LazyLock<crate::dataview::save::Save> =
    bn4_save!("saves/light_hp_999_rs_jp.raw", true, false, RedSun);
static EXE4RS_LIGHT1000: LazyLock<crate::dataview::save::Save> =
    bn4_save!("saves/light_hp_1000_rs_jp.raw", true, false, RedSun);
static EXE4RS_T: SaveTemplates = LazyLock::new(|| {
    vec![
        (
            "dark-hp-997",
            tango_gamesupport_common_dataview::wrap_save(Box::new(EXE4RS_DARK997.clone())),
        ),
        (
            "light-hp-999",
            tango_gamesupport_common_dataview::wrap_save(Box::new(EXE4RS_LIGHT999.clone())),
        ),
        (
            "light-hp-1000",
            tango_gamesupport_common_dataview::wrap_save(Box::new(EXE4RS_LIGHT1000.clone())),
        ),
    ]
});

fn parse_save(
    data: &[u8],
    is_jp: bool,
    variant: dataview::save::Variant,
) -> Result<tango_gamesupport::BoxedSave, Error> {
    let save = dataview::save::Save::new(data)?;
    let game_info = save.game_info();
    let region_ok = if is_jp {
        game_info.region.jp
    } else {
        game_info.region.us
    };
    if game_info.variant != variant || !region_ok {
        return Err(Error::IncompatibleSave);
    }
    Ok(tango_gamesupport_common_dataview::wrap_save(Box::new(save)))
}

pub static EXE4RS: Game = Game {
    family: &EXE4_FAMILY,
    variant: 0,
    rom_code: b"B4WJ",
    revision: 0x01,
    crc32: 0xcf0e8b05,
    region: Region::JP,
    parse_save_fn: |data| parse_save(data, true, dataview::save::Variant::RedSun),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common_dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::B4WJ_01,
            charset.unwrap_or(dataview::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_B4WJ_01,
    save_templates: Some(&EXE4RS_T),
    logo_image: Some(&EXE4RS_LOGO),
    background: Some(BACKGROUND),
};

// ---------------- EXE4 Blue Moon (JP) ----------------
static EXE4BM_DARK997: LazyLock<crate::dataview::save::Save> =
    bn4_save!("saves/dark_hp_997_bm_jp.raw", true, false, BlueMoon);
static EXE4BM_LIGHT999: LazyLock<crate::dataview::save::Save> =
    bn4_save!("saves/light_hp_999_bm_jp.raw", true, false, BlueMoon);
static EXE4BM_LIGHT1000: LazyLock<crate::dataview::save::Save> =
    bn4_save!("saves/light_hp_1000_bm_jp.raw", true, false, BlueMoon);
static EXE4BM_T: SaveTemplates = LazyLock::new(|| {
    vec![
        (
            "dark-hp-997",
            tango_gamesupport_common_dataview::wrap_save(Box::new(EXE4BM_DARK997.clone())),
        ),
        (
            "light-hp-999",
            tango_gamesupport_common_dataview::wrap_save(Box::new(EXE4BM_LIGHT999.clone())),
        ),
        (
            "light-hp-1000",
            tango_gamesupport_common_dataview::wrap_save(Box::new(EXE4BM_LIGHT1000.clone())),
        ),
    ]
});

pub static EXE4BM: Game = Game {
    family: &EXE4_FAMILY,
    variant: 1,
    rom_code: b"B4BJ",
    revision: 0x01,
    crc32: 0x709bbf07,
    region: Region::JP,
    parse_save_fn: |data| parse_save(data, true, dataview::save::Variant::BlueMoon),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common_dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::B4BJ_01,
            charset.unwrap_or(dataview::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_B4BJ_01,
    save_templates: Some(&EXE4BM_T),
    logo_image: Some(&EXE4BM_LOGO),
    background: Some(BACKGROUND),
};

// ---------------- BN4 Red Sun (US) ----------------
static BN4RS_DARK997: LazyLock<crate::dataview::save::Save> =
    bn4_save!("saves/dark_hp_997_rs_us.raw", false, true, RedSun);
static BN4RS_LIGHT999: LazyLock<crate::dataview::save::Save> =
    bn4_save!("saves/light_hp_999_rs_us.raw", false, true, RedSun);
static BN4RS_LIGHT1000: LazyLock<crate::dataview::save::Save> =
    bn4_save!("saves/light_hp_1000_rs_us.raw", false, true, RedSun);
static BN4RS_T: SaveTemplates = LazyLock::new(|| {
    vec![
        (
            "dark-hp-997",
            tango_gamesupport_common_dataview::wrap_save(Box::new(BN4RS_DARK997.clone())),
        ),
        (
            "light-hp-999",
            tango_gamesupport_common_dataview::wrap_save(Box::new(BN4RS_LIGHT999.clone())),
        ),
        (
            "light-hp-1000",
            tango_gamesupport_common_dataview::wrap_save(Box::new(BN4RS_LIGHT1000.clone())),
        ),
    ]
});

pub static BN4RS: Game = Game {
    family: &BN4_FAMILY,
    variant: 0,
    rom_code: b"B4WE",
    revision: 0x00,
    crc32: 0x2120695c,
    region: Region::US,
    parse_save_fn: |data| parse_save(data, false, dataview::save::Variant::RedSun),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common_dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::B4WE_00,
            charset.unwrap_or(dataview::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_B4WE_00,
    save_templates: Some(&BN4RS_T),
    logo_image: Some(&BN4RS_LOGO),
    background: Some(BACKGROUND),
};

// ---------------- BN4 Blue Moon (US) ----------------
static BN4BM_DARK997: LazyLock<crate::dataview::save::Save> =
    bn4_save!("saves/dark_hp_997_bm_us.raw", false, true, BlueMoon);
static BN4BM_LIGHT999: LazyLock<crate::dataview::save::Save> =
    bn4_save!("saves/light_hp_999_bm_us.raw", false, true, BlueMoon);
static BN4BM_LIGHT1000: LazyLock<crate::dataview::save::Save> =
    bn4_save!("saves/light_hp_1000_bm_us.raw", false, true, BlueMoon);
static BN4BM_T: SaveTemplates = LazyLock::new(|| {
    vec![
        (
            "dark-hp-997",
            tango_gamesupport_common_dataview::wrap_save(Box::new(BN4BM_DARK997.clone())),
        ),
        (
            "light-hp-999",
            tango_gamesupport_common_dataview::wrap_save(Box::new(BN4BM_LIGHT999.clone())),
        ),
        (
            "light-hp-1000",
            tango_gamesupport_common_dataview::wrap_save(Box::new(BN4BM_LIGHT1000.clone())),
        ),
    ]
});

pub static BN4BM: Game = Game {
    family: &BN4_FAMILY,
    variant: 1,
    rom_code: b"B4BE",
    revision: 0x00,
    crc32: 0x758a46e9,
    region: Region::US,
    parse_save_fn: |data| parse_save(data, false, dataview::save::Variant::BlueMoon),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common_dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::B4BE_00,
            charset.unwrap_or(dataview::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_B4BE_00,
    save_templates: Some(&BN4BM_T),
    logo_image: Some(&BN4BM_LOGO),
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

pub static EXE4_FAMILY: Family = Family {
    id: "exe4",
    games: &[&EXE4RS, &EXE4BM],
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
    translations: family_translations!("exe4"),
};

pub static BN4_FAMILY: Family = Family {
    id: "bn4",
    games: &[&BN4RS, &BN4BM],
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
    translations: family_translations!("bn4"),
};

/// Every game family this crate provides. The app aggregates the
/// `FAMILIES` of the enabled crates into its registry + localizer.
pub static FAMILIES: &[&Family] = &[&EXE4_FAMILY, &BN4_FAMILY];

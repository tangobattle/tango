pub use tango_gamesupport_bn6_dataview as dataview;
#[cfg(feature = "ui")]
pub use tango_gamesupport_bn6_ui as ui;

pub mod pvp;

use std::sync::LazyLock;
use tango_gamesupport::{BackgroundRef, Error, Family, Game, LazyImage, Region, SaveTemplates, Volume};

const MATCH_TYPES: &[usize] = &[1, 1];
const BACKGROUND: BackgroundRef = BackgroundRef {
    volume: Volume::Vol2,
    tga: "19.tga",
};
/// Every cartridge in this family, as its ROM header names it.
///
/// A match needs both seats' engine support and a factory only
/// holds its own, so this is where the peer's gets resolved — the
/// one place that knows what this family's siblings are.
pub static FAMILY: &[tango_backend_mgba::Seat] = &[
    (b"BR5J", 0, &pvp::PVP_BR5J_00),
    (b"BR6J", 0, &pvp::PVP_BR6J_00),
    (b"BR5E", 0, &pvp::PVP_BR5E_00),
    (b"BR6E", 0, &pvp::PVP_BR6E_00),
];

static ENGINE_PVP_BR5J_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_BR5J_00, FAMILY);
static ENGINE_PVP_BR6J_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_BR6J_00, FAMILY);
static ENGINE_PVP_BR5E_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_BR5E_00, FAMILY);
static ENGINE_PVP_BR6E_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_BR6E_00, FAMILY);

static EXE6G_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/exe6-0.png")).unwrap());
static EXE6F_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/exe6-1.png")).unwrap());
static BN6G_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/bn6-0.png")).unwrap());
static BN6F_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/bn6-1.png")).unwrap());

macro_rules! bn6_save {
    ($file:expr, $region:ident, $variant:ident) => {
        LazyLock::new(|| {
            crate::dataview::save::Save::from_wram(
                include_bytes!($file),
                crate::dataview::save::GameInfo {
                    region: crate::dataview::save::Region::$region,
                    variant: crate::dataview::save::Variant::$variant,
                },
            )
            .unwrap()
        })
    };
}

// ---------------- EXE6 Gregar JP (BR5J_00) ----------------
static EXE6G_MEGA: LazyLock<crate::dataview::save::Save> = bn6_save!("saves/g_jp.raw", JP, Gregar);
static EXE6G_T: SaveTemplates = LazyLock::new(|| {
    vec![(
        "",
        tango_gamesupport_common::dataview::wrap_save(Box::new(EXE6G_MEGA.clone())),
    )]
});

fn parse_save(
    data: &[u8],
    region: dataview::save::Region,
    variant: dataview::save::Variant,
) -> Result<tango_gamesupport::BoxedSave, Error> {
    let save = dataview::save::Save::new(data)?;
    if save.game_info() != &(dataview::save::GameInfo { region, variant }) {
        return Err(Error::IncompatibleSave);
    }
    Ok(tango_gamesupport_common::dataview::wrap_save(Box::new(save)))
}

pub static EXE6G: Game = Game {
    family: &EXE6_FAMILY,
    variant: 0,
    rom_code: b"BR5J",
    revision: 0x00,
    crc32: 0x6285918a,
    region: Region::JP,
    parse_save_fn: |data| parse_save(data, dataview::save::Region::JP, dataview::save::Variant::Gregar),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common::dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::BR5J_00,
            charset.unwrap_or(dataview::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_BR5J_00,
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    save_templates: Some(&EXE6G_T),
    logo_image: Some(&EXE6G_LOGO),
    background: Some(BACKGROUND),
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
};

// ---------------- EXE6 Falzar JP (BR6J_00) ----------------
static EXE6F_MEGA: LazyLock<crate::dataview::save::Save> = bn6_save!("saves/f_jp.raw", JP, Falzar);
static EXE6F_T: SaveTemplates = LazyLock::new(|| {
    vec![(
        "",
        tango_gamesupport_common::dataview::wrap_save(Box::new(EXE6F_MEGA.clone())),
    )]
});

pub static EXE6F: Game = Game {
    family: &EXE6_FAMILY,
    variant: 1,
    rom_code: b"BR6J",
    revision: 0x00,
    crc32: 0x2dfb603e,
    region: Region::JP,
    parse_save_fn: |data| parse_save(data, dataview::save::Region::JP, dataview::save::Variant::Falzar),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common::dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::BR6J_00,
            charset.unwrap_or(dataview::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_BR6J_00,
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    save_templates: Some(&EXE6F_T),
    logo_image: Some(&EXE6F_LOGO),
    background: Some(BACKGROUND),
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
};

// ---------------- BN6 Gregar US (BR5E_00) ----------------
static BN6G_MEGA: LazyLock<crate::dataview::save::Save> = bn6_save!("saves/g_us.raw", US, Gregar);
static BN6G_T: SaveTemplates = LazyLock::new(|| {
    vec![(
        "",
        tango_gamesupport_common::dataview::wrap_save(Box::new(BN6G_MEGA.clone())),
    )]
});

pub static BN6G: Game = Game {
    family: &BN6_FAMILY,
    variant: 0,
    rom_code: b"BR5E",
    revision: 0x00,
    crc32: 0x79452182,
    region: Region::US,
    parse_save_fn: |data| parse_save(data, dataview::save::Region::US, dataview::save::Variant::Gregar),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common::dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::BR5E_00,
            charset.unwrap_or(dataview::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_BR5E_00,
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    save_templates: Some(&BN6G_T),
    logo_image: Some(&BN6G_LOGO),
    background: Some(BACKGROUND),
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
};

// ---------------- BN6 Falzar US (BR6E_00) ----------------
static BN6F_MEGA: LazyLock<crate::dataview::save::Save> = bn6_save!("saves/f_us.raw", US, Falzar);
static BN6F_T: SaveTemplates = LazyLock::new(|| {
    vec![(
        "",
        tango_gamesupport_common::dataview::wrap_save(Box::new(BN6F_MEGA.clone())),
    )]
});

pub static BN6F: Game = Game {
    family: &BN6_FAMILY,
    variant: 1,
    rom_code: b"BR6E",
    revision: 0x00,
    crc32: 0xdee6f2a9,
    region: Region::US,
    parse_save_fn: |data| parse_save(data, dataview::save::Region::US, dataview::save::Variant::Falzar),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common::dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::BR6E_00,
            charset.unwrap_or(dataview::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_BR6E_00,
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    save_templates: Some(&BN6F_T),
    logo_image: Some(&BN6F_LOGO),
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

pub static EXE6_FAMILY: Family = Family {
    id: "exe6",
    games: &[&EXE6G, &EXE6F],
    translations: family_translations!("exe6"),
};

pub static BN6_FAMILY: Family = Family {
    id: "bn6",
    games: &[&BN6G, &BN6F],
    translations: family_translations!("bn6"),
};

/// Every game family this crate provides. The app aggregates the
/// `FAMILIES` of the enabled crates into its registry + localizer.
pub static FAMILIES: &[&Family] = &[&EXE6_FAMILY, &BN6_FAMILY];

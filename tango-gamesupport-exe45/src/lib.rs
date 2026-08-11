pub use tango_gamesupport_exe45_dataview as dataview;
#[cfg(feature = "ui")]
pub use tango_gamesupport_exe45_ui as ui;

pub mod pvp;

use std::sync::LazyLock;
use tango_gamesupport::{BackgroundRef, Family, Game, LazyImage, Region, SaveTemplates, Volume};

const MATCH_TYPES: &[usize] = &[1, 1];
const BACKGROUND: BackgroundRef = BackgroundRef {
    volume: Volume::Vol1,
    tga: "04.tga",
};
/// Every cartridge in this family, as its ROM header names it.
///
/// A match needs both seats' engine support and a factory only
/// holds its own, so this is where the peer's gets resolved — the
/// one place that knows what this family's siblings are.
pub static FAMILY: &[tango_backend_mgba::Seat] = &[(b"BR4J", 0, &pvp::PVP_BR4J_00)];

static ENGINE_PVP_BR4J_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_BR4J_00, FAMILY);

static LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/exe45.png")).unwrap());

static EXE45_SAVE: LazyLock<crate::dataview::save::Save> =
    LazyLock::new(|| crate::dataview::save::Save::from_wram(include_bytes!("saves/any.raw")).unwrap());
static EXE45_T: SaveTemplates = LazyLock::new(|| {
    vec![(
        "",
        tango_gamesupport_common_dataview::wrap_save(Box::new(EXE45_SAVE.clone())),
    )]
});

pub static EXE45: Game = Game {
    family: &EXE45_FAMILY,
    variant: 0,
    rom_code: b"BR4J",
    revision: 0x00,
    crc32: 0xa646601b,
    rom_size: 0x800000,
    region: Region::JP,
    parse_save_fn: |data| {
        Ok(tango_gamesupport_common_dataview::wrap_save(Box::new(
            dataview::save::Save::new(data)?,
        )))
    },
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common_dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::BR4J_00,
            charset.unwrap_or(dataview::rom::CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_BR4J_00,
    save_templates: Some(&EXE45_T),
    logo_image: Some(&LOGO),
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

pub static EXE45_FAMILY: Family = Family {
    id: "exe45",
    games: &[&EXE45],
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
    translations: family_translations!("exe45"),
};

/// Every game family this crate provides. The app aggregates the
/// `FAMILIES` of the enabled crates into its registry + localizer.
pub static FAMILIES: &[&Family] = &[&EXE45_FAMILY];

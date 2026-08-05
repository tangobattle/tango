pub use tango_gamesupport_bn5_dataview as dataview;
#[cfg(feature = "ui")]
pub use tango_gamesupport_bn5_ui as ui;

pub mod pvp;

use std::sync::LazyLock;
use tango_gamesupport::{BackgroundRef, Error, Family, Game, LazyImage, Region, SaveTemplates, Volume};

const MATCH_TYPES: &[usize] = &[1, 1];
const BACKGROUND: BackgroundRef = BackgroundRef {
    volume: Volume::Vol2,
    tga: "16.tga",
};
/// Every cartridge in this family, as its ROM header names it.
///
/// A match needs both seats' engine support and a factory only
/// holds its own, so this is where the peer's gets resolved — the
/// one place that knows what this family's siblings are.
pub static FAMILY: &[tango_backend_mgba::Seat] = &[
    (b"BRBJ", 0, &pvp::PVP_BRBJ_00),
    (b"BRKJ", 0, &pvp::PVP_BRKJ_00),
    (b"BRBE", 0, &pvp::PVP_BRBE_00),
    (b"BRKE", 0, &pvp::PVP_BRKE_00),
];

static ENGINE_PVP_BRBJ_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_BRBJ_00, FAMILY);
static ENGINE_PVP_BRKJ_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_BRKJ_00, FAMILY);
static ENGINE_PVP_BRBE_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_BRBE_00, FAMILY);
static ENGINE_PVP_BRKE_00: tango_backend_mgba::GbaBackend =
    tango_backend_mgba::GbaBackend::new(&pvp::PVP_BRKE_00, FAMILY);

static EXE5B_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/exe5-0.png")).unwrap());
static EXE5C_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/exe5-1.png")).unwrap());
static BN5P_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/bn5-0.png")).unwrap());
static BN5C_LOGO: LazyImage = LazyLock::new(|| image::load_from_memory(include_bytes!("logos/bn5-1.png")).unwrap());

macro_rules! bn5_save {
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

// ---------------- EXE5 Blues (Protoman) JP ----------------
static EXE5B_DARK: LazyLock<crate::dataview::save::Save> = bn5_save!("saves/dark_protoman_jp.raw", JP, Protoman);
static EXE5B_LIGHT: LazyLock<crate::dataview::save::Save> = bn5_save!("saves/light_protoman_jp.raw", JP, Protoman);
static EXE5B_T: SaveTemplates = LazyLock::new(|| {
    vec![
        (
            "dark",
            tango_gamesupport_common_dataview::wrap_save(Box::new(EXE5B_DARK.clone())),
        ),
        (
            "light",
            tango_gamesupport_common_dataview::wrap_save(Box::new(EXE5B_LIGHT.clone())),
        ),
    ]
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
    Ok(tango_gamesupport_common_dataview::wrap_save(Box::new(save)))
}

pub static EXE5B: Game = Game {
    family: &EXE5_FAMILY,
    variant: 0,
    rom_code: b"BRBJ",
    revision: 0x00,
    crc32: 0xc73f23c0,
    region: Region::JP,
    parse_save_fn: |data| parse_save(data, dataview::save::Region::JP, dataview::save::Variant::Protoman),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common_dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::BRBJ_00,
            charset.unwrap_or(dataview::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_BRBJ_00,
    save_templates: Some(&EXE5B_T),
    logo_image: Some(&EXE5B_LOGO),
    background: Some(BACKGROUND),
};

// ---------------- EXE5 Colonel JP ----------------
static EXE5C_DARK: LazyLock<crate::dataview::save::Save> = bn5_save!("saves/dark_colonel_jp.raw", JP, Colonel);
static EXE5C_LIGHT: LazyLock<crate::dataview::save::Save> = bn5_save!("saves/light_colonel_jp.raw", JP, Colonel);
static EXE5C_T: SaveTemplates = LazyLock::new(|| {
    vec![
        (
            "dark",
            tango_gamesupport_common_dataview::wrap_save(Box::new(EXE5C_DARK.clone())),
        ),
        (
            "light",
            tango_gamesupport_common_dataview::wrap_save(Box::new(EXE5C_LIGHT.clone())),
        ),
    ]
});

pub static EXE5C: Game = Game {
    family: &EXE5_FAMILY,
    variant: 1,
    rom_code: b"BRKJ",
    revision: 0x00,
    crc32: 0x16842635,
    region: Region::JP,
    parse_save_fn: |data| parse_save(data, dataview::save::Region::JP, dataview::save::Variant::Colonel),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common_dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::BRKJ_00,
            charset.unwrap_or(dataview::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_BRKJ_00,
    save_templates: Some(&EXE5C_T),
    logo_image: Some(&EXE5C_LOGO),
    background: Some(BACKGROUND),
};

// ---------------- BN5 Protoman US ----------------
static BN5P_DARK: LazyLock<crate::dataview::save::Save> = bn5_save!("saves/dark_protoman_us.raw", US, Protoman);
static BN5P_LIGHT: LazyLock<crate::dataview::save::Save> = bn5_save!("saves/light_protoman_us.raw", US, Protoman);
static BN5P_T: SaveTemplates = LazyLock::new(|| {
    vec![
        (
            "dark",
            tango_gamesupport_common_dataview::wrap_save(Box::new(BN5P_DARK.clone())),
        ),
        (
            "light",
            tango_gamesupport_common_dataview::wrap_save(Box::new(BN5P_LIGHT.clone())),
        ),
    ]
});

pub static BN5P: Game = Game {
    family: &BN5_FAMILY,
    variant: 0,
    rom_code: b"BRBE",
    revision: 0x00,
    crc32: 0xa73e83a4,
    region: Region::US,
    parse_save_fn: |data| parse_save(data, dataview::save::Region::US, dataview::save::Variant::Protoman),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common_dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::BRBE_00,
            charset.unwrap_or(dataview::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_BRBE_00,
    save_templates: Some(&BN5P_T),
    logo_image: Some(&BN5P_LOGO),
    background: Some(BACKGROUND),
};

// ---------------- BN5 Colonel US ----------------
static BN5C_DARK: LazyLock<crate::dataview::save::Save> = bn5_save!("saves/dark_colonel_us.raw", US, Colonel);
static BN5C_LIGHT: LazyLock<crate::dataview::save::Save> = bn5_save!("saves/light_colonel_us.raw", US, Colonel);
static BN5C_T: SaveTemplates = LazyLock::new(|| {
    vec![
        (
            "dark",
            tango_gamesupport_common_dataview::wrap_save(Box::new(BN5C_DARK.clone())),
        ),
        (
            "light",
            tango_gamesupport_common_dataview::wrap_save(Box::new(BN5C_LIGHT.clone())),
        ),
    ]
});

pub static BN5C: Game = Game {
    family: &BN5_FAMILY,
    variant: 1,
    rom_code: b"BRKE",
    revision: 0x00,
    crc32: 0xa552f683,
    region: Region::US,
    parse_save_fn: |data| parse_save(data, dataview::save::Region::US, dataview::save::Variant::Colonel),
    load_rom_assets_fn: Some(|rom, wram, charset| {
        tango_gamesupport_common_dataview::wrap_assets(Box::new(dataview::rom::Assets::new(
            &dataview::rom::BRKE_00,
            charset.unwrap_or(dataview::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        )))
    }),
    pvp: &ENGINE_PVP_BRKE_00,
    save_templates: Some(&BN5C_T),
    logo_image: Some(&BN5C_LOGO),
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

pub static EXE5_FAMILY: Family = Family {
    id: "exe5",
    games: &[&EXE5B, &EXE5C],
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
    translations: family_translations!("exe5"),
};

pub static BN5_FAMILY: Family = Family {
    id: "bn5",
    games: &[&BN5P, &BN5C],
    match_types: MATCH_TYPES,
    players_colored_by_seat: false,
    #[cfg(feature = "ui")]
    save_editor: &ui::SAVE_EDITOR,
    translations: family_translations!("bn5"),
};

/// Every game family this crate provides. The app aggregates the
/// `FAMILIES` of the enabled crates into its registry + localizer.
pub static FAMILIES: &[&Family] = &[&EXE5_FAMILY, &BN5_FAMILY];

#[cfg(test)]
mod tests {
    use super::*;

    /// What separates a bundled dark template from its light twin is
    /// the karma system's state: karma bottomed out vs. at the cap,
    /// with the three HP-loss battles the dark saves carry.
    #[test]
    fn templates_split_on_karma() {
        for (dark, light) in [
            (&EXE5B_DARK, &EXE5B_LIGHT),
            (&EXE5C_DARK, &EXE5C_LIGHT),
            (&BN5P_DARK, &BN5P_LIGHT),
            (&BN5C_DARK, &BN5C_LIGHT),
        ] {
            assert_eq!((dark.karma(), dark.dark_hp_losses()), (0, 3));
            assert_eq!((light.karma(), light.dark_hp_losses()), (dataview::save::KARMA_MAX, 0));
        }
    }
}

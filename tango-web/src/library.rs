//! The game registry (ported from the desktop client's `library::game`)
//! plus the web library's import helpers: identify picked ROM files
//! against the registry and normalize their stored names.
//!
//! Every game Tango supports lives in its own `tango-gamesupport-<game>`
//! crate, feature-gated exactly like the desktop build; [`FAMILIES`] is
//! the one list that enables them.

use std::collections::HashMap;
use std::sync::LazyLock;

use fluent_templates::fluent_bundle::concurrent::FluentBundle;
use fluent_templates::fluent_bundle::FluentResource;

pub use tango_gamesupport::{Family, GameRef};

/// Every game family enabled in this build, collected from the per-game
/// crates that are compiled in. This is the sole extension point.
pub static FAMILIES: LazyLock<Vec<&'static Family>> = LazyLock::new(|| {
    #[allow(unused_mut)]
    let mut families: Vec<&'static Family> = Vec::new();
    #[cfg(feature = "gamesupport-bn1")]
    families.extend_from_slice(tango_gamesupport_bn1::FAMILIES);
    #[cfg(feature = "gamesupport-bn2")]
    families.extend_from_slice(tango_gamesupport_bn2::FAMILIES);
    #[cfg(feature = "gamesupport-bn3")]
    families.extend_from_slice(tango_gamesupport_bn3::FAMILIES);
    #[cfg(feature = "gamesupport-bn4")]
    families.extend_from_slice(tango_gamesupport_bn4::FAMILIES);
    #[cfg(feature = "gamesupport-bn5")]
    families.extend_from_slice(tango_gamesupport_bn5::FAMILIES);
    #[cfg(feature = "gamesupport-bn6")]
    families.extend_from_slice(tango_gamesupport_bn6::FAMILIES);
    #[cfg(feature = "gamesupport-exe45")]
    families.extend_from_slice(tango_gamesupport_exe45::FAMILIES);
    families
});

/// The flat game registry, derived from [`FAMILIES`].
pub static GAMES: LazyLock<Vec<GameRef>> = LazyLock::new(|| tango_gamesupport::games_of(&FAMILIES));

/// Identify a clean ROM dump against the registry.
pub fn detect(rom: &[u8]) -> Option<GameRef> {
    tango_gamesupport::detect(&GAMES, rom)
}

/// Look a registered game up by `(family, variant)`.
#[allow(dead_code)] // netplay settings resolution (M3)
pub fn find_by_family_and_variant(family: &str, variant: u8) -> Option<GameRef> {
    tango_gamesupport::find_by_family_and_variant(&GAMES, family, variant)
}

/// A game's stable identity string, e.g. `"bn6-0"` — the config's
/// last-game/last-save key.
pub fn game_slug(game: GameRef) -> String {
    let (family, variant) = game.family_and_variant();
    format!("{family}-{variant}")
}

/// Resolve a persisted [`game_slug`] back to its registration.
pub fn find_by_slug(slug: &str) -> Option<GameRef> {
    GAMES.iter().copied().find(|g| game_slug(g) == slug)
}

/// Which registered games can load this SRAM dump. Saves live in one
/// flat `saves/` directory like the desktop's; each game's own parser
/// decides compatibility (region/variant validation included).
pub fn save_compatible_games(sram: &[u8]) -> Vec<GameRef> {
    GAMES
        .iter()
        .copied()
        .filter(|g| g.parse_save(sram).is_ok())
        .collect()
}

pub const ROM_EXTENSIONS: &[&str] = &["gba", "srl"];
pub const SAVE_EXTENSIONS: &[&str] = &["sav"];

pub fn has_extension(name: &str, extensions: &[&str]) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| extensions.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

/// An imported ROM identified against the registry.
pub struct RomInfo {
    pub game: GameRef,
    #[allow(dead_code)] // shown by the importer log only, so far
    pub crc32: u32,
}

/// Identify picked ROM bytes. Only clean dumps of registered games
/// import — same contract as the desktop scanner.
pub fn rom_info(name: &str, bytes: &[u8]) -> anyhow::Result<RomInfo> {
    let game = detect(bytes)
        .ok_or_else(|| anyhow::anyhow!("{name}: not a clean dump of a supported game"))?;
    Ok(RomInfo {
        game,
        crc32: crc32fast::hash(bytes),
    })
}

/// The stored name is normalized to the cartridge, not the picked file:
/// `<family>-<variant> (<CODE>r<rev>).gba`. Re-importing the same ROM
/// overwrites itself instead of piling up copies, and revision variants
/// of one game stay distinct files.
pub fn normalized_file_name(info: &RomInfo) -> String {
    let (family, variant) = info.game.family_and_variant();
    let (code, revision) = info.game.rom_code_and_revision();
    format!(
        "{family}-{variant} ({}r{revision}).gba",
        String::from_utf8_lossy(code)
    )
}

/// The scanned ROM library: which registered games have an imported
/// ROM in OPFS, and under what stored file name.
#[derive(Clone, Default, PartialEq)]
pub struct Library {
    /// One entry per imported ROM, registry order (families group
    /// naturally because [`GAMES`] is built family by family).
    pub entries: Vec<LibraryEntry>,
}

#[derive(Clone, PartialEq)]
pub struct LibraryEntry {
    pub game: GameRef,
    /// The stored file name inside `roms/`.
    pub file: String,
}

impl Library {
    /// Read every file in `roms/` and identify it against the registry.
    /// Unrecognized files are skipped (imports are already gated on
    /// detection, so these are leftovers from older builds at worst).
    pub async fn scan(storage: &crate::storage::Storage) -> Library {
        let files = match crate::storage::list_files(storage.roms()).await {
            Ok(files) => files,
            Err(e) => {
                log::error!("couldn't list roms/: {e}");
                return Library::default();
            }
        };
        let mut found = Vec::new();
        for (file, handle) in files {
            let Ok(bytes) = crate::storage::read_handle(&handle).await else {
                continue;
            };
            if let Some(game) = detect(&bytes) {
                found.push(LibraryEntry { game, file });
            }
        }
        // Registry order, one entry per game (a duplicate import of the
        // same game under two names keeps the first).
        let mut entries = Vec::new();
        for game in GAMES.iter().copied() {
            if let Some(entry) = found.iter().find(|e| e.game == game) {
                entries.push(entry.clone());
            }
        }
        Library { entries }
    }

    pub fn by_slug(&self, slug: &str) -> Option<&LibraryEntry> {
        self.entries.iter().find(|e| game_slug(e.game) == slug)
    }
}

// ---------- game-name localization ----------
//
// Ported from the desktop's dedicated game-string path: each family owns
// a Fluent bundle keyed by bare names (`name`, `short`, `variant-<n>`,
// …). The web build resolves en-US only for now; the full locale set
// arrives with the i18n port (M5).

static FAMILY_LOCALES: LazyLock<HashMap<&'static str, FluentBundle<FluentResource>>> =
    LazyLock::new(|| {
        let mut map = HashMap::new();
        for family in FAMILIES.iter() {
            let Some((_, ftl)) = family.translations.iter().find(|(lang, _)| *lang == "en-US")
            else {
                continue;
            };
            // The fragments are checked in; on a partial parse keep what
            // parsed (a missing key just falls back).
            let res = match FluentResource::try_new(ftl.to_string()) {
                Ok(r) => r,
                Err((r, _errs)) => r,
            };
            let mut bundle =
                FluentBundle::new_concurrent(vec![unic_langid::langid!("en-US")]);
            bundle.set_use_isolating(false);
            let _ = bundle.add_resource(res);
            map.insert(family.id, bundle);
        }
        map
    });

/// Look the bare `key` up in `family`'s en-US bundle.
fn family_str(family: &str, key: &str) -> Option<String> {
    let bundle = FAMILY_LOCALES.get(family)?;
    let msg = bundle.get_message(key)?;
    let pattern = msg.value()?;
    let mut errors = vec![];
    let out = bundle.format_pattern(pattern, None, &mut errors);
    if !errors.is_empty() {
        return None;
    }
    Some(out.into_owned())
}

/// Best-effort full display name (e.g. "Mega Man Battle Network 6:
/// Cybeast Gregar"). Looks up the family's `variant-<variant>` string;
/// falls back to the family `name`, then to "<family> v<variant>".
pub fn display_name(game: GameRef) -> String {
    let (family, variant) = game.family_and_variant();
    family_str(family, &format!("variant-{variant}"))
        .or_else(|| family_str(family, "name"))
        .unwrap_or_else(|| format!("{family} v{variant}"))
}

/// Short tag (e.g. "BN6") via the family's `short` string.
#[allow(dead_code)] // compact pickers (M3 lobby)
pub fn short_name(game: GameRef) -> String {
    let (family, _) = game.family_and_variant();
    family_str(family, "short").unwrap_or_else(|| game_slug(game))
}

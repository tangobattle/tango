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

/// A game's stable identity string, e.g. `"bn6-0"` — used in
/// normalized file names and fresh-save sentinels.
pub fn game_slug(game: GameRef) -> String {
    let (family, variant) = game.family_and_variant();
    format!("{family}-{variant}")
}

/// Resolve a persisted [`game_slug`] back to its registration.
pub fn find_by_slug(slug: &str) -> Option<GameRef> {
    GAMES.iter().copied().find(|g| game_slug(g) == slug)
}

/// Every family in registry order (families group naturally because
/// [`GAMES`] is built family by family).
pub fn families() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for g in GAMES.iter() {
        let fam = g.family_and_variant().0;
        if !out.contains(&fam) {
            out.push(fam);
        }
    }
    out
}

/// All registered games belonging to a family. The family string is
/// region-specific (`exe3` JP vs `bn3` US), so members differ only by
/// color variant.
pub fn games_in_family(family: &str) -> impl Iterator<Item = GameRef> + '_ {
    GAMES
        .iter()
        .copied()
        .filter(move |g| g.family_and_variant().0 == family)
}

/// One row of the family picker, mirroring the desktop's
/// `loadout::family_options`: every supported family — not just the
/// owned ones, so users can see what tango knows about — with
/// un-owned families disabled.
pub struct FamilyOption {
    pub family: &'static str,
    pub display: String,
    /// `false` unless *every* game in this family has an imported ROM.
    pub available: bool,
}

/// The family picker's rows: available families stable-sort to the
/// top (then own-region first, then by family string) so the live
/// ones lead — the desktop's exact ordering.
pub fn family_options(library: &Library) -> Vec<FamilyOption> {
    let mut options: Vec<FamilyOption> = families()
        .into_iter()
        .map(|fam| FamilyOption {
            family: fam,
            display: family_display_name(fam),
            available: games_in_family(fam).all(|g| library.by_game(g).is_some()),
        })
        .collect();
    options.sort_by(|a, b| {
        (!a.available)
            .cmp(&(!b.available))
            .then_with(|| {
                let ar = !family_matches_language(a.family);
                let br = !family_matches_language(b.family);
                ar.cmp(&br)
            })
            .then_with(|| a.family.cmp(b.family))
    });
    options
}

/// Does any game in `family` match the UI language's region? Used to
/// sort own-region families first. The web build's UI language is
/// en-US until the i18n port (M5), so US families lead.
fn family_matches_language(family: &str) -> bool {
    games_in_family(family).any(|g| g.region() == tango_gamesupport::Region::US)
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
/// import — same contract as the desktop scanner. The rejection names
/// what the file looked like (code/revision/CRC32) so a bad dump or a
/// build without that game's support is diagnosable from the console.
pub fn rom_info(name: &str, bytes: &[u8]) -> anyhow::Result<RomInfo> {
    let crc32 = crc32fast::hash(bytes);
    let Some(game) = detect(bytes) else {
        let code = bytes
            .get(0xac..0xb0)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .unwrap_or_else(|| "????".into());
        let revision = bytes.get(0xbc).copied().unwrap_or(0);
        anyhow::bail!(
            "{name}: not a clean dump of a supported game \
             (code {code} rev {revision}, crc32 {crc32:08x}, {} bytes, \
             {} games registered)",
            bytes.len(),
            GAMES.len()
        );
    };
    Ok(RomInfo { game, crc32 })
}

/// The stored name is normalized to the cartridge, not the picked file:
/// `<family>-<variant> (<CODE>r<rev>).gba`. Re-importing the same ROM
/// overwrites itself instead of piling up copies, revision variants of
/// one game stay distinct files, and the scan can recover the game
/// from the name alone ([`game_from_normalized_name`]).
pub fn normalized_file_name(info: &RomInfo) -> String {
    let (family, variant) = info.game.family_and_variant();
    let (code, revision) = info.game.rom_code_and_revision();
    format!(
        "{family}-{variant} ({}r{revision}).gba",
        String::from_utf8_lossy(code)
    )
}

/// Invert [`normalized_file_name`]: `"bn1-0 (AREEr0).gba"` → the
/// registration whose code+revision match. Returns `None` for names
/// this build didn't write (or whose game isn't compiled in).
fn game_from_normalized_name(name: &str) -> Option<GameRef> {
    let inner = name
        .strip_suffix(".gba")?
        .rsplit_once(" (")?
        .1
        .strip_suffix(')')?;
    let (code, revision) = inner.rsplit_once('r')?;
    let code: &[u8; 4] = code.as_bytes().try_into().ok()?;
    let revision: u8 = revision.parse().ok()?;
    tango_gamesupport::find_by_rom_info(&GAMES, code, revision)
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
    /// Identify every file in `roms/` against the registry. Imports
    /// already validated content (header + CRC32) and stored each ROM
    /// under its [`normalized_file_name`], so the scan trusts that
    /// name — the web stand-in for the desktop scanner's
    /// stat-fingerprint gate; re-reading and re-hashing the whole
    /// library froze the tab for minutes on a debug build. Files whose
    /// names don't parse (hand-placed leftovers) get the full
    /// read + detect treatment.
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
            if let Some(game) = game_from_normalized_name(&file) {
                found.push(LibraryEntry { game, file });
                continue;
            }
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

    pub fn by_game(&self, game: GameRef) -> Option<&LibraryEntry> {
        self.entries.iter().find(|e| e.game == game)
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

/// Localized match-type label for a (mode, subtype) pair (e.g.
/// "Single" / "Triple"), via the family's own bundle; falls back to
/// "M.S".
pub fn match_type_name(game: GameRef, match_type: u8, match_subtype: u8) -> String {
    let (family, _) = game.family_and_variant();
    family_str(family, &format!("match-type-{match_type}-{match_subtype}"))
        .unwrap_or_else(|| format!("{match_type}.{match_subtype}"))
}

/// Best-effort family display name (e.g. "Mega Man Battle Network 6")
/// via the family's `name` string; falls back to the raw family string.
pub fn family_display_name(family: &str) -> String {
    family_str(family, "name").unwrap_or_else(|| family.to_string())
}

/// Localized label for a save template via the family's `save-<name>`
/// string (the empty-string default template uses `save-megaman`, the
/// key the games' own bundles carry for it). `None` when the family's
/// bundle has no such key — the caller falls back to the raw name.
pub fn save_template_label(game: GameRef, raw: &str) -> Option<String> {
    let (family, _) = game.family_and_variant();
    let key = if raw.is_empty() {
        "save-megaman".to_string()
    } else {
        format!("save-{raw}")
    };
    family_str(family, &key)
}

/// Short *variant* tag (e.g. "Gregar", "Blue Moon") via the family's
/// `variant-<n>-short` string — the bare color/team name without the
/// series title, for disambiguating variants within a family. Falls
/// back to the family short tag for single-variant families.
pub fn variant_short_name(game: GameRef) -> String {
    let (family, variant) = game.family_and_variant();
    family_str(family, &format!("variant-{variant}-short")).unwrap_or_else(|| short_name(game))
}

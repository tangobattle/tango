//! Per-game registration: struct, dispatch table, helpers.
//!
//! Mirrors the structure of `tango/src/game.rs` from the legacy app —
//! each gamedb variant has its own `&'static Game` constant in a
//! per-family submodule. The struct surfaces `match_types`,
//! `save_templates`, and the per-game `tango_pvp::hooks::Hooks` so PVP /
//! replay code has one lookup point.

use crate::bnlc;
use crate::i18n::t_opt;
use crate::rom::GameRef;
use std::sync::LazyLock;

mod bn1;
mod bn2;
mod bn3;
mod bn4;
mod bn5;
mod bn6;
mod exe45;

pub type SaveTemplates = LazyLock<Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))>>;

/// Lazily-decoded bundled image (logo). The `include_bytes!` blob is
/// held in .rodata; the decode runs on first access. Consumers convert
/// to whatever pixel format they need (typically `.to_rgba8()` for
/// upload as an iced texture).
pub type LazyImage = LazyLock<image::DynamicImage>;

/// Points at a background TGA inside a BNLC volume's shared `exe.dat`
/// asset archive. The full path in the zip is `exe/data/bg/<tga>`.
/// Resolved at runtime via `crate::bnlc::read_shared_file`; if BNLC
/// isn't installed the caller falls back to no background.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BackgroundRef {
    pub volume: bnlc::Volume,
    pub tga: &'static str,
}

/// Per-game registration. Each gamedb variant has a `&'static Game`
/// constant in the appropriate `bnX` / `exe45` submodule, and the
/// dispatch table in `from_gamedb_entry` maps the gamedb's
/// `family_and_variant` to the right entry.
pub struct Game {
    /// The gamedb entry this Game wraps. The gamedb entry exposes the
    /// rom_code / region / variant / parse_save / etc.
    pub gamedb_entry: &'static (dyn tango_gamedb::Game + Send + Sync),
    /// PVP / replay hooks for the underlying ROM. Used by the replay
    /// playback / export / netplay session pipelines.
    pub hooks: &'static (dyn tango_pvp::hooks::Hooks + Send + Sync),
    /// Length-per-mode list. Entry `i` is how many subtypes mode `i`
    /// has — e.g. BN6 is `[1, 1]` (single battle, triple battle, one
    /// subtype each). Drives the match-type pick_list in the lobby.
    pub match_types: &'static [usize],
    /// Bundled save templates for this game. Each entry is a
    /// `(template_name, save)` pair; the empty-string name is the
    /// default template. Lazily parsed from `include_bytes!` blobs on
    /// first access.
    pub save_templates: &'static SaveTemplates,
    /// Logo for the game. Decoded on first access.
    pub logo_image: &'static LazyImage,
    /// Pointer to the BNLC-hosted background TGA. Decoded lazily by
    /// the session view; if BNLC isn't installed the background is
    /// silently omitted.
    pub background: BackgroundRef,
}

/// Returns the per-game registration for a given gamedb entry, or
/// None when the gamedb entry isn't one we have a Game impl for.
pub fn from_gamedb_entry(entry: GameRef) -> Option<&'static Game> {
    Some(match entry.family_and_variant() {
        ("exe1", 0) => &bn1::EXE1,
        ("bn1", 0) => &bn1::BN1,

        ("exe2", 0) => &bn2::EXE2,
        ("bn2", 0) => &bn2::BN2,

        ("exe3", 0) => &bn3::EXE3W,
        ("exe3", 1) => &bn3::EXE3B,
        ("bn3", 0) => &bn3::BN3W,
        ("bn3", 1) => &bn3::BN3B,

        ("exe4", 0) => &bn4::EXE4RS,
        ("exe4", 1) => &bn4::EXE4BM,
        ("bn4", 0) => &bn4::BN4RS,
        ("bn4", 1) => &bn4::BN4BM,

        ("exe5", 0) => &bn5::EXE5B,
        ("exe5", 1) => &bn5::EXE5C,
        ("bn5", 0) => &bn5::BN5P,
        ("bn5", 1) => &bn5::BN5C,

        ("exe6", 0) => &bn6::EXE6G,
        ("exe6", 1) => &bn6::EXE6F,
        ("bn6", 0) => &bn6::BN6G,
        ("bn6", 1) => &bn6::BN6F,

        ("exe45", 0) => &exe45::EXE45,

        _ => return None,
    })
}

// ---------- ranged helpers (unchanged from the old single-file game.rs) ----------

pub fn region_to_language(region: tango_gamedb::Region) -> unic_langid::LanguageIdentifier {
    match region {
        tango_gamedb::Region::US => unic_langid::langid!("en-US"),
        tango_gamedb::Region::JP => unic_langid::langid!("ja-JP"),
    }
}

/// Best-effort full display name (e.g. "Mega Man Battle Network 6:
/// Cybeast Gregar"). Looks up `game-<family>.variant-<variant>` per
/// the legacy Fluent attribute scheme; falls back to the base
/// `game-<family>` value, then to "<family> v<variant>".
pub fn display_name(lang: &unic_langid::LanguageIdentifier, game: GameRef) -> String {
    // Truly dynamic key (one per family/variant pair) — bypass the
    // literal-only t!/t_opt! macros and hit the Fluent loader directly.
    // `try_lookup` returns None on miss or format error (vs `lookup`
    // which panics on format errors and returns a sentinel on miss).
    let (family, variant) = game.family_and_variant();
    t_opt(lang, &format!("game-{family}.variant-{variant}"))
        .or_else(|| t_opt(lang, &format!("game-{family}")))
        .unwrap_or_else(|| format!("{family} v{variant}"))
}

/// Short tag (e.g. "BN6"). Same lookup pattern via the `.short`
/// attribute; falls back to `<family> v<variant>` so unknowns still
/// produce something identifying.
pub fn short_name(lang: &unic_langid::LanguageIdentifier, game: GameRef) -> String {
    let (family, variant) = game.family_and_variant();
    t_opt(lang, &format!("game-{family}.short")).unwrap_or_else(|| format!("{family} v{variant}"))
}

/// Short *variant* tag (e.g. "White", "Blue Moon") via
/// `game-<family>.variant-<variant>-short` — the bare color/team name
/// without the series title, for disambiguating saves/templates within
/// a family. Falls back to the family short tag (e.g. "BN1") for
/// single-variant families that don't define a per-variant short, so the
/// label stays concise in every case.
pub fn variant_short_name(lang: &unic_langid::LanguageIdentifier, game: GameRef) -> String {
    let (family, variant) = game.family_and_variant();
    t_opt(lang, &format!("game-{family}.variant-{variant}-short")).unwrap_or_else(|| short_name(lang, game))
}

/// Localized match-type label for a (mode, subtype) pair (e.g.
/// "Single" / "Triple" / "Lightweight"). Falls back to "M.S" for
/// pairs the locale doesn't name.
pub fn match_type_name(
    lang: &unic_langid::LanguageIdentifier,
    family: &str,
    match_type: u8,
    match_subtype: u8,
) -> String {
    t_opt(lang, &format!("game-{family}.match-type-{match_type}-{match_subtype}"))
        .unwrap_or_else(|| format!("{match_type}.{match_subtype}"))
}

/// All gamedb games belonging to a family (e.g. "bn3" → US White + US
/// Blue). The family string is region-specific (`exe3` JP vs `bn3` US
/// are distinct families), so the members differ only by color variant.
pub fn games_in_family(family: &str) -> impl Iterator<Item = GameRef> + '_ {
    tango_gamedb::GAMES
        .iter()
        .copied()
        .filter(move |g| g.family_and_variant().0 == family)
}

/// Best-effort family display name (e.g. "Mega Man Battle Network 3").
/// The `game-<family>` Fluent base value is the family name; falls back
/// to the raw family string. Mirrors the lookup `lobby_view` already
/// uses for the opponent's game label.
pub fn family_display_name(lang: &unic_langid::LanguageIdentifier, family: &str) -> String {
    t_opt(lang, &format!("game-{family}")).unwrap_or_else(|| family.to_string())
}

/// Resolve a (possibly persisted) family string to its `&'static`
/// gamedb form, or None if no game uses it. Lets restored config —
/// owned `String`s — drive the `&'static str` family state.
pub fn family_static(family: &str) -> Option<&'static str> {
    tango_gamedb::GAMES
        .iter()
        .map(|g| g.family_and_variant().0)
        .find(|f| *f == family)
}

/// Does any game in `family` match the UI language's region? Used to
/// sort the family picker so the user's own-region families lead.
pub fn family_matches_language(lang: &unic_langid::LanguageIdentifier, family: &str) -> bool {
    games_in_family(family).any(|g| region_to_language(g.region()).matches(lang, true, true))
}

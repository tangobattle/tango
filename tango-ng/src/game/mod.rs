//! Per-game registration: trait, dispatch table, helpers.
//!
//! Mirrors the structure of `tango/src/game.rs` from the legacy app —
//! each gamedb variant has its own `&'static dyn Game` constant in a
//! per-family submodule. The trait surfaces `match_types`,
//! `save_templates`, `load_rom_assets` (with patch overrides), and the
//! per-game `tango_pvp::hooks::Hooks` so PVP / replay code has one
//! lookup point.

use crate::i18n::t;
use crate::rom::GameRef;
use crate::rom_overrides::{OverridenAssets, Overrides};
use std::any::Any;

mod bn1;
mod bn2;
mod bn3;
mod bn4;
mod bn5;
mod bn6;
mod exe45;

/// Per-game registration trait. Each gamedb variant has an
/// implementation in the appropriate `bnX` / `exe45` submodule, and
/// the dispatch table in `from_gamedb_entry` maps the gamedb's
/// `family_and_variant` to the right `&'static dyn Game`.
pub trait Game
where
    Self: Any + Send + Sync,
{
    /// The gamedb entry this Game wraps. The gamedb entry exposes the
    /// rom_code / region / variant / parse_save / etc.
    fn gamedb_entry(&self) -> &'static (dyn tango_gamedb::Game + Send + Sync);

    /// PVP / replay hooks for the underlying ROM. Used by the replay
    /// playback / export / netplay session pipelines.
    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync);

    /// Length-per-mode list. Entry `i` is how many subtypes mode `i`
    /// has — e.g. BN6 is `[1, 1]` (single battle, triple battle, one
    /// subtype each). Drives the match-type pick_list in the lobby.
    fn match_types(&self) -> &'static [usize];

    /// Build the rom Assets, layering patch-driven `overrides` on top
    /// of gamedb's per-game defaults. The default impl converts the
    /// override charset into the gamedb-friendly `Option<&[&str]>`
    /// shape and wraps the gamedb result with `OverridenAssets`.
    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &Overrides,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        let charset_owned: Option<Vec<&str>> = overrides
            .charset
            .as_ref()
            .map(|c| c.iter().map(|s| s.as_str()).collect());
        let inner = self
            .gamedb_entry()
            .load_rom_assets(rom, wram, charset_owned.as_deref());
        Box::new(OverridenAssets::new(inner, overrides.clone()))
    }

    /// Bundled save templates for this game. Each entry is a
    /// `(template_name, save)` pair; the empty-string name is the
    /// default template.
    fn save_templates(
        &self,
    ) -> &'static [(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))];
}

impl PartialEq for &'static (dyn Game + Send + Sync) {
    fn eq(&self, other: &Self) -> bool {
        (*self).type_id() == (*other).type_id()
    }
}
impl Eq for &'static (dyn Game + Send + Sync) {}

impl std::hash::Hash for &'static (dyn Game + Send + Sync) {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (*self).type_id().hash(state);
    }
}

impl std::fmt::Debug for &'static (dyn Game + Send + Sync) {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (*self).type_id().fmt(f)
    }
}

/// Returns the per-game registration for a given gamedb entry, or
/// None when the gamedb entry isn't one we have a Game impl for.
pub fn from_gamedb_entry(entry: GameRef) -> Option<&'static (dyn Game + Send + Sync)> {
    Some(match entry.family_and_variant() {
        ("exe1", 0) => bn1::EXE1,
        ("bn1", 0) => bn1::BN1,

        ("exe2", 0) => bn2::EXE2,
        ("bn2", 0) => bn2::BN2,

        ("exe3", 0) => bn3::EXE3W,
        ("exe3", 1) => bn3::EXE3B,
        ("bn3", 0) => bn3::BN3W,
        ("bn3", 1) => bn3::BN3B,

        ("exe4", 0) => bn4::EXE4RS,
        ("exe4", 1) => bn4::EXE4BM,
        ("bn4", 0) => bn4::BN4RS,
        ("bn4", 1) => bn4::BN4BM,

        ("exe5", 0) => bn5::EXE5B,
        ("exe5", 1) => bn5::EXE5C,
        ("bn5", 0) => bn5::BN5P,
        ("bn5", 1) => bn5::BN5C,

        ("exe6", 0) => bn6::EXE6G,
        ("exe6", 1) => bn6::EXE6F,
        ("bn6", 0) => bn6::BN6G,
        ("bn6", 1) => bn6::BN6F,

        ("exe45", 0) => exe45::EXE45,

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
    let (family, variant) = game.family_and_variant();
    let key = format!("game-{family}.variant-{variant}");
    let s = t(lang, &key);
    if !s.starts_with("⟦") {
        return s;
    }
    let base = t(lang, &format!("game-{family}"));
    if !base.starts_with("⟦") {
        return base;
    }
    format!("{family} v{variant}")
}

/// Short tag (e.g. "BN6"). Same lookup pattern via the `.short`
/// attribute; falls back to `<family> v<variant>` so unknowns still
/// produce something identifying.
pub fn short_name(lang: &unic_langid::LanguageIdentifier, game: GameRef) -> String {
    let (family, variant) = game.family_and_variant();
    let key = format!("game-{family}.short");
    let s = t(lang, &key);
    if s.starts_with("⟦") {
        format!("{family} v{variant}")
    } else {
        s
    }
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
    let key = format!("game-{family}.match-type-{match_type}-{match_subtype}");
    let s = t(lang, &key);
    if s.starts_with("⟦") {
        format!("{match_type}.{match_subtype}")
    } else {
        s
    }
}

pub fn sort_games(lang: &unic_langid::LanguageIdentifier, games: &mut [GameRef]) {
    games.sort_by_key(|g| {
        (
            if region_to_language(g.region()).matches(lang, true, true) {
                0
            } else {
                1
            },
            g.family_and_variant(),
        )
    });
}

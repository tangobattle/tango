//! The single point where games are registered with the app.
//!
//! Every game Tango supports lives in its own `tango-gamesupport-<game>`
//! crate, which groups its variants and their localized strings into one
//! or more [`tango_gamesupport::Family`] values exported as `FAMILIES`.
//! [`FAMILIES`] below is the one feature-gated list that enables them;
//! from it we derive the [`GAMES`] registry ([`detect`] /
//! [`find_by_family_and_variant`] / [`find_by_rom_info`]) *and* the
//! game-name localizer ([`family_str`] and the display helpers). To add a
//! game, enable its crate's feature and list it here — nothing else in
//! the app is game-specific.
//!
//! Game-name localization is deliberately separate from the app's general
//! `crate::i18n` path: each family owns its own Fluent bundle keyed by
//! *bare* names (`name`, `short`, `variant-<n>`, `match-type-<m>-<s>`,
//! `save-<template>`), so there's no `game-<family>` key prefix to keep in
//! sync — the family supplies the namespace.

use crate::library::rom::GameRef;
use std::collections::HashMap;
use std::sync::LazyLock;

use fluent_templates::fluent_bundle::concurrent::FluentBundle;
use fluent_templates::fluent_bundle::FluentResource;

pub use tango_gamesupport::{Family, Game, Region};

/// Every game family enabled in this build, collected from the per-game
/// crates that are compiled in. Each crate is an optional
/// `tango-gamesupport-<game>` dependency gated behind its own
/// `gamesupport-<game>` feature, so this list reflects exactly the
/// enabled features. This is the sole extension point.
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
pub fn find_by_family_and_variant(family: &str, variant: u8) -> Option<GameRef> {
    tango_gamesupport::find_by_family_and_variant(&GAMES, family, variant)
}

/// Look a registered game up by ROM code + revision.
pub fn find_by_rom_info(code: &[u8; 4], revision: u8) -> Option<GameRef> {
    tango_gamesupport::find_by_rom_info(&GAMES, code, revision)
}

/// Identity now that a [`GameRef`] *is* the full registration. Kept so
/// the call sites that previously mapped a bare gamedb entry to its
/// app-level game read unchanged; every registered game is supported.
pub fn from_gamedb_entry(entry: GameRef) -> Option<GameRef> {
    Some(entry)
}

// ---------- game-name localization (dedicated path, separate from i18n) ----------

/// Per-family Fluent bundles, one per `(family id, locale)`, built from
/// each enabled family's `translations`. Keyed by the family id (so a
/// borrowed `&str` family looks up directly) then by language.
static FAMILY_LOCALES: LazyLock<
    HashMap<&'static str, HashMap<unic_langid::LanguageIdentifier, FluentBundle<FluentResource>>>,
> = LazyLock::new(|| {
    let mut map: HashMap<&'static str, HashMap<unic_langid::LanguageIdentifier, FluentBundle<FluentResource>>> =
        HashMap::new();
    for family in FAMILIES.iter() {
        let by_lang = map.entry(family.id).or_default();
        for (lang_str, ftl) in family.translations {
            let Ok(lang) = lang_str.parse::<unic_langid::LanguageIdentifier>() else {
                continue;
            };
            // The fragments are checked in; on a partial parse keep
            // what parsed (a missing key just falls back).
            let res = match FluentResource::try_new(ftl.to_string()) {
                Ok(r) => r,
                Err((r, _errs)) => r,
            };
            let mut bundle = FluentBundle::new_concurrent(vec![lang.clone()]);
            // Plain strings, no placeholders — skip the bidi isolation
            // marks fluent wraps args in by default.
            bundle.set_use_isolating(false);
            let _ = bundle.add_resource(res);
            by_lang.insert(lang, bundle);
        }
    }
    map
});

/// Look the bare `key` up in `family`'s bundle for `lang`, falling back to
/// the en-US fragment. Returns `None` if the family/key isn't defined.
/// This is the dedicated game-string path — game names never go through
/// `crate::i18n::t_opt`.
pub fn family_str(family: &str, lang: &unic_langid::LanguageIdentifier, key: &str) -> Option<String> {
    fn get(bundle: &FluentBundle<FluentResource>, key: &str) -> Option<String> {
        let msg = bundle.get_message(key)?;
        let pattern = msg.value()?;
        let mut errors = vec![];
        let out = bundle.format_pattern(pattern, None, &mut errors);
        if !errors.is_empty() {
            return None;
        }
        Some(out.into_owned())
    }

    let by_lang = FAMILY_LOCALES.get(family)?;
    if let Some(bundle) = by_lang.get(lang) {
        if let Some(s) = get(bundle, key) {
            return Some(s);
        }
    }
    if *lang != crate::i18n::FALLBACK_LANG {
        if let Some(bundle) = by_lang.get(&crate::i18n::FALLBACK_LANG) {
            if let Some(s) = get(bundle, key) {
                return Some(s);
            }
        }
    }
    None
}

pub fn region_to_language(region: Region) -> unic_langid::LanguageIdentifier {
    match region {
        Region::US => unic_langid::langid!("en-US"),
        Region::JP => unic_langid::langid!("ja-JP"),
    }
}

/// Best-effort full display name (e.g. "Mega Man Battle Network 6:
/// Cybeast Gregar"). Looks up the family's `variant-<variant>` string;
/// falls back to the family `name`, then to "<family> v<variant>".
pub fn display_name(lang: &unic_langid::LanguageIdentifier, game: GameRef) -> String {
    let (family, variant) = game.family_and_variant();
    family_str(family, lang, &format!("variant-{variant}"))
        .or_else(|| family_str(family, lang, "name"))
        .unwrap_or_else(|| format!("{family} v{variant}"))
}

/// Short tag (e.g. "BN6") via the family's `short` string; falls back to
/// `<family> v<variant>` so unknowns still produce something identifying.
pub fn short_name(lang: &unic_langid::LanguageIdentifier, game: GameRef) -> String {
    let (family, variant) = game.family_and_variant();
    family_str(family, lang, "short").unwrap_or_else(|| format!("{family} v{variant}"))
}

/// Short *variant* tag (e.g. "White", "Blue Moon") via the family's
/// `variant-<variant>-short` string — the bare color/team name without
/// the series title, for disambiguating saves/templates within a family.
/// Falls back to the family short tag for single-variant families.
pub fn variant_short_name(lang: &unic_langid::LanguageIdentifier, game: GameRef) -> String {
    let (family, variant) = game.family_and_variant();
    family_str(family, lang, &format!("variant-{variant}-short")).unwrap_or_else(|| short_name(lang, game))
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
    family_str(family, lang, &format!("match-type-{match_type}-{match_subtype}"))
        .unwrap_or_else(|| format!("{match_type}.{match_subtype}"))
}

/// All registered games belonging to a family (e.g. "bn3" → US White + US
/// Blue). The family string is region-specific (`exe3` JP vs `bn3` US
/// are distinct families), so the members differ only by color variant.
pub fn games_in_family(family: &str) -> impl Iterator<Item = GameRef> + '_ {
    GAMES
        .iter()
        .copied()
        .filter(move |g| g.family_and_variant().0 == family)
}

/// Best-effort family display name (e.g. "Mega Man Battle Network 3") via
/// the family's `name` string; falls back to the raw family string.
pub fn family_display_name(lang: &unic_langid::LanguageIdentifier, family: &str) -> String {
    family_str(family, lang, "name").unwrap_or_else(|| family.to_string())
}

/// Resolve a (possibly persisted) family string to its `&'static`
/// form, or None if no game uses it. Lets restored config — owned
/// `String`s — drive the `&'static str` family state.
pub fn family_static(family: &str) -> Option<&'static str> {
    GAMES.iter().map(|g| g.family_and_variant().0).find(|f| *f == family)
}

/// Does any game in `family` match the UI language's region? Used to
/// sort the family picker so the user's own-region families lead.
pub fn family_matches_language(lang: &unic_langid::LanguageIdentifier, family: &str) -> bool {
    games_in_family(family).any(|g| region_to_language(g.region()).matches(lang, true, true))
}

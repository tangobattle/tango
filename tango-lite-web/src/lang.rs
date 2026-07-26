//! Which locale the game names come out in.
//!
//! There is no language picker and no app-string localization here — a
//! lite build's own chrome is a dozen words of English. What *is*
//! localized is the games, because "MEGAMAN6_GXX" is not a thing anyone
//! calls a game and `bn6 v1` is worse. Each `tango-gamesupport-<game>`
//! crate already ships Fluent fragments for its family, and
//! [`tango_library::game`] already resolves them; all this module
//! decides is which language to ask for.
//!
//! The answer is the browser's, matched against the locales the
//! families actually carry, with an exact-tag pass before a
//! language-only one so `pt-BR` prefers `pt-BR` over `pt-PT` but
//! `de-AT` still lands on `de-DE`. No match falls back to en-US, which
//! is also what an untranslated key does inside a bundle.

use std::sync::LazyLock;

use tango_library::game;
use tango_library::lang::{FALLBACK_LANG, SUPPORTED_LANGS};
use tango_library::rom::GameRef;

/// Resolved once: the navigator's list doesn't change without a reload.
static LANG: LazyLock<unic_langid::LanguageIdentifier> = LazyLock::new(negotiate);

pub fn lang() -> &'static unic_langid::LanguageIdentifier {
    &LANG
}

fn negotiate() -> unic_langid::LanguageIdentifier {
    let Some(window) = web_sys::window() else {
        return FALLBACK_LANG;
    };
    let requested: Vec<unic_langid::LanguageIdentifier> = window
        .navigator()
        .languages()
        .iter()
        .filter_map(|tag| tag.as_string())
        .filter_map(|tag| tag.parse().ok())
        .collect();

    for want in &requested {
        if let Some(exact) = SUPPORTED_LANGS.iter().find(|have| *have == want) {
            return exact.clone();
        }
    }
    // Region-blind second pass: a browser set to `de-AT` or plain `de`
    // should still get the German fragments.
    for want in &requested {
        if let Some(same_language) = SUPPORTED_LANGS.iter().find(|have| have.language == want.language) {
            return same_language.clone();
        }
    }
    FALLBACK_LANG
}

/// The game's full name, e.g. "Mega Man Battle Network 6: Cybeast
/// Gregar".
pub fn game_name(game: GameRef) -> String {
    game::display_name(lang(), game)
}

/// Just the variant, e.g. "Cybeast Gregar" — for lists where every row
/// is the same family and repeating the series title is noise.
pub fn variant_name(game: GameRef) -> String {
    game::variant_short_name(lang(), game)
}

/// The name for a `(family, variant)` off the wire, which may be a game
/// this build has no support compiled in for — hence the raw fallback
/// rather than a lookup that can't fail.
pub fn game_name_of(family: &str, variant: u8) -> String {
    match game::find_by_family_and_variant(family, variant) {
        Some(game) => game_name(game),
        None => format!("{family} v{variant}"),
    }
}

/// The game's own word for a match type — "Single"/"Triple", or
/// whatever that family calls its modes.
pub fn match_type_name(game: GameRef, mode: u8, subtype: u8) -> String {
    game::match_type_name(lang(), game.family_and_variant().0, mode, subtype)
}

//! UI string localization, copied from `tango/src/i18n.rs` (minus the
//! iced-specific `LanguageChoice` picker type). The Fluent bundles are
//! tango's own — `static_loader!` embeds the sibling crate's
//! `tango/locales` tree at compile time, so the two frontends share one
//! set of translations. Game names use a separate per-family path (see
//! `game.rs`); this module is for the app chrome.

pub use fluent_templates::fluent_bundle::FluentValue;
use fluent_templates::Loader;

// Macros live at crate root via `#[macro_export]`; re-export them
// under `crate::i18n::*` too so callers can `use crate::i18n::{t,
// t_opt};` and pick up the macros alongside the underlying fns.
#[allow(unused_imports)]
pub use crate::{t, t_opt};

pub const FALLBACK_LANG: unic_langid::LanguageIdentifier = unic_langid::langid!("en-US");

/// Locales the app exposes in the language picker. Strings the
/// non-en locales don't translate fall back to en-US via the
/// fluent_templates static_loader's fallback_language.
pub const SUPPORTED_LANGS: &[unic_langid::LanguageIdentifier] = &[
    unic_langid::langid!("en-US"),
    unic_langid::langid!("ja-JP"),
    unic_langid::langid!("zh-CN"),
    unic_langid::langid!("zh-TW"),
    unic_langid::langid!("de-DE"),
    unic_langid::langid!("es-419"),
    unic_langid::langid!("fr-FR"),
    unic_langid::langid!("nl-NL"),
    unic_langid::langid!("pt-BR"),
    unic_langid::langid!("ru-RU"),
    unic_langid::langid!("vi-VN"),
];

fluent_templates::static_loader! {
    static LOCALES = {
        // Compile-time relative to this crate's CARGO_MANIFEST_DIR —
        // tango's locales tree, NOT a copy.
        locales: "../tango/locales",
        fallback_language: "en-US",
        // Disable Fluent's BiDi isolation, which otherwise wraps every
        // interpolated placeholder in U+2068/U+2069 control chars.
        // Matches what game.rs does for its own bundles.
        customise: |bundle| bundle.set_use_isolating(false),
    };
}

/// Look up `key` in the bundle; returns `None` if the locale (and
/// the fallback locale) don't define it, OR if the template references
/// a placeholder the caller didn't pass — `try_lookup` (fluent-templates
/// 0.14) downgrades format errors to `None` instead of panicking the
/// way `lookup` does.
pub fn t_opt(lang: &unic_langid::LanguageIdentifier, key: &str) -> Option<String> {
    LOCALES.try_lookup(lang, key)
}

pub fn t(lang: &unic_langid::LanguageIdentifier, key: &str) -> String {
    // The `⟦…⟧` wrapping is a debug-only visual sentinel so a
    // missing string sticks out in the UI. Never pattern-match it
    // — use `t_opt` if you need the missing-key signal.
    t_opt(lang, key).unwrap_or_else(|| format!("⟦{key}⟧"))
}

/// Like [`t_opt`], but substitutes Fluent placeholders. Returns
/// `None` if the locale (and fallback locale) don't define `key`
/// or if formatting fails (e.g. a placeholder we didn't pass).
pub fn t_args_opt(
    lang: &unic_langid::LanguageIdentifier,
    key: &str,
    args: &[(&'static str, FluentValue<'_>)],
) -> Option<String> {
    let map: std::collections::HashMap<std::borrow::Cow<'static, str>, FluentValue<'_>> = args
        .iter()
        .map(|(k, v)| (std::borrow::Cow::Borrowed(*k), v.clone()))
        .collect();
    LOCALES.try_lookup_with_args(lang, key, &map)
}

/// Like [`t`], but substitutes Fluent placeholders. Pass each
/// `(name, value)` as a borrowed slice so callers don't have to build
/// a HashMap inline:
///
/// ```ignore
/// t_args(lang, "lobby-latency", &[("ms", 42i64.into())])
/// ```
pub fn t_args(lang: &unic_langid::LanguageIdentifier, key: &str, args: &[(&'static str, FluentValue<'_>)]) -> String {
    t_args_opt(lang, key, args).unwrap_or_else(|| format!("⟦{key}⟧"))
}

/// Look up `$key` (string literal, enforced at compile time) in the
/// bundle and return a `String`. Extra `name = value` pairs are
/// passed as fluent placeholders via `FluentValue::from`.
///
/// ```ignore
/// t!(lang, "lobby-ready");
/// t!(lang, "lobby-latency", ms = 42i64);
/// ```
#[macro_export]
macro_rules! t {
    ($lang:expr, $key:literal $(,)?) => {
        $crate::i18n::t($lang, $key)
    };
    ($lang:expr, $key:literal, $($k:ident = $v:expr),+ $(,)?) => {
        $crate::i18n::t_args(
            $lang,
            $key,
            &[$((stringify!($k), $crate::i18n::FluentValue::from($v))),+],
        )
    };
}

/// Like [`t!`] but returns `Option<String>` — `None` if the locale
/// (and fallback locale) don't define `$key`. Use when you need to
/// branch on "string actually defined".
#[macro_export]
macro_rules! t_opt {
    ($lang:expr, $key:literal $(,)?) => {
        $crate::i18n::t_opt($lang, $key)
    };
    ($lang:expr, $key:literal, $($k:ident = $v:expr),+ $(,)?) => {
        $crate::i18n::t_args_opt(
            $lang,
            $key,
            &[$((stringify!($k), $crate::i18n::FluentValue::from($v))),+],
        )
    };
}

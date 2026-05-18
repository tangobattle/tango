pub use fluent_templates::fluent_bundle::FluentValue;
use fluent_templates::Loader;

pub const FALLBACK_LANG: unic_langid::LanguageIdentifier = unic_langid::langid!("en-US");

fluent_templates::static_loader! {
    pub static LOCALES = {
        locales: "./locales",
        fallback_language: "en-US",
    };
}

/// Look up `key` in the bundle; returns `None` if the locale (and
/// the fallback locale) don't define it. Prefer this over [`t`]
/// when the caller branches on "is the string actually defined".
pub fn t_opt(lang: &unic_langid::LanguageIdentifier, key: &str) -> Option<String> {
    LOCALES.lookup(lang, key)
}

pub fn t(lang: &unic_langid::LanguageIdentifier, key: &str) -> String {
    // The `⟦…⟧` wrapping is a debug-only visual sentinel so a
    // missing string sticks out in the UI. Never pattern-match it
    // — use `t_opt` if you need the missing-key signal.
    t_opt(lang, key).unwrap_or_else(|| format!("⟦{key}⟧"))
}

/// Like [`t_opt`], but substitutes Fluent placeholders. Returns
/// `None` if the locale (and fallback locale) don't define `key`.
pub fn t_args_opt(
    lang: &unic_langid::LanguageIdentifier,
    key: &str,
    args: &[(&'static str, FluentValue<'_>)],
) -> Option<String> {
    let map: std::collections::HashMap<&str, FluentValue<'_>> =
        args.iter().map(|(k, v)| (*k, v.clone())).collect();
    LOCALES.lookup_with_args(lang, key, &map)
}

/// Like [`t`], but substitutes Fluent placeholders. Pass each
/// `(name, value)` as a borrowed slice so callers don't have to build
/// a HashMap inline:
///
/// ```ignore
/// t_args(lang, "welcome-step-roms-detected", &[("count", 4.into())])
/// ```
pub fn t_args(
    lang: &unic_langid::LanguageIdentifier,
    key: &str,
    args: &[(&'static str, FluentValue<'_>)],
) -> String {
    t_args_opt(lang, key, args).unwrap_or_else(|| format!("⟦{key}⟧"))
}

/// Picker option for the language dropdown. Holds the
/// `LanguageIdentifier` (what gets serialized into config) plus
/// the endonym from each locale's `LANGUAGE` Fluent key. The
/// `Display` impl renders the endonym so the picker shows
/// "日本語" instead of "ja-JP".
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct LanguageChoice {
    pub id: unic_langid::LanguageIdentifier,
    pub label: String,
}

impl LanguageChoice {
    /// Build a [`LanguageChoice`] for `id` by reading the
    /// `LANGUAGE` key from `id`'s own locale (so users see their
    /// language's name in its own script). Falls back to the
    /// locale code if the key is missing.
    pub fn new(id: unic_langid::LanguageIdentifier) -> Self {
        let label = t_opt(&id, "LANGUAGE").unwrap_or_else(|| id.to_string());
        Self { id, label }
    }
}

impl std::fmt::Display for LanguageChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

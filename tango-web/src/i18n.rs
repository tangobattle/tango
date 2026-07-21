//! UI strings, the desktop's fluent setup verbatim: a static_loader
//! over ./locales (en-US fallback per key), the `t!`/`t_opt!` macros,
//! and the language picker's endonym choices. The resolved language
//! lives in a GlobalSignal so every component re-renders on change;
//! the initial value comes from config, else the browser's own
//! `navigator.language`.

pub use fluent_templates::fluent_bundle::FluentValue;
use fluent_templates::Loader;

use dioxus::prelude::*;

#[allow(unused_imports)]
pub use crate::{t, t_opt};

pub const FALLBACK_LANG: unic_langid::LanguageIdentifier = unic_langid::langid!("en-US");

/// Locales the app exposes in the language picker — the desktop's set.
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
        locales: "./locales",
        fallback_language: "en-US",
        // Disable Fluent's BiDi isolation, which otherwise wraps every
        // interpolated placeholder in U+2068/U+2069 control chars.
        customise: |bundle| bundle.set_use_isolating(false),
    };
}

/// The resolved UI language, readable from any component. Set at app
/// start ([`init`]) and by the settings picker.
pub static LANG: GlobalSignal<unic_langid::LanguageIdentifier> = Signal::global(|| FALLBACK_LANG);

/// Resolve the startup language: the persisted config choice if valid,
/// else the best [`SUPPORTED_LANGS`] match for `navigator.language`.
pub fn init(config_language: Option<&str>) {
    let lang = config_language
        .and_then(|s| s.parse::<unic_langid::LanguageIdentifier>().ok())
        .filter(|l| SUPPORTED_LANGS.contains(l))
        .or_else(|| {
            let requested = web_sys::window()?.navigator().language()?;
            let requested: unic_langid::LanguageIdentifier = requested.parse().ok()?;
            SUPPORTED_LANGS
                .iter()
                .find(|l| l.matches(&requested, true, true))
                .cloned()
        })
        .unwrap_or(FALLBACK_LANG);
    *LANG.write() = lang;
}

pub fn t_opt(lang: &unic_langid::LanguageIdentifier, key: &str) -> Option<String> {
    LOCALES.try_lookup(lang, key)
}

pub fn t(lang: &unic_langid::LanguageIdentifier, key: &str) -> String {
    // The `⟦…⟧` wrapping is a debug-only visual sentinel so a missing
    // string sticks out in the UI.
    t_opt(lang, key).unwrap_or_else(|| format!("⟦{key}⟧"))
}

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

pub fn t_args(
    lang: &unic_langid::LanguageIdentifier,
    key: &str,
    args: &[(&'static str, FluentValue<'_>)],
) -> String {
    t_args_opt(lang, key, args).unwrap_or_else(|| format!("⟦{key}⟧"))
}

/// Look up `$key` (string literal) in the bundle. Extra `name = value`
/// pairs pass as fluent placeholders.
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

/// Like [`t!`] but returns `Option<String>`.
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

/// Picker option for the language dropdown: the id plus the endonym
/// from each locale's own `LANGUAGE` key.
pub fn language_label(id: &unic_langid::LanguageIdentifier) -> String {
    t_opt(id, "LANGUAGE").unwrap_or_else(|| id.to_string())
}

//! This crate's own Fluent bundle: the save-editor strings, split out of
//! the app's `locales/` when the save view moved here. Same shape as
//! tango's `i18n` module — `t!`/`t_opt!` take literal keys so a typo'd
//! key is greppable, and missing strings render as `⟦key⟧`.

pub use fluent_templates::fluent_bundle::FluentValue;
use fluent_templates::Loader;

#[allow(unused_imports)]
pub use crate::{t, t_opt};

fluent_templates::static_loader! {
    static LOCALES = {
        locales: "./locales",
        fallback_language: "en-US",
        // Match the app bundle: no BiDi isolation control chars.
        customise: |bundle| bundle.set_use_isolating(false),
    };
}

pub fn t_opt(lang: &unic_langid::LanguageIdentifier, key: &str) -> Option<String> {
    LOCALES.try_lookup(lang, key)
}

pub fn t(lang: &unic_langid::LanguageIdentifier, key: &str) -> String {
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

pub fn t_args(lang: &unic_langid::LanguageIdentifier, key: &str, args: &[(&'static str, FluentValue<'_>)]) -> String {
    t_args_opt(lang, key, args).unwrap_or_else(|| format!("⟦{key}⟧"))
}

/// Look up `$key` (string literal, enforced at compile time) in this
/// crate's bundle and return a `String`.
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

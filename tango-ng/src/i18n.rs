pub use fluent_templates::fluent_bundle::FluentValue;
use fluent_templates::Loader;

pub const FALLBACK_LANG: unic_langid::LanguageIdentifier = unic_langid::langid!("en-US");

fluent_templates::static_loader! {
    pub static LOCALES = {
        locales: "./locales",
        fallback_language: "en-US",
    };
}

pub fn t(lang: &unic_langid::LanguageIdentifier, key: &str) -> String {
    LOCALES
        .lookup(lang, key)
        .unwrap_or_else(|| format!("⟦{key}⟧"))
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
    let map: std::collections::HashMap<&str, FluentValue<'_>> =
        args.iter().map(|(k, v)| (*k, v.clone())).collect();
    LOCALES
        .lookup_with_args(lang, key, &map)
        .unwrap_or_else(|| format!("⟦{key}⟧"))
}

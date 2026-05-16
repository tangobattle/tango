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

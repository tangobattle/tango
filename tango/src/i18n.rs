pub const FALLBACK_LANG: unic_langid::LanguageIdentifier = unic_langid::langid!("en-US");
fluent_templates::static_loader! {
    pub static LOCALES = {
        locales: "./locales",
        fallback_language: "en-US",
    };
}

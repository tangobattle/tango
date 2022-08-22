pub const FALLBACK_LANG: &str = "en-US";
fluent_templates::static_loader! {
    pub static LOCALES = {
        locales: "./locales",
        fallback_language: "en-US",
    };
}

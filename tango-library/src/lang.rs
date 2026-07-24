//! The locale set, shared by the app's own Fluent bundles and the
//! per-family game-name bundles in [`crate::game`].
//!
//! Only the identifiers live here. The app's translated strings stay
//! with the frontend that renders them; the game families carry their
//! own bundles.

/// Strings a locale doesn't translate fall back to this one.
pub const FALLBACK_LANG: unic_langid::LanguageIdentifier = unic_langid::langid!("en-US");

/// Locales the app exposes in the language picker.
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

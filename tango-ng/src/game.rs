//! Game registration + name localization, copied from `tango/src/game.rs`
//! (trimmed to what tango-ng uses so far). The `#[cfg]` list in [`FAMILIES`]
//! is the sole extension point for adding games.

use crate::rom::GameRef;
use std::collections::HashMap;
use std::sync::LazyLock;

use fluent_templates::fluent_bundle::concurrent::FluentBundle;
use fluent_templates::fluent_bundle::FluentResource;

pub use tango_gamesupport::Family;

pub const FALLBACK_LANG: unic_langid::LanguageIdentifier = unic_langid::langid!("en-US");

/// Every game family enabled in this build, collected from the per-game
/// crates that are compiled in. Each crate is an optional
/// `tango-gamesupport-<game>` dependency gated behind its own
/// `gamesupport-<game>` feature, so this list reflects exactly the
/// enabled features.
pub static FAMILIES: LazyLock<Vec<&'static Family>> = LazyLock::new(|| {
    #[allow(unused_mut)]
    let mut families: Vec<&'static Family> = Vec::new();
    #[cfg(feature = "gamesupport-bn1")]
    families.extend_from_slice(tango_gamesupport_bn1::FAMILIES);
    #[cfg(feature = "gamesupport-bn2")]
    families.extend_from_slice(tango_gamesupport_bn2::FAMILIES);
    #[cfg(feature = "gamesupport-bn3")]
    families.extend_from_slice(tango_gamesupport_bn3::FAMILIES);
    #[cfg(feature = "gamesupport-bn4")]
    families.extend_from_slice(tango_gamesupport_bn4::FAMILIES);
    #[cfg(feature = "gamesupport-bn5")]
    families.extend_from_slice(tango_gamesupport_bn5::FAMILIES);
    #[cfg(feature = "gamesupport-bn6")]
    families.extend_from_slice(tango_gamesupport_bn6::FAMILIES);
    #[cfg(feature = "gamesupport-exe45")]
    families.extend_from_slice(tango_gamesupport_exe45::FAMILIES);
    families
});

/// The flat game registry, derived from [`FAMILIES`].
pub static GAMES: LazyLock<Vec<GameRef>> = LazyLock::new(|| tango_gamesupport::games_of(&FAMILIES));

/// Identify a clean ROM dump against the registry.
pub fn detect(rom: &[u8]) -> Option<GameRef> {
    tango_gamesupport::detect(&GAMES, rom)
}

/// Per-family Fluent bundles, one per `(family id, locale)`, built from
/// each enabled family's `translations`. Keyed by the family id then by
/// language.
static FAMILY_LOCALES: LazyLock<
    HashMap<&'static str, HashMap<unic_langid::LanguageIdentifier, FluentBundle<FluentResource>>>,
> = LazyLock::new(|| {
    let mut map: HashMap<&'static str, HashMap<unic_langid::LanguageIdentifier, FluentBundle<FluentResource>>> =
        HashMap::new();
    for family in FAMILIES.iter() {
        let by_lang = map.entry(family.id).or_default();
        for (lang_str, ftl) in family.translations {
            let Ok(lang) = lang_str.parse::<unic_langid::LanguageIdentifier>() else {
                continue;
            };
            // The fragments are checked in; on a partial parse keep
            // what parsed (a missing key just falls back).
            let res = match FluentResource::try_new(ftl.to_string()) {
                Ok(r) => r,
                Err((r, _errs)) => r,
            };
            let mut bundle = FluentBundle::new_concurrent(vec![lang.clone()]);
            // Plain strings, no placeholders — skip the bidi isolation
            // marks fluent wraps args in by default.
            bundle.set_use_isolating(false);
            let _ = bundle.add_resource(res);
            by_lang.insert(lang, bundle);
        }
    }
    map
});

/// Look the bare `key` up in `family`'s bundle for `lang`, falling back to
/// the en-US fragment. Returns `None` if the family/key isn't defined.
fn family_str(family: &str, lang: &unic_langid::LanguageIdentifier, key: &str) -> Option<String> {
    fn get(bundle: &FluentBundle<FluentResource>, key: &str) -> Option<String> {
        let msg = bundle.get_message(key)?;
        let pattern = msg.value()?;
        let mut errors = vec![];
        let out = bundle.format_pattern(pattern, None, &mut errors);
        if !errors.is_empty() {
            return None;
        }
        Some(out.into_owned())
    }

    let by_lang = FAMILY_LOCALES.get(family)?;
    if let Some(bundle) = by_lang.get(lang) {
        if let Some(s) = get(bundle, key) {
            return Some(s);
        }
    }
    if *lang != FALLBACK_LANG {
        if let Some(bundle) = by_lang.get(&FALLBACK_LANG) {
            if let Some(s) = get(bundle, key) {
                return Some(s);
            }
        }
    }
    None
}

/// Best-effort full display name (e.g. "Mega Man Battle Network 6:
/// Cybeast Gregar"). Looks up the family's `variant-<variant>` string;
/// falls back to the family `name`, then to "<family> v<variant>".
pub fn display_name(lang: &unic_langid::LanguageIdentifier, game: GameRef) -> String {
    let (family, variant) = game.family_and_variant();
    family_str(family, lang, &format!("variant-{variant}"))
        .or_else(|| family_str(family, lang, "name"))
        .unwrap_or_else(|| format!("{family} v{variant}"))
}

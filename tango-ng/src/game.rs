use crate::i18n::t;
use crate::rom::GameRef;

pub fn region_to_language(region: tango_gamedb::Region) -> unic_langid::LanguageIdentifier {
    match region {
        tango_gamedb::Region::US => unic_langid::langid!("en-US"),
        tango_gamedb::Region::JP => unic_langid::langid!("ja-JP"),
    }
}

/// Best-effort display name. Looks up `game-<family>-v<variant>` in the
/// active locale; falls back to "<family> v<variant>" when missing.
pub fn display_name(lang: &unic_langid::LanguageIdentifier, game: GameRef) -> String {
    let (family, variant) = game.family_and_variant();
    let key = format!("game-{family}-v{variant}");
    let s = t(lang, &key);
    if s.starts_with("⟦") {
        format!("{family} v{variant}")
    } else {
        s
    }
}

pub fn sort_games(lang: &unic_langid::LanguageIdentifier, games: &mut [GameRef]) {
    games.sort_by_key(|g| {
        (
            if region_to_language(g.region()).matches(lang, true, true) {
                0
            } else {
                1
            },
            g.family_and_variant(),
        )
    });
}

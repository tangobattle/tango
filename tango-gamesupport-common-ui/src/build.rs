//! Canonical localized presentation for structured build violations. Both the
//! save editor and the host application's PvP warning use these functions so a
//! rule has one sentence in one Fluent bundle.

use tango_gamesupport::{BuildChip, BuildViolationFormat, BuildViolationKind};

use crate::i18n::t;

pub fn chip_label(lang: &unic_langid::LanguageIdentifier, chip: &BuildChip) -> String {
    let name = chip
        .name
        .clone()
        .unwrap_or_else(|| t!(lang, "build-chip-unknown", id = chip.id as i64));
    format!("{name} {}", chip.code)
}

pub fn violation_reason(lang: &unic_langid::LanguageIdentifier, kind: BuildViolationKind) -> String {
    match kind {
        BuildViolationKind::ChipIllegalForGame => t!(lang, "build-violation-chip-illegal-for-game"),
        BuildViolationKind::TooManyCopiesOfChip { used, limit } => t!(
            lang,
            "build-violation-too-many-copies-of-chip",
            used = used as i64,
            limit = limit as i64
        ),
        BuildViolationKind::TooManyNaviChips { used, limit } => t!(
            lang,
            "build-violation-too-many-navi-chips",
            used = used as i64,
            limit = limit as i64
        ),
        BuildViolationKind::TooManyMegaChips { used, limit } => t!(
            lang,
            "build-violation-too-many-mega-chips",
            used = used as i64,
            limit = limit as i64
        ),
        BuildViolationKind::TooManyGigaChips { used, limit } => t!(
            lang,
            "build-violation-too-many-giga-chips",
            used = used as i64,
            limit = limit as i64
        ),
        BuildViolationKind::TooManyDarkChips { used, limit } => t!(
            lang,
            "build-violation-too-many-dark-chips",
            used = used as i64,
            limit = limit as i64
        ),
        BuildViolationKind::RegularChipExceedsMemory { used, limit } => t!(
            lang,
            "build-violation-regular-chip-exceeds-memory",
            used = used as i64,
            limit = limit as i64
        ),
        BuildViolationKind::TagChipsExceedMemory { used, limit } => t!(
            lang,
            "build-violation-tag-chips-exceed-memory",
            used = used as i64,
            limit = limit as i64
        ),
    }
}

pub fn chip_violation(
    lang: &unic_langid::LanguageIdentifier,
    chips: &[&BuildChip],
    kind: BuildViolationKind,
) -> String {
    let chips = chips
        .iter()
        .map(|chip| chip_label(lang, chip))
        .collect::<Vec<_>>()
        .join(", ");
    let reason = violation_reason(lang, kind);
    t!(lang, "build-violation-chip", chips = chips, reason = reason)
}

pub fn folder_not_full(lang: &unic_langid::LanguageIdentifier, used: usize, required: usize) -> String {
    t!(
        lang,
        "build-violation-folder-not-full",
        used = used as i64,
        required = required as i64
    )
}

pub fn format(lang: &unic_langid::LanguageIdentifier, violation: BuildViolationFormat<'_>) -> String {
    match violation {
        BuildViolationFormat::FolderNotFull { used, required } => folder_not_full(lang, used, required),
        BuildViolationFormat::Chips { chips, kind } => chip_violation(lang, chips, kind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_language_formats_build_violations_without_english_fallback() {
        let english: unic_langid::LanguageIdentifier = "en-US".parse().unwrap();
        let english_text = folder_not_full(&english, 29, 30);
        let kinds = [
            BuildViolationKind::ChipIllegalForGame,
            BuildViolationKind::TooManyCopiesOfChip { used: 6, limit: 5 },
            BuildViolationKind::TooManyNaviChips { used: 6, limit: 5 },
            BuildViolationKind::TooManyMegaChips { used: 6, limit: 5 },
            BuildViolationKind::TooManyGigaChips { used: 2, limit: 1 },
            BuildViolationKind::TooManyDarkChips { used: 4, limit: 3 },
            BuildViolationKind::RegularChipExceedsMemory { used: 45, limit: 40 },
            BuildViolationKind::TagChipsExceedMemory { used: 65, limit: 60 },
        ];
        let english_reasons = kinds.map(|kind| violation_reason(&english, kind));

        for language in [
            "de-DE", "es-419", "fr-FR", "ja-JP", "nl-NL", "pt-BR", "ru-RU", "vi-VN", "zh-CN", "zh-TW",
        ] {
            let language = language.parse().unwrap();
            assert_ne!(folder_not_full(&language, 29, 30), english_text);
            for (kind, english_reason) in kinds.iter().copied().zip(&english_reasons) {
                assert_ne!(violation_reason(&language, kind), *english_reason);
            }
        }
    }

    #[test]
    fn formatting_uses_the_language_passed_by_the_current_view() {
        let english = "en-US".parse().unwrap();
        let japanese = "ja-JP".parse().unwrap();
        let kind = BuildViolationKind::TooManyNaviChips { used: 6, limit: 5 };

        assert_ne!(violation_reason(&english, kind), violation_reason(&japanese, kind));
    }
}

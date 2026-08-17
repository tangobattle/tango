//! Game-support-owned build legality. Raw subject types, game-specific rules,
//! grouping, ordering, and localization all stop here; the host application
//! receives only [`tango_gamesupport::BuildViolation`]'s opaque formatter.

pub use crate::dataview::build::{
    BuildChip, BuildNavicustPart, BuildPatchCard, BuildViolation, BuildViolationKind, NavicustViolationKind,
    PatchCardViolationKind,
};
use crate::i18n::t;

#[derive(Debug, Clone, PartialEq, Eq)]
enum PresentedBuildViolation {
    FolderNotFull {
        used: usize,
        required: usize,
    },
    Chips {
        chips: Vec<BuildChip>,
        kind: BuildViolationKind,
    },
    PatchCards {
        patch_cards: Vec<BuildPatchCard>,
        kind: PatchCardViolationKind,
    },
    NavicustParts {
        parts: Vec<BuildNavicustPart>,
        kind: NavicustViolationKind,
    },
    NavicustMaterializationMismatch,
}

pub fn chip_label(lang: &unic_langid::LanguageIdentifier, chip: &BuildChip) -> String {
    let name = chip
        .name
        .clone()
        .unwrap_or_else(|| t!(lang, "build-chip-unknown", id = chip.id as i64));
    if chip.code.is_empty() {
        name
    } else {
        format!("{name} {}", chip.code)
    }
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

/// The single presentation boundary for legality text. Detailed reports pass
/// a subject; warnings attached to an already-labelled row pass `None`.
pub fn format_violation(
    lang: &unic_langid::LanguageIdentifier,
    subject: Option<&str>,
    reason: String,
) -> String {
    match subject.filter(|subject| !subject.is_empty()) {
        Some(subject) => t!(
            lang,
            "build-violation",
            subject = subject.to_string(),
            reason = reason
        ),
        None => reason,
    }
}

/// The reason attached directly to one editor slot. It preserves useful
/// limit details while leaving the already-visible item label out.
pub fn slot_violation_reason(lang: &unic_langid::LanguageIdentifier, kind: BuildViolationKind) -> String {
    format_violation(lang, None, violation_reason(lang, kind))
}

pub fn navicust_slot_warning(lang: &unic_langid::LanguageIdentifier) -> String {
    format_violation(lang, None, navicust_violation_reason(lang))
}

pub fn patch_card_slot_warning(
    lang: &unic_langid::LanguageIdentifier,
    mb: u8,
    used: u32,
    limit: u32,
) -> String {
    format_violation(
        lang,
        None,
        patch_card_violation_reason(
            lang,
            PatchCardViolationKind::TotalMbExceeded { used, limit },
            Some(mb),
        ),
    )
}

pub fn navicust_part_label(lang: &unic_langid::LanguageIdentifier, part: &BuildNavicustPart) -> String {
    part.name
        .clone()
        .unwrap_or_else(|| t!(lang, "build-navicust-part-unknown", id = part.id as i64))
}

pub fn navicust_violation(
    lang: &unic_langid::LanguageIdentifier,
    parts: &[&BuildNavicustPart],
    kind: NavicustViolationKind,
) -> String {
    let subject = parts
        .iter()
        .map(|part| navicust_part_label(lang, part))
        .collect::<Vec<_>>()
        .join(", ");
    match kind {
        NavicustViolationKind::MaterializationMismatch => {
            format_violation(lang, Some(&subject), navicust_violation_reason(lang))
        }
    }
}

fn navicust_violation_reason(lang: &unic_langid::LanguageIdentifier) -> String {
    t!(lang, "build-violation-navicust-invalid-shape-reason")
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
    format_violation(lang, Some(&chips), reason)
}

pub fn patch_card_label(lang: &unic_langid::LanguageIdentifier, patch_card: &BuildPatchCard) -> String {
    let name = patch_card
        .name
        .clone()
        .unwrap_or_else(|| t!(lang, "build-patch-card-unknown", id = patch_card.id as i64));
    format!("{name} ({}MB)", patch_card.mb)
}

pub fn patch_card_violation(
    lang: &unic_langid::LanguageIdentifier,
    patch_cards: &[&BuildPatchCard],
    kind: PatchCardViolationKind,
) -> String {
    let subject = patch_cards
        .iter()
        .map(|patch_card| patch_card_label(lang, patch_card))
        .collect::<Vec<_>>()
        .join(", ");
    format_violation(
        lang,
        Some(&subject),
        patch_card_violation_reason(lang, kind, None),
    )
}

fn patch_card_violation_reason(
    lang: &unic_langid::LanguageIdentifier,
    kind: PatchCardViolationKind,
    contribution_mb: Option<u8>,
) -> String {
    match kind {
        PatchCardViolationKind::TotalMbExceeded { used, limit } => match contribution_mb {
            Some(mb) => t!(
                lang,
                "build-violation-patch-card-exceeds-memory-with-contribution",
                mb = mb as i64,
                used = used as i64,
                limit = limit as i64
            ),
            None => t!(
                lang,
                "build-violation-patch-cards-exceed-memory",
                used = used as i64,
                limit = limit as i64
            ),
        },
    }
}

pub fn folder_not_full(lang: &unic_langid::LanguageIdentifier, used: usize, required: usize) -> String {
    t!(
        lang,
        "build-violation-folder-not-full",
        used = used as i64,
        required = required as i64
    )
}

fn format_presented(lang: &unic_langid::LanguageIdentifier, violation: &PresentedBuildViolation) -> String {
    match violation {
        PresentedBuildViolation::FolderNotFull { used, required } => folder_not_full(lang, *used, *required),
        PresentedBuildViolation::Chips { chips, kind } => {
            chip_violation(lang, &chips.iter().collect::<Vec<_>>(), *kind)
        }
        PresentedBuildViolation::PatchCards { patch_cards, kind } => {
            patch_card_violation(lang, &patch_cards.iter().collect::<Vec<_>>(), *kind)
        }
        PresentedBuildViolation::NavicustParts { parts, kind } => {
            navicust_violation(lang, &parts.iter().collect::<Vec<_>>(), *kind)
        }
        PresentedBuildViolation::NavicustMaterializationMismatch => {
            t!(lang, "build-violation-navicust-materialization")
        }
    }
}

fn present_build_violations(violations: Vec<BuildViolation>) -> Vec<PresentedBuildViolation> {
    let mut presented = vec![];
    let mut chip_violations: Vec<BuildViolation> = violations
        .iter()
        .filter(|violation| matches!(violation, BuildViolation::Chip { .. }))
        .cloned()
        .collect();
    chip_violations.sort_by_key(|violation| match violation {
        BuildViolation::Chip { slot, chip, .. } => (chip.id, *slot),
        _ => unreachable!("filtered to chip violations"),
    });
    let mut chip_violations = chip_violations.into_iter();

    for source_violation in violations {
        let violation = if matches!(source_violation, BuildViolation::Chip { .. }) {
            chip_violations.next().expect("one sorted entry per chip violation")
        } else {
            source_violation
        };
        match violation {
            BuildViolation::FolderNotFull { used, required } => {
                presented.push(PresentedBuildViolation::FolderNotFull { used, required });
            }
            BuildViolation::Chip { chip, kind, .. } => {
                let group_idx = presented
                    .iter()
                    .position(|group| {
                        matches!(
                            group,
                            PresentedBuildViolation::Chips {
                                kind: grouped_kind,
                                ..
                            } if grouped_kind == &kind
                        )
                    })
                    .unwrap_or_else(|| {
                        presented.push(PresentedBuildViolation::Chips { chips: vec![], kind });
                        presented.len() - 1
                    });
                let PresentedBuildViolation::Chips { chips, .. } = &mut presented[group_idx] else {
                    unreachable!("chip violation group has a chip presentation")
                };
                if !chips.contains(&chip) {
                    chips.push(chip);
                }
            }
            BuildViolation::PatchCard { patch_card, kind, .. } => {
                let group_idx = presented
                    .iter()
                    .position(|group| {
                        matches!(
                            group,
                            PresentedBuildViolation::PatchCards {
                                kind: grouped_kind,
                                ..
                            } if grouped_kind == &kind
                        )
                    })
                    .unwrap_or_else(|| {
                        presented.push(PresentedBuildViolation::PatchCards {
                            patch_cards: vec![],
                            kind,
                        });
                        presented.len() - 1
                    });
                let PresentedBuildViolation::PatchCards { patch_cards, .. } = &mut presented[group_idx] else {
                    unreachable!("patch-card violation group has a patch-card presentation")
                };
                if !patch_cards.contains(&patch_card) {
                    patch_cards.push(patch_card);
                }
            }
            BuildViolation::NavicustPart { part, kind, .. } => {
                let group_idx = presented
                    .iter()
                    .position(|group| {
                        matches!(
                            group,
                            PresentedBuildViolation::NavicustParts {
                                kind: grouped_kind,
                                ..
                            } if grouped_kind == &kind
                        )
                    })
                    .unwrap_or_else(|| {
                        presented.push(PresentedBuildViolation::NavicustParts { parts: vec![], kind });
                        presented.len() - 1
                    });
                let PresentedBuildViolation::NavicustParts { parts, .. } = &mut presented[group_idx] else {
                    unreachable!("NaviCust violation group has a NaviCust presentation")
                };
                if !parts.contains(&part) {
                    parts.push(part);
                }
            }
            BuildViolation::NavicustMaterializationMismatch => {
                presented.push(PresentedBuildViolation::NavicustMaterializationMismatch);
            }
        }
    }

    for group in &mut presented {
        match group {
            PresentedBuildViolation::Chips { chips, .. } => chips.sort_by_key(|chip| chip.id),
            PresentedBuildViolation::PatchCards { patch_cards, .. } => {
                patch_cards.sort_by_key(|patch_card| patch_card.id)
            }
            PresentedBuildViolation::NavicustParts { parts, .. } => {
                parts.sort_by_key(|part| (part.id, part.row, part.col))
            }
            PresentedBuildViolation::FolderNotFull { .. }
            | PresentedBuildViolation::NavicustMaterializationMismatch => {}
        }
    }
    presented
}

/// Convert game-support's raw report into the only legality type exposed to
/// the host. Each closure owns its already-grouped subjects and localizes on
/// demand, so no game-specific enum or formatter function crosses the seam.
pub fn opaque_violations(violations: Vec<BuildViolation>) -> Vec<tango_gamesupport::BuildViolation> {
    present_build_violations(violations)
        .into_iter()
        .map(|violation| tango_gamesupport::BuildViolation::new(move |lang| format_presented(lang, &violation)))
        .collect()
}

/// Battle Network's concrete rule report collapsed into the shared save-view
/// metadata plus opaque opponent warnings.
pub fn report(violations: Vec<BuildViolation>) -> crate::editor::BuildReport {
    use crate::editor::view::Tab;

    let mut error_tabs = std::collections::HashSet::new();
    let mut blocks_save = false;
    for violation in &violations {
        match violation {
            BuildViolation::FolderNotFull { .. } => {
                error_tabs.insert(Tab::Folder);
                // A folder must be complete for the save to be structurally
                // usable. Legality violations within a complete folder stay
                // advisory and do not disable Save.
                blocks_save = true;
            }
            BuildViolation::Chip { .. } => {
                error_tabs.insert(Tab::Folder);
            }
            BuildViolation::PatchCard { .. } => {
                error_tabs.insert(Tab::PatchCards);
            }
            BuildViolation::NavicustPart { .. } | BuildViolation::NavicustMaterializationMismatch => {
                error_tabs.insert(Tab::Navicust);
            }
        }
    }
    crate::editor::BuildReport {
        error_tabs,
        blocks_save,
        warnings: opaque_violations(violations),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot_warning_texts(lang: &unic_langid::LanguageIdentifier) -> Vec<String> {
        [
            BuildViolationKind::ChipIllegalForGame,
            BuildViolationKind::TooManyCopiesOfChip { used: 6, limit: 5 },
            BuildViolationKind::TooManyNaviChips { used: 6, limit: 5 },
            BuildViolationKind::TooManyMegaChips { used: 6, limit: 5 },
            BuildViolationKind::TooManyGigaChips { used: 2, limit: 1 },
            BuildViolationKind::TooManyDarkChips { used: 4, limit: 3 },
            BuildViolationKind::RegularChipExceedsMemory { used: 45, limit: 40 },
            BuildViolationKind::TagChipsExceedMemory { used: 65, limit: 60 },
        ]
        .into_iter()
        .map(|kind| slot_violation_reason(lang, kind))
        .chain([
            navicust_slot_warning(lang),
            patch_card_slot_warning(lang, 6, 101, 80),
            format_violation(
                lang,
                None,
                t!(
                    lang,
                    "build-violation-patch-card4-wrong-slot-reason",
                    actual_slot = "0A",
                    expected_slot = "0C"
                ),
            ),
            format_violation(
                lang,
                None,
                t!(
                    lang,
                    "build-violation-patch-card4-not-in-catalog-reason",
                    actual_slot = "0A"
                ),
            ),
            t!(
                lang,
                "build-violation-partycust-gauge-with-program",
                cost = 3,
                used = 8,
                limit = 6
            ),
            t!(lang, "build-violation-partycust-copies", used = 10, limit = 9),
            t!(lang, "build-violation-chip-illegal-for-program-deck"),
            t!(lang, "build-violation-program-deck-exceeds-memory", used = 101, limit = 80),
            t!(lang, "build-violation-slot-in-chip-exceeds-memory", used = 41, limit = 40),
            t!(lang, "build-violation-program-deck-missing-navi"),
        ])
        .collect()
    }

    #[test]
    fn slot_warnings_omit_item_names_but_keep_details() {
        let english: unic_langid::LanguageIdentifier = "en-US".parse().unwrap();
        assert_eq!(
            format_violation(&english, Some("Shadow"), "Reason.".to_string()),
            "Shadow: Reason."
        );
        assert_eq!(
            format_violation(&english, None, "Reason.".to_string()),
            "Reason."
        );
        assert_eq!(folder_not_full(&english, 0, 1), "Folder contains 0 of the required 1 chip.");
        assert_eq!(
            violation_reason(
                &english,
                BuildViolationKind::TooManyCopiesOfChip { used: 1, limit: 0 }
            ),
            "1 copy is installed; the limit is 0."
        );
        assert_eq!(
            violation_reason(
                &english,
                BuildViolationKind::TooManyNaviChips { used: 1, limit: 0 }
            ),
            "The folder contains 1 Navi chip; the limit is 0."
        );
        assert_eq!(
            t!(
                &english,
                "build-violation-partycust-gauge-with-program",
                cost = 1,
                used = 2,
                limit = 1
            ),
            "This program uses 1 block; the total is 2; the limit is 1."
        );
        assert_eq!(
            t!(&english, "build-violation-partycust-copies", used = 1, limit = 0),
            "1 copy equipped; the limit is 0."
        );
        let english_texts = slot_warning_texts(&english);
        assert_eq!(
            english_texts,
            [
                "Not legal for this game or version.",
                "6 copies are installed; the limit is 5.",
                "The folder contains 6 Navi chips; the limit is 5.",
                "The folder contains 6 Mega chips; the limit is 5.",
                "The folder contains 2 Giga chips; the limit is 1.",
                "The folder contains 4 Dark chips; the limit is 3.",
                "The Regular chip uses 45MB; the limit is 40MB.",
                "The Tag chips use 65MB; the limit is 60MB.",
                "Placed on grid with invalid shape.",
                "This patch card uses 6MB; the total is 101MB; the limit is 80MB.",
                "Installed in Mod Card slot 0A; belongs in 0C.",
                "Mod Card slot 0A is not in this game's catalog.",
                "This program uses 3 blocks; the total is 8; the limit is 6.",
                "10 copies equipped; the limit is 9.",
                "Not a legal program chip for this deck slot.",
                "The wired deck uses 101MB; its capacity is 80MB.",
                "This SLOT IN chip uses 41MB; the limit is 40MB.",
                "The program deck has no legal Navi chip.",
            ]
        );

        for language in [
            "de-DE", "es-419", "fr-FR", "ja-JP", "nl-NL", "pt-BR", "ru-RU", "vi-VN", "zh-CN", "zh-TW",
        ] {
            for (localized, english) in slot_warning_texts(&language.parse().unwrap())
                .into_iter()
                .zip(&english_texts)
            {
                assert_ne!(localized, *english);
            }
        }
    }

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
        let patch_card = BuildPatchCard {
            id: 1,
            name: None,
            mb: 40,
        };
        let english_patch_card = patch_card_violation(
            &english,
            &[&patch_card],
            PatchCardViolationKind::TotalMbExceeded { used: 90, limit: 80 },
        );
        let navicust_part = BuildNavicustPart {
            id: 1,
            name: None,
            col: 2,
            row: 3,
        };
        let english_navicust = navicust_violation(
            &english,
            &[&navicust_part],
            NavicustViolationKind::MaterializationMismatch,
        );
        let english_navicust_generic = t!(&english, "build-violation-navicust-materialization");

        for language in [
            "de-DE", "es-419", "fr-FR", "ja-JP", "nl-NL", "pt-BR", "ru-RU", "vi-VN", "zh-CN", "zh-TW",
        ] {
            let language = language.parse().unwrap();
            assert_ne!(folder_not_full(&language, 29, 30), english_text);
            for (kind, english_reason) in kinds.iter().copied().zip(&english_reasons) {
                assert_ne!(violation_reason(&language, kind), *english_reason);
            }
            assert_ne!(
                patch_card_violation(
                    &language,
                    &[&patch_card],
                    PatchCardViolationKind::TotalMbExceeded { used: 90, limit: 80 },
                ),
                english_patch_card
            );
            assert_ne!(
                navicust_violation(
                    &language,
                    &[&navicust_part],
                    NavicustViolationKind::MaterializationMismatch,
                ),
                english_navicust
            );
            assert_ne!(
                t!(&language, "build-violation-navicust-materialization"),
                english_navicust_generic
            );
        }
    }

    #[test]
    fn battle_network_report_owns_tab_and_save_metadata() {
        let partial = report(vec![
            BuildViolation::FolderNotFull { used: 29, required: 30 },
            BuildViolation::NavicustMaterializationMismatch,
        ]);

        assert!(partial.blocks_save);
        assert!(partial.error_tabs.contains(&crate::editor::view::Tab::Folder));
        assert!(partial.error_tabs.contains(&crate::editor::view::Tab::Navicust));
        assert_eq!(partial.warnings.len(), 2);

        let illegal = report(vec![BuildViolation::Chip {
            slot: 0,
            chip: BuildChip {
                id: 1,
                code: "A".to_string(),
                name: Some("Cannon".to_string()),
            },
            kind: BuildViolationKind::TooManyCopiesOfChip { used: 6, limit: 5 },
        }]);
        assert!(!illegal.blocks_save);
    }

    #[test]
    fn opaque_chip_warnings_are_grouped_and_ordered_by_id() {
        let violations = vec![
            BuildViolation::Chip {
                slot: 0,
                chip: BuildChip {
                    id: 2,
                    code: "L".to_string(),
                    name: Some("HiCannon".to_string()),
                },
                kind: BuildViolationKind::ChipIllegalForGame,
            },
            BuildViolation::Chip {
                slot: 20,
                chip: BuildChip {
                    id: 1,
                    code: "A".to_string(),
                    name: Some("Cannon".to_string()),
                },
                kind: BuildViolationKind::TooManyCopiesOfChip { used: 6, limit: 5 },
            },
        ];
        let warnings = opaque_violations(violations);
        let english = "en-US".parse().unwrap();

        assert!(warnings[0].format(&english).starts_with("Cannon A:"));
        assert!(warnings[1].format(&english).starts_with("HiCannon L:"));
    }

    #[test]
    fn formatting_uses_the_language_passed_by_the_current_view() {
        let english = "en-US".parse().unwrap();
        let japanese = "ja-JP".parse().unwrap();
        let kind = BuildViolationKind::TooManyNaviChips { used: 6, limit: 5 };

        assert_ne!(violation_reason(&english, kind), violation_reason(&japanese, kind));
    }

    #[test]
    fn navicust_shape_warning_omits_grid_coordinates() {
        let english = "en-US".parse().unwrap();
        let part = BuildNavicustPart {
            id: 1,
            name: Some("Attack+1".to_string()),
            col: 2,
            row: 3,
        };

        assert_eq!(
            navicust_violation(&english, &[&part], NavicustViolationKind::MaterializationMismatch,),
            "Attack+1: Placed on grid with invalid shape."
        );
    }
}

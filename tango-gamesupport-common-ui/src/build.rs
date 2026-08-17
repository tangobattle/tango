//! UI adaptation for headless build violations. Names, grouping, editor
//! placement, save policy, and localization stop here; the host receives only
//! opaque [`tango_gamesupport::BuildWarnings`] providers.

pub use crate::dataview::build::{BuildViolationKind, NavicustViolationKind, PatchCardViolationKind};
use crate::dataview::build::BuildViolation as RawBuildViolation;
use crate::i18n::t;

#[derive(Debug, Default)]
struct Names {
    chips: std::collections::HashMap<usize, String>,
    patch_cards: std::collections::HashMap<usize, String>,
    navicust_parts: std::collections::HashMap<usize, String>,
}

impl Names {
    fn from_violations(violations: &[RawBuildViolation], assets: &dyn crate::dataview::rom::Assets) -> Self {
        let mut names = Self::default();
        for violation in violations {
            match violation {
                RawBuildViolation::Chip { id, .. } => {
                    if let Some(name) = assets.chip(*id).and_then(|info| info.name()) {
                        names.chips.entry(*id).or_insert(name);
                    }
                }
                RawBuildViolation::PatchCard { id, .. } => {
                    if let Some(name) = assets.patch_card56(*id).and_then(|info| info.name()) {
                        names.patch_cards.entry(*id).or_insert(name);
                    }
                }
                RawBuildViolation::NavicustPart { id, .. } => {
                    if let Some(name) = assets.navicust_part(*id).and_then(|info| info.name()) {
                        names.navicust_parts.entry(*id).or_insert(name);
                    }
                }
                RawBuildViolation::FolderNotFull { .. } | RawBuildViolation::NavicustMaterializationMismatch => {}
            }
        }
        names
    }
}

#[derive(Debug)]
struct Warnings {
    violations: Vec<RawBuildViolation>,
    names: Names,
}

fn chip_label(lang: &unic_langid::LanguageIdentifier, id: usize, code: &str, name: Option<&str>) -> String {
    let name = name
        .map(str::to_owned)
        .unwrap_or_else(|| t!(lang, "build-chip-unknown", id = id as i64));
    if code.is_empty() {
        name
    } else {
        format!("{name} {code}")
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

fn navicust_violation_reason(lang: &unic_langid::LanguageIdentifier) -> String {
    t!(lang, "build-violation-navicust-invalid-shape-reason")
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

fn navicust_part_label(lang: &unic_langid::LanguageIdentifier, id: usize, name: Option<&str>) -> String {
    name.map(str::to_owned)
        .unwrap_or_else(|| t!(lang, "build-navicust-part-unknown", id = id as i64))
}

fn patch_card_label(lang: &unic_langid::LanguageIdentifier, id: usize, mb: u8, name: Option<&str>) -> String {
    let name = name
        .map(str::to_owned)
        .unwrap_or_else(|| t!(lang, "build-patch-card-unknown", id = id as i64));
    format!("{name} ({mb}MB)")
}

fn group_by_kind<'a, K: Copy + PartialEq>(
    findings: impl IntoIterator<Item = (&'a RawBuildViolation, K)>,
) -> Vec<(K, Vec<&'a RawBuildViolation>)> {
    let mut groups: Vec<(K, Vec<&RawBuildViolation>)> = vec![];
    for (finding, kind) in findings {
        let group_idx = groups
            .iter()
            .position(|(grouped_kind, _)| grouped_kind == &kind)
            .unwrap_or_else(|| {
                groups.push((kind, vec![]));
                groups.len() - 1
            });
        groups[group_idx].1.push(finding);
    }
    groups
}

impl Warnings {
    fn new(violations: Vec<RawBuildViolation>, assets: &dyn crate::dataview::rom::Assets) -> Self {
        let names = Names::from_violations(&violations, assets);
        Self { violations, names }
    }

    fn chip_warnings(&self, lang: &unic_langid::LanguageIdentifier) -> Vec<String> {
        let mut findings = self
            .violations
            .iter()
            .filter(|violation| matches!(violation, RawBuildViolation::Chip { .. }))
            .collect::<Vec<_>>();
        findings.sort_by_key(|violation| match violation {
            RawBuildViolation::Chip { slot, id, .. } => (*id, *slot),
            _ => unreachable!("filtered to chip violations"),
        });
        group_by_kind(findings.into_iter().map(|violation| {
            let RawBuildViolation::Chip { kind, .. } = violation else {
                unreachable!("filtered to chip violations")
            };
            (violation, *kind)
        }))
        .into_iter()
        .map(|(kind, findings)| {
            let mut seen = std::collections::HashSet::new();
            let subject = findings
                .into_iter()
                .filter_map(|finding| {
                    let RawBuildViolation::Chip { id, code, .. } = finding else {
                        return None;
                    };
                    seen.insert((*id, *code)).then(|| {
                        chip_label(lang, *id, &code.to_string(), self.names.chips.get(id).map(String::as_str))
                    })
                })
                .collect::<Vec<_>>()
                .join(", ");
            format_violation(lang, Some(&subject), violation_reason(lang, kind))
        })
        .collect()
    }

    fn patch_card_warnings(&self, lang: &unic_langid::LanguageIdentifier) -> Vec<String> {
        let mut findings = self
            .violations
            .iter()
            .filter(|violation| matches!(violation, RawBuildViolation::PatchCard { .. }))
            .collect::<Vec<_>>();
        findings.sort_by_key(|violation| match violation {
            RawBuildViolation::PatchCard { slot, id, .. } => (*id, *slot),
            _ => unreachable!("filtered to Patch Card violations"),
        });
        group_by_kind(findings.into_iter().map(|violation| {
            let RawBuildViolation::PatchCard { kind, .. } = violation else {
                unreachable!("filtered to Patch Card violations")
            };
            (violation, *kind)
        }))
        .into_iter()
        .map(|(kind, findings)| {
            let mut seen = std::collections::HashSet::new();
            let subject = findings
                .into_iter()
                .filter_map(|finding| {
                    let RawBuildViolation::PatchCard { id, mb, .. } = finding else {
                        return None;
                    };
                    seen.insert(*id).then(|| {
                        patch_card_label(lang, *id, *mb, self.names.patch_cards.get(id).map(String::as_str))
                    })
                })
                .collect::<Vec<_>>()
                .join(", ");
            format_violation(lang, Some(&subject), patch_card_violation_reason(lang, kind, None))
        })
        .collect()
    }

    fn navicust_warnings(&self, lang: &unic_langid::LanguageIdentifier) -> Vec<String> {
        let mut findings = self
            .violations
            .iter()
            .filter(|violation| matches!(violation, RawBuildViolation::NavicustPart { .. }))
            .collect::<Vec<_>>();
        findings.sort_by_key(|violation| match violation {
            RawBuildViolation::NavicustPart { slot, id, row, col, .. } => (*id, *row, *col, *slot),
            _ => unreachable!("filtered to NaviCust violations"),
        });
        group_by_kind(findings.into_iter().map(|violation| {
            let RawBuildViolation::NavicustPart { kind, .. } = violation else {
                unreachable!("filtered to NaviCust violations")
            };
            (violation, *kind)
        }))
        .into_iter()
        .map(|(kind, findings)| {
            let mut seen = std::collections::HashSet::new();
            let subject = findings
                .into_iter()
                .filter_map(|finding| {
                    let RawBuildViolation::NavicustPart { id, row, col, .. } = finding else {
                        return None;
                    };
                    seen.insert((*id, *row, *col)).then(|| {
                        navicust_part_label(lang, *id, self.names.navicust_parts.get(id).map(String::as_str))
                    })
                })
                .collect::<Vec<_>>()
                .join(", ");
            match kind {
                NavicustViolationKind::MaterializationMismatch => {
                    format_violation(lang, Some(&subject), navicust_violation_reason(lang))
                }
            }
        })
        .collect()
    }
}

impl tango_gamesupport::BuildWarnings for Warnings {
    fn format(&self, lang: &unic_langid::LanguageIdentifier) -> Vec<String> {
        let mut warnings = self
            .violations
            .iter()
            .filter_map(|violation| match violation {
                RawBuildViolation::FolderNotFull { used, required } => Some(folder_not_full(lang, *used, *required)),
                _ => None,
            })
            .collect::<Vec<_>>();
        warnings.extend(self.chip_warnings(lang));
        warnings.extend(self.patch_card_warnings(lang));
        warnings.extend(self.navicust_warnings(lang));
        warnings.extend(self.violations.iter().filter_map(|violation| {
            matches!(violation, RawBuildViolation::NavicustMaterializationMismatch)
                .then(|| t!(lang, "build-violation-navicust-materialization"))
        }));
        warnings
    }
}

fn violation_tab(violation: &RawBuildViolation) -> crate::editor::view::Tab {
    use crate::editor::view::Tab;

    match violation {
        RawBuildViolation::FolderNotFull { .. } | RawBuildViolation::Chip { .. } => Tab::Folder,
        RawBuildViolation::PatchCard { .. } => Tab::PatchCards,
        RawBuildViolation::NavicustPart { .. } | RawBuildViolation::NavicustMaterializationMismatch => Tab::Navicust,
    }
}

/// Every localized shared-rule error belonging to one save-editor tab.
/// Keeping this beside [`Warnings`] means the tab tooltip uses the exact same
/// grouping, names, and translations as the in-match build warning.
pub fn tab_errors(
    lang: &unic_langid::LanguageIdentifier,
    tab: crate::editor::view::Tab,
    save: &crate::editor::Save,
    assets: &crate::editor::Assets,
) -> Vec<String> {
    let violations = crate::dataview::build::violations(save, assets)
        .into_iter()
        .filter(|violation| violation_tab(violation) == tab)
        .collect();
    tango_gamesupport::BuildWarnings::format(&Warnings::new(violations, assets), lang)
}

fn report_metadata(violations: &[RawBuildViolation]) -> (std::collections::HashSet<crate::editor::view::Tab>, bool) {
    let mut error_tabs = std::collections::HashSet::new();
    let mut blocks_save = false;
    for violation in violations {
        error_tabs.insert(violation_tab(violation));
        match violation {
            RawBuildViolation::FolderNotFull { .. } => {
                // A folder must be complete for the save to be structurally
                // usable. Legality violations within a complete folder stay
                // advisory and do not disable Save.
                blocks_save = true;
            }
            RawBuildViolation::Chip { .. }
            | RawBuildViolation::PatchCard { .. }
            | RawBuildViolation::NavicustPart { .. }
            | RawBuildViolation::NavicustMaterializationMismatch => {}
        }
    }
    (error_tabs, blocks_save)
}

/// Adapt headless violations into save-editor-only metadata.
pub fn report(violations: &[RawBuildViolation]) -> crate::editor::BuildReport {
    let (error_tabs, blocks_save) = report_metadata(violations);
    crate::editor::BuildReport {
        error_tabs,
        blocks_save,
    }
}

/// Build the shared BN warning provider independently of editor metadata.
pub fn warnings(
    save: &crate::editor::Save,
    assets: &crate::editor::Assets,
) -> Vec<tango_gamesupport::OpaqueBuildWarnings> {
    let violations = crate::dataview::build::violations(save, assets);
    if violations.is_empty() {
        vec![]
    } else {
        vec![std::sync::Arc::new(Warnings::new(violations, assets)) as tango_gamesupport::OpaqueBuildWarnings]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tango_gamesupport::BuildWarnings as _;

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
        let warnings = Warnings {
            violations: vec![
                RawBuildViolation::PatchCard {
                    slot: 0,
                    id: 1,
                    mb: 40,
                    kind: PatchCardViolationKind::TotalMbExceeded { used: 90, limit: 80 },
                },
                RawBuildViolation::NavicustPart {
                    slot: 0,
                    id: 1,
                    col: 2,
                    row: 3,
                    kind: NavicustViolationKind::MaterializationMismatch,
                },
                RawBuildViolation::NavicustMaterializationMismatch,
            ],
            names: Names::default(),
        };
        let english_warnings = warnings.format(&english);

        for language in [
            "de-DE", "es-419", "fr-FR", "ja-JP", "nl-NL", "pt-BR", "ru-RU", "vi-VN", "zh-CN", "zh-TW",
        ] {
            let language = language.parse().unwrap();
            assert_ne!(folder_not_full(&language, 29, 30), english_text);
            for (kind, english_reason) in kinds.iter().copied().zip(&english_reasons) {
                assert_ne!(violation_reason(&language, kind), *english_reason);
            }
            for (localized, english) in warnings.format(&language).into_iter().zip(&english_warnings) {
                assert_ne!(localized, *english);
            }
        }
    }

    #[test]
    fn editor_metadata_is_adapted_from_headless_violations() {
        let (error_tabs, blocks_save) = report_metadata(&[
            RawBuildViolation::FolderNotFull { used: 29, required: 30 },
            RawBuildViolation::NavicustMaterializationMismatch,
        ]);

        assert!(blocks_save);
        assert!(error_tabs.contains(&crate::editor::view::Tab::Folder));
        assert!(error_tabs.contains(&crate::editor::view::Tab::Navicust));

        let (_, blocks_save) = report_metadata(&[RawBuildViolation::Chip {
            slot: 0,
            id: 1,
            code: crate::dataview::save::ChipCode::A,
            kind: BuildViolationKind::TooManyCopiesOfChip { used: 6, limit: 5 },
        }]);
        assert!(!blocks_save);
    }

    #[test]
    fn chip_warnings_are_grouped_and_ordered_by_id() {
        let warnings = Warnings {
            violations: vec![
                RawBuildViolation::Chip {
                    slot: 0,
                    id: 2,
                    code: crate::dataview::save::ChipCode::L,
                    kind: BuildViolationKind::ChipIllegalForGame,
                },
                RawBuildViolation::Chip {
                    slot: 20,
                    id: 1,
                    code: crate::dataview::save::ChipCode::A,
                    kind: BuildViolationKind::TooManyCopiesOfChip { used: 6, limit: 5 },
                },
            ],
            names: Names {
                chips: [(1, "Cannon".to_string()), (2, "HiCannon".to_string())]
                    .into_iter()
                    .collect(),
                ..Names::default()
            },
        };
        let english = "en-US".parse().unwrap();
        let warnings = warnings.format(&english);

        assert!(warnings[0].starts_with("Cannon A:"));
        assert!(warnings[1].starts_with("HiCannon L:"));
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
        let warnings = Warnings {
            violations: vec![RawBuildViolation::NavicustPart {
                slot: 0,
                id: 1,
                col: 2,
                row: 3,
                kind: NavicustViolationKind::MaterializationMismatch,
            }],
            names: Names {
                navicust_parts: [(1, "Attack+1".to_string())].into_iter().collect(),
                ..Names::default()
            },
        };

        assert_eq!(
            warnings.format(&english),
            ["Attack+1: Placed on grid with invalid shape."]
        );
    }
}

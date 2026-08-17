//! Headless build-legality rules shared by the Battle Network games.
//! The violations contain only domain data; frontends adapt them separately.

use crate::{rom, save};

pub const MAX_COPIES_PER_PART: usize = 9;
pub const MAX_PATCH_CARD56_MB: u32 = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildViolationKind {
    ChipIllegalForGame,
    TooManyCopiesOfChip { used: usize, limit: usize },
    TooManyNaviChips { used: usize, limit: usize },
    TooManyMegaChips { used: usize, limit: usize },
    TooManyGigaChips { used: usize, limit: usize },
    TooManyDarkChips { used: usize, limit: usize },
    RegularChipExceedsMemory { used: u32, limit: u32 },
    TagChipsExceedMemory { used: u32, limit: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchCardViolationKind {
    TotalMbExceeded { used: u32, limit: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavicustViolationKind {
    MaterializationMismatch,
}

/// One headless legality finding. Subjects retain their save/ROM facts, but no
/// display names. Name snapshots, grouping, localization, and editor placement
/// belong to the consuming UI adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildViolation {
    FolderNotFull {
        used: usize,
        required: usize,
    },
    Chip {
        slot: usize,
        id: usize,
        code: save::ChipCode,
        kind: BuildViolationKind,
    },
    PatchCard {
        slot: usize,
        id: usize,
        mb: u8,
        kind: PatchCardViolationKind,
    },
    NavicustPart {
        slot: usize,
        id: usize,
        col: u8,
        row: u8,
        kind: NavicustViolationKind,
    },
    NavicustMaterializationMismatch,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FolderSelections {
    pub regular: Option<usize>,
    pub tags: Option<[usize; 2]>,
}

pub struct FolderUsage {
    pub navi: usize,
    pub mega: usize,
    pub giga: usize,
    pub dark: usize,
    pub copies: std::collections::HashMap<usize, usize>,
}

impl FolderUsage {
    pub fn scan(save: &dyn save::Save, assets: &dyn rom::Assets, folder_idx: usize) -> Self {
        use rom::ChipClass;
        let mut navi = 0;
        let mut mega = 0;
        let mut giga = 0;
        let mut dark = 0;
        let mut copies = std::collections::HashMap::new();
        if let Some(view) = save.view_chips() {
            for slot in 0..view.folder_size() {
                let Some(chip) = view.chip(folder_idx, slot) else {
                    continue;
                };
                *copies.entry(chip.id).or_insert(0) += 1;
                let Some(info) = assets.chip(chip.id) else {
                    continue;
                };
                if info.dark() {
                    dark += 1;
                    continue;
                }
                match info.class() {
                    ChipClass::Navi => navi += 1,
                    ChipClass::Mega => mega += 1,
                    ChipClass::Giga => giga += 1,
                    _ => {}
                }
            }
        }
        Self {
            navi,
            mega,
            giga,
            dark,
            copies,
        }
    }

    pub fn chip_issues(
        &self,
        assets: &dyn rom::Assets,
        chip_id: usize,
        limits: &save::FolderLimits,
        additional: usize,
    ) -> Vec<BuildViolationKind> {
        use rom::ChipClass;
        let mut issues = vec![];
        if !assets.chip_is_legal(chip_id) {
            issues.push(BuildViolationKind::ChipIllegalForGame);
        }
        let Some(info) = assets.chip(chip_id) else {
            return issues;
        };
        let copies = self.copies.get(&chip_id).copied().unwrap_or(0) + additional;
        let copy_limit = (limits.max_copies)(info.as_ref());
        if copies > copy_limit {
            issues.push(BuildViolationKind::TooManyCopiesOfChip {
                used: copies,
                limit: copy_limit,
            });
        }
        if info.dark() {
            let used = self.dark + additional;
            if let Some(limit) = limits.dark_limit.filter(|&limit| used > limit) {
                issues.push(BuildViolationKind::TooManyDarkChips { used, limit });
            }
        } else {
            let issue = match info.class() {
                ChipClass::Navi => {
                    let used = self.navi + additional;
                    limits
                        .navi_limit
                        .filter(|&limit| used > limit)
                        .map(|limit| BuildViolationKind::TooManyNaviChips { used, limit })
                }
                ChipClass::Mega => {
                    let used = self.mega + additional;
                    limits
                        .mega_limit
                        .filter(|&limit| used > limit)
                        .map(|limit| BuildViolationKind::TooManyMegaChips { used, limit })
                }
                ChipClass::Giga => {
                    let used = self.giga + additional;
                    limits
                        .giga_limit
                        .filter(|&limit| used > limit)
                        .map(|limit| BuildViolationKind::TooManyGigaChips { used, limit })
                }
                _ => None,
            };
            if let Some(issue) = issue {
                issues.push(issue);
            }
        }
        issues
    }

    pub fn issues_for_slot(
        &self,
        save: &dyn save::Save,
        assets: &dyn rom::Assets,
        folder_idx: usize,
        slot: usize,
        limits: &save::FolderLimits,
        selections: FolderSelections,
    ) -> Vec<BuildViolationKind> {
        let Some(view) = save.view_chips() else {
            return vec![];
        };
        let Some(chip) = view.chip(folder_idx, slot) else {
            return vec![];
        };
        let mut issues = self.chip_issues(assets, chip.id, limits, 0);
        let Some(info) = assets.chip(chip.id) else {
            return issues;
        };
        let mb_at = |slot: usize| {
            view.chip(folder_idx, slot)
                .and_then(|chip| assets.chip(chip.id))
                .map_or(0u32, |chip| chip.mb() as u32)
        };
        if selections.regular == Some(slot) {
            if let Some(limit) = limits.reg_memory.map(u32::from) {
                let used = info.mb() as u32;
                if used > limit {
                    issues.push(BuildViolationKind::RegularChipExceedsMemory { used, limit });
                }
            }
        }
        if let Some(tags) = selections.tags.filter(|tags| tags.contains(&slot)) {
            if let Some(limit) = limits.tag_memory {
                let used = mb_at(tags[0]) + mb_at(tags[1]);
                if used > limit {
                    issues.push(BuildViolationKind::TagChipsExceedMemory { used, limit });
                }
            }
        }
        issues
    }
}

pub fn folder_violations(save: &dyn save::Save, assets: &dyn rom::Assets) -> Vec<BuildViolation> {
    let Some(view) = save.view_chips() else {
        return vec![];
    };
    let folder_idx = view.equipped_folder_index();
    let folder_size = view.folder_size();
    let limits = save
        .view_navi()
        .map(|navi| navi.folder_limits(assets))
        .unwrap_or_default();
    let usage = FolderUsage::scan(save, assets, folder_idx);
    let selections = FolderSelections {
        regular: view.regular_chip_index(folder_idx).flatten(),
        tags: view.tag_chip_indexes(folder_idx).flatten(),
    };
    let mut violations = vec![];
    let used = (0..folder_size)
        .filter(|&slot| view.chip(folder_idx, slot).is_some())
        .count();
    if used < folder_size {
        violations.push(BuildViolation::FolderNotFull {
            used,
            required: folder_size,
        });
    }
    for slot in 0..folder_size {
        let Some(chip) = view.chip(folder_idx, slot) else {
            continue;
        };
        for kind in usage.issues_for_slot(save, assets, folder_idx, slot, &limits, selections) {
            violations.push(BuildViolation::Chip {
                slot,
                id: chip.id,
                code: chip.code,
                kind,
            });
        }
    }
    violations
}

pub fn patch_card56_total_mb(save: &dyn save::Save, assets: &dyn rom::Assets) -> Option<u32> {
    let view = save.view_patch_card56s()?;
    Some(
        (0..view.count())
            .filter_map(|slot| view.patch_card(slot))
            .filter(|card| card.enabled)
            .map(|card| assets.patch_card56(card.id).map(|info| info.mb() as u32).unwrap_or(0))
            .sum(),
    )
}

fn patch_card56_overflow_slots_from_mb(
    cards: impl IntoIterator<Item = (usize, bool, u32)>,
) -> std::collections::BTreeSet<usize> {
    let mut running = 0u32;
    let mut slots = std::collections::BTreeSet::new();
    for (slot, enabled, mb) in cards {
        if !enabled {
            continue;
        }
        running += mb;
        // The registered list is ordered. Only cards that add MB after the
        // budget is exhausted are the subjects of the violation.
        if mb != 0 && running > MAX_PATCH_CARD56_MB {
            slots.insert(slot);
        }
    }
    slots
}

pub fn patch_card56_overflow_slots(
    save: &dyn save::Save,
    assets: &dyn rom::Assets,
) -> std::collections::BTreeSet<usize> {
    let Some(view) = save.view_patch_card56s() else {
        return Default::default();
    };
    patch_card56_overflow_slots_from_mb((0..view.count()).filter_map(|slot| {
        let card = view.patch_card(slot)?;
        let mb = assets.patch_card56(card.id).map(|info| info.mb() as u32).unwrap_or(0);
        Some((slot, card.enabled, mb))
    }))
}

pub fn patch_card56_violations(save: &dyn save::Save, assets: &dyn rom::Assets) -> Vec<BuildViolation> {
    let Some(view) = save.view_patch_card56s() else {
        return vec![];
    };
    let used = patch_card56_total_mb(save, assets).unwrap_or(0);
    if used <= MAX_PATCH_CARD56_MB {
        return vec![];
    }
    let kind = PatchCardViolationKind::TotalMbExceeded {
        used,
        limit: MAX_PATCH_CARD56_MB,
    };
    patch_card56_overflow_slots(save, assets)
        .into_iter()
        .filter_map(|slot| view.patch_card(slot).map(|card| (slot, card.id)))
        .map(|(slot, id)| BuildViolation::PatchCard {
            slot,
            id,
            mb: assets.patch_card56(id).map_or(0, |info| info.mb()),
            kind,
        })
        .collect()
}

pub fn navicust_materialization_issues(
    save: &dyn save::Save,
    assets: &dyn rom::Assets,
) -> (std::collections::BTreeSet<usize>, bool) {
    let Some(view) = save.view_navicust() else {
        return (Default::default(), false);
    };
    let [cols, rows] = view.size();
    let actual = view.materialized();
    let expected = crate::navicust::materialize(view.as_ref(), [rows, cols], assets);
    navicust_mismatch_slots(view.as_ref(), &actual, &expected)
}

fn navicust_mismatch_slots(
    view: &dyn save::NavicustView,
    actual: &crate::navicust::MaterializedNavicust,
    expected: &crate::navicust::MaterializedNavicust,
) -> (std::collections::BTreeSet<usize>, bool) {
    if actual == expected {
        return (Default::default(), false);
    }
    let mut slots = std::collections::BTreeSet::new();
    if actual.dim() == expected.dim() {
        for (actual, expected) in actual.iter().zip(expected.iter()) {
            if actual == expected {
                continue;
            }
            for slot in [*actual, *expected].into_iter().flatten() {
                if view.navicust_part(slot).is_some() {
                    slots.insert(slot);
                }
            }
        }
    }
    (slots, true)
}

pub fn navicust_violations(save: &dyn save::Save, assets: &dyn rom::Assets) -> Vec<BuildViolation> {
    let (slots, mismatched) = navicust_materialization_issues(save, assets);
    if !mismatched {
        return vec![];
    }
    let Some(view) = save.view_navicust() else {
        return vec![];
    };
    if slots.is_empty() {
        return vec![BuildViolation::NavicustMaterializationMismatch];
    }
    slots
        .into_iter()
        .filter_map(|slot| {
            let part = view.navicust_part(slot)?;
            Some(BuildViolation::NavicustPart {
                slot,
                id: part.id,
                col: part.col,
                row: part.row,
                kind: NavicustViolationKind::MaterializationMismatch,
            })
        })
        .collect()
}

/// Run every shared Battle Network legality check against one exact save and
/// its effective (already patch-overridden) ROM dataview.
pub fn violations(save: &dyn save::Save, assets: &dyn rom::Assets) -> Vec<BuildViolation> {
    let mut violations = folder_violations(save, assets);
    violations.extend(patch_card56_violations(save, assets));
    violations.extend(navicust_violations(save, assets));
    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    struct View {
        parts: Vec<Option<save::NavicustPart>>,
    }

    impl save::NavicustView for View {
        fn count(&self) -> usize {
            self.parts.len()
        }
        fn size(&self) -> [usize; 2] {
            [2, 1]
        }
        fn navicust_part(&self, i: usize) -> Option<save::NavicustPart> {
            self.parts.get(i).cloned().flatten()
        }
        fn materialized(&self) -> crate::navicust::MaterializedNavicust {
            ndarray::Array2::from_elem([1, 2], None)
        }
        fn navicust_color_bar(&self) -> Vec<Option<rom::NavicustPartColor>> {
            vec![]
        }
    }

    fn part(id: usize) -> Option<save::NavicustPart> {
        Some(save::NavicustPart {
            id,
            col: 0,
            row: 0,
            rot: 0,
            compressed: true,
        })
    }

    #[test]
    fn mismatched_cells_implicate_both_real_piece_slots() {
        let view = View {
            parts: vec![part(1), part(2)],
        };
        let actual = ndarray::arr2(&[[Some(1), None]]);
        let expected = ndarray::arr2(&[[Some(0), None]]);
        assert_eq!(
            navicust_mismatch_slots(&view, &actual, &expected),
            ([0, 1].into_iter().collect(), true)
        );
    }

    #[test]
    fn phantom_slot_still_fails_legality() {
        let view = View { parts: vec![part(1)] };
        let actual = ndarray::arr2(&[[Some(9), None]]);
        let expected = ndarray::arr2(&[[None, None]]);
        assert_eq!(
            navicust_mismatch_slots(&view, &actual, &expected),
            (Default::default(), true)
        );
    }

    #[test]
    fn patch_card_mb_overflow_is_attributed_only_to_the_overflowing_suffix() {
        assert_eq!(
            patch_card56_overflow_slots_from_mb([
                (0, true, 40),
                (1, false, 70),
                (2, true, 40),
                (3, true, 0),
                (4, true, 10),
            ]),
            [4].into_iter().collect()
        );
    }
}

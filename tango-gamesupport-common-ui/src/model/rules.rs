//! The games' own limits on what a legal save looks like, and the
//! tallies that answer them.
//!
//! These were scattered through the editors that render them, but their
//! tallies are shared model logic. Some are hard guards in
//! [`crate::model::edit`]; folder class/copy limits are advisory and let a
//! frontend explain an illegal choice without blocking it.

use crate::model::SaveModel;
use tango_gamesupport::BuildViolationKind;

/// Maximum number of copies of one NaviCust part (by id) allowed on the
/// grid.
pub const MAX_COPIES_PER_PART: usize = 9;

/// Total MB budget across enabled PatchCard56s.
pub const MAX_PATCH_CARD56_MB: u32 = 80;

/// Slot selections whose legality depends on memory rather than chip class or
/// copy count. The editor can supply its staged Tag pair; the save checker and
/// read-only viewer use the pair currently stored in the save.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FolderSelections {
    pub regular: Option<usize>,
    pub tags: Option<[usize; 2]>,
}

/// Mega/Giga class usage and per-chip copies in one folder, used to honor
/// the equipped navi's [`crate::dataview::save::FolderLimits`] both in the
/// editor UI (red explanations on existing and would-be violations). Built by
/// scanning the folder's 30 slots; cheap enough to rebuild per edit / per
/// frame.
pub struct FolderUsage {
    pub navi: usize,
    pub mega: usize,
    pub giga: usize,
    pub dark: usize,
    /// Copies installed per chip id (codes collapsed — the copy cap is
    /// per chip, not per code).
    pub copies: std::collections::HashMap<usize, usize>,
}

impl FolderUsage {
    /// Tally the equipped folder's 30 slots.
    pub fn scan(save: &SaveModel, folder_idx: usize) -> Self {
        use crate::dataview::rom::ChipClass;
        let assets = save.assets.as_ref();
        let mut navi = 0;
        let mut mega = 0;
        let mut giga = 0;
        let mut dark = 0;
        let mut copies: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        if let Some(view) = save.save.view_chips() {
            for slot in 0..view.folder_size() {
                let Some(c) = view.chip(folder_idx, slot) else { continue };
                *copies.entry(c.id).or_insert(0) += 1;
                let Some(chip) = assets.chip(c.id) else {
                    continue;
                };
                if chip.dark() {
                    dark += 1;
                    continue;
                }
                match chip.class() {
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

    /// Chip legality after applying `additional` hypothetical copies to this
    /// folder's current usage. Pass zero for an installed slot and one for a
    /// library candidate. Slot-selection rules such as REG/TAG are layered on
    /// by [`Self::issues_for_slot`] because a hypothetical chip has no slot.
    pub fn chip_issues(
        &self,
        save: &SaveModel,
        chip_id: usize,
        limits: &crate::dataview::save::FolderLimits,
        additional: usize,
    ) -> Vec<BuildViolationKind> {
        use crate::dataview::rom::ChipClass;
        let mut issues = vec![];
        if !save.chip_is_legal(chip_id) {
            issues.push(BuildViolationKind::ChipIllegalForGame);
        }
        let Some(info) = save.assets.chip(chip_id) else {
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
            let class_issue = match info.class() {
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
            if let Some(issue) = class_issue {
                issues.push(issue);
            }
        }
        issues
    }

    /// Every issue attached to one existing folder slot. Copy/class issues are
    /// shared by every equivalent chip because no one copy is intrinsically
    /// the illegal one. Regular/Tag memory are checked here too, against the
    /// supplied selections, so save validation and both folder views consume
    /// the same structured result.
    pub fn issues_for_slot(
        &self,
        save: &SaveModel,
        folder_idx: usize,
        slot: usize,
        limits: &crate::dataview::save::FolderLimits,
        selections: FolderSelections,
    ) -> Vec<BuildViolationKind> {
        let Some(view) = save.save.view_chips() else {
            return vec![];
        };
        let Some(chip) = view.chip(folder_idx, slot) else {
            return vec![];
        };
        let chip_id = chip.id;
        let mut issues = self.chip_issues(save, chip_id, limits, 0);
        let Some(info) = save.assets.chip(chip_id) else {
            return issues;
        };

        let mb_at = |slot: usize| {
            view.chip(folder_idx, slot)
                .and_then(|chip| save.assets.chip(chip.id))
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

/// Every legality problem in the equipped Battle Network folder. Chip
/// problems remain attached to their source slots; any grouping belongs to
/// the presentation consuming this report.
pub fn folder_violations(save: &SaveModel) -> Vec<tango_gamesupport::BuildViolation> {
    use tango_gamesupport::{BuildChip, BuildViolation};

    let Some(view) = save.save.view_chips() else {
        return vec![];
    };
    let folder_idx = view.equipped_folder_index();
    let folder_size = view.folder_size();
    let limits = save
        .save
        .view_navi()
        .map(|nv| nv.folder_limits(&*save.assets))
        .unwrap_or_default();
    let usage = FolderUsage::scan(save, folder_idx);
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
        let display = BuildChip {
            id: chip.id,
            code: chip.code.to_string(),
            name: save.assets.chip(chip.id).and_then(|info| info.name()),
        };
        for kind in usage.issues_for_slot(save, folder_idx, slot, &limits, selections) {
            violations.push(BuildViolation::Chip {
                slot,
                chip: display.clone(),
                kind,
            });
        }
    }

    violations
}

/// New index of an element originally at `i` after an ordered move that takes
/// the element at `from` and reinserts it at `to` (i.e. `vec.remove(from);
/// vec.insert(to, x)`). Elements between the two endpoints shift by one toward
/// the vacated side; everything outside the range is unchanged. Used to keep
/// slot-indexed references (REG/TAG, staged tags) aligned with a drag reorder.
pub fn reorder_index(i: usize, from: usize, to: usize) -> usize {
    if i == from {
        to
    } else if from < to && i > from && i <= to {
        i - 1
    } else if from > to && i >= to && i < from {
        i + 1
    } else {
        i
    }
}

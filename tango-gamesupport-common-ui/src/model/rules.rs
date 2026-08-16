//! The games' own limits on what a legal save looks like, and the
//! tallies that answer them.
//!
//! These were scattered through the editors that render them, but their
//! tallies are shared model logic. Some are hard guards in
//! [`crate::model::edit`]; folder class/copy limits are advisory and let a
//! frontend explain an illegal choice without blocking it.

use crate::model::SaveModel;

/// Maximum number of copies of one NaviCust part (by id) allowed on the
/// grid.
pub const MAX_COPIES_PER_PART: usize = 9;

/// Total MB budget across enabled PatchCard56s.
pub const MAX_PATCH_CARD56_MB: u32 = 80;

/// Why a chip either makes the current folder illegal or would cross a limit
/// if added. The view turns these structured answers into localized hover
/// text; the limit itself is advisory in the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderLimitViolation {
    IllegalForGame,
    Copies { used: usize, limit: usize },
    Navi { used: usize, limit: usize },
    Mega { used: usize, limit: usize },
    Giga { used: usize, limit: usize },
    Dark { used: usize, limit: usize },
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

    /// Which limit one more copy of `chip_id` would cross, or `None` when it
    /// stays inside them. The folder-full check is separate. Unknown chips
    /// have no warning.
    pub fn add_violation(
        &self,
        save: &SaveModel,
        chip_id: usize,
        limits: &crate::dataview::save::FolderLimits,
    ) -> Option<FolderLimitViolation> {
        use crate::dataview::rom::ChipClass;
        if !save.chip_is_legal(chip_id) {
            return Some(FolderLimitViolation::IllegalForGame);
        }
        let Some(info) = save.assets.chip(chip_id) else {
            return None;
        };
        let copies = self.copies.get(&chip_id).copied().unwrap_or(0);
        let copy_limit = (limits.max_copies)(info.as_ref());
        if copies >= copy_limit {
            return Some(FolderLimitViolation::Copies {
                used: copies,
                limit: copy_limit,
            });
        }
        if info.dark() {
            return limits
                .dark_limit
                .filter(|&limit| self.dark >= limit)
                .map(|limit| FolderLimitViolation::Dark { used: self.dark, limit });
        }
        match info.class() {
            ChipClass::Navi => limits
                .navi_limit
                .filter(|&limit| self.navi >= limit)
                .map(|limit| FolderLimitViolation::Navi { used: self.navi, limit }),
            ChipClass::Mega => limits
                .mega_limit
                .filter(|&limit| self.mega >= limit)
                .map(|limit| FolderLimitViolation::Mega { used: self.mega, limit }),
            ChipClass::Giga => limits
                .giga_limit
                .filter(|&limit| self.giga >= limit)
                .map(|limit| FolderLimitViolation::Giga { used: self.giga, limit }),
            _ => None,
        }
    }

    /// Every folder limit the existing `chip_id` copies currently violate.
    /// All equivalent copies/class members receive the same answer because no
    /// one copy is intrinsically the illegal one; reordering the folder must
    /// not move the warning to an arbitrary different chip.
    pub fn violations_for_chip(
        &self,
        save: &SaveModel,
        chip_id: usize,
        limits: &crate::dataview::save::FolderLimits,
    ) -> Vec<FolderLimitViolation> {
        use crate::dataview::rom::ChipClass;
        let mut violations = vec![];
        if !save.chip_is_legal(chip_id) {
            violations.push(FolderLimitViolation::IllegalForGame);
        }
        let Some(info) = save.assets.chip(chip_id) else {
            return violations;
        };
        let copies = self.copies.get(&chip_id).copied().unwrap_or(0);
        let copy_limit = (limits.max_copies)(info.as_ref());
        if copies > copy_limit {
            violations.push(FolderLimitViolation::Copies {
                used: copies,
                limit: copy_limit,
            });
        }
        if info.dark() {
            if let Some(limit) = limits.dark_limit.filter(|&limit| self.dark > limit) {
                violations.push(FolderLimitViolation::Dark { used: self.dark, limit });
            }
            return violations;
        }
        let class_violation = match info.class() {
            ChipClass::Navi => limits
                .navi_limit
                .filter(|&limit| self.navi > limit)
                .map(|limit| FolderLimitViolation::Navi { used: self.navi, limit }),
            ChipClass::Mega => limits
                .mega_limit
                .filter(|&limit| self.mega > limit)
                .map(|limit| FolderLimitViolation::Mega { used: self.mega, limit }),
            ChipClass::Giga => limits
                .giga_limit
                .filter(|&limit| self.giga > limit)
                .map(|limit| FolderLimitViolation::Giga { used: self.giga, limit }),
            _ => None,
        };
        if let Some(violation) = class_violation {
            violations.push(violation);
        }
        violations
    }
}

/// Whether the equipped Battle Network folder can be committed. A valid
/// folder is full, contains only chips accepted by the effective game, stays
/// within all copy/class limits, and keeps its Regular/Tag selections within
/// their memory budgets. The editor may stage violations so the user can see
/// and fix them; this is the final Save gate.
pub fn folder_is_valid(save: &SaveModel) -> bool {
    let Some(view) = save.save.view_chips() else {
        return true;
    };
    let folder_idx = view.equipped_folder_index();
    let limits = save
        .save
        .view_navi()
        .map(|nv| nv.folder_limits(&*save.assets))
        .unwrap_or_default();
    let usage = FolderUsage::scan(save, folder_idx);

    // A Battle Network folder must be full, and every installed chip must be
    // free of the same advisory violations painted red in the editor.
    for slot in 0..view.folder_size() {
        let Some(chip) = view.chip(folder_idx, slot) else {
            return false;
        };
        if !usage.violations_for_chip(save, chip.id, &limits).is_empty() {
            return false;
        }
    }

    let mb_of = |slot: usize| {
        view.chip(folder_idx, slot)
            .and_then(|chip| save.assets.chip(chip.id))
            .map_or(0u32, |chip| chip.mb() as u32)
    };
    if let (Some(cap), Some(Some(regular))) = (limits.reg_memory, view.regular_chip_index(folder_idx)) {
        if mb_of(regular) > cap as u32 {
            return false;
        }
    }
    if let (Some(budget), Some(Some([a, b]))) = (limits.tag_memory, view.tag_chip_indexes(folder_idx)) {
        if mb_of(a) + mb_of(b) > budget {
            return false;
        }
    }
    true
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

//! The games' own limits on what a legal save looks like, and the
//! tallies that answer them.
//!
//! These were scattered through the editors that render them, but they
//! are rules rather than presentation — the apply path in [`crate::edit`]
//! enforces exactly the same ones, and a second frontend needs them to
//! grey out an un-addable chip just as this one does.

use crate::SaveModel;

/// Number of chip slots in an equipped folder for the Battle Network
/// games. Games whose folder is a different size report their own
/// through [`tango_dataview::save::ChipsView::folder_size`]; prefer that
/// wherever a view is in hand, and keep this for the places that size a
/// buffer before one is.
pub const MAX_FOLDER_CHIPS: usize = 30;

/// Maximum number of copies of one NaviCust part (by id) allowed on the
/// grid.
pub const MAX_COPIES_PER_PART: usize = 9;

/// Total MB budget across enabled PatchCard56s.
pub const MAX_PATCH_CARD56_MB: u32 = 80;

/// Mega/Giga class usage and per-chip copies in one folder, used to honor
/// the equipped navi's [`tango_dataview::save::FolderLimits`] both in an
/// editor UI (greying out un-addable library chips) and in the apply path
/// ([`crate::edit::apply_chip_edit`]). Built by scanning the folder's 30
/// slots; cheap enough to rebuild per edit / per frame.
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
        use tango_dataview::rom::ChipClass;
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

    /// Whether one more copy of `chip_id` fits under `limits` — the
    /// per-chip copy cap plus the mega/giga class cap. The folder-full
    /// (30-slot) check is separate. Unknown chips aren't blocked.
    pub fn can_add(&self, save: &SaveModel, chip_id: usize, limits: &tango_dataview::save::FolderLimits) -> bool {
        use tango_dataview::rom::ChipClass;
        let Some(info) = save.assets.chip(chip_id) else {
            return true;
        };
        if self.copies.get(&chip_id).copied().unwrap_or(0) >= (limits.max_copies)(info.as_ref()) {
            return false;
        }
        if info.dark() {
            return limits.dark_limit.map(|limit| self.dark < limit).unwrap_or(true);
        }
        match info.class() {
            ChipClass::Navi => limits.navi_limit.map(|limit| self.navi < limit).unwrap_or(true),
            ChipClass::Mega => limits.mega_limit.map(|limit| self.mega < limit).unwrap_or(true),
            ChipClass::Giga => limits.giga_limit.map(|limit| self.giga < limit).unwrap_or(true),
            _ => true,
        }
    }
}

/// Whether the equipped folder satisfies the navi's
/// [`tango_dataview::save::FolderLimits`] — the mega/giga class caps, the
/// per-chip copy cap, and Regular/Tag memory. `true` when the game defines
/// no limits. Gates Save: a folder pane blocks *adding* a violation, but
/// cross-tab edits can still leave an already-built folder illegal (e.g.
/// pulling a MegFldr part on the Navi tab lowers the mega cap under the
/// chips already in the folder), and a save edited elsewhere may arrive
/// over a limit.
pub fn folder_limits_satisfied(save: &SaveModel) -> bool {
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
    if limits.navi_limit.map(|limit| usage.navi > limit).unwrap_or(false)
        || limits.mega_limit.map(|limit| usage.mega > limit).unwrap_or(false)
        || limits.giga_limit.map(|limit| usage.giga > limit).unwrap_or(false)
        || limits.dark_limit.map(|limit| usage.dark > limit).unwrap_or(false)
    {
        return false;
    }
    // Per-chip copy cap.
    for (&id, &count) in &usage.copies {
        if let Some(chip) = save.assets.chip(id) {
            if count > (limits.max_copies)(chip.as_ref()) {
                return false;
            }
        }
    }
    let mb_of = |slot: usize| {
        view.chip(folder_idx, slot)
            .and_then(|c| save.assets.chip(c.id))
            .map_or(0u32, |c| c.mb() as u32)
    };
    // The Regular chip must fit Regular memory.
    if let Some(cap) = limits.reg_memory {
        if let Some(Some(reg)) = view.regular_chip_index(folder_idx) {
            if mb_of(reg) > cap as u32 {
                return false;
            }
        }
    }
    // The Tag pair's combined MB must fit Tag memory.
    if let Some(budget) = limits.tag_memory {
        if let Some(Some([a, b])) = view.tag_chip_indexes(folder_idx) {
            if mb_of(a) + mb_of(b) > budget {
                return false;
            }
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

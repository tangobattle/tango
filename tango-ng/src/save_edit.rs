//! The save editors' staged-edit types and their in-memory appliers,
//! ported from `tango/src/save_edit.rs` (the folder editor first; the
//! other sections' appliers come over with their editors). Resolve
//! against the ROM assets, write through the dataview's mutable views,
//! and rebuild derived mirrors (anti-cheat folder/library) so they
//! stay in sync. No disk I/O — [`commit`] checksums and writes.

use crate::loaded::{Loaded, MAX_FOLDER_CHIPS};

/// A single folder edit staged by the folder editor. Applied to the
/// loaded save in memory; not persisted until the user hits Save.
#[derive(Debug, Clone)]
pub enum ChipEdit {
    /// Add chip `chip_id` with `code` to the first empty folder slot.
    AddChip {
        chip_id: usize,
        code: tango_dataview::save::ChipCode,
    },
    /// Empty `slot` (the rest shift up; REG/TAG pointers follow).
    RemoveChip { slot: usize },
    /// Ordered move from `from` to `to`; both slots must be filled.
    MoveChip { from: usize, to: usize },
    /// Empty every folder slot (and clear REG/TAG).
    ClearFolder,
    /// Toggle `slot` as the folder's Regular chip.
    ToggleRegular { slot: usize },
    /// Set (or clear, with `None`) the folder's Tag chip pair.
    SetTags(Option<[usize; 2]>),
}

/// New index of an element originally at `i` after an ordered move
/// (`remove(from); insert(to, x)`) — keeps slot-indexed references
/// (REG/TAG) aligned with a reorder. From tango's save_view.
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

/// The equipped folder's class/copy tallies, for limit enforcement
/// (tango's save_view/folder.rs FolderUsage).
pub struct FolderUsage {
    pub navi: usize,
    pub mega: usize,
    pub giga: usize,
    pub dark: usize,
    /// Copies installed per chip id (codes collapsed).
    pub copies: std::collections::HashMap<usize, usize>,
}

impl FolderUsage {
    /// Tally the equipped folder's 30 slots.
    pub fn scan(loaded: &Loaded, folder_idx: usize) -> Self {
        use tango_dataview::rom::ChipClass;
        let assets = loaded.assets.as_ref();
        let mut navi = 0;
        let mut mega = 0;
        let mut giga = 0;
        let mut dark = 0;
        let mut copies: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        if let Some(view) = loaded.save.view_chips() {
            for slot in 0..MAX_FOLDER_CHIPS {
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
    /// per-chip copy cap plus the class caps. The folder-full (30-slot)
    /// check is separate. Unknown chips aren't blocked.
    pub fn can_add(&self, loaded: &Loaded, chip_id: usize, limits: &tango_dataview::save::FolderLimits) -> bool {
        use tango_dataview::rom::ChipClass;
        let Some(info) = loaded.assets.chip(chip_id) else {
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

/// Apply one staged [`ChipEdit`] to the loaded save's equipped folder,
/// in memory (tango's apply_chip_edit, verbatim in behavior).
pub fn apply_chip_edit(loaded: &mut Loaded, edit: ChipEdit) {
    use tango_dataview::save::Chip;

    let folder_idx = match loaded.save.view_chips() {
        Some(v) => v.equipped_folder_index(),
        None => return,
    };

    // Concrete write op, resolved while only immutable borrows are held.
    enum Op {
        Chip { slot: usize, chip: Chip },
        Clear { slot: usize },
        Regular { value: Option<usize> },
        Tags(Option<[usize; 2]>),
    }
    let ops: Vec<Op> = match edit {
        ChipEdit::AddChip { chip_id, code } => {
            let limits = loaded
                .save
                .view_navi()
                .map(|nv| nv.folder_limits(&*loaded.assets))
                .unwrap_or_default();
            if !FolderUsage::scan(loaded, folder_idx).can_add(loaded, chip_id, &limits) {
                return;
            }
            let (chips, regular, tags) = folder_snapshot(loaded, folder_idx);
            // First empty slot; no-op if the folder is full. New chips go
            // in at the top, sliding the chips above the gap down into it.
            let Some(gap) = (0..MAX_FOLDER_CHIPS).find(|&i| chips[i].is_none()) else {
                return;
            };
            let mut new_chips = chips;
            new_chips.insert(0, Some(Chip { id: chip_id, code }));
            new_chips.remove(gap + 1);

            let remap = |i: usize| if i < gap { i + 1 } else { i };
            let new_regular = regular.map(remap);
            let new_tags = tags.map(|[a, b]| [remap(a), remap(b)]);

            let mut ops: Vec<Op> = new_chips
                .into_iter()
                .enumerate()
                .map(|(i, c)| match c {
                    Some(chip) => Op::Chip { slot: i, chip },
                    None => Op::Clear { slot: i },
                })
                .collect();
            ops.push(Op::Regular { value: new_regular });
            ops.push(Op::Tags(new_tags));
            ops
        }
        ChipEdit::RemoveChip { slot } => {
            let (chips, regular, tags) = folder_snapshot(loaded, folder_idx);
            let mut new_chips = chips;
            new_chips.remove(slot);
            new_chips.push(None);

            let new_regular = match regular {
                Some(r) if r == slot => None,
                Some(r) if r > slot => Some(r - 1),
                other => other,
            };
            let new_tags = match tags {
                Some([a, b]) if a == slot || b == slot => None,
                Some([a, b]) => Some([if a > slot { a - 1 } else { a }, if b > slot { b - 1 } else { b }]),
                None => None,
            };

            let mut ops: Vec<Op> = new_chips
                .into_iter()
                .enumerate()
                .map(|(i, c)| match c {
                    Some(chip) => Op::Chip { slot: i, chip },
                    None => Op::Clear { slot: i },
                })
                .collect();
            ops.push(Op::Regular { value: new_regular });
            ops.push(Op::Tags(new_tags));
            ops
        }
        ChipEdit::MoveChip { from, to } => {
            if from == to || from >= MAX_FOLDER_CHIPS || to >= MAX_FOLDER_CHIPS {
                return;
            }
            let (chips, regular, tags) = folder_snapshot(loaded, folder_idx);
            if chips[from].is_none() || chips[to].is_none() {
                return;
            }
            let mut new_chips = chips;
            let moved = new_chips.remove(from);
            new_chips.insert(to, moved);

            let remap = |i: usize| reorder_index(i, from, to);
            let new_regular = regular.map(remap);
            let new_tags = tags.map(|[a, b]| [remap(a), remap(b)]);

            let mut ops: Vec<Op> = new_chips
                .into_iter()
                .enumerate()
                .map(|(i, c)| match c {
                    Some(chip) => Op::Chip { slot: i, chip },
                    None => Op::Clear { slot: i },
                })
                .collect();
            ops.push(Op::Regular { value: new_regular });
            ops.push(Op::Tags(new_tags));
            ops
        }
        ChipEdit::ClearFolder => {
            let mut ops: Vec<Op> = (0..MAX_FOLDER_CHIPS).map(|slot| Op::Clear { slot }).collect();
            ops.push(Op::Regular { value: None });
            ops.push(Op::Tags(None));
            ops
        }
        ChipEdit::ToggleRegular { slot } => {
            let current = loaded
                .save
                .view_chips()
                .and_then(|v| v.regular_chip_index(folder_idx))
                .flatten();
            // Setting a new Regular requires its MB to fit Regular
            // memory. Clearing is free.
            if current != Some(slot) {
                let limits = loaded
                    .save
                    .view_navi()
                    .map(|nv| nv.folder_limits(&*loaded.assets))
                    .unwrap_or_default();
                if let Some(cap) = limits.reg_memory {
                    let fits = loaded
                        .save
                        .view_chips()
                        .and_then(|v| v.chip(folder_idx, slot))
                        .and_then(|c| loaded.assets.chip(c.id))
                        .is_none_or(|c| c.mb() <= cap);
                    if !fits {
                        return;
                    }
                }
            }
            vec![Op::Regular {
                value: if current == Some(slot) { None } else { Some(slot) },
            }]
        }
        ChipEdit::SetTags(pair) => {
            // Reject a pair whose combined MB busts Tag memory. `None`
            // clears the pair and is always allowed.
            if let Some([a, b]) = pair {
                let limits = loaded
                    .save
                    .view_navi()
                    .map(|nv| nv.folder_limits(&*loaded.assets))
                    .unwrap_or_default();
                if let Some(budget) = limits.tag_memory {
                    let lr: &Loaded = loaded;
                    let mb_of = |slot: usize| {
                        lr.save
                            .view_chips()
                            .and_then(|v| v.chip(folder_idx, slot))
                            .and_then(|c| lr.assets.chip(c.id))
                            .map_or(0u32, |c| c.mb() as u32)
                    };
                    if mb_of(a) + mb_of(b) > budget {
                        return;
                    }
                }
            }
            vec![Op::Tags(pair)]
        }
    };

    if let Some(mut chips) = loaded.save.view_chips_mut() {
        for op in ops {
            match op {
                Op::Chip { slot, chip } => {
                    chips.set_chip(folder_idx, slot, chip);
                }
                Op::Clear { slot } => {
                    chips.clear_chip(folder_idx, slot);
                }
                Op::Regular { value } => {
                    chips.set_regular_chip_index(folder_idx, value);
                }
                Op::Tags(pair) => {
                    chips.set_tag_chip_indexes(folder_idx, pair);
                }
            }
        }
        // Keep the anti-cheat folder/library mirror in sync, so commit
        // only has to checksum + write.
        chips.rebuild_anticheat();
    }
}

/// The equipped folder's 30 slots + REG/TAG, snapshotted with only
/// immutable borrows held.
fn folder_snapshot(
    loaded: &Loaded,
    folder_idx: usize,
) -> (
    Vec<Option<tango_dataview::save::Chip>>,
    Option<usize>,
    Option<[usize; 2]>,
) {
    let v = loaded.save.view_chips();
    let chips: Vec<Option<tango_dataview::save::Chip>> = (0..MAX_FOLDER_CHIPS)
        .map(|i| v.as_ref().and_then(|v| v.chip(folder_idx, i)))
        .collect();
    let regular = v.as_ref().and_then(|v| v.regular_chip_index(folder_idx)).flatten();
    let tags = v.as_ref().and_then(|v| v.tag_chip_indexes(folder_idx)).flatten();
    (chips, regular, tags)
}

/// Write every staged edit to the `.sav` on disk in one shot: rebuild
/// the checksum, dump the SRAM, write the file (tango's commit path).
pub fn commit(loaded: &mut Loaded, path: &std::path::Path) -> anyhow::Result<()> {
    loaded.save.rebuild_checksum();
    let sram = loaded.save.to_sram_dump();
    std::fs::write(path, sram)?;
    Ok(())
}

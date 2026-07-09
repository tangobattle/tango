//! The save editors' staged-edit types and their in-memory appliers,
//! ported from `tango/src/save_edit.rs` (the folder editor first; the
//! other sections' appliers come over with their editors). Resolve
//! against the ROM assets, write through the dataview's mutable views,
//! and rebuild derived mirrors (anti-cheat folder/library) so they
//! stay in sync. No disk I/O — [`commit`] checksums and writes.

use crate::loaded::{Editability, Loaded, MAX_FOLDER_CHIPS};

/// Per-part navicust copy cap (tango's navicust editor).
pub const MAX_COPIES_PER_PART: usize = 9;
/// Total MB an enabled BN5/BN6 patch-card set may use.
pub const MAX_PATCH_CARD56_MB: u32 = 80;
/// Number of BN4 patch-card catalog slots (0A-0F).
pub const NUM_PATCH_CARD4_SLOTS: usize = 6;

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

/// A single navicust edit staged by the navicust editor.
#[derive(Debug, Clone)]
pub enum NavicustEdit {
    /// Place a part into the first empty navicust slot.
    AddPart(tango_dataview::save::NavicustPart),
    /// Empty navicust slot `slot` (the rest shift up - placement order
    /// drives the color bar).
    RemovePart { slot: usize },
    /// Remove every installed part.
    ClearAll,
}

/// Apply one staged [`NavicustEdit`] in memory, then rebuild the
/// materialized WRAM grid so the viewer re-bakes from live state
/// (tango's apply_navicust_edit).
pub fn apply_navicust_edit(loaded: &mut Loaded, edit: NavicustEdit) {
    use tango_dataview::save::NavicustPart;

    enum Op {
        Set { slot: usize, part: NavicustPart },
        Clear { slot: usize },
    }
    let ops: Vec<Op> = match edit {
        NavicustEdit::AddPart(part) => {
            let slot = match loaded.save.view_navicust() {
                Some(v) => {
                    let copies = (0..v.count())
                        .filter(|&i| v.navicust_part(i).is_some_and(|p| p.id == part.id))
                        .count();
                    if copies >= MAX_COPIES_PER_PART {
                        return;
                    }
                    (0..v.count()).find(|&i| v.navicust_part(i).is_none())
                }
                None => None,
            };
            match slot {
                Some(slot) => vec![Op::Set { slot, part }],
                None => return,
            }
        }
        NavicustEdit::RemovePart { slot } => {
            let parts: Vec<Option<NavicustPart>> = match loaded.save.view_navicust() {
                Some(v) => (0..v.count()).map(|i| v.navicust_part(i)).collect(),
                None => return,
            };
            let mut parts = parts;
            if slot < parts.len() {
                parts.remove(slot);
                parts.push(None);
            }
            parts
                .into_iter()
                .enumerate()
                .map(|(i, p)| match p {
                    Some(part) => Op::Set { slot: i, part },
                    None => Op::Clear { slot: i },
                })
                .collect()
        }
        NavicustEdit::ClearAll => {
            let count = match loaded.save.view_navicust() {
                Some(v) => v.count(),
                None => return,
            };
            (0..count).map(|slot| Op::Clear { slot }).collect()
        }
    };

    if let Some(mut nc) = loaded.save.view_navicust_mut() {
        for op in ops {
            match op {
                Op::Set { slot, part } => {
                    nc.set_navicust_part(slot, Some(part));
                }
                Op::Clear { slot } => {
                    nc.set_navicust_part(slot, None);
                }
            }
        }
    }
    // Rebuild the materialized grid + color bar in the in-memory save;
    // the viewer re-bakes its image from it on the next push.
    let assets = loaded.assets.as_ref();
    if let Some(mut nc) = loaded.save.view_navicust_mut() {
        nc.rebuild_materialized(assets);
    }
}

/// A staged equipped-navi selection.
#[derive(Debug, Clone)]
pub enum NaviEdit {
    SetNavi(usize),
}

/// Apply a staged [`NaviEdit`] in memory. Switching the equipped navi
/// flips whether an editable navicust / patch-card list exists, so the
/// cached editability re-probes here.
pub fn apply_navi_edit(loaded: &mut Loaded, edit: NaviEdit) {
    match edit {
        NaviEdit::SetNavi(navi) => {
            if let Some(mut nv) = loaded.save.view_navi_mut() {
                nv.set_navi(navi);
            }
        }
    }
    loaded.editability = Editability::probe(&mut *loaded.save);
}

/// A single BN5/BN6 patch-card edit.
#[derive(Debug, Clone)]
pub enum PatchCard56Edit {
    /// Register patch card `id` (appended, enabled).
    AddCard { id: usize },
    /// Unregister the card in `slot` (the rest shift up).
    RemoveCard { slot: usize },
    /// Ordered move within the dense registered list.
    MoveCard { from: usize, to: usize },
    /// Unregister every patch card.
    ClearAll,
}

/// Apply one staged [`PatchCard56Edit`] in memory (tango's
/// apply_patch_card56_edit: list cap, MB budget, anti-cheat rebuild).
pub fn apply_patch_card56_edit(loaded: &mut Loaded, edit: PatchCard56Edit) {
    use tango_dataview::save::{PatchCard, PatchCardsView, PatchCardsViewMut};

    let cards: Vec<PatchCard> = match loaded.save.view_patch_cards() {
        Some(PatchCardsView::PatchCard56s(v)) => (0..v.count()).filter_map(|i| v.patch_card(i)).collect(),
        _ => return,
    };
    let max = loaded.assets.num_patch_card56s();
    let card_mb = |id: usize| loaded.assets.patch_card56(id).map(|c| c.mb() as u32).unwrap_or(0);
    let enabled_mb = |list: &[PatchCard]| -> u32 { list.iter().filter(|c| c.enabled).map(|c| card_mb(c.id)).sum() };

    let mut new_cards = cards.clone();
    match edit {
        PatchCard56Edit::AddCard { id } => {
            if new_cards.len() >= max
                || new_cards.iter().any(|c| c.id == id)
                || enabled_mb(&new_cards) + card_mb(id) > MAX_PATCH_CARD56_MB
            {
                return;
            }
            new_cards.push(PatchCard { id, enabled: true });
        }
        PatchCard56Edit::RemoveCard { slot } => {
            if slot >= new_cards.len() {
                return;
            }
            new_cards.remove(slot);
        }
        PatchCard56Edit::MoveCard { from, to } => {
            if from == to || from >= new_cards.len() || to >= new_cards.len() {
                return;
            }
            let card = new_cards.remove(from);
            new_cards.insert(to, card);
        }
        PatchCard56Edit::ClearAll => new_cards.clear(),
    }

    if let Some(PatchCardsViewMut::PatchCard56s(mut v)) = loaded.save.view_patch_cards_mut() {
        // Grow to cover both lengths, rewrite, shrink (set_patch_card
        // only writes below the current count).
        v.set_count(cards.len().max(new_cards.len()));
        for (slot, card) in new_cards.iter().enumerate() {
            v.set_patch_card(slot, card.clone());
        }
        v.set_count(new_cards.len());
        v.rebuild_anticheat();
    }
}

/// A single BN4 patch-card edit (slot-based: every card belongs to one
/// fixed catalog slot).
#[derive(Debug, Clone)]
pub enum PatchCard4Edit {
    /// Install card `id` into its own catalog slot, enabled.
    AddCard { id: usize },
    /// Clear catalog slot `slot`.
    RemoveCard { slot: usize },
    /// Toggle slot `slot` between enabled and disabled.
    ToggleCard { slot: usize },
    /// Clear every slot.
    ClearAll,
}

/// Apply one staged [`PatchCard4Edit`] in memory.
pub fn apply_patch_card4_edit(loaded: &mut Loaded, edit: PatchCard4Edit) {
    use tango_dataview::save::{PatchCard, PatchCardsView, PatchCardsViewMut};

    enum Op {
        Set { slot: usize, card: Option<PatchCard> },
        ClearAll,
    }
    let op = match edit {
        PatchCard4Edit::AddCard { id } => {
            let slot = loaded.assets.patch_card4(id).map(|c| c.slot() as usize);
            match slot {
                Some(slot) if slot < NUM_PATCH_CARD4_SLOTS => Op::Set {
                    slot,
                    card: Some(PatchCard { id, enabled: true }),
                },
                _ => return,
            }
        }
        PatchCard4Edit::RemoveCard { slot } => Op::Set { slot, card: None },
        PatchCard4Edit::ToggleCard { slot } => {
            let current = match loaded.save.view_patch_cards() {
                Some(PatchCardsView::PatchCard4s(v)) => v.patch_card(slot),
                _ => None,
            };
            match current {
                Some(c) => Op::Set {
                    slot,
                    card: Some(PatchCard {
                        id: c.id,
                        enabled: !c.enabled,
                    }),
                },
                None => return,
            }
        }
        PatchCard4Edit::ClearAll => Op::ClearAll,
    };

    if let Some(PatchCardsViewMut::PatchCard4s(mut v)) = loaded.save.view_patch_cards_mut() {
        match op {
            Op::Set { slot, card } => {
                v.set_patch_card(slot, card);
            }
            Op::ClearAll => {
                for slot in 0..NUM_PATCH_CARD4_SLOTS {
                    v.set_patch_card(slot, None);
                }
            }
        }
        v.rebuild_anticheat();
    }
}

/// A single auto-battle-data edit (the deck derives from per-chip use
/// counts).
#[derive(Debug, Clone)]
pub enum AutoBattleDataEdit {
    SetUseCount { id: usize, count: usize },
    SetSecondaryUseCount { id: usize, count: usize },
    ClearAll,
}

/// Apply one staged [`AutoBattleDataEdit`] in memory, then rebuild the
/// materialized deck so the preview reflects it live.
pub fn apply_auto_battle_data_edit(loaded: &mut Loaded, edit: AutoBattleDataEdit) {
    match edit {
        AutoBattleDataEdit::SetUseCount { id, count } => {
            if let Some(mut v) = loaded.save.view_auto_battle_data_mut() {
                v.set_chip_use_count(id, count);
            }
        }
        AutoBattleDataEdit::SetSecondaryUseCount { id, count } => {
            if let Some(mut v) = loaded.save.view_auto_battle_data_mut() {
                v.set_secondary_chip_use_count(id, count);
            }
        }
        AutoBattleDataEdit::ClearAll => {
            // Zero the counts themselves - clearing only the cache would
            // be undone by the next rebuild.
            let num_chips = loaded.assets.num_chips();
            if let Some(mut v) = loaded.save.view_auto_battle_data_mut() {
                for id in 0..num_chips {
                    v.set_chip_use_count(id, 0);
                    v.set_secondary_chip_use_count(id, 0);
                }
            }
        }
    }
    let assets = loaded.assets.as_ref();
    if let Some(mut v) = loaded.save.view_auto_battle_data_mut() {
        v.rebuild_materialized(assets);
    }
}

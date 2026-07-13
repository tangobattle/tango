//! The save editors' staged-edit types ([`ChipEdit`] & friends) and
//! their in-memory appliers: resolve against the ROM assets, write
//! through the dataview's mutable views, and rebuild any derived
//! mirrors (anti-cheat folder/library, materialized auto battle data)
//! so they stay in sync. No disk I/O — the commit path only checksums
//! and writes. The save view surfaces edits as
//! [`crate::save_view::Outcome::Edit`]; the App routes them here.

use crate::selection;

/// A single folder edit staged by the folder editor. Applied to the
/// loaded save in memory; not persisted to disk until the user hits
/// Save (the host's save-edit commit).
#[derive(Debug, Clone)]
pub enum ChipEdit {
    /// Add chip `chip_id` with `code` to the first empty folder slot.
    AddChip {
        chip_id: usize,
        code: tango_dataview::save::ChipCode,
    },
    /// Empty `slot`.
    RemoveChip { slot: usize },
    /// Reorder: move the chip at `from` to `to` (an ordered move that shifts
    /// the chips in between). Both slots must be filled — the editor never
    /// drags an empty slot or drops into a gap. REG/TAG slot pointers follow
    /// the moved chips.
    MoveChip { from: usize, to: usize },
    /// Empty every folder slot (and clear REG/TAG).
    ClearFolder,
    /// Toggle `slot` as the folder's Regular chip (clear if already set).
    ToggleRegular { slot: usize },
    /// Set (or clear, with `None`) the folder's Tag chip pair.
    SetTags(Option<[usize; 2]>),
}

/// A single navicust edit staged by the navicust editor. Applied to the
/// loaded save in memory; not persisted to disk until the user hits Save.
#[derive(Debug, Clone)]
pub enum NavicustEdit {
    /// Place a part into the first empty navicust slot.
    AddPart(tango_dataview::save::NavicustPart),
    /// Empty navicust slot `slot`.
    RemovePart { slot: usize },
    /// Remove every installed part.
    ClearAll,
}

/// A staged navi-selection edit. Applied to the loaded save in memory;
/// not persisted to disk until the user hits Save.
#[derive(Debug, Clone)]
pub enum NaviEdit {
    /// Set the equipped navi to this index.
    SetNavi(usize),
}

/// A single BN5/BN6 patch-card edit staged by the editor. Applied to the
/// loaded save in memory; not persisted to disk until the user hits Save.
#[derive(Debug, Clone)]
pub enum PatchCard56Edit {
    /// Register patch card `id` (append to the list, enabled).
    AddCard { id: usize },
    /// Unregister the patch card in `slot` (shift the rest up).
    RemoveCard { slot: usize },
    /// Reorder: move the card at `from` to `to` (an ordered move that shifts
    /// the cards in between). The registered list is dense, so both ends are
    /// always valid.
    MoveCard { from: usize, to: usize },
    /// Unregister every patch card.
    ClearAll,
}

/// A single BN4 patch-card edit staged by the editor. Applied to the
/// loaded save in memory; not persisted to disk until the user hits Save.
/// BN4 is slot-based: every card belongs to one fixed catalog slot
/// (0A–0F), so adding a card installs it into its own slot (replacing
/// whatever was there).
#[derive(Debug, Clone)]
pub enum PatchCard4Edit {
    /// Install patch card `id` into its own catalog slot, enabled.
    AddCard { id: usize },
    /// Clear catalog slot `slot`.
    RemoveCard { slot: usize },
    /// Toggle slot `slot`'s card between enabled and disabled.
    ToggleCard { slot: usize },
    /// Clear every slot.
    ClearAll,
}

/// A single auto-battle-data edit staged by the editor. Applied to the
/// loaded save in memory; not persisted to disk until the user hits
/// Save. The deck is derived from per-chip use counts, so these set
/// those counts; the applier rebuilds the materialized deck after each
/// so the preview shows the change live.
#[derive(Debug, Clone)]
pub enum AutoBattleDataEdit {
    /// Set chip `id`'s primary use count.
    SetUseCount { id: usize, count: usize },
    /// Set chip `id`'s secondary use count (Standard chips only).
    SetSecondaryUseCount { id: usize, count: usize },
    /// Zero every chip's use counts, emptying the deck.
    ClearAll,
}

/// One staged edit to the loaded save, unifying the per-editor edit
/// types so hosts can route every editor through a single effect.
#[derive(Debug, Clone)]
pub enum Edit {
    Chips(ChipEdit),
    Navicust(NavicustEdit),
    Navi(NaviEdit),
    PatchCard56s(PatchCard56Edit),
    PatchCard4s(PatchCard4Edit),
    AutoBattleData(AutoBattleDataEdit),
}

/// Apply one staged edit to the in-memory loaded save. The UI reads
/// `loaded.save` directly, so the change shows immediately; nothing is
/// written to disk until the host's save-edit commit.
pub fn apply_edit(loaded: &mut selection::Loaded, edit: Edit) {
    match edit {
        Edit::Chips(e) => apply_chip_edit(loaded, e),
        Edit::Navicust(e) => apply_navicust_edit(loaded, e),
        Edit::Navi(e) => apply_navi_edit(loaded, e),
        Edit::PatchCard56s(e) => apply_patch_card56_edit(loaded, e),
        Edit::PatchCard4s(e) => apply_patch_card4_edit(loaded, e),
        Edit::AutoBattleData(e) => apply_auto_battle_data_edit(loaded, e),
    }
}

/// Apply one staged [`ChipEdit`] to a loaded save's
/// equipped folder, in memory. Resolves chip-id/code against the ROM
/// assets first (so the immutable borrows drop before the mutable chip
/// view is taken), then writes via [`ChipsViewMut`] and rebuilds the
/// anti-cheat folder/library mirror so it stays in sync with the edit.
/// No disk I/O — the commit path only checksums and writes.
pub fn apply_chip_edit(loaded: &mut selection::Loaded, edit: ChipEdit) {
    use crate::save_view::folder::MAX_FOLDER_CHIPS;
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
            // Enforce the equipped navi's folder limits (mega/giga class
            // caps + the per-chip copy cap).
            let limits = loaded
                .save
                .view_navi()
                .map(|nv| nv.folder_limits(&*loaded.assets))
                .unwrap_or_default();
            if !crate::save_view::folder::FolderUsage::scan(loaded, folder_idx).can_add(loaded, chip_id, &limits) {
                return;
            }
            let (chips, regular, tags) = {
                let v = loaded.save.view_chips();
                let chips: Vec<Option<Chip>> = (0..MAX_FOLDER_CHIPS)
                    .map(|i| v.as_ref().and_then(|v| v.chip(folder_idx, i)))
                    .collect();
                let regular = v.as_ref().and_then(|v| v.regular_chip_index(folder_idx)).flatten();
                let tags = v.as_ref().and_then(|v| v.tag_chip_indexes(folder_idx)).flatten();
                (chips, regular, tags)
            };
            // First empty slot; no-op if the folder is full. New chips go in at
            // the top, sliding the chips above the gap down into it. REG/TAG
            // slot pointers shift down with them.
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
            // Remove the chip and shift everything below it up one so the
            // folder has no gap (the freed slot ends up empty at the end).
            // REG/TAG indexes are remapped to follow the shift, and
            // cleared if they pointed at the removed chip.
            let (chips, regular, tags) = {
                let v = loaded.save.view_chips();
                let chips: Vec<Option<Chip>> = (0..MAX_FOLDER_CHIPS)
                    .map(|i| v.as_ref().and_then(|v| v.chip(folder_idx, i)))
                    .collect();
                let regular = v.as_ref().and_then(|v| v.regular_chip_index(folder_idx)).flatten();
                let tags = v.as_ref().and_then(|v| v.tag_chip_indexes(folder_idx)).flatten();
                (chips, regular, tags)
            };
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
            // Ordered move (remove at `from`, insert at `to`). Both ends must
            // be filled — the editor never drags an empty slot or drops into a
            // gap — and REG/TAG slot pointers follow the permutation.
            if from == to || from >= MAX_FOLDER_CHIPS || to >= MAX_FOLDER_CHIPS {
                return;
            }
            let (chips, regular, tags) = {
                let v = loaded.save.view_chips();
                let chips: Vec<Option<Chip>> = (0..MAX_FOLDER_CHIPS)
                    .map(|i| v.as_ref().and_then(|v| v.chip(folder_idx, i)))
                    .collect();
                let regular = v.as_ref().and_then(|v| v.regular_chip_index(folder_idx)).flatten();
                let tags = v.as_ref().and_then(|v| v.tag_chip_indexes(folder_idx)).flatten();
                (chips, regular, tags)
            };
            if chips[from].is_none() || chips[to].is_none() {
                return;
            }
            let mut new_chips = chips;
            let moved = new_chips.remove(from);
            new_chips.insert(to, moved);

            let remap = |i: usize| crate::save_view::reorder_index(i, from, to);
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
            // Clicking the regular chip again clears it; otherwise set it.
            let current = loaded
                .save
                .view_chips()
                .and_then(|v| v.regular_chip_index(folder_idx))
                .flatten();
            // Setting a new Regular requires its MB to fit Regular memory
            // (the editor greys the toggle out otherwise). Clearing is free.
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
            // Reject a pair whose combined MB busts Tag memory (the editor
            // greys out the toggle that would form it, so this is a
            // backstop). `None` clears the pair and is always allowed.
            if let Some([a, b]) = pair {
                let limits = loaded
                    .save
                    .view_navi()
                    .map(|nv| nv.folder_limits(&*loaded.assets))
                    .unwrap_or_default();
                if let Some(budget) = limits.tag_memory {
                    let lr: &selection::Loaded = loaded;
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
        // Keep the anti-cheat folder/library mirror in sync with the edit, so
        // commit only has to checksum + write (see SaveEditCommit).
        chips.rebuild_anticheat();
    }
}

/// Apply one staged [`NavicustEdit`] to a loaded save's
/// navicust, in memory. Writes the part slots, then rebuilds the
/// materialized WRAM grid cache so it stays in sync with the edit (and
/// the editor's live color-bar preview reflects it). No disk I/O — the
/// commit path only checksums and writes. A no-op on saves whose navi
/// view isn't the (writable) Navicust variant.
pub fn apply_navicust_edit(loaded: &mut selection::Loaded, edit: NavicustEdit) {
    use tango_dataview::save::NavicustPart;

    // Resolve any reads (empty-slot search, slot count) under an
    // immutable borrow first, so it's dropped before the mutable view.
    enum Op {
        Set { slot: usize, part: NavicustPart },
        Clear { slot: usize },
    }
    let ops: Vec<Op> = match edit {
        NavicustEdit::AddPart(part) => {
            // First empty slot; no-op if every slot is full or the part is
            // already at its per-part copy cap.
            let slot = match loaded.save.view_navicust() {
                Some(v) => {
                    let copies = (0..v.count())
                        .filter(|&i| v.navicust_part(i).is_some_and(|p| p.id == part.id))
                        .count();
                    if copies >= crate::save_view::navicust::editor::MAX_COPIES_PER_PART {
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
            // Drop the part and shift everything after it up one slot, so
            // the placement order (which drives the color bar) has no gap.
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

    // Rebuild the materialized grid + color bar in the in-memory save so
    // the editor (which renders the color bar straight from the save) shows
    // the change live. Disjoint field borrows: assets vs save.
    let assets = loaded.assets.as_ref();
    if let Some(mut nc) = loaded.save.view_navicust_mut() {
        nc.rebuild_materialized(assets);
    }
}

/// Apply a staged [`NaviEdit`] (the equipped-navi selection) to
/// the loaded save in memory. No disk I/O — the commit path checksums and
/// writes. A no-op on saves without a writable navi view.
pub fn apply_navi_edit(loaded: &mut selection::Loaded, edit: NaviEdit) {
    match edit {
        NaviEdit::SetNavi(navi) => {
            if let Some(mut nv) = loaded.save.view_navi_mut() {
                nv.set_navi(navi);
            }
        }
    }
    // Switching the equipped navi flips whether an editable navicust and patch
    // card list exist: a link navi has neither, the player's own navi does. The
    // editability flags (and the baked read-only navicust grid image) are cached
    // on `Loaded` (the per-frame view only holds `&Loaded`), so refresh them
    // here — otherwise they stay stale until the save is reselected, leaving the
    // NaviCust / patch card editors disabled (or showing the wrong grid) after a
    // navi swap.
    loaded.refresh_editability();
    loaded.rebuild_navicust_render();
}

/// Apply one staged [`PatchCard56Edit`] to a loaded save's
/// registered patch-card list, in memory. Reads the current list under an
/// immutable borrow, computes the new list, rewrites the slots via
/// [`PatchCard56sViewMut`], then rebuilds the anti-cheat mirror so it
/// stays in sync with the edit. No disk I/O — the commit path only
/// checksums and writes. A no-op on saves whose patch-card view isn't the
/// (writable) PatchCard56s variant.
pub fn apply_patch_card56_edit(loaded: &mut selection::Loaded, edit: PatchCard56Edit) {
    use tango_dataview::save::{PatchCard, PatchCardsView, PatchCardsViewMut};

    let cards: Vec<PatchCard> = match loaded.save.view_patch_cards() {
        Some(PatchCardsView::PatchCard56s(v)) => (0..v.count()).filter_map(|i| v.patch_card(i)).collect(),
        _ => return,
    };
    // You can register at most one of each card the ROM defines, so the
    // game's own card count is the list cap.
    let max = loaded.assets.num_patch_card56s();
    // Total MB of the currently-enabled cards, used to keep the enabled set
    // within the in-game budget (see `MAX_PATCH_CARD56_MB`).
    let card_mb = |id: usize| loaded.assets.patch_card56(id).map(|c| c.mb() as u32).unwrap_or(0);
    let enabled_mb = |list: &[PatchCard]| -> u32 { list.iter().filter(|c| c.enabled).map(|c| card_mb(c.id)).sum() };

    let mut new_cards = cards.clone();
    match edit {
        PatchCard56Edit::AddCard { id } => {
            // No-op if the list is full, the card is already registered, or it
            // wouldn't fit the MB budget. Every registered card is active (no
            // on/off state), so the library disables non-fitting cards and this
            // is just a guard. Appended, enabled, at the end of the list.
            if new_cards.len() >= max
                || new_cards.iter().any(|c| c.id == id)
                || enabled_mb(&new_cards) + card_mb(id) > crate::save_view::patch_cards::MAX_PATCH_CARD56_MB
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
            // Ordered move within the dense registered list.
            if from == to || from >= new_cards.len() || to >= new_cards.len() {
                return;
            }
            let card = new_cards.remove(from);
            new_cards.insert(to, card);
        }
        PatchCard56Edit::ClearAll => new_cards.clear(),
    }

    if let Some(PatchCardsViewMut::PatchCard56s(mut v)) = loaded.save.view_patch_cards_mut() {
        // `set_patch_card` only writes slots below the current count, so
        // grow to cover both lengths first, rewrite every kept entry, then
        // shrink to the final length. Trailing bytes past the new count are
        // ignored by the reader (which bounds reads on the count).
        v.set_count(cards.len().max(new_cards.len()));
        for (slot, card) in new_cards.iter().enumerate() {
            v.set_patch_card(slot, card.clone());
        }
        v.set_count(new_cards.len());
        // Keep the anti-cheat mirror in sync with the edit, so commit only
        // has to checksum + write (see SaveEditCommit).
        v.rebuild_anticheat();
    }
}

/// Number of BN4 patch-card catalog slots (0A–0F).
const NUM_PATCH_CARD4_SLOTS: usize = 6;

/// Apply one staged [`PatchCard4Edit`] to a loaded save's BN4
/// patch cards, in memory. BN4 is slot-based: every card belongs to one
/// fixed catalog slot, so adding routes the card to its own `slot()`
/// (replacing whatever was there). No MB budget, no list shifting. After
/// writing it rebuilds the anti-cheat mirror so it stays in sync with the
/// edit. No disk I/O — the commit path only checksums and writes. A no-op
/// on saves whose patch-card view isn't the PatchCard4s variant.
pub fn apply_patch_card4_edit(loaded: &mut selection::Loaded, edit: PatchCard4Edit) {
    use tango_dataview::save::{PatchCard, PatchCardsView, PatchCardsViewMut};

    // Resolve the slot/card to write under immutable borrows first, so they
    // drop before the mutable patch-card view is taken.
    enum Op {
        Set { slot: usize, card: Option<PatchCard> },
        ClearAll,
    }
    let op = match edit {
        PatchCard4Edit::AddCard { id } => {
            // Route the card to its own catalog slot.
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
        // Keep the anti-cheat mirror in sync with the edit, so commit only
        // has to checksum + write (see SaveEditCommit).
        v.rebuild_anticheat();
    }
}

/// Apply one staged [`AutoBattleDataEdit`] to a loaded save's
/// auto-battle data, in memory. The deck is derived from per-chip use
/// counts, so each edit sets a count (or zeroes them all) and then
/// rebuilds the materialized WRAM deck so the editor's live preview — which
/// reads the materialized cache — reflects the change. No disk I/O; the
/// commit path checksums and writes. A no-op on saves without a writable
/// auto-battle-data view (only BN4/BN5 have one).
pub fn apply_auto_battle_data_edit(loaded: &mut selection::Loaded, edit: AutoBattleDataEdit) {
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
            // Zero every chip's counts so the rebuilt deck is empty —
            // clearing only the materialized cache would be undone by the
            // next edit's rebuild (which reads the still-nonzero counts).
            let num_chips = loaded.assets.num_chips();
            if let Some(mut v) = loaded.save.view_auto_battle_data_mut() {
                for id in 0..num_chips {
                    v.set_chip_use_count(id, 0);
                    v.set_secondary_chip_use_count(id, 0);
                }
            }
        }
    }

    // Rebuild the materialized deck from the updated counts so the editor's
    // preview (which reads the materialized cache) shows the change live.
    // Disjoint field borrows: assets vs save.
    let assets = loaded.assets.as_ref();
    if let Some(mut v) = loaded.save.view_auto_battle_data_mut() {
        v.rebuild_materialized(assets);
    }
}

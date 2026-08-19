//! Headless legality for Battle Chip Challenge's program deck.

use tango_gamesupport_common_dataview::save::Save as _;

use crate::save::{DECK_SLOTS, NAVI_SLOT, PROGRAM_CHIP_IDS};

const R_SLOT: usize = 9;
const L_SLOT: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeckViolationKind {
    ChipIllegalForProgramDeck,
    ProgramDeckExceedsMemory { used: u32, limit: u32 },
    SlotInChipExceedsMemory { used: u32, limit: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeckViolation {
    MissingNavi,
    Chip {
        slot: usize,
        id: usize,
        kind: DeckViolationKind,
    },
}

fn violations_from_parts(
    chips: &[Option<(usize, u16)>],
    navi: Option<(usize, u16)>,
    capacity_bonus: u32,
    slot_in: u32,
) -> Vec<DeckViolation> {
    let mut violations = vec![];

    if navi.is_none() {
        violations.push(DeckViolation::MissingNavi);
    }
    for (slot, chip) in chips.iter().enumerate() {
        let Some((id, _)) = chip else { continue };
        if !PROGRAM_CHIP_IDS.contains(id) {
            violations.push(DeckViolation::Chip {
                slot,
                id: *id,
                kind: DeckViolationKind::ChipIllegalForProgramDeck,
            });
        }
    }

    let used: u32 = chips[..R_SLOT].iter().flatten().map(|(_, mb)| *mb as u32).sum();
    let capacity = navi.map(|(_, mb)| mb as u32 + capacity_bonus);
    if let Some(limit) = capacity.filter(|&limit| used > limit) {
        let kind = DeckViolationKind::ProgramDeckExceedsMemory { used, limit };
        // The Navi's MB is the wired deck's capacity, so attribute a total
        // capacity violation to the under-capacity Navi rather than painting
        // every otherwise-valid wired chip red.
        if let Some((id, _)) = navi {
            violations.push(DeckViolation::Chip {
                slot: NAVI_SLOT,
                id,
                kind,
            });
        }
    }
    for slot in [R_SLOT, L_SLOT] {
        let Some((id, mb)) = chips[slot] else { continue };
        let used = mb as u32;
        if used > slot_in {
            violations.push(DeckViolation::Chip {
                slot,
                id,
                kind: DeckViolationKind::SlotInChipExceedsMemory { used, limit: slot_in },
            });
        }
    }

    violations
}

/// Validate the equipped deck from save and ROM ground truth. Empty program
/// slots are legal; the Navi socket, program-chip id range, wired capacity,
/// and each R/L SLOT IN ceiling are not.
pub fn violations(save: &crate::save::Save, assets: &crate::rom::Assets) -> Vec<DeckViolation> {
    let Some(view) = save.view_chips() else {
        return vec![];
    };
    let deck = view.equipped_folder_index();
    let chips: Vec<Option<(usize, u16)>> = (0..DECK_SLOTS)
        .map(|slot| {
            let id = view.chip(deck, slot)?.id;
            Some((id, assets.chip_info(id).map(|info| info.mb()).unwrap_or(0)))
        })
        .collect();
    let navi = view
        .chip(deck, NAVI_SLOT)
        .map(|chip| (chip.id, assets.chip_info(chip.id).map(|info| info.mb()).unwrap_or(0)));
    violations_from_parts(
        &chips,
        navi,
        save.mb_capacity_bonus(deck) as u32,
        save.slot_in_max(deck),
    )
}

fn candidate_violations_from_parts(
    chips: &[Option<(usize, u16)>],
    current_capacity: Option<u32>,
    slot_in: u32,
    capacity_bonus: u32,
    slot: usize,
    id: usize,
    mb: u16,
) -> Vec<DeckViolationKind> {
    if slot == NAVI_SLOT {
        let used = chips[..R_SLOT].iter().flatten().map(|(_, mb)| *mb as u32).sum();
        let limit = mb as u32 + capacity_bonus;
        return (used > limit)
            .then_some(DeckViolationKind::ProgramDeckExceedsMemory { used, limit })
            .into_iter()
            .collect();
    }
    if slot >= DECK_SLOTS {
        return vec![];
    }

    let mut violations = vec![];
    if !PROGRAM_CHIP_IDS.contains(&id) {
        violations.push(DeckViolationKind::ChipIllegalForProgramDeck);
    }
    if slot == R_SLOT || slot == L_SLOT {
        let used = mb as u32;
        if used > slot_in {
            violations.push(DeckViolationKind::SlotInChipExceedsMemory { used, limit: slot_in });
        }
    } else if let Some(limit) = current_capacity {
        let current = chips[slot].map_or(0, |(_, mb)| mb as u32);
        let used = chips[..R_SLOT].iter().flatten().map(|(_, mb)| *mb as u32).sum::<u32>() - current + mb as u32;
        if used > limit {
            violations.push(DeckViolationKind::ProgramDeckExceedsMemory { used, limit });
        }
    }
    violations
}

/// Preview the legality consequences of placing one chip into one deck slot.
/// Editors use this to disable invalid choices before the save is mutated.
pub fn candidate_violations(
    save: &crate::save::Save,
    assets: &crate::rom::Assets,
    slot: usize,
    id: usize,
) -> Vec<DeckViolationKind> {
    let Some(view) = save.view_chips() else {
        return vec![];
    };
    let deck = view.equipped_folder_index();
    let chips: Vec<Option<(usize, u16)>> = (0..DECK_SLOTS)
        .map(|slot| {
            let id = view.chip(deck, slot)?.id;
            Some((id, assets.chip_info(id).map(|info| info.mb()).unwrap_or(0)))
        })
        .collect();
    let current_capacity = view.chip(deck, NAVI_SLOT).map(|chip| {
        assets.chip_info(chip.id).map(|info| info.mb()).unwrap_or(0) as u32 + save.mb_capacity_bonus(deck) as u32
    });
    let mb = assets.chip_info(id).map(|info| info.mb()).unwrap_or(0);
    candidate_violations_from_parts(
        &chips,
        current_capacity,
        save.slot_in_max(deck),
        save.mb_capacity_bonus(deck) as u32,
        slot,
        id,
        mb,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chip(id: usize, mb: u16) -> Option<(usize, u16)> {
        Some((id, mb))
    }

    #[test]
    fn empty_program_slots_are_legal_but_a_navi_is_required() {
        assert_eq!(
            violations_from_parts(&vec![None; DECK_SLOTS], None, 0, 40),
            vec![DeckViolation::MissingNavi]
        );
    }

    #[test]
    fn wired_capacity_flags_only_the_navi_while_slot_in_flags_the_chip() {
        let mut chips = vec![None; DECK_SLOTS];
        chips[0] = chip(2, 30);
        chips[1] = chip(1, 20);
        chips[R_SLOT] = chip(3, 41);
        chips[L_SLOT] = chip(4, 40);

        assert_eq!(
            violations_from_parts(&chips, Some((200, 40)), 0, 40),
            vec![
                DeckViolation::Chip {
                    slot: NAVI_SLOT,
                    id: 200,
                    kind: DeckViolationKind::ProgramDeckExceedsMemory { used: 50, limit: 40 },
                },
                DeckViolation::Chip {
                    slot: R_SLOT,
                    id: 3,
                    kind: DeckViolationKind::SlotInChipExceedsMemory { used: 41, limit: 40 },
                },
            ]
        );
    }

    #[test]
    fn navi_chip_ids_are_illegal_in_program_slots() {
        let mut chips = vec![None; DECK_SLOTS];
        chips[4] = chip(200, 0);

        assert!(
            violations_from_parts(&chips, Some((200, 100)), 0, 40).contains(&DeckViolation::Chip {
                slot: 4,
                id: 200,
                kind: DeckViolationKind::ChipIllegalForProgramDeck,
            })
        );
    }

    #[test]
    fn oversized_program_candidates_remain_detectably_illegal() {
        let mut chips = vec![None; DECK_SLOTS];
        chips[0] = chip(1, 30);

        assert_eq!(
            candidate_violations_from_parts(&chips, Some(40), 20, 0, 1, 2, 20),
            vec![DeckViolationKind::ProgramDeckExceedsMemory { used: 50, limit: 40 }]
        );
        assert_eq!(
            candidate_violations_from_parts(&chips, Some(100), 20, 0, R_SLOT, 2, 21),
            vec![DeckViolationKind::SlotInChipExceedsMemory { used: 21, limit: 20 }]
        );
    }

    #[test]
    fn a_navi_candidate_is_illegal_when_its_capacity_would_be_too_low() {
        let mut chips = vec![None; DECK_SLOTS];
        chips[0] = chip(1, 30);
        chips[1] = chip(2, 20);

        assert_eq!(
            candidate_violations_from_parts(&chips, Some(100), 40, 5, NAVI_SLOT, 200, 40),
            vec![DeckViolationKind::ProgramDeckExceedsMemory { used: 50, limit: 45 }]
        );
    }
}

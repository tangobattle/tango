//! Headless legality for Battle Network 4's slot-bound Mod Cards.

use crate::save::PATCH_CARD4_SLOTS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchCard4ViolationKind {
    NotInCatalog,
    WrongSlot { expected_slot: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchCard4Violation {
    pub slot: usize,
    pub id: usize,
    pub enabled: bool,
    pub kind: PatchCard4ViolationKind,
}

fn binding_violation(
    slot: usize,
    id: usize,
    enabled: bool,
    expected_slot: Option<usize>,
) -> Option<PatchCard4Violation> {
    let kind = match expected_slot {
        None => PatchCard4ViolationKind::NotInCatalog,
        Some(expected_slot) if expected_slot != slot => PatchCard4ViolationKind::WrongSlot { expected_slot },
        Some(_) => return None,
    };
    Some(PatchCard4Violation {
        slot,
        id,
        enabled,
        kind,
    })
}

/// Validate every stored Mod Card against BN4's own card catalog. The save
/// stores six independent bindings; a card is legal only in the slot declared
/// by its catalog entry, whether the binding is currently enabled or disabled.
pub fn patch_card4_violations(save: &crate::save::Save, assets: &crate::rom::Assets) -> Vec<PatchCard4Violation> {
    let cards = save.view_patch_card4s();
    (0..PATCH_CARD4_SLOTS)
        .filter_map(|slot| {
            let card = cards.patch_card(slot)?;
            binding_violation(
                slot,
                card.id,
                card.enabled,
                assets.patch_card4(card.id).map(|info| info.slot as usize),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_bound_to_its_catalog_slot_is_legal() {
        assert_eq!(binding_violation(2, 7, true, Some(2)), None);
    }

    #[test]
    fn wrong_slot_bindings_are_attributed_even_when_disabled() {
        assert_eq!(
            binding_violation(0, 7, false, Some(2)),
            Some(PatchCard4Violation {
                slot: 0,
                id: 7,
                enabled: false,
                kind: PatchCard4ViolationKind::WrongSlot { expected_slot: 2 },
            })
        );
    }

    #[test]
    fn catalog_gaps_are_illegal_bindings() {
        assert_eq!(
            binding_violation(1, 0, true, None),
            Some(PatchCard4Violation {
                slot: 1,
                id: 0,
                enabled: true,
                kind: PatchCard4ViolationKind::NotInCatalog,
            })
        );
    }
}

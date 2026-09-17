//! Game-specific staged edits, without a frontend dependency.
use crate::{rom as bn4_rom, save as bn4_save};
use tango_gamesupport_common_dataview::model::edit::{GameEdit, Invalidation};

#[derive(Debug, Clone)]
pub enum PatchCard4Edit {
    /// Install card `id` into its own catalog slot, enabled.
    AddCard { id: usize },
    /// Empty catalog slot `slot`.
    RemoveCard { slot: usize },
    /// Toggle slot `slot`'s card between enabled and disabled.
    ToggleCard { slot: usize },
    /// Empty every slot.
    ClearAll,
}

impl GameEdit for PatchCard4Edit {
    fn apply(&self, save: &mut tango_gamesupport_common_dataview::model::SaveModel) -> Invalidation {
        use tango_gamesupport_common_dataview::save::PatchCard;

        // The card's home slot resolves through the ROM catalog; read it
        // before the save is borrowed mutably.
        let add_slot = match self {
            PatchCard4Edit::AddCard { id } => {
                let Some(slot) = save
                    .assets
                    .underlying_any()
                    .downcast_ref::<bn4_rom::Assets>()
                    .and_then(|a| a.patch_card4(*id))
                    .map(|c| c.slot as usize)
                    .filter(|&s| s < crate::save::PATCH_CARD4_SLOTS)
                else {
                    return Invalidation::default();
                };
                Some(slot)
            }
            _ => None,
        };

        let Some(bn4) = save.save.as_mut().as_any_mut().downcast_mut::<bn4_save::Save>() else {
            return Invalidation::default();
        };
        let mut v = bn4.view_patch_card4s_mut();

        match self {
            PatchCard4Edit::AddCard { id } => {
                v.set_patch_card(add_slot.unwrap(), Some(PatchCard { id: *id, enabled: true }));
            }
            PatchCard4Edit::RemoveCard { slot } => {
                v.set_patch_card(*slot, None);
            }
            PatchCard4Edit::ToggleCard { slot } => {
                let Some(c) = v.patch_card(*slot) else {
                    return Invalidation::default();
                };
                v.set_patch_card(
                    *slot,
                    Some(PatchCard {
                        id: c.id,
                        enabled: !c.enabled,
                    }),
                );
            }
            PatchCard4Edit::ClearAll => {
                for slot in 0..crate::save::PATCH_CARD4_SLOTS {
                    v.set_patch_card(slot, None);
                }
            }
        }

        // Keep the anti-cheat mirror in sync with the edit.
        v.rebuild_anticheat();
        Invalidation::default()
    }
}

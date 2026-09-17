//! Game-specific staged edits, without a frontend dependency.
use crate::{
    rom,
    save::{Cross, Save, SaveSet},
};
use tango_gamesupport_common_dataview::{
    model::edit::{GameEdit, Invalidation},
    save::Save as _,
};

/// Play the cartridge's other in-game file: point the editor at it, and
/// stamp it as the cartridge's most recently saved file so everything
/// that reads these bytes — a session, a recording, the priming walk's
/// file select — lands on it too.
///
/// Re-reads the set from the dump the loaded save carries, staged edits
/// included, since they live in those bytes.
#[derive(Debug)]
pub struct PlayFile(pub u8);

impl GameEdit for PlayFile {
    fn apply(&self, model: &mut tango_gamesupport_common_dataview::model::SaveModel) -> Invalidation {
        let Some(mut next) = model
            .save
            .as_any()
            .downcast_ref::<Save>()
            .and_then(|save| SaveSet::parse(&save.to_sram_dump()).ok())
            .and_then(|set| set.save(self.0))
        else {
            return Invalidation::default();
        };
        next.make_current();
        model.save = Box::new(next);
        // A different file can differ in what it offers to edit, so the
        // cached capability flags are re-probed against the new save.
        tango_gamesupport_common_dataview::model::refresh_editability(model);
        Invalidation::default()
    }
}

/// Set the cross the played file brings.
#[derive(Debug)]
pub struct SetCross(pub Cross);

impl GameEdit for SetCross {
    fn apply(&self, model: &mut tango_gamesupport_common_dataview::model::SaveModel) -> Invalidation {
        if let Some(save) = model.save.as_any_mut().downcast_mut::<Save>() {
            save.set_cross(self.0);
        }
        Invalidation::default()
    }
}

/// Put a navi in a party slot, or empty it — the CHANGE the game's own
/// PARTY STATUS card offers. The dropdown only lists navis this file
/// recruited and not the other slot's, so no duplicate can be minted;
/// the save layer re-syncs the mirror bits the load checks a team
/// against, takes a departing member's programs back off, and packs the
/// pair the way the game's own machine compacts it (so emptying the
/// first slot moves the second up, loadout and all).
#[derive(Debug)]
pub struct SetPartyNavi {
    pub slot: usize,
    pub navi: Option<usize>,
}

impl GameEdit for SetPartyNavi {
    fn apply(&self, model: &mut tango_gamesupport_common_dataview::model::SaveModel) -> Invalidation {
        if let Some(save) = model.save.as_any_mut().downcast_mut::<Save>() {
            save.view_party_mut().set_navi(self.slot, self.navi);
        }
        Invalidation::default()
    }
}

/// Put one more party program on a slot's member: the member's record takes
/// the summed bonuses and the slot entry takes the program list. Only the fixed
/// loadout length gates the offer; gauge and copy-limit errors block Save until
/// the user corrects them.
#[derive(Debug)]
pub struct AddPartyProgram {
    pub slot: usize,
    pub program: usize,
}

impl GameEdit for AddPartyProgram {
    fn apply(&self, model: &mut tango_gamesupport_common_dataview::model::SaveModel) -> Invalidation {
        let Some(assets) = model.assets.underlying_any().downcast_ref::<rom::Assets>() else {
            return Invalidation::default();
        };
        if let Some(save) = model.save.as_any_mut().downcast_mut::<Save>() {
            save.view_party_mut().add_party_program(self.slot, assets, self.program);
        }
        Invalidation::default()
    }
}

/// Take the program a member equips in position `at` back off.
#[derive(Debug)]
pub struct RemovePartyProgram {
    pub slot: usize,
    pub at: usize,
}

impl GameEdit for RemovePartyProgram {
    fn apply(&self, model: &mut tango_gamesupport_common_dataview::model::SaveModel) -> Invalidation {
        let Some(assets) = model.assets.underlying_any().downcast_ref::<rom::Assets>() else {
            return Invalidation::default();
        };
        if let Some(save) = model.save.as_any_mut().downcast_mut::<Save>() {
            save.view_party_mut().remove_party_program(self.slot, assets, self.at);
        }
        Invalidation::default()
    }
}

/// A slot panel's clear-all: the slot empties, member and all. The save
/// layer takes the departing navi's programs off with it and packs the
/// pair, so clearing the first slot moves the second up into it the way
/// the game's own machine compacts.
#[derive(Debug)]
pub struct ClearPartySlot(pub usize);

impl GameEdit for ClearPartySlot {
    fn apply(&self, model: &mut tango_gamesupport_common_dataview::model::SaveModel) -> Invalidation {
        if let Some(save) = model.save.as_any_mut().downcast_mut::<Save>() {
            save.view_party_mut().set_navi(self.0, None);
        }
        Invalidation::default()
    }
}

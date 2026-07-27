//! Save-view state: the section tabs, the persistent view/edit state,
//! and the action/outcome vocabulary the save editor speaks.
//!
//! This is the *state* half only — public because [`crate::Game`]'s
//! `save_ui` trait speaks these types. The rendering half (the shell,
//! the shared components, everything that actually draws) is private
//! gamesupport knowledge and lives in `tango-gamesupport-common`.

use crate::loaded::OpenSave;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Cover,
    Navicust,
    Folder,
    PatchCards,
    AutoBattleData,
    /// Battle Chip Challenge's wired deck board — BCC's replacement for
    /// the flat Folder list.
    ProgramDeck,
}

impl Tab {
    /// Which [`crate::model::Editability`] section gates this tab's
    /// editor body (and its inclusion in the one global edit session) —
    /// the default answer for [`crate::save_ui::SaveUi::tab_editable`];
    /// games whose section lives outside the shared model override it
    /// there.
    pub fn editable_on(self, e: &crate::model::Editability) -> bool {
        match self {
            Tab::Cover => false,
            Tab::Navicust => e.navicust,
            Tab::Folder | Tab::ProgramDeck => e.folder,
            Tab::PatchCards => e.patch_cards,
            Tab::AutoBattleData => e.auto_battle_data,
        }
    }
}

#[derive(Default, Clone, Copy)]
pub struct RenderOpts {
    pub folder_grouped: bool,
}

/// The tab list for this save: the game's own sections (via its
/// [`crate::save_ui::SaveUi`]), with Cover prepended in streamer mode.
/// The equipped navi (emblem / name / HP / buster) is not a tab — it
/// lives in the persistent strip above the body (see [`view`]), so it's
/// always on screen regardless of the active section.
pub fn available_tabs(loaded: &OpenSave, streamer_mode: bool) -> Vec<Tab> {
    let mut tabs = vec![];
    if streamer_mode {
        tabs.push(Tab::Cover);
    }
    tabs.extend(loaded.save_ui.tabs(loaded));
    tabs
}

/// Persistent UI state for [`view`]. The active tab + folder
/// grouping live here so callers don't have to mirror the fields
/// themselves; apply incoming [`Action`]s via [`State::apply`].
/// The `body_scroll_id` is per-instance unique so multiple
/// save_views on screen at once (e.g. play tab + in-session
/// opponent panel) have distinct scrollable identities.
#[derive(Clone)]
pub struct State {
    pub active_tab: Option<Tab>,
    pub folder_grouped: bool,
    pub body_scroll_id: iced::widget::Id,
    /// Id of the sub-tab strip scrollable, so [`Action::SelectTab`] can
    /// `snap_to` it (resetting its horizontal scroll to the start, in lockstep
    /// with the [`tab_scroll`] fade mirror) the same way it resets the body.
    pub tab_scroll_id: iced::widget::Id,
    /// The in-progress save edit, or `None` when not editing. It's one
    /// global toggle for the whole save: while `Some`, every editable tab
    /// shows its editor, and one Save / Cancel commits / discards them all.
    /// Bundling every editor's scratch state here means leaving edit mode
    /// (or swapping saves) is a single `editing = None`.
    pub editing: Option<EditState>,
    /// Sort order for the chip library pane. A persistent UI preference
    /// (kept across edit sessions), so it lives outside [`EditState`].
    pub library_sort: LibrarySort,
    /// Sort order for the navicust palette pane (persistent preference).
    pub navicust_sort: NavicustSort,
    /// Sort order for the BN5/BN6 patch-card library pane (persistent
    /// preference).
    pub patch_card56_sort: PatchCard56Sort,
    /// Sort order for the auto-battle-data chip library pane (persistent
    /// preference).
    pub auto_battle_data_sort: AutoBattleDataSort,
    /// Entrance restarted on each sub-tab switch — the tab body
    /// (and the per-tab extras in the strip's tail) slides in,
    /// direction following the strip's order like the app's
    /// top-level tabs.
    pub enter: crate::anim::Enter,
    /// Starting offset for `enter`. Horizontal (sign following
    /// the direction of travel along the strip) for sub-tab
    /// switches; vertical for whole-body swaps (edit mode toggles,
    /// a different game/save selected).
    pub enter_from: iced::Vector,
    /// The sub-tab that was active before the last [`Action::SelectTab`].
    /// Lets the view tell whether a control in the strip's tail (the
    /// Edit affordance) was already on screen on the previous tab and
    /// skip re-animating it.
    pub prev_tab: Option<Tab>,
    /// Show/hide transition for the edit-mode Save / Cancel pair
    /// in the strip's tail. They slide in horizontally when edit
    /// mode opens and back out when it closes — and because this
    /// is keyed on the mode (not the sub-tab), they stay planted
    /// while the user flips between editor tabs.
    pub edit_anim: crate::anim::Transition,
    /// Show/hide transition for the navi picker over the [tab strip + body]
    /// region, and the authority on whether the picker is open at all — the
    /// change-navi card toggles it, and `active_tab` stays put underneath (the
    /// picker isn't a tab). The incoming side (the picker on open, the tab
    /// strip + body on dismiss) slides up into place — a plain vertical slide,
    /// matching every other screen/tab transition (no fade).
    pub navi_select: crate::anim::Transition,
    /// Horizontal scroll offset of the sub-tab strip (relative, 0..=1). Tracked
    /// so the strip's edge fades only appear on the side that has hidden tabs.
    pub tab_scroll: f32,
}

// Reorder bookkeeping is a rule, not a rendering concern — the chip
// editor's apply path needs the identical arithmetic to keep REG/TAG
// slots aligned, so it lives with the model.
pub use crate::model::rules::reorder_index;

/// Everything an in-progress save edit needs that's thrown away when the
/// edit ends. Held as [`State::editing`]'s `Option` payload so one
/// assignment clears it all.
#[derive(Clone, Default)]
pub struct EditState {
    /// Folder editor: in-progress tag-chip selection (≤2 raw slot
    /// indexes). Seeded from the equipped folder's tag pair on entering
    /// edit mode; a committed pair is written to the save only when
    /// exactly two are selected (see [`State::toggle_tag`]).
    pub tags: Vec<usize>,
    /// Folder editor: chip library filter text.
    pub library_filter: String,
    /// Navicust editor: the part currently picked up from the palette
    /// (id + orientation + compression), drawn as a ghost under the cursor.
    pub held_part: Option<HeldPart>,
    /// Navicust editor: per-part picker orientation (`id -> (rot,
    /// compressed)`). Each palette row's rotate / (de)compress buttons edit
    /// this; picking a part up keeps it in sync, so a part is always picked
    /// up in the orientation shown. Missing id = default (rot 0, compressed).
    pub part_orient: std::collections::HashMap<usize, (u8, bool)>,
    /// Navicust editor: palette filter text.
    pub navicust_filter: String,
    /// BN5/BN6 patch-card editor: library filter text.
    pub patch_card56_filter: String,
    /// Auto-battle-data editor: chip library filter text.
    pub auto_battle_data_filter: String,
    /// Slot-targeted editors (BCC's program deck board): the deck slot
    /// the library pane is currently aimed at, or `None` when no slot is
    /// picked. Slot indexes are the game's `ChipsView` slot indexes.
    pub selected_deck_slot: Option<usize>,
}

impl EditState {
    /// The orientation a palette part is shown / picked up in: an explicit
    /// per-part override, else the default (rotation 0, compressed — the
    /// smaller shape parts are usually placed in).
    pub fn orient_of(&self, id: usize) -> (u8, bool) {
        self.part_orient.get(&id).copied().unwrap_or((0, true))
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn new() -> Self {
        Self {
            active_tab: None,
            folder_grouped: true,
            body_scroll_id: iced::widget::Id::unique(),
            tab_scroll_id: iced::widget::Id::unique(),
            editing: None,
            library_sort: LibrarySort::Id,
            navicust_sort: NavicustSort::Id,
            patch_card56_sort: PatchCard56Sort::Id,
            auto_battle_data_sort: AutoBattleDataSort::Id,
            enter: crate::anim::Enter::default(),
            enter_from: iced::Vector::new(24.0, 0.0),
            prev_tab: None,
            edit_anim: crate::anim::Transition::swap(false),
            navi_select: crate::anim::Transition::new(false),
            tab_scroll: 0.0,
        }
    }

    /// Enter the global save edit mode. It's a single toggle for the whole
    /// save: every editable tab (Folder, Navi, Patch Cards) shows its
    /// editor while set, and one Save / Cancel commits / discards them all.
    /// Seeds the tag toggles from the equipped folder's current tag pair so
    /// they start in the right state. Needs `loaded` (the read view), so
    /// the play tab calls this rather than routing through [`Self::apply`].
    pub fn enter_edit(&mut self, loaded: &OpenSave) {
        // A fresh EditState — every editor opens with clean scratch state.
        self.editing = Some(EditState {
            // Seed the tag toggles from the equipped folder's tag pair, if
            // the game has tag chips and a pair is set.
            tags: loaded
                .save
                .view_chips()
                .and_then(|v| {
                    let folder = v.equipped_folder_index();
                    v.tag_chip_indexes(folder)
                })
                .flatten()
                .map(|[a, b]| vec![a, b])
                .unwrap_or_default(),
            ..Default::default()
        });
        // Mode change, not navigation — the editor body rises in
        // while the Save / Cancel pair slides into the tail.
        let now = iced::time::Instant::now();
        self.enter_from = iced::Vector::new(0.0, 20.0);
        self.enter.start(now);
        self.edit_anim.set(true, now);
    }

    /// Drop any in-progress edit without animation bookkeeping
    /// beyond the exit transition — used by hosts that reset the
    /// edit state out-of-band (e.g. the App when the loaded save
    /// is swapped out from under the view).
    pub fn clear_editing(&mut self) {
        self.editing = None;
        self.edit_anim.set(false, iced::time::Instant::now());
    }

    /// Toggle `slot` in the in-progress tag selection (capped at two).
    /// Returns the pair to commit to the save: `Some([a, b])` once two
    /// slots are selected, else `None` (which clears the tag pairing —
    /// a lone tag chip isn't a valid state in-game).
    pub fn toggle_tag(&mut self, slot: usize) -> Option<[usize; 2]> {
        let edit = self.editing.as_mut()?;
        if let Some(pos) = edit.tags.iter().position(|&s| s == slot) {
            edit.tags.remove(pos);
        } else if edit.tags.len() < 2 {
            edit.tags.push(slot);
        }
        match edit.tags.as_slice() {
            [a, b] => Some([*a, *b]),
            _ => None,
        }
    }

    /// Remap the in-progress tag selection when `removed_slot`'s chip is
    /// removed and the chips below it shift up one: drop that slot and
    /// shift any higher selected slots down, mirroring the save-side
    /// compaction.
    pub fn compact_tags(&mut self, removed_slot: usize) {
        let Some(edit) = self.editing.as_mut() else { return };
        edit.tags.retain(|&s| s != removed_slot);
        for s in edit.tags.iter_mut() {
            if *s > removed_slot {
                *s -= 1;
            }
        }
    }

    /// Remap the in-progress tag selection through a chip reorder (ordered
    /// move from `from` to `to`), so the staged TAG toggles keep pointing at
    /// the same chips after a drag — the mirror of [`compact_tags`] for moves.
    pub fn move_tags(&mut self, from: usize, to: usize) {
        let Some(edit) = self.editing.as_mut() else { return };
        for s in edit.tags.iter_mut() {
            *s = reorder_index(*s, from, to);
        }
    }

    /// Shift the staged tag selection when a chip is added at the top: the run
    /// of chips above the first empty slot (`gap`) slides down one, so any
    /// staged tag in that run moves down with it.
    pub fn shift_tags_for_top_insert(&mut self, gap: usize) {
        let Some(edit) = self.editing.as_mut() else { return };
        for s in edit.tags.iter_mut() {
            if *s < gap {
                *s += 1;
            }
        }
    }

    /// Fold an `Action` into view-local state. Returns a Task the
    /// caller should run — used for save-view-internal side
    /// effects (notably the scroll-to-top snap on a tab change)
    /// so hosts don't have to know about them. Read-only embedders
    /// (the session setup drawers) call this directly; embedders
    /// that surface edits / copies / launches call
    /// [`apply`](Self::apply), which also computes the host-side
    /// [`Outcome`].
    pub fn fold(&mut self, action: &Action) -> iced::Task<Action> {
        match action {
            Action::SelectTab(t) => {
                if self.active_tab != Some(*t) {
                    // The strip lays tabs out in declaration order,
                    // so the discriminants double as positions:
                    // moving right enters from the right, moving
                    // left from the left.
                    if let Some(prev) = self.active_tab {
                        let dx = if (*t as u8) > (prev as u8) { 24.0 } else { -24.0 };
                        self.enter_from = iced::Vector::new(dx, 0.0);
                    }
                    self.prev_tab = self.active_tab;
                    self.active_tab = Some(*t);
                    self.enter.start(iced::time::Instant::now());
                    // Picking a real tab is never the navi picker; keep the swap
                    // in lockstep with `active_tab`.
                    self.navi_select.set(false, iced::time::Instant::now());
                }
                // Reset the body scroll and the sub-tab strip scroll to the
                // start, and clear the strip's fade mirror to match. The strip's
                // offset snaps back to 0 on a tab click anyway, but `on_scroll`
                // only fires from event handling — never from this relayout — so
                // `tab_scroll` would otherwise stay stale and leave the left
                // edge fade stuck on over a strip that's actually at the start.
                self.tab_scroll = 0.0;
                iced::Task::batch([
                    iced::widget::operation::snap_to(
                        self.body_scroll_id.clone(),
                        iced::widget::scrollable::RelativeOffset::START,
                    ),
                    iced::widget::operation::snap_to(
                        self.tab_scroll_id.clone(),
                        iced::widget::scrollable::RelativeOffset::START,
                    ),
                ])
            }
            Action::ToggleFolderGrouped(g) => {
                self.folder_grouped = *g;
                iced::Task::none()
            }
            Action::TabScrolled(x) => {
                self.tab_scroll = *x;
                iced::Task::none()
            }
            // Save and Cancel both leave the global edit mode; the host
            // runs the commit/discard side effect (covering every tab).
            // Dropping the whole EditState clears every editor's scratch.
            Action::SaveEdit | Action::CancelEdit => {
                self.editing = None;
                // Returning read-only body rises in (mirroring
                // `enter_edit`) while Save / Cancel slide back out.
                let now = iced::time::Instant::now();
                self.enter_from = iced::Vector::new(0.0, 20.0);
                self.enter.start(now);
                self.edit_anim.set(false, now);
                // Leaving edit mode closes the navi picker too (the whole edit
                // session is ending, so this snaps rather than swaps).
                self.navi_select.set(false, now);
                iced::Task::none()
            }
            Action::LibraryFilterChanged(s) => {
                if let Some(e) = self.editing.as_mut() {
                    e.library_filter = s.clone();
                }
                iced::Task::none()
            }
            Action::LibrarySortChanged(s) => {
                self.library_sort = *s;
                iced::Task::none()
            }
            // ----- Navicust editor: state-local folds -----
            Action::PickUpPalettePart { id } => {
                if let Some(e) = self.editing.as_mut() {
                    // Toggle: clicking the held part deselects it; otherwise
                    // pick it up in the orientation set in the picker.
                    if e.held_part.is_some_and(|h| h.id == *id) {
                        e.held_part = None;
                    } else {
                        let (rot, compressed) = e.orient_of(*id);
                        e.held_part = Some(HeldPart {
                            id: *id,
                            rot,
                            compressed,
                            grab_row: 0,
                            grab_col: 0,
                        });
                    }
                }
                iced::Task::none()
            }
            Action::RotateHeld => {
                // Scroll-wheel rotate over the grid: rotates the held part
                // and the picker entry together (so they stay in sync).
                if let Some(e) = self.editing.as_mut() {
                    if let Some(mut h) = e.held_part {
                        h.rot = (h.rot + 1) % 4;
                        h.rotate_grab_cw();
                        e.held_part = Some(h);
                        e.part_orient.insert(h.id, (h.rot, h.compressed));
                    }
                }
                iced::Task::none()
            }
            Action::RotatePart { id } => {
                if let Some(e) = self.editing.as_mut() {
                    let (rot, compressed) = e.orient_of(*id);
                    let rot = (rot + 1) % 4;
                    e.part_orient.insert(*id, (rot, compressed));
                    if let Some(h) = e.held_part.as_mut() {
                        if h.id == *id {
                            h.rot = rot;
                            h.rotate_grab_cw();
                        }
                    }
                }
                iced::Task::none()
            }
            Action::ToggleCompressPart { id } => {
                if let Some(e) = self.editing.as_mut() {
                    let (rot, compressed) = e.orient_of(*id);
                    let compressed = !compressed;
                    e.part_orient.insert(*id, (rot, compressed));
                    if let Some(h) = e.held_part.as_mut() {
                        if h.id == *id {
                            h.compressed = compressed;
                            // The shape changes entirely, so the old grab
                            // point no longer maps to a cell — re-center.
                            h.grab_row = 0;
                            h.grab_col = 0;
                        }
                    }
                }
                iced::Task::none()
            }
            Action::ClearHeld => {
                if let Some(e) = self.editing.as_mut() {
                    e.held_part = None;
                }
                iced::Task::none()
            }
            Action::NavicustFilterChanged(s) => {
                if let Some(e) = self.editing.as_mut() {
                    e.navicust_filter = s.clone();
                }
                iced::Task::none()
            }
            Action::NavicustSortChanged(s) => {
                self.navicust_sort = *s;
                iced::Task::none()
            }
            // ----- BN5/BN6 patch-card editor: state-local folds -----
            Action::PatchCard56FilterChanged(s) => {
                if let Some(e) = self.editing.as_mut() {
                    e.patch_card56_filter = s.clone();
                }
                iced::Task::none()
            }
            Action::PatchCard56SortChanged(s) => {
                self.patch_card56_sort = *s;
                iced::Task::none()
            }
            // ----- Auto-battle-data editor: state-local folds -----
            Action::AutoBattleDataFilterChanged(s) => {
                if let Some(e) = self.editing.as_mut() {
                    e.auto_battle_data_filter = s.clone();
                }
                iced::Task::none()
            }
            Action::AutoBattleDataSortChanged(s) => {
                self.auto_battle_data_sort = *s;
                iced::Task::none()
            }
            // Toggle the navi picker, which shows over the [tab strip + body]
            // region: it slides up while the tab content drops, and clicking
            // the card again drops it back to the tab underneath — `active_tab`
            // never moves, the picker just covers it. The host opens the edit
            // session on the way in — it needs `&OpenSave` to seed tag state,
            // same as `EnterEdit`.
            Action::EnterEditNavi => {
                let now = iced::time::Instant::now();
                self.navi_select.set(!self.navi_select.shown(), now);
                iced::Task::none()
            }
            // Picking a navi closes the picker (still inside the edit session):
            // it drops away while the tab strip + body rise back in. The host
            // stages the chosen navi via its own Effect.
            Action::SetNavi(_) => {
                self.navi_select.set(false, iced::time::Instant::now());
                iced::Task::none()
            }
            // Slot-targeted editors (the BCC program deck board): picking a
            // slot is view-local — the library pane re-aims at it.
            Action::SelectDeckSlot(slot) => {
                if let Some(e) = self.editing.as_mut() {
                    e.selected_deck_slot = *slot;
                }
                iced::Task::none()
            }
            // EnterEdit needs `&OpenSave` (to seed tag state), and the
            // mutation / copy actions surface as host [`Outcome`]s —
            // all are computed in `outcome`, so they're no-ops here.
            Action::EnterEdit
            | Action::SetDeckChip { .. }
            | Action::ClearDeckChip { .. }
            | Action::AddChip { .. }
            | Action::RemoveChip { .. }
            | Action::ReorderChips(_)
            | Action::ClearFolder
            | Action::ToggleRegular { .. }
            | Action::ToggleTag { .. }
            | Action::PlaceHeld { .. }
            | Action::PickUpInstalledPart { .. }
            | Action::ClearNavicust
            | Action::AddPatchCard56 { .. }
            | Action::RemovePatchCard56 { .. }
            | Action::ReorderPatchCard56s(_)
            | Action::ClearPatchCard56s
            | Action::Game(_)
            | Action::SetChipUseCount { .. }
            | Action::SetSecondaryChipUseCount { .. }
            | Action::ClearAutoBattleData
            | Action::CopyTab(_)
            | Action::CopyTabImage(_)
            | Action::PlayClicked
            | Action::TrainingClicked => iced::Task::none(),
        }
    }

    /// Apply an `Action`: fold it into view-local state, then translate
    /// it into the [`Outcome`] the host must act on. `loaded` feeds the
    /// arms that read the save (copy rendering, edit-session seeding,
    /// drop-target resolution).
    pub fn apply(&mut self, action: &Action, loaded: Option<&OpenSave>) -> (iced::Task<Action>, Option<Outcome>) {
        let task = self.fold(action);
        let outcome = self.outcome(action, loaded);
        (task, outcome)
    }

    /// The host-side work an action implies — staged edits, clipboard
    /// copies, session launches — plus the edit-session scratch updates
    /// (staged tags, held part) that must stay aligned with the edit.
    /// `None` means the action was fully folded into view state; hosts
    /// forward [`fold`](Self::fold)'s task instead.
    fn outcome(&mut self, action: &Action, loaded: Option<&OpenSave>) -> Option<Outcome> {
        use crate::model::edit::{AutoBattleDataEdit, ChipEdit, Edit, NaviEdit, NavicustEdit, PatchCard56Edit};
        match action {
            Action::CopyTab(tab) => {
                let opts = RenderOpts {
                    folder_grouped: self.folder_grouped,
                };
                // Only a copy that actually produced text earns the
                // "Copied!" flash.
                let text = loaded.and_then(|l| tab_as_text(*tab, l, opts))?;
                crate::copy_feedback::flash(&copy_flash_key(*tab, false));
                Some(Outcome::CopyText(text))
            }
            Action::CopyTabImage(tab) => {
                let img = loaded.and_then(|l| tab_as_image(*tab, l))?;
                crate::copy_feedback::flash(&copy_flash_key(*tab, true));
                Some(Outcome::CopyImage(img))
            }
            Action::PlayClicked => Some(Outcome::Play),
            Action::TrainingClicked => Some(Outcome::Training),
            // ----- Folder editor -----
            // EnterEdit needs the read view to seed tag state + build the
            // per-slot chip pickers, so it can't be folded without `loaded`.
            Action::EnterEdit => {
                if let Some(l) = loaded {
                    self.enter_edit(l);
                }
                None
            }
            // Same global edit session as EnterEdit, but reached from the
            // navi strip's change-navi button (`fold` already pointed the
            // body at the picker). Don't re-seed if a session is already
            // open — that would wipe in-progress scratch (staged tags, a
            // held navicust part); the user is just hopping to the navi.
            Action::EnterEditNavi => {
                if self.editing.is_none() {
                    if let Some(l) = loaded {
                        self.enter_edit(l);
                    }
                }
                None
            }
            // One global Save / Cancel for the whole save.
            Action::SaveEdit => Some(Outcome::Commit),
            Action::CancelEdit => Some(Outcome::Cancel),
            Action::AddChip { chip_id, code } => {
                // New chips are inserted at the top, sliding the existing
                // run down into the first empty slot — so shift the staged
                // TAG selection to match.
                if let Some(gap) = loaded.and_then(|l| l.save.view_chips()).and_then(|v| {
                    let fi = v.equipped_folder_index();
                    (0..v.folder_size()).find(|&i| v.chip(fi, i).is_none())
                }) {
                    self.shift_tags_for_top_insert(gap);
                }
                Some(Outcome::Edit(Edit::Chips(ChipEdit::AddChip {
                    chip_id: *chip_id,
                    code: *code,
                })))
            }
            Action::RemoveChip { slot } => {
                // Mirror the save-side compaction in the in-progress tag
                // selection (drop + shift), so the TAG toggles stay
                // aligned with the shifted chips.
                self.compact_tags(*slot);
                Some(Outcome::Edit(Edit::Chips(ChipEdit::RemoveChip { slot: *slot })))
            }
            Action::ReorderChips(ev) => {
                // Only a completed drop reorders; pick-up / cancel are
                // visual-only.
                use sweeten::widget::drag::DragEvent;
                let DragEvent::Dropped { index, target_index } = *ev else {
                    return None;
                };
                let from = index;
                // Live folder occupancy, to validate + resolve the drop.
                let filled = loaded.and_then(|l| l.save.view_chips()).map(|v| {
                    let fi = v.equipped_folder_index();
                    (0..v.folder_size())
                        .map(|i| v.chip(fi, i).is_some())
                        .collect::<Vec<bool>>()
                })?;
                // Can't drag an empty slot.
                if !filled.get(from).copied().unwrap_or(false) {
                    return None;
                }
                // Dropping onto an empty slot drops the chip in at the end
                // of the packed list (the first empty slot above the target
                // = right after the last chip), never leaving a gap.
                let to = if filled.get(target_index).copied().unwrap_or(false) {
                    target_index
                } else {
                    filled.iter().rposition(|&f| f)?
                };
                if from == to {
                    return None;
                }
                // Keep the staged TAG selection aligned with the move.
                self.move_tags(from, to);
                Some(Outcome::Edit(Edit::Chips(ChipEdit::MoveChip { from, to })))
            }
            Action::ClearFolder => {
                if let Some(e) = self.editing.as_mut() {
                    e.tags.clear();
                }
                Some(Outcome::Edit(Edit::Chips(ChipEdit::ClearFolder)))
            }
            // ----- Slot-targeted deck editor (BCC program deck) -----
            Action::SetDeckChip { slot, chip_id, code } => Some(Outcome::Edit(Edit::Chips(ChipEdit::SetChip {
                slot: *slot,
                chip: Some(tango_dataview::save::Chip {
                    id: *chip_id,
                    code: *code,
                }),
            }))),
            Action::ClearDeckChip { slot } => Some(Outcome::Edit(Edit::Chips(ChipEdit::SetChip {
                slot: *slot,
                chip: None,
            }))),
            Action::ToggleRegular { slot } => Some(Outcome::Edit(Edit::Chips(ChipEdit::ToggleRegular { slot: *slot }))),
            Action::ToggleTag { slot } => {
                // `toggle_tag` updates the in-progress UI selection and
                // hands back the pair to commit (Some([a,b]) at two, else
                // None to clear).
                let pair = self.toggle_tag(*slot);
                Some(Outcome::Edit(Edit::Chips(ChipEdit::SetTags(pair))))
            }
            // ----- Navicust editor -----
            Action::PlaceHeld { col, row } => {
                // Build the part from the held state (already folded), then
                // drop it so the cursor is free again.
                let edit = self.editing.as_mut();
                let part = edit.and_then(|e| {
                    let p = e.held_part.map(|h| tango_dataview::save::NavicustPart {
                        id: h.id,
                        col: *col,
                        row: *row,
                        rot: h.rot,
                        compressed: h.compressed,
                    });
                    e.held_part = None;
                    p
                });
                part.map(|p| Outcome::Edit(Edit::Navicust(NavicustEdit::AddPart(p))))
            }
            Action::PickUpInstalledPart { slot, col, row } => {
                // Read the part being removed so it becomes the held part —
                // the user can re-place / rotate it.
                let held = loaded.and_then(|l| {
                    if let Some(v) = l.save.view_navicust() {
                        v.navicust_part(*slot)
                    } else {
                        None
                    }
                });
                if let (Some(p), Some(e)) = (held, self.editing.as_mut()) {
                    // Grab the part at the clicked cell: store that cell's
                    // offset from the part's center anchor so it stays
                    // under the cursor while dragging.
                    e.held_part = Some(HeldPart {
                        id: p.id,
                        rot: p.rot,
                        compressed: p.compressed,
                        grab_row: *row as i8 - p.row as i8,
                        grab_col: *col as i8 - p.col as i8,
                    });
                    // Keep the picker entry in sync so picking is
                    // consistent: the part now shows this rotation /
                    // compression in the palette too.
                    e.part_orient.insert(p.id, (p.rot, p.compressed));
                }
                Some(Outcome::Edit(Edit::Navicust(NavicustEdit::RemovePart { slot: *slot })))
            }
            Action::ClearNavicust => {
                if let Some(e) = self.editing.as_mut() {
                    e.held_part = None;
                }
                Some(Outcome::Edit(Edit::Navicust(NavicustEdit::ClearAll)))
            }
            // ----- BN5/BN6 patch-card editor -----
            Action::AddPatchCard56 { id } => {
                Some(Outcome::Edit(Edit::PatchCard56s(PatchCard56Edit::AddCard { id: *id })))
            }
            Action::RemovePatchCard56 { slot } => {
                Some(Outcome::Edit(Edit::PatchCard56s(PatchCard56Edit::RemoveCard {
                    slot: *slot,
                })))
            }
            Action::ClearPatchCard56s => Some(Outcome::Edit(Edit::PatchCard56s(PatchCard56Edit::ClearAll))),
            Action::ReorderPatchCard56s(ev) => {
                // Registered list is dense, so any drop is a valid ordered
                // move; pick-up / cancel are visual-only.
                use sweeten::widget::drag::DragEvent;
                match ev {
                    DragEvent::Dropped { index, target_index } if index != target_index => {
                        Some(Outcome::Edit(Edit::PatchCard56s(PatchCard56Edit::MoveCard {
                            from: *index,
                            to: *target_index,
                        })))
                    }
                    _ => None,
                }
            }
            // ----- Game-specific editors -----
            Action::Game(e) => Some(Outcome::Edit(Edit::Game(e.clone()))),
            // ----- Auto Battle Data editor -----
            Action::SetChipUseCount { id, count } => {
                Some(Outcome::Edit(Edit::AutoBattleData(AutoBattleDataEdit::SetUseCount {
                    id: *id,
                    count: *count,
                })))
            }
            Action::SetSecondaryChipUseCount { id, count } => Some(Outcome::Edit(Edit::AutoBattleData(
                AutoBattleDataEdit::SetSecondaryUseCount { id: *id, count: *count },
            ))),
            Action::ClearAutoBattleData => Some(Outcome::Edit(Edit::AutoBattleData(AutoBattleDataEdit::ClearAll))),
            // ----- Navi editor -----
            Action::SetNavi(navi) => Some(Outcome::Edit(Edit::Navi(NaviEdit::SetNavi(*navi)))),
            _ => None,
        }
    }
}

/// What the host must do in response to an applied [`Action`] —
/// everything the save view can't do itself because it needs App-level
/// collaborators (the in-memory loaded save, the clipboard, the
/// session host).
pub enum Outcome {
    /// Stage one edit into the loaded save in memory (the UI reads it
    /// live; nothing hits disk until [`Outcome::Commit`]).
    Edit(crate::model::edit::Edit),
    /// Copy plain text to the clipboard.
    CopyText(String),
    /// Copy a raster image to the clipboard.
    CopyImage(image::RgbaImage),
    /// The embedder-defined Play button was pressed.
    Play,
    /// The embedder-defined Training button was pressed.
    Training,
    /// Write every staged edit (folder + navicust + patch cards + auto
    /// battle data) to the .sav on disk in one shot.
    Commit,
    /// Discard all staged edits, reloading the on-disk original.
    Cancel,
}

/// User-driven changes the embedded save view wants to surface. The
/// caller `.map`s its top-level Message onto this and dispatches:
/// most variants just need `state.apply(&action)`; the Copy
/// variants need the caller's `tab_as_text` / `tab_as_image` plus
/// a clipboard write.
#[derive(Debug, Clone)]
pub enum Action {
    SelectTab(Tab),
    /// The sub-tab strip was scrolled; carries the new relative x offset
    /// (0..=1), used only to drive the strip's edge fades.
    TabScrolled(f32),
    ToggleFolderGrouped(bool),
    CopyTab(Tab),
    CopyTabImage(Tab),
    /// Embedder-defined "start single-player here" action.
    /// Emitted by the Play button rendered in the save_view tab
    /// strip when [`view`] is called with `play_button = Some(_)`.
    /// The play tab routes this to `Effect::StartSinglePlayer`;
    /// other embedders (replay, opponent panel) pass `None` and
    /// the button isn't rendered.
    PlayClicked,
    /// Embedder-defined "start training here" action, routed by the
    /// play tab to `Effect::StartTraining`. Nothing raises it while the
    /// Training button is hidden (see the actions row in [`view`]); the
    /// route stays wired so restoring the button is a local change.
    #[allow(dead_code)]
    TrainingClicked,
    // ----- Folder editor (only emitted when `view`'s `editable` is set) -----
    /// Enter folder edit mode. The play tab seeds tag state via
    /// [`State::enter_edit`]; the rest is handled in [`State::apply`].
    /// Edits are staged live in the loaded save but not written to disk
    /// until [`Action::SaveEdit`].
    EnterEdit,
    /// Focus the navi picker as the edit body — fired by clicking the navi
    /// strip's card, which is only a button while the global edit session is
    /// open. The navi has no tab of its own, so this points the body at the
    /// picker (handled in [`State::apply`]); the host opens the session if it
    /// somehow isn't already (it needs `&OpenSave`, like [`Action::EnterEdit`]).
    EnterEditNavi,
    /// Finish editing: commit the staged folder to the save file on
    /// disk, then leave edit mode.
    SaveEdit,
    /// Discard all staged edits (reverts the loaded save to the
    /// on-disk original) and leave edit mode.
    CancelEdit,
    /// Library pane: add this chip+code to the first empty folder slot.
    AddChip {
        chip_id: usize,
        code: tango_dataview::save::ChipCode,
    },
    /// Folder pane: empty `slot`.
    RemoveChip {
        slot: usize,
    },
    /// Folder pane: a drag-reorder gesture from the draggable folder list
    /// (carries sweeten's raw [`DragEvent`]; only a completed drop between two
    /// filled slots actually moves a chip — see the play tab's handler).
    ReorderChips(sweeten::widget::drag::DragEvent),
    /// Folder pane: empty every slot (and clear REG/TAG).
    ClearFolder,
    /// Toggle `slot` as the folder's Regular chip — set it, or clear it
    /// if it's already the regular chip.
    ToggleRegular {
        slot: usize,
    },
    /// Toggle `slot`'s membership in the Tag chip pair.
    ToggleTag {
        slot: usize,
    },
    // ----- Slot-targeted deck editor (BCC program deck; only emitted
    // when `editable` is set) -----
    /// Board pane: aim the library at this deck slot (`None` clears the
    /// selection).
    SelectDeckSlot(Option<usize>),
    /// Library pane: install this chip into deck slot `slot`, replacing
    /// whatever occupied it.
    SetDeckChip {
        slot: usize,
        chip_id: usize,
        code: tango_dataview::save::ChipCode,
    },
    /// Board pane: empty deck slot `slot`.
    ClearDeckChip {
        slot: usize,
    },
    /// Library pane: the filter text changed.
    LibraryFilterChanged(String),
    /// Library pane: the sort order changed.
    LibrarySortChanged(LibrarySort),
    // ----- Navicust editor (only emitted when `editable` is set) -----
    /// Palette: pick up part `id` in the orientation shown in the picker.
    PickUpPalettePart {
        id: usize,
    },
    /// Rotate the held part 90° clockwise (grid scroll-wheel).
    RotateHeld,
    /// Palette: rotate this part's picker entry 90° clockwise.
    RotatePart {
        id: usize,
    },
    /// Palette: toggle this part's picker entry between its compressed
    /// and uncompressed shape.
    ToggleCompressPart {
        id: usize,
    },
    /// Drop the held part without placing it.
    ClearHeld,
    /// Place the held part with its center on grid cell `(col, row)`.
    PlaceHeld {
        col: u8,
        row: u8,
    },
    /// Pick an installed part back up — it's removed and becomes held.
    /// `(col, row)` is the cell that was clicked, so the part can be
    /// grabbed at that point rather than re-centered on the cursor.
    PickUpInstalledPart {
        slot: usize,
        col: u8,
        row: u8,
    },
    /// Remove every installed part.
    ClearNavicust,
    // ----- Navi editor (only emitted when `editable` is set) -----
    /// Set the equipped navi to this index.
    SetNavi(usize),
    /// Palette: the filter text changed.
    NavicustFilterChanged(String),
    /// Palette: the sort order changed.
    NavicustSortChanged(NavicustSort),
    // ----- BN5/BN6 patch-card editor (only emitted when `editable` is set) -----
    /// Library pane: register patch card `id` (appended to the list,
    /// enabled).
    AddPatchCard56 {
        id: usize,
    },
    /// List pane: unregister the patch card in `slot`.
    RemovePatchCard56 {
        slot: usize,
    },
    /// List pane: a drag-reorder gesture (carries sweeten's raw [`DragEvent`];
    /// only a completed drop reorders — see the play tab's handler).
    ReorderPatchCard56s(sweeten::widget::drag::DragEvent),
    /// List pane: unregister every patch card.
    ClearPatchCard56s,
    /// Library pane: the filter text changed.
    PatchCard56FilterChanged(String),
    /// Library pane: the sort order changed.
    PatchCard56SortChanged(PatchCard56Sort),
    // ----- Game-specific editors (only emitted when `editable` is set) -----
    /// A staged edit whose model belongs to one game (BN4's Mod Cards):
    /// the game's UI crate builds a [`crate::model::edit::GameEdit`]
    /// and this carries it to the host unchanged. `Arc` keeps the
    /// message `Clone`.
    Game(std::sync::Arc<dyn crate::model::edit::GameEdit>),
    // ----- Auto Battle Data editor (only emitted when `editable` is set) -----
    /// Library pane: set chip `id`'s primary use count (the count that
    /// drives the materialized deck for every section).
    SetChipUseCount {
        id: usize,
        count: usize,
    },
    /// Library pane: set chip `id`'s secondary use count (drives the
    /// secondary-standard section — only meaningful for Standard chips).
    SetSecondaryChipUseCount {
        id: usize,
        count: usize,
    },
    /// Deck pane: zero every chip's use counts, emptying the deck.
    ClearAutoBattleData,
    /// Library pane: the filter text changed.
    AutoBattleDataFilterChanged(String),
    /// Library pane: the sort order changed.
    AutoBattleDataSortChanged(AutoBattleDataSort),
}

/// Stable copy-feedback key for a tab's copy buttons — shared between
/// the view (which renders the "Copied!" flash) and the host tabs'
/// update paths (which fire it once the copy actually lands on the
/// clipboard). See [`crate::copy_feedback`].
pub fn copy_flash_key(tab: Tab, image: bool) -> String {
    format!("save-view-copy-{}-{}", if image { "image" } else { "text" }, tab as u8)
}

/// A save-view tab as TSV text for clipboard "copy as text", or `None` for
/// tabs without a text form — the game's own [`crate::save_ui::SaveUi`]
/// decides.
pub fn tab_as_text(tab: Tab, loaded: &OpenSave, opts: RenderOpts) -> Option<String> {
    loaded.save_ui.tab_as_text(tab, loaded, opts)
}

/// Render a save-view tab to an RGBA image for clipboard "copy as image",
/// or `None` for tabs without an image form.
pub fn tab_as_image(tab: Tab, loaded: &OpenSave) -> Option<image::RgbaImage> {
    loaded.save_ui.tab_as_image(tab, loaded)
}

// ---------------------------------------------------------------------
// Editor scratch/preference types referenced by [`State`] and
// [`Action`]. They live here (not with the components that render them)
// because they're state the shell carries; the label strings for the
// sort pickers stay with the components, which own the i18n bundle.

/// Sort order for the folder editor's chip-library (right) pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibrarySort {
    Id,
    Name,
    Code,
    Attack,
    Element,
    Mb,
}

impl LibrarySort {
    pub const ALL: [LibrarySort; 6] = [
        LibrarySort::Id,
        LibrarySort::Name,
        LibrarySort::Code,
        LibrarySort::Attack,
        LibrarySort::Element,
        LibrarySort::Mb,
    ];
}

/// Sort order for the navicust editor's palette pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavicustSort {
    Id,
    Name,
    Color,
}

impl NavicustSort {
    pub const ALL: [NavicustSort; 3] = [NavicustSort::Id, NavicustSort::Name, NavicustSort::Color];
}

/// Sort order for the BN5/BN6 patch-card editor's library pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchCard56Sort {
    Id,
    Name,
    Mb,
}

impl PatchCard56Sort {
    pub const ALL: [PatchCard56Sort; 3] = [PatchCard56Sort::Id, PatchCard56Sort::Name, PatchCard56Sort::Mb];
}

/// Sort order for the auto-battle-data editor's chip library pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoBattleDataSort {
    Id,
    Name,
    Used,
}

impl AutoBattleDataSort {
    pub const ALL: [AutoBattleDataSort; 3] = [
        AutoBattleDataSort::Id,
        AutoBattleDataSort::Name,
        AutoBattleDataSort::Used,
    ];
}

/// A navicust part picked up from the palette: its id plus the
/// orientation + compression it'll be dropped with. Lives in the
/// save-view state because the palette (which sets it) and the editor
/// canvas (which draws its ghost) are separate widgets.
#[derive(Debug, Clone, Copy)]
pub struct HeldPart {
    pub id: usize,
    pub rot: u8,
    pub compressed: bool,
    /// Where on the part it was grabbed: the offset (in the *current*
    /// orientation) of the grabbed cell from the part's center anchor,
    /// as `(row, col)`. Keeps that cell under the cursor as it's dragged
    /// instead of snapping the center there. `(0, 0)` for palette
    /// pick-ups (no meaningful grab point).
    pub grab_row: i8,
    pub grab_col: i8,
}

impl HeldPart {
    /// Rotate the grab point 90° clockwise to track [`Self::rot`] being
    /// advanced — keeps the grabbed cell under the cursor through a
    /// rotate. Mirrors the clockwise cell map in the navicust editor's
    /// `rotated_offsets`: `(dy, dx) -> (dx, -dy)`.
    pub fn rotate_grab_cw(&mut self) {
        let (r, c) = (self.grab_row, self.grab_col);
        self.grab_row = c;
        self.grab_col = -r;
    }
}

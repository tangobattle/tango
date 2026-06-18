use crate::i18n::t;
use crate::selection::Loaded;
use crate::style::{self, TEXT_BODY, TEXT_CAPTION, TEXT_DISPLAY};
use crate::widgets::{muted_color, muted_text_style};
use iced::widget::{button, container, image as iced_image, scrollable, stack, text, tooltip, Image, Space};
use sweeten::widget::{column, pick_list, row, text_input};

/// Save view is read-only — every interactive bit (NCP hover, chip
/// hover) is handled by tooltip/canvas widgets that manage their own
/// state internally, so render fns never emit caller-visible messages.
/// The Element is generic over the embedder's Message type.
use iced::{Alignment, ContentFit, Element, Fill, Length};
use tango_dataview::rom::NavicustPartColor;
use tango_dataview::save::Save;
use unic_langid::LanguageIdentifier;

pub(crate) mod abd;
pub(crate) mod folder;
pub mod navicust;
pub(crate) mod patch_cards;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Cover,
    Navi,
    Folder,
    PatchCards,
    AutoBattleData,
}

#[derive(Default, Clone, Copy)]
pub struct RenderOpts {
    pub folder_grouped: bool,
}


/// A sort mode paired with its localized label, for the editors' sort
/// pick_lists — the picker renders options via `Display`, which can't
/// reach the language, so the label is resolved up front. Equality is
/// by mode so the picker can match the current selection.
#[derive(Clone)]
struct SortChoice<S> {
    sort: S,
    label: String,
}

impl<S: PartialEq> PartialEq for SortChoice<S> {
    fn eq(&self, other: &Self) -> bool {
        self.sort == other.sort
    }
}

impl<S> std::fmt::Display for SortChoice<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// The filter box + sort picker strip shared by all four editor library
/// panes (folder, navicust palette, patch cards, auto battle data).
/// `search_placeholder` is the filter box's pre-resolved placeholder
/// text (`t!` only takes literal keys, so the lookup stays at the call
/// site); `sort_label` is the sort enum's `label` method.
fn library_header<'a, S: Copy + PartialEq + 'static>(
    lang: &LanguageIdentifier,
    search_placeholder: String,
    filter_value: &str,
    on_filter: fn(String) -> Action,
    sorts: &[S],
    current: S,
    sort_label: fn(S, &LanguageIdentifier) -> String,
    on_sort: fn(S) -> Action,
) -> Element<'a, Action> {
    let filter_input = text_input(&search_placeholder, filter_value)
        .on_input(on_filter)
        .padding(style::CONTROL_PADDING)
        .size(TEXT_BODY)
        .width(Fill)
        .style(crate::widgets::chunky_text_input);
    let sort_options: Vec<SortChoice<S>> = sorts
        .iter()
        .map(|&sort| SortChoice {
            sort,
            label: sort_label(sort, lang),
        })
        .collect();
    let sort_selected = sort_options.iter().find(|c| c.sort == current).cloned();
    let sort_pick = pick_list(sort_options, sort_selected, move |c: SortChoice<S>| on_sort(c.sort))
        .padding(style::CONTROL_PADDING)
        .text_size(TEXT_BODY)
        .style(crate::widgets::chunky_pick_list);
    container(
        row![
            filter_input,
            text(t!(lang, "save-edit-sort"))
                .size(TEXT_CAPTION)
                .style(muted_text_style),
            sort_pick,
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding(style::HEADER_PADDING)
    .into()
}

/// One editor pane: a header strip pinned above a scrollable body, on
/// the standard pane plate. Every pane in the four editors is this
/// shape.
fn editor_pane<'a>(
    header: impl Into<Element<'a, Action>>,
    body: impl Into<Element<'a, Action>>,
) -> Element<'a, Action> {
    container(column![
        header.into(),
        scrollable(body.into())
            .style(crate::widgets::chunky_scrollable)
            .height(Fill)
            .width(Fill)
    ])
    .width(Fill)
    .height(Fill)
    .style(crate::widgets::pane)
    .into()
}

/// The editors' two-pane layout: working set on the left, library /
/// palette on the right.
fn editor_panes<'a>(left: Element<'a, Action>, right: Element<'a, Action>) -> Element<'a, Action> {
    row![left, right]
        .spacing(style::PANE_GAP)
        .width(Fill)
        .height(Fill)
        .into()
}

/// The red "Clear" button atop every save-editor pane — identical across the
/// folder / navicust / patch-card / auto-battle-data panes apart from the
/// action it fires.
fn clear_all_button<'a>(lang: &LanguageIdentifier, action: Action) -> Element<'a, Action> {
    crate::widgets::labeled_icon_button(
        lucide_icons::Icon::Trash2,
        t!(lang, "save-edit-clear"),
        action,
        style::CONTROL_PADDING,
        crate::widgets::danger_button,
    )
}

/// Standard editor-pane header chrome: a body-size title, any inline stat
/// captions (`extras`), a flexible spacer, then the clear-all button. Shared
/// by the navicust / patch-card / auto-battle-data panes; the folder pane adds
/// a second stats line and builds its own column.
fn editor_header<'a>(
    lang: &LanguageIdentifier,
    title: String,
    extras: Vec<Element<'a, Action>>,
    clear_action: Action,
) -> Element<'a, Action> {
    let mut header = row![text(title).size(TEXT_BODY)].spacing(8).align_y(Alignment::Center);
    for extra in extras {
        header = header.push(extra);
    }
    header = header
        .push(Space::new().width(Fill))
        .push(clear_all_button(lang, clear_action));
    container(header).width(Fill).padding(style::HEADER_PADDING).into()
}

/// Caption text that turns danger-red when an editor budget is blown
/// (folder class limits, patch-card MB, folder over-fill) and reads
/// muted otherwise.
fn limit_caption<'a>(label: String, over: bool) -> iced::widget::Text<'a> {
    text(label)
        .size(TEXT_CAPTION)
        .style(move |theme: &iced::Theme| iced::widget::text::Style {
            color: Some(if over {
                theme.palette().danger
            } else {
                muted_color(theme)
            }),
        })
}

/// The chips the player owns (their pack), as `(id, name, code)`, in
/// `sort` order — one row per owned chip+code. Only chips+codes with a
/// pack count > 0 are returned, so a folder can only be built from what
/// the save legitimately holds. Ties fall back to id for a stable order.



pub fn available_tabs(save: &dyn Save, streamer_mode: bool) -> Vec<Tab> {
    let mut tabs = vec![];
    if streamer_mode {
        tabs.push(Tab::Cover);
    }
    if save.view_navi().is_some() {
        tabs.push(Tab::Navi);
    }
    if save.view_chips().is_some() {
        tabs.push(Tab::Folder);
    }
    if save.view_patch_cards().is_some() {
        tabs.push(Tab::PatchCards);
    }
    if save.view_auto_battle_data().is_some() {
        tabs.push(Tab::AutoBattleData);
    }
    tabs
}

pub fn render<M: 'static>(
    lang: &LanguageIdentifier,
    tab: Tab,
    loaded: &Loaded,
    opts: RenderOpts,
) -> Element<'static, M> {
    match tab {
        Tab::Cover => render_cover(lang, loaded),
        Tab::Navi => navicust::render_navi(lang, loaded),
        Tab::Folder => folder::render_folder(lang, loaded, opts.folder_grouped),
        Tab::PatchCards => patch_cards::render_patch_cards(lang, loaded),
        Tab::AutoBattleData => abd::render_auto_battle_data(lang, loaded),
    }
}

/// Per-tab Lucide icon glyph used by the tab strip in [`view`].
fn tab_icon(tab: Tab) -> lucide_icons::Icon {
    use lucide_icons::Icon;
    match tab {
        Tab::Cover => Icon::Eye,
        Tab::Navi => Icon::Bot,
        Tab::Folder => Icon::Files,
        Tab::PatchCards => Icon::CreditCard,
        Tab::AutoBattleData => Icon::Swords,
    }
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
    body_scroll_id: iced::widget::Id,
    /// The in-progress save edit, or `None` when not editing. It's one
    /// global toggle for the whole save: while `Some`, every editable tab
    /// shows its editor, and one Save / Cancel commits / discards them all.
    /// Bundling every editor's scratch state here means leaving edit mode
    /// (or swapping saves) is a single `editing = None`.
    pub editing: Option<EditState>,
    /// Sort order for the chip library pane. A persistent UI preference
    /// (kept across edit sessions), so it lives outside [`EditState`].
    pub library_sort: folder::LibrarySort,
    /// Sort order for the navicust palette pane (persistent preference).
    pub navicust_sort: navicust::NavicustSort,
    /// Sort order for the BN5/BN6 patch-card library pane (persistent
    /// preference).
    pub patch_card56_sort: patch_cards::PatchCard56Sort,
    /// Sort order for the auto-battle-data chip library pane (persistent
    /// preference).
    pub auto_battle_data_sort: abd::AutoBattleDataSort,
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
    pub held_part: Option<navicust::HeldPart>,
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
            editing: None,
            library_sort: folder::LibrarySort::Id,
            navicust_sort: navicust::NavicustSort::Id,
            patch_card56_sort: patch_cards::PatchCard56Sort::Id,
            auto_battle_data_sort: abd::AutoBattleDataSort::Id,
            enter: crate::anim::Enter::default(),
            enter_from: iced::Vector::new(24.0, 0.0),
            prev_tab: None,
            edit_anim: crate::anim::Transition::swap(false),
        }
    }

    /// Enter the global save edit mode. It's a single toggle for the whole
    /// save: every editable tab (Folder, Navi, Patch Cards) shows its
    /// editor while set, and one Save / Cancel commits / discards them all.
    /// Seeds the tag toggles from the equipped folder's current tag pair so
    /// they start in the right state. Needs `loaded` (the read view), so
    /// the play tab calls this rather than routing through [`Self::apply`].
    pub fn enter_edit(&mut self, loaded: &Loaded) {
        // A fresh EditState — every editor opens with clean scratch state.
        let mut edit = EditState::default();
        // Seed the tag toggles from the equipped folder's tag pair, if
        // the game has tag chips and a pair is set.
        edit.tags = loaded
            .save
            .view_chips()
            .and_then(|v| {
                let folder = v.equipped_folder_index();
                v.tag_chip_indexes(folder)
            })
            .flatten()
            .map(|[a, b]| vec![a, b])
            .unwrap_or_default();
        self.editing = Some(edit);
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
        let Some(edit) = self.editing.as_mut() else { return None };
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

    /// Apply an `Action` to the state. `CopyTab` is left for the
    /// caller to handle (clipboard side-effects can't happen inside
    /// `apply`); everything else is folded in. Returns a Task the
    /// caller should run — used for save-view-internal side
    /// effects (notably the scroll-to-top snap on a tab change)
    /// so hosts don't have to know about them.
    pub fn apply(&mut self, action: &Action) -> iced::Task<Action> {
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
                }
                iced::widget::operation::snap_to(
                    self.body_scroll_id.clone(),
                    iced::widget::scrollable::RelativeOffset::START,
                )
            }
            Action::ToggleFolderGrouped(g) => {
                self.folder_grouped = *g;
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
                    if e.held_part.map_or(false, |h| h.id == *id) {
                        e.held_part = None;
                    } else {
                        let (rot, compressed) = e.orient_of(*id);
                        e.held_part = Some(navicust::HeldPart {
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
            // EnterEdit needs `&Loaded` (to seed tag state), and the
            // mutation actions become host Effects — all are driven by
            // the embedder (play tab), so they're no-ops here.
            Action::EnterEdit
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
            | Action::TogglePatchCard56 { .. }
            | Action::ClearPatchCard56s
            | Action::AddPatchCard4 { .. }
            | Action::RemovePatchCard4 { .. }
            | Action::TogglePatchCard4 { .. }
            | Action::ClearPatchCard4s
            | Action::SetChipUseCount { .. }
            | Action::SetSecondaryChipUseCount { .. }
            | Action::ClearAutoBattleData
            | Action::CopyTab(_)
            | Action::CopyTabImage(_)
            | Action::PlayClicked => iced::Task::none(),
        }
    }
}

/// User-driven changes the embedded save view wants to surface. The
/// caller `.map`s its top-level Message onto this and dispatches:
/// most variants just need `state.apply(&action)`; the Copy
/// variants need the caller's `tab_as_text` / `tab_as_image` plus
/// a clipboard write.
#[derive(Debug, Clone)]
pub enum Action {
    SelectTab(Tab),
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
    // ----- Folder editor (only emitted when `view`'s `editable` is set) -----
    /// Enter folder edit mode. The play tab seeds tag state via
    /// [`State::enter_edit`]; the rest is handled in [`State::apply`].
    /// Edits are staged live in the loaded save but not written to disk
    /// until [`Action::SaveEdit`].
    EnterEdit,
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
    /// Library pane: the filter text changed.
    LibraryFilterChanged(String),
    /// Library pane: the sort order changed.
    LibrarySortChanged(folder::LibrarySort),
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
    /// Palette: the filter text changed.
    NavicustFilterChanged(String),
    /// Palette: the sort order changed.
    NavicustSortChanged(navicust::NavicustSort),
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
    /// List pane: toggle the patch card in `slot` between enabled and
    /// disabled.
    TogglePatchCard56 {
        slot: usize,
    },
    /// List pane: unregister every patch card.
    ClearPatchCard56s,
    /// Library pane: the filter text changed.
    PatchCard56FilterChanged(String),
    /// Library pane: the sort order changed.
    PatchCard56SortChanged(patch_cards::PatchCard56Sort),
    // ----- BN4 patch-card editor (only emitted when `editable` is set) -----
    /// A slot's dropdown picked card `id` — install it into its own catalog
    /// slot, enabled (replacing whatever card occupied that slot).
    AddPatchCard4 {
        id: usize,
    },
    /// A slot's dropdown picked "None" — clear catalog slot `slot`.
    RemovePatchCard4 {
        slot: usize,
    },
    /// Toggle slot `slot`'s card between enabled and disabled.
    TogglePatchCard4 {
        slot: usize,
    },
    /// Clear every slot.
    ClearPatchCard4s,
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
    AutoBattleDataSortChanged(abd::AutoBattleDataSort),
}

/// Wholesale save-view widget: tab strip with Lucide icons, optional
/// per-tab extras (folder group toggle, copy buttons), and the body.
/// Embedders just call this and `.map(Message::SaveViewAction)`.
///
/// `play_button`:
///   * `None`        — no Play button in the tab strip.
///   * `Some(true)`  — Play button rendered and enabled.
///   * `Some(false)` — Play button rendered but disabled (e.g.
///     while a netplay lobby is active and singleplayer would
///     conflict with the open session).
/// `editable`: when `true` (only the play tab passes this) and the
/// loaded save supports it, the Folder tab gains an Edit button that
/// flips its body into the in-place chip-deck editor. Replay /
/// opponent panels pass `false`, so they never show the affordance.
pub fn view<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a Loaded,
    state: &'a State,
    streamer_mode: bool,
    play_button: Option<bool>,
    inline_actions: bool,
    editable: bool,
) -> Element<'a, Action> {
    use crate::widgets;
    use iced::{Alignment, Fill};

    let available = available_tabs(loaded.save.as_ref(), streamer_mode);
    if available.is_empty() {
        return placeholder(t!(lang, "save-empty"));
    }
    let active = state
        .active_tab
        .filter(|t| available.contains(t))
        .unwrap_or(available[0]);

    // Body entrance — restarted on sub-tab switches (sliding
    // along the strip's direction of travel), edit-mode toggles
    // and game/save swaps (rising in vertically). The Play button
    // in the strip's tail is a fixture and never animates;
    // everything else in the tail slides horizontally only (these
    // are controls in a fixed-height row, not part of the body).
    let now = iced::time::Instant::now();
    let enter = state.enter.progress(now);
    let enter_from = state.enter_from;
    let entered = move |el: Element<'a, Action>| -> Element<'a, Action> {
        match enter {
            Some(p) => crate::anim::slide_in(el, p, enter_from),
            None => el,
        }
    };
    // Tail buttons animate only when their content actually
    // changed — a sub-tab switch (horizontal enter). A game/save
    // swap rises the body in, but the strip's buttons are
    // typically identical across saves, and re-animating them
    // there reads as a glitch. Edit-mode toggles run their own
    // two-phase swap below instead.
    let tail_slide = enter.filter(|_| enter_from.x != 0.0);
    let extras_dx = if enter_from.x != 0.0 { enter_from.x } else { 24.0 };
    let extras_entered = move |el: Element<'a, Action>| -> Element<'a, Action> {
        match tail_slide {
            Some(p) => crate::anim::slide_in(el, p, iced::Vector::new(extras_dx, 0.0)),
            None => el,
        }
    };
    // Edit-mode tail morph: the per-tab extras and the Save /
    // Cancel pair fade-through swap in both directions, so the
    // Edit affordance visibly turns into Save / Cancel and back.
    let (edit_side, edit_swap) = crate::anim::swap_phase(&state.edit_anim, now);
    let render_edit_buttons = editable && edit_side;
    // True while one of the in-place editors is open. Suppresses the
    // Play button (single-player would fight the open edit session) and
    // selects the editable body below.
    // One global edit toggle: while set, every editable tab shows its
    // editor (gated by that feature's editability), and one Save / Cancel
    // commits / discards them all.
    let editing_session = editable && state.editing.is_some();
    let folder_editing = editing_session && loaded.chips_editable;
    let navicust_editing = editing_session && loaded.navicust_editable;
    let patch_cards_editing = editing_session && loaded.patch_cards_editable;
    let auto_battle_data_editing = editing_session && loaded.auto_battle_data_editable;

    // Tab strip: tabs left, extras+Play right. We split into two
    // rows so the tab list can wrap onto a second line without
    // dragging the extras/Play tail with it. The tail is a
    // separate row, sized to its content and capped to the tab
    // button height so the strip's overall height doesn't grow
    // when active-tab extras (folder group toggle, copy buttons)
    // change.
    // Height of a small `widgets::tab_button`: TEXT_BODY at iced's
    // default 1.3 line height plus the [6, 14] chip padding. The
    // tail is pinned to exactly this so both halves of the strip
    // share a centerline (Start-aligned row + equal heights =
    // aligned); a stale hand-tuned 31.0 here had the chips riding
    // ~2px high against the tail buttons.
    const TAB_STRIP_HEIGHT: f32 = style::TEXT_BODY * 1.3 + 12.0;
    let mut tabs_only = row![].spacing(2).align_y(Alignment::Center);
    for tab in &available {
        let label = match tab {
            Tab::Cover => t!(lang, "save-tab-cover"),
            Tab::Navi => t!(lang, "save-tab-navi"),
            Tab::Folder => t!(lang, "save-tab-folder"),
            Tab::PatchCards => t!(lang, "save-tab-patch-cards"),
            Tab::AutoBattleData => t!(lang, "save-tab-auto-battle-data"),
        };
        tabs_only = tabs_only.push(widgets::tab_button(
            tab_icon(*tab),
            label,
            Action::SelectTab(*tab),
            *tab == active,
        ));
    }
    let tabs_only = tabs_only.wrap();
    // The whole tail morphs as ONE unit between its two sides —
    // (extras + Edit + Play) and (Save / Cancel) — so entering or
    // leaving edit mode dissolves everything together. Animating
    // only the extras left the Play button popping out instantly
    // and the exiting controls shifting into its freed space.
    let mut side = row![].spacing(6).align_y(Alignment::Center);
    if render_edit_buttons {
        // Save / Cancel are keyed on the mode, not the active
        // sub-tab — they stay planted while the user flips
        // between editor tabs.
        if inline_actions {
            side = side.push(edit_buttons(lang, loaded));
        }
    } else {
        if inline_actions {
            // Per-control entrances: a control carried over from
            // the previous sub-tab (the copy button lives on most
            // tabs) stays anchored in the strip; only controls
            // that actually appeared slide in. Suppressed while
            // the edit-mode morph runs — the whole side is moving
            // then.
            let prev_kinds = state.prev_tab.map(|p| extra_kinds(p, loaded)).unwrap_or_default();
            for kind in extra_kinds(active, loaded) {
                let el = render_extra(lang, state, active, kind);
                let carried = enter_from.x != 0.0 && prev_kinds.contains(&kind);
                let el = if edit_swap.is_some() || carried {
                    el
                } else {
                    extras_entered(el)
                };
                side = side.push(el);
            }
            if tab_has_edit(active, loaded, editable) {
                let edit_btn: Element<'a, Action> = widgets::labeled_icon_button(
                    lucide_icons::Icon::Pencil,
                    t!(lang, "save-edit"),
                    Action::EnterEdit,
                    [4.0, 10.0],
                    widgets::neutral,
                );
                // If the previous tab had the Edit affordance too, the
                // button never left the strip — re-animating it would
                // read as a glitch. Only sub-tab switches (horizontal
                // enters) can carry it over; vertical enters are
                // whole-body swaps where everything is new.
                let carried_over =
                    enter_from.x != 0.0 && state.prev_tab.map_or(false, |p| tab_has_edit(p, loaded, editable));
                let el = if edit_swap.is_some() || carried_over {
                    edit_btn
                } else {
                    extras_entered(edit_btn)
                };
                side = side.push(el);
            }
        }
        if let Some(enabled) = play_button {
            use lucide_icons::Icon;
            let label = row![Icon::Play.widget(), text(t!(lang, "play-play"))]
                .spacing(6)
                .align_y(Alignment::Center);
            let mut btn = button(label).padding([4, 10]);
            if enabled {
                btn = btn.style(widgets::primary_button).on_press(Action::PlayClicked);
            } else {
                btn = btn.style(widgets::neutral);
            }
            side = side.push(btn);
        }
    }
    let mut tail: Element<'a, Action> = side.into();
    if let Some(phase) = edit_swap {
        tail = crate::anim::swap_transform(tail, phase, iced::Vector::new(32.0, 0.0), crate::widgets::plate_color);
    }
    let tab_row = row![
        container(tabs_only).width(Fill),
        container(tail)
            .height(Length::Fixed(TAB_STRIP_HEIGHT))
            .align_y(Alignment::Center),
    ]
    .spacing(8)
    .align_y(Alignment::Start);

    let tab_pane = container(tab_row.padding([4, 8])).width(Fill).style(widgets::pane);

    // The folder editor lays out two side-by-side panes, each with its
    // own scrollbar, and wants the full available height — so it bypasses
    // the shared Shrink-height body scrollable the read-only views use.
    if folder_editing && active == Tab::Folder {
        let editor = folder::render_folder_edit(lang, loaded, state);
        return column![tab_pane, entered(editor)]
            .spacing(style::PANE_GAP)
            .width(Fill)
            .height(Fill)
            .into();
    }
    if navicust_editing && active == Tab::Navi {
        let editor = navicust::render_navicust_edit(lang, loaded, state);
        return column![tab_pane, entered(editor)]
            .spacing(style::PANE_GAP)
            .width(Fill)
            .height(Fill)
            .into();
    }
    if patch_cards_editing && active == Tab::PatchCards {
        let editor = patch_cards::render_patch_cards_edit(lang, loaded, state);
        return column![tab_pane, entered(editor)]
            .spacing(style::PANE_GAP)
            .width(Fill)
            .height(Fill)
            .into();
    }
    if auto_battle_data_editing && active == Tab::AutoBattleData {
        let editor = abd::render_auto_battle_data_edit(lang, loaded, state);
        return column![tab_pane, entered(editor)]
            .spacing(style::PANE_GAP)
            .width(Fill)
            .height(Fill)
            .into();
    }

    // The Cover tab is a single full-height pane (logo banner), so it skips
    // the shrink-height body scrollable the other read-only views use.
    if active == Tab::Cover {
        let cover = render_cover::<Action>(lang, loaded);
        return column![tab_pane, entered(cover)]
            .spacing(style::PANE_GAP)
            .width(Fill)
            .height(Fill)
            .into();
    }

    let opts = RenderOpts {
        folder_grouped: state.folder_grouped,
    };
    let body = render::<Action>(lang, active, loaded, opts);
    // Body: each render_* returns one-or-more pane-styled
    // containers stacked into an Element. We wrap that whole
    // group in a Shrink-height scrollable so when its panes don't
    // fill the available space the column hugs them, and when
    // they do the user can scroll past the visible window. The
    // per-instance id is what [`State::apply`] snaps to the top
    // on tab changes.
    let body_scrollable = scrollable(body)
        .id(state.body_scroll_id.clone())
        .style(crate::widgets::chunky_scrollable)
        .width(Fill);
    column![tab_pane, entered(body_scrollable.into())]
        .spacing(style::PANE_GAP)
        .width(Fill)
        .into()
}

/// The global edit mode's Save / Cancel pair, shown in the tab
/// strip's tail while edit mode is on (or sliding out). One pair
/// for the whole save: they commit / discard the edits on *all*
/// tabs at once. Save is gated on a legal folder when chips are
/// editable — a full 30 chips with no folder-limit violations (an
/// incomplete or over-limit folder can't be written over the
/// save); navicust / patch-card layouts are always valid to write.
fn edit_buttons<'a>(lang: &'a LanguageIdentifier, loaded: &'a Loaded) -> Element<'a, Action> {
    use crate::widgets;
    use lucide_icons::Icon;
    let can_save = !loaded.chips_editable || {
        let full = loaded.save.view_chips().map_or(true, |v| {
            let folder = v.equipped_folder_index();
            (0..folder::MAX_FOLDER_CHIPS).all(|i| v.chip(folder, i).is_some())
        });
        full && folder::folder_limits_satisfied(loaded)
    };
    row![
        widgets::labeled_icon_button(
            Icon::X,
            t!(lang, "save-edit-cancel"),
            Action::CancelEdit,
            [4.0, 10.0],
            widgets::neutral,
        ),
        widgets::labeled_icon_button_maybe(
            Icon::Check,
            t!(lang, "save-edit-save"),
            can_save.then_some(Action::SaveEdit),
            [4.0, 10.0],
            widgets::primary_button,
        ),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .into()
}

/// Whether `tab` offers the Edit affordance for this save. Split
/// from [`tab_extras`] so the view can keep the Edit button
/// unanimated when a sub-tab switch carries it over.
fn tab_has_edit(tab: Tab, loaded: &Loaded, editable: bool) -> bool {
    editable
        && match tab {
            // Only saves with a writable chip view (BN4/5/6);
            // `chips_editable` is the cached `view_chips_mut()`
            // probe.
            Tab::Folder => loaded.chips_editable,
            // Only BN4/5/6 (writable navicust) — and only saves
            // that actually have a navicust grid (LinkNavi BN4.5
            // navis have nothing to edit).
            Tab::Navi => {
                loaded.navicust_editable
                    && matches!(
                        loaded.save.view_navi(),
                        Some(tango_dataview::save::NaviView::Navicust(_))
                    )
            }
            // BN4 (PatchCard4s) and BN5/BN6 (PatchCard56s) are
            // both writable, each via its own editor.
            Tab::PatchCards => loaded.patch_cards_editable,
            // Only BN4/BN5 (writable auto-battle data).
            Tab::AutoBattleData => loaded.auto_battle_data_editable,
            _ => false,
        }
}

/// One control in the tab strip's tail. Identified per-kind (not
/// per-row) so the view can keep a control that exists on both
/// the previous and current sub-tab anchored in place instead of
/// re-animating it — the copy button lives on most tabs and only
/// its target changes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExtraKind {
    /// Folder-only: the group-by-identity toggle.
    FolderGroup,
    /// Navi-only, and only for saves with an actual navicust grid
    /// (LinkNavi BN4.5 navis have nothing to render): copy the
    /// grid as an image.
    CopyImage,
    /// Copy the tab as text — present on every tab except Cover.
    Copy,
}

/// The tail controls `tab` shows, in display order. The Edit
/// affordance is tracked separately — see [`tab_has_edit`].
fn extra_kinds(tab: Tab, loaded: &Loaded) -> Vec<ExtraKind> {
    match tab {
        Tab::Folder => vec![ExtraKind::FolderGroup, ExtraKind::Copy],
        Tab::Navi => {
            let has_navicust = matches!(
                loaded.save.view_navi(),
                Some(tango_dataview::save::NaviView::Navicust(_))
            );
            if has_navicust {
                vec![ExtraKind::CopyImage, ExtraKind::Copy]
            } else {
                vec![ExtraKind::Copy]
            }
        }
        Tab::PatchCards | Tab::AutoBattleData => vec![ExtraKind::Copy],
        Tab::Cover => vec![],
    }
}

/// Stable copy-feedback key for a tab's copy buttons — shared between
/// the view (which renders the "Copied!" flash) and the host tabs'
/// update paths (which fire it once the copy actually lands on the
/// clipboard). See [`crate::copy_feedback`].
pub fn copy_flash_key(tab: Tab, image: bool) -> String {
    format!("save-view-copy-{}-{}", if image { "image" } else { "text" }, tab as u8)
}

/// Build one tail control. `tab` parameterizes the copy actions'
/// target.
fn render_extra<'a>(lang: &'a LanguageIdentifier, state: &'a State, tab: Tab, kind: ExtraKind) -> Element<'a, Action> {
    use crate::widgets;
    use lucide_icons::Icon;
    match kind {
        ExtraKind::FolderGroup => iced::widget::checkbox(state.folder_grouped)
            .label(t!(lang, "folder-group"))
            .on_toggle(Action::ToggleFolderGrouped)
            .size(TEXT_BODY)
            .text_size(12)
            .style(crate::widgets::chunky_checkbox)
            .into(),
        ExtraKind::CopyImage => widgets::copy_icon_button(
            &copy_flash_key(tab, true),
            Icon::ImageDown,
            TEXT_BODY,
            t!(lang, "save-copy-image"),
            t!(lang, "copied"),
            Some(Action::CopyTabImage(tab)),
            [4.0, 10.0],
        ),
        ExtraKind::Copy => widgets::copy_icon_button(
            &copy_flash_key(tab, false),
            Icon::ClipboardCopy,
            TEXT_BODY,
            t!(lang, "save-copy"),
            t!(lang, "copied"),
            Some(Action::CopyTab(tab)),
            [4.0, 10.0],
        ),
    }
}

/// Plain-text representation of the active save-view tab, for the
/// clipboard. `None` = tab not exportable in this form. The Folder
/// branch honors `opts.folder_grouped`, mirroring [`render_folder`]'s
/// collapsed-by-identity layout so the clipboard matches what the
/// user sees.
pub fn tab_as_text(_lang: &LanguageIdentifier, tab: Tab, loaded: &Loaded, opts: RenderOpts) -> Option<String> {
    let assets = loaded.assets.as_ref();
    match tab {
        Tab::Folder => {
            let chips_view = loaded.save.view_chips()?;
            let folder_idx = chips_view.equipped_folder_index();
            // Read-only display treats "unsupported" and "unset" the
            // same — flatten the outer Option away.
            let regular_idx = chips_view.regular_chip_index(folder_idx).flatten();
            let tag_idxs = chips_view.tag_chip_indexes(folder_idx).flatten();

            let mut chips: Vec<Option<tango_dataview::save::Chip>> =
                (0..folder::MAX_FOLDER_CHIPS).map(|i| chips_view.chip(folder_idx, i)).collect();
            let regular_display_idx = if !assets.regular_chip_is_in_place() {
                if let Some(ri) = regular_idx {
                    let c = chips.remove(0);
                    chips.insert(ri, c);
                    Some(ri)
                } else {
                    None
                }
            } else {
                regular_idx
            };

            let mut out = String::new();
            if opts.folder_grouped {
                let mut grouped_map: indexmap::IndexMap<Option<tango_dataview::save::Chip>, folder::GroupedChip> =
                    indexmap::IndexMap::new();
                for (i, chip) in chips.iter().enumerate() {
                    let g = grouped_map.entry(chip.clone()).or_default();
                    g.count += 1;
                    if regular_display_idx == Some(i) {
                        g.is_regular = true;
                    }
                    if let Some(t) = tag_idxs {
                        if t[0] == i {
                            g.has_tag1 = true;
                        }
                        if t[1] == i {
                            g.has_tag2 = true;
                        }
                    }
                }
                for (chip, g) in &grouped_map {
                    let Some(c) = chip else {
                        out.push_str(&format!("{}\t---\n", g.count));
                        continue;
                    };
                    let name = assets
                        .chip(c.id)
                        .and_then(|info| info.name())
                        .unwrap_or_else(|| "???".to_string());
                    out.push_str(&format!("{}\t{name}\t{}", g.count, c.code));
                    if g.is_regular {
                        out.push_str("\t[REG]");
                    }
                    for _ in 0..(g.has_tag1 as usize + g.has_tag2 as usize) {
                        out.push_str("\t[TAG]");
                    }
                    out.push('\n');
                }
            } else {
                for (i, chip) in chips.iter().enumerate() {
                    let Some(c) = chip else {
                        out.push_str("---\n");
                        continue;
                    };
                    let name = assets
                        .chip(c.id)
                        .and_then(|info| info.name())
                        .unwrap_or_else(|| "???".to_string());
                    out.push_str(&format!("{name}\t{}", c.code));
                    if regular_display_idx == Some(i) {
                        out.push_str("\t[REG]");
                    }
                    if let Some(ti) = tag_idxs {
                        if ti.contains(&i) {
                            out.push_str("\t[TAG]");
                        }
                    }
                    out.push('\n');
                }
            }
            Some(out)
        }
        Tab::PatchCards => {
            let view = loaded.save.view_patch_cards()?;
            let mut out = String::new();
            match view {
                tango_dataview::save::PatchCardsView::PatchCard56s(v) => {
                    for i in 0..v.count() {
                        let Some(card) = v.patch_card(i) else { continue };
                        let info = assets.patch_card56(card.id);
                        let name = info
                            .as_ref()
                            .and_then(|c| c.name())
                            .unwrap_or_else(|| format!("#{}", card.id));
                        let mb = info.as_ref().map(|c| c.mb()).unwrap_or(0);
                        out.push_str(&format!(
                            "{name}\t{mb}MB\t{}\n",
                            if card.enabled { "ON" } else { "off" }
                        ));
                    }
                }
                tango_dataview::save::PatchCardsView::PatchCard4s(v) => {
                    for i in 0..6 {
                        let Some(card) = v.patch_card(i) else { continue };
                        let info = assets.patch_card4(card.id);
                        let name = info
                            .as_ref()
                            .and_then(|c| c.name())
                            .unwrap_or_else(|| format!("#{}", card.id));
                        out.push_str(&format!(
                            "0{}\t{name}\t{}\n",
                            ['A', 'B', 'C', 'D', 'E', 'F'][i],
                            if card.enabled { "ON" } else { "off" }
                        ));
                    }
                }
            }
            Some(out)
        }
        Tab::AutoBattleData => {
            let view = loaded.save.view_auto_battle_data()?;
            let mat = view.materialized();
            let chip_name = |id: Option<usize>| match id {
                Some(id) => assets
                    .chip(id)
                    .and_then(|c| c.name())
                    .unwrap_or_else(|| format!("#{id}")),
                None => "—".to_string(),
            };
            let mut out = String::new();
            let mut section = |title: &str, ids: &[Option<usize>]| {
                out.push_str(&format!("[{title}]\n"));
                for id in ids {
                    out.push_str(&chip_name(*id));
                    out.push('\n');
                }
                out.push('\n');
            };
            section("Secondary standard", mat.secondary_standard_chips());
            section("Standard", mat.standard_chips());
            section("Mega", mat.mega_chips());
            section("Giga", &[mat.giga_chip()]);
            section("Combos", mat.combos());
            section("Program advance", &[mat.program_advance()]);
            Some(out)
        }
        Tab::Navi => {
            let view = loaded.save.view_navi()?;
            let mut out = String::new();
            match view {
                tango_dataview::save::NaviView::LinkNavi(v) => {
                    let id = v.navi();
                    let name = assets
                        .navi(id)
                        .and_then(|n| n.name())
                        .unwrap_or_else(|| format!("#{id}"));
                    out.push_str(&format!("{name}\n"));
                }
                tango_dataview::save::NaviView::Navicust(v) => {
                    // Style name first (BN3 only), then two TSV
                    // columns: solid parts on the left, plus parts on
                    // the right, lined up row-by-row to match the
                    // side-by-side layout the UI renders. Shorter
                    // column gets blank cells; the trailing tab keeps
                    // a paste into Google Sheets / Excel parsing as
                    // two columns even when the last solid row has
                    // no plus partner.
                    if let Some(style_id) = v.style() {
                        if let Some(name) = assets.style(style_id).and_then(|s| s.name()) {
                            out.push_str(&name);
                            out.push('\n');
                        }
                    }
                    let mut solid = Vec::new();
                    let mut plus = Vec::new();
                    for i in 0..v.count() {
                        let Some(part) = v.navicust_part(i) else {
                            continue;
                        };
                        let Some(info) = assets.navicust_part(part.id) else {
                            continue;
                        };
                        let name = info.name().unwrap_or_else(|| format!("#{}", part.id));
                        if info.is_solid() {
                            solid.push(name);
                        } else {
                            plus.push(name);
                        }
                    }
                    for i in 0..solid.len().max(plus.len()) {
                        let s = solid.get(i).map(String::as_str).unwrap_or("");
                        let p = plus.get(i).map(String::as_str).unwrap_or("");
                        out.push_str(s);
                        out.push('\t');
                        out.push_str(p);
                        out.push('\n');
                    }
                }
            }
            Some(out)
        }
        Tab::Cover => None,
    }
}

/// Render a save-view tab to an RGBA image, for clipboard
/// "copy as image". Currently only Navi/Navicust supports this
/// (the rendered grid is already an image; we just hand back a
/// fresh copy). Returns `None` for tabs without a meaningful
/// image representation.
pub fn tab_as_image(tab: Tab, loaded: &Loaded) -> Option<image::RgbaImage> {
    let nv = loaded.save.view_navi()?;
    let v = match nv {
        tango_dataview::save::NaviView::Navicust(v) => v,
        _ => return None,
    };
    if !matches!(tab, Tab::Navi) {
        return None;
    }
    let layout = loaded.assets.navicust_layout()?;
    let materialized = v.materialized();
    let lang = crate::game::region_to_language(loaded.game.region());
    // Clipboard / export path: render at native (high) resolution.
    Some(navicust::grid::render(
        &materialized,
        &layout,
        v.as_ref(),
        loaded.assets.as_ref(),
        &lang,
        None,
    ))
}

fn render_cover<M: 'static>(_lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    // The cover tab carries no save data of its own — it just shows the
    // game's logo(s), decoded once in Loaded::build. Logos vary in
    // aspect ratio, so each is sized to a fixed height and Contain'd.
    let inner: Element<'static, M> = match loaded.logos.as_slice() {
        // Two variant logos in the family (e.g. Gregar/Falzar) — stack
        // them vertically with a left/right stagger, the way the Legacy
        // Collection lays out twin-version covers: the loaded variant
        // (logos[0]) sits up and to the left, its sibling down and to
        // the right.
        [top_logo, bottom_logo, ..] => {
            const H: f32 = 140.0;
            const STAGGER: f32 = 64.0;
            // Each logo sized to its own aspect ratio at height H (so
            // neither gets squished), returned alongside its width.
            let sized = |&(w, h, ref handle): &(u32, u32, iced_image::Handle)| -> (f32, Element<'static, M>) {
                let disp_w = H * (w as f32) / (h as f32);
                (
                    disp_w,
                    Image::new(handle.clone())
                        .content_fit(ContentFit::Contain)
                        .width(Length::Fixed(disp_w))
                        .height(Length::Fixed(H))
                        .into(),
                )
            };
            let (top_w, top_img) = sized(top_logo);
            let (bottom_w, bottom_img) = sized(bottom_logo);
            // Shared lane width so the pair centers as a unit: the top
            // logo hugs the lane's left edge, the bottom logo its right
            // edge, leaving STAGGER of diagonal offset between them.
            let lane = top_w.max(bottom_w) + STAGGER;
            let top = container(top_img)
                .width(Length::Fixed(lane))
                .align_x(iced::alignment::Horizontal::Left);
            let bottom = container(bottom_img)
                .width(Length::Fixed(lane))
                .align_x(iced::alignment::Horizontal::Right);
            column![top, bottom].spacing(20).into()
        }
        // Single logo — centered banner.
        [(_, _, handle), ..] => Image::new(handle.clone())
            .content_fit(ContentFit::Contain)
            .width(Fill)
            .height(Length::Fixed(220.0))
            .into(),
        // No registered logo — render an empty cover.
        [] => Space::new().into(),
    };
    container(column![inner].width(Fill).align_x(Alignment::Center))
        .width(Fill)
        // Fill the tab body's height, with the logo(s) centered vertically.
        .height(Fill)
        .align_y(iced::alignment::Vertical::Center)
        // Extra breathing room above/below the logo(s); standard
        // horizontal inset.
        .padding([crate::style::PANE_PADDING + 24.0, crate::style::PANE_PADDING + 24.0])
        .style(crate::widgets::pane)
        .into()
}



/// 28×28 chip icon. Empty (`None`) renders a same-sized spacer so empty
/// rows keep the same height as filled ones.
fn chip_icon<'a>(loaded: &'a Loaded, chip_id: Option<usize>) -> Element<'a, Action> {
    match chip_id.and_then(|id| loaded.chip_icons.get(id).cloned().flatten()) {
        Some(h) => Image::new(h)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .filter_method(iced_image::FilterMethod::Nearest)
            .content_fit(ContentFit::Contain)
            .into(),
        None => Space::new()
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .into(),
    }
}

/// The "✕" button that removes a chip / patch-card from its slot, backing the
/// row out to the library. Identical across the folder and patch-card editors
/// apart from the action it fires.
fn remove_button<'a>(action: Action) -> Element<'a, Action> {
    button(lucide_icons::Icon::X.widget().size(TEXT_BODY))
        .padding([3, 8])
        .style(crate::widgets::neutral)
        .on_press(action)
        .into()
}

/// Build the chip popover — scaled artwork above its description — and wrap
/// `inner` with it as a follow-cursor tooltip. Returns `inner` unchanged when
/// the chip has neither artwork nor a description. `accent` tints the popover
/// background to match the chip's class stripe.
fn chip_popover<'a, M: 'a>(
    inner: Element<'a, M>,
    image_handle: Option<(u32, u32, iced_image::Handle)>,
    description: Option<String>,
    accent: Option<iced::Color>,
) -> Element<'a, M> {
    if description.is_none() && image_handle.is_none() {
        return inner;
    }
    let mut tip = column![].spacing(6);
    if let Some((w, h, h_handle)) = image_handle {
        tip = tip.push(
            Image::new(h_handle)
                .width(Length::Fixed(w as f32 * 2.0))
                .height(Length::Fixed(h as f32 * 2.0))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain),
        );
    }
    if let Some(desc) = description {
        tip = tip.push(text(desc).size(TEXT_CAPTION));
    }
    tooltip(
        inner,
        container(tip).padding(8).style(chip_tooltip_style(accent)),
        tooltip::Position::FollowCursor,
    )
    .gap(8)
    .into()
}

/// Wrap `inner` so hovering anywhere over it shows the chip's full image
/// + description (the read-only list's chip popover). No-op when the
/// chip has neither, or for an empty slot.
fn with_chip_tooltip<'a>(
    loaded: &'a Loaded,
    chip_id: Option<usize>,
    accent: Option<iced::Color>,
    inner: Element<'a, Action>,
) -> Element<'a, Action> {
    let Some(id) = chip_id else { return inner };
    let info = loaded.assets.chip(id);
    let description = info.as_ref().and_then(|i| i.description());
    // Program advances have no meaningful standalone artwork, so their
    // popover is description-only.
    let is_pa = info
        .as_ref()
        .map_or(false, |i| i.class() == tango_dataview::rom::ChipClass::ProgramAdvance);
    let image_handle = if is_pa {
        None
    } else {
        loaded.chip_images.get(id).cloned().flatten()
    };
    chip_popover(inner, image_handle, description, accent)
}

/// Wrap an editor row's content with the class-accent stripe + zebra
/// background, matching the read-only chip list.
fn edit_row_wrap<'a>(
    inner: Element<'a, Action>,
    accent: Option<iced::Color>,
    row_idx: usize,
    leading: Option<Element<'a, Action>>,
) -> Element<'a, Action> {
    let stripe: Element<'a, Action> = container(Space::new())
        .width(Length::Fixed(6.0))
        .height(Length::Fill)
        .style(move |_t: &iced::Theme| container::Style {
            background: accent.map(iced::Background::Color),
            ..Default::default()
        })
        .into();
    // `leading` (e.g. the library's add arrow) sits in the gutter to the
    // left of the accent stripe.
    let mut r = row![].height(Length::Shrink).align_y(Alignment::Center);
    if let Some(lead) = leading {
        r = r.push(container(lead).padding([0, 6]));
    }
    r = r.push(stripe).push(container(inner).width(Fill));
    container(r)
        .width(Fill)
        .style(crate::widgets::zebra_row(row_idx))
        .into()
}

/// Element-icon / ATK / MB stat cells shared by both editor panes,
/// matching the read-only chip list's columns. The MB cell collapses to
/// nothing when the game doesn't use MB.
fn chip_stat_cells<'a>(loaded: &'a Loaded, chip_id: usize, chips_have_mb: bool) -> [Element<'a, Action>; 3] {
    let info = loaded.assets.chip(chip_id);
    let element: Element<'a, Action> = info
        .as_ref()
        .map(|i| i.element())
        .and_then(|id| loaded.element_icons.get(&id).cloned())
        .map(|h| {
            Image::new(h)
                .width(Length::Fixed(28.0))
                .height(Length::Fixed(28.0))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain)
                .into()
        })
        .unwrap_or_else(|| Space::new().width(Length::Fixed(28.0)).into());
    let power = info.as_ref().map(|i| i.attack_power()).unwrap_or(0);
    let mb = info.as_ref().map(|i| i.mb()).unwrap_or(0);
    let atk: Element<'a, Action> =
        container(text(if power > 0 { format!("{power}") } else { String::new() }).size(TEXT_BODY))
            .width(Length::Fixed(46.0))
            .align_x(iced::alignment::Horizontal::Right)
            .into();
    let mb_cell: Element<'a, Action> = if chips_have_mb {
        container(text(if mb > 0 { format!("{mb}MB") } else { String::new() }).size(TEXT_CAPTION))
            .width(Length::Fixed(42.0))
            .align_x(iced::alignment::Horizontal::Right)
            .into()
    } else {
        Space::new().into()
    };
    [element, atk, mb_cell]
}

/// One folder slot in the editor's left pane. Filled slots show the
/// A muted grip glyph marking a row as drag-to-reorder. The whole row is the
/// drag surface (sweeten's `Column` owns the gesture); this is just the visual
/// affordance, in a fixed-width cell so rows line up. Wrapped in a `mouse_area`
/// only to show the grab-hand cursor on hover — it sets no handlers, so it
/// doesn't capture the press (the drag gesture still reaches the column).
fn drag_handle<'a>() -> Element<'a, Action> {
    use lucide_icons::Icon;
    let grip = container(Icon::GripVertical.widget().size(TEXT_BODY).style(muted_text_style))
        .width(Length::Fixed(16.0))
        .align_x(iced::alignment::Horizontal::Center);
    iced::widget::mouse_area(grip)
        .interaction(iced::mouse::Interaction::Grab)
        .into()
}

/// Drag styling shared by the reorderable folder / patch-card columns. The
/// default sweeten style tints the rows that shift aside with the theme's
/// *primary* color (green in this app); we don't want any overlay, so it's
/// turned off. The floating ghost is softened to a plain panel.
fn reorder_drag_style(theme: &iced::Theme) -> sweeten::widget::column::Style {
    let ep = theme.extended_palette();
    let ghost = {
        let mut c = ep.background.weak.color;
        c.a = 0.92;
        c
    };
    sweeten::widget::column::Style {
        scale: 1.02,
        // No tint on the rows that move to open a gap.
        moved_item_overlay: iced::Color::TRANSPARENT,
        ghost_border: iced::Border {
            width: 1.0,
            color: ep.background.strong.color,
            radius: 4.0.into(),
        },
        ghost_background: iced::Background::Color(ghost),
    }
}

/// Small toggle button used for the REG / TAG columns in the folder editor
/// and the patch-card ON column: tinted in `on_color` when active, neutral
/// (greyed) when not. A `None` message renders it disabled (greyed,
/// unclickable) — for the folder REG/TAG toggles when the chip's MB won't
/// fit Regular/Tag memory, or the patch-card toggle when enabling would
/// blow the MB budget.
fn edit_toggle_maybe<'a>(
    label: &'static str,
    on: bool,
    on_color: iced::Color,
    msg: Option<Action>,
) -> Element<'a, Action> {
    let mut b = button(text(label).size(TEXT_CAPTION)).padding([4, 8]);
    if let Some(msg) = msg {
        b = b.on_press(msg);
    }
    if on {
        b.style(move |theme: &iced::Theme, status| crate::widgets::tinted_button(theme, status, on_color))
            .into()
    } else {
        b.style(crate::widgets::neutral).into()
    }
}

// `code = None` skips the code badge (Auto Battle Data slots
// have a chip id but no code). `show_count_cell` toggles the
// leading "N×" column — on for the folder's grouped mode, off
// for ABD.
fn chip_row<M: 'static>(
    loaded: &Loaded,
    chip_id: Option<usize>,
    code: Option<String>,
    g: &folder::GroupedChip,
    show_count_cell: bool,
    chips_have_mb: bool,
    row_idx: usize,
    is_first: bool,
    is_last: bool,
) -> Element<'static, M> {
    let info = chip_id.and_then(|id| loaded.assets.chip(id));
    let chip_class = info.as_ref().map(|i| i.class());
    let dark = info.as_ref().map(|i| i.dark()).unwrap_or(false);
    let accent = class_accent(chip_class, dark);
    let is_empty_slot = chip_id.is_none();

    // Chip icon — in-game sprite at 28 px so it reads as a chip
    // graphic rather than a row decoration. Empty slots reserve the
    // same 28 px square so their rows match filled rows' height.
    let icon: Element<'static, M> = match chip_id.and_then(|id| loaded.chip_icons.get(id).cloned().flatten()) {
        Some(h) => Image::new(h)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .filter_method(iced_image::FilterMethod::Nearest)
            .content_fit(ContentFit::Contain)
            .into(),
        None => Space::new()
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .into(),
    };

    // Element icon. Same 14→28 (2× native) scaling as the chip
    // icon so both sprites read at the same size and stay on an
    // integer multiple of their source — anything else makes
    // cosmic-text's resampler eat the pixel grid.
    let element_id = info.as_ref().map(|i| i.element());
    let element_icon: Element<'static, M> = element_id
        .and_then(|id| loaded.element_icons.get(&id).cloned())
        .map(|h| {
            Image::new(h)
                .width(Length::Fixed(28.0))
                .height(Length::Fixed(28.0))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain)
                .into()
        })
        .unwrap_or_else(|| Space::new().width(Length::Fixed(28.0)).into());

    let name_text = info
        .as_ref()
        .and_then(|i| i.name())
        .unwrap_or_else(|| "???".to_string());
    let power = info.as_ref().map(|i| i.attack_power()).unwrap_or(0);
    let mb = info.as_ref().map(|i| i.mb()).unwrap_or(0);

    // Name only — chip code lives in its own right-aligned
    // column below so every row's letters line up cleanly with
    // the element / power / MB stats.
    let title: Element<'static, M> = if is_empty_slot {
        text("—").size(TEXT_BODY).style(muted_text_style).into()
    } else {
        text(name_text).size(TEXT_BODY).into()
    };

    // REG / TAG indicators sit inline with the title so the
    // row stays single-line and the card height stops growing
    // with metadata.
    let mut indicator_row = row![].spacing(4).align_y(Alignment::Center);
    if g.is_regular {
        indicator_row = indicator_row.push(badge("REG", iced::Color::from_rgb8(0xff, 0x42, 0xa5)));
    }
    // Tag chips come in pairs (tag1 + tag2). For the chip list
    // it's the chip-IS-a-tag-chip status the user cares about,
    // not which slot — collapse both flags into a single "TAG".
    for _ in 0..(g.has_tag1 as usize + g.has_tag2 as usize) {
        indicator_row = indicator_row.push(badge("TAG", iced::Color::from_rgb8(0x29, 0xa1, 0x21)));
    }

    // Right-side stats: fixed-width right-aligned columns so the
    // numbers line up vertically across rows. Both inherit the theme's
    // text color — no hard-coded white/yellow that breaks on light.
    // `code.filter().map(...)` consumes the String into the
    // Text widget so the resulting Element is `'static` (a
    // `&String` borrow would tie the Element to this stack
    // frame). The filter drops empty codes so we don't render a
    // blank fixed-width slot.
    let code_text: Option<Element<'static, M>> = code.filter(|s| !s.is_empty()).map(|code| {
        container(text(code).size(TEXT_BODY).font(iced::Font::MONOSPACE))
            .width(Length::Fixed(22.0))
            .align_x(iced::alignment::Horizontal::Right)
            .into()
    });
    let power_text: Element<'static, M> =
        container(text(if power > 0 { format!("{power}") } else { String::new() }).size(TEXT_BODY))
            .width(Length::Fixed(50.0))
            .align_x(iced::alignment::Horizontal::Right)
            .into();
    let mb_text: Element<'static, M> = if chips_have_mb {
        container(text(if mb > 0 { format!("{mb}MB") } else { String::new() }).size(TEXT_CAPTION))
            .width(Length::Fixed(50.0))
            .align_x(iced::alignment::Horizontal::Right)
            .into()
    } else {
        Space::new().width(Length::Fixed(0.0)).into()
    };

    // Count column on the left for grouped mode. Theme-aware text:
    // full strength for count > 1, muted for count == 1 (since "1×" is
    // visual noise) — both readable on light + dark.
    let mut r = row![].spacing(10).align_y(Alignment::Center);
    if show_count_cell {
        let count_is_one = g.count == 1;
        r = r.push(
            text(format!("{}×", g.count))
                .size(TEXT_BODY)
                .width(Length::Fixed(22.0))
                .style(move |theme: &iced::Theme| iced::widget::text::Style {
                    color: Some(if count_is_one {
                        muted_color(theme)
                    } else {
                        theme.palette().text
                    }),
                }),
        );
    }
    r = r
        .push(icon)
        .push(container(row![title, indicator_row].spacing(8).align_y(Alignment::Center)).width(Length::Fill))
        .push(element_icon);
    if let Some(code_text) = code_text {
        r = r.push(code_text);
    }
    r = r.push(power_text).push(mb_text);

    let card = card_wrap(r.padding([3, 12]).into(), accent, row_idx, is_first, is_last);
    // Hover tooltip with chip image preview + description.
    // Always rendered when the chip has either; the folder list
    // and Auto Battle Data both want this affordance, so it
    // lives at the bottom of chip_row instead of as a wrapper
    // the callers have to remember to use.
    let Some(id) = chip_id else {
        return card;
    };
    let description = loaded.assets.chip(id).and_then(|info| info.description());
    // Program advances show description only — no standalone chip image.
    let image_handle = if chip_class == Some(tango_dataview::rom::ChipClass::ProgramAdvance) {
        None
    } else {
        loaded.chip_images.get(id).cloned().flatten()
    };
    chip_popover(card, image_handle, description, accent)
}

/// Tooltip chrome for chip hovers — same shape as
/// [`tooltip_style`] but takes the chip's class accent so
/// mega / giga / dark chips get a background that matches the
/// row's left-edge stripe. Standard chips (accent = None) fall
/// back to the default near-black tooltip.
fn chip_tooltip_style(accent: Option<iced::Color>) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme: &iced::Theme| {
        let bg = accent.unwrap_or_else(|| iced::Color::from_rgba8(0, 0, 0, 0.85));
        container::Style {
            background: Some(iced::Background::Color(bg)),
            text_color: Some(iced::Color::WHITE),
            border: iced::Border {
                radius: 4.0.into(),
                width: 1.0,
                color: iced::Color::from_rgba8(255, 255, 255, 0.2),
            },
            ..Default::default()
        }
    }
}

/// Wraps the inner row content with a 4 px colored stripe on the
/// left for mega/giga/dark chip class accents. The outer container
/// carries the standard zebra-row style so every chip row matches
/// the patch-card / ABD / settings-bindings tables visually; the
/// accent strip sits as a sibling element on the left and paints
/// over the zebra wash where present. Rows without an accent
/// reserve the same 6 px gutter so columns line up across rows.
fn card_wrap<M: 'static>(
    inner: Element<'static, M>,
    accent: Option<iced::Color>,
    row_idx: usize,
    is_first: bool,
    is_last: bool,
) -> Element<'static, M> {
    // Match the pane's `radius: 4.0` on edge rows so the strip's solid
    // accent and the zebra wash don't paint into the pane's rounded
    // corners. The strip only ever touches the left edge, so just the
    // top-left / bottom-left corners need rounding there.
    let r = 4.0_f32;
    let mut strip_radius = iced::border::Radius::new(0.0);
    if is_first {
        strip_radius = strip_radius.top_left(r);
    }
    if is_last {
        strip_radius = strip_radius.bottom_left(r);
    }
    let mut outer_radius = iced::border::Radius::new(0.0);
    if is_first {
        outer_radius = outer_radius.top(r);
    }
    if is_last {
        outer_radius = outer_radius.bottom(r);
    }
    let strip: Element<'static, M> = container(iced::widget::Space::new())
        .width(Length::Fixed(6.0))
        .height(Length::Fill)
        .style(move |_theme: &iced::Theme| container::Style {
            background: accent.map(iced::Background::Color),
            border: iced::Border {
                radius: strip_radius,
                ..Default::default()
            },
            ..Default::default()
        })
        .into();
    let body: Element<'static, M> = container(inner).width(Fill).into();
    container(row![strip, body].height(Length::Shrink))
        .width(Fill)
        .style(move |theme: &iced::Theme| {
            let mut s = crate::widgets::zebra_row(row_idx)(theme);
            s.border.radius = outer_radius;
            s
        })
        .into()
}

/// Accent color for the left edge of a chip row. None = no accent (the
/// row reads as a default chip with no class adornment).
fn class_accent(class: Option<tango_dataview::rom::ChipClass>, dark: bool) -> Option<iced::Color> {
    if dark {
        return Some(iced::Color::from_rgb8(0x4a, 0x55, 0x82));
    }
    match class {
        Some(tango_dataview::rom::ChipClass::Mega) => Some(iced::Color::from_rgb8(0x52, 0x84, 0x9c)),
        Some(tango_dataview::rom::ChipClass::Giga) => Some(iced::Color::from_rgb8(0xc4, 0x52, 0x84)),
        _ => None,
    }
}

fn badge<M: 'static>(label: &'static str, color: iced::Color) -> Element<'static, M> {
    container(text(label).size(10).color(iced::Color::WHITE))
        .padding([1, 4])
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(color)),
            border: iced::Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn colored_badge<M: 'static>(label: String, bg: iced::Color, text_color: iced::Color) -> Element<'static, M> {
    // Same dimensions as the NaviCust parts badges so the
    // patch-card effect chips and the NCP parts read as
    // family — chunkier than a chrome chip but smaller than a
    // CTA button.
    colored_badge_sized(label, bg, text_color, TEXT_BODY, [3.0, 8.0])
}

/// Variant that lets callers (NCP parts list) pick a larger text size
/// when the badge is being used as primary content rather than chrome.
fn colored_badge_sized<M: 'static>(
    label: String,
    bg: iced::Color,
    text_color: iced::Color,
    size: f32,
    padding: [f32; 2],
) -> Element<'static, M> {
    container(text(label).size(size).color(text_color))
        .padding(padding)
        .style(move |_theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(bg)),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn effect_badge<M: 'static>(e: &tango_dataview::rom::PatchCard56Effect, enabled: bool) -> Element<'static, M> {
    let name = e.name.clone().unwrap_or_else(|| "???".to_string());
    let bg = if enabled {
        if e.is_debuff {
            iced::Color::from_rgb8(0xb5, 0x5a, 0xde)
        } else {
            iced::Color::from_rgb8(0xff, 0xbd, 0x18)
        }
    } else {
        iced::Color::from_rgb8(0xbd, 0xbd, 0xbd)
    };
    colored_badge(name, bg, iced::Color::BLACK)
}

// muted_color / muted_text_style / success_text_style /
// danger_text_style now live in `crate::widgets`. Kept here as
// nothing — every call site outside this module reaches the
// widgets module directly.

fn tooltip_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba8(0, 0, 0, 0.85))),
        text_color: Some(iced::Color::WHITE),
        border: iced::Border {
            radius: 4.0.into(),
            width: 1.0,
            color: iced::Color::from_rgba8(255, 255, 255, 0.2),
        },
        ..Default::default()
    }
}

fn placeholder<M: 'static>(msg: String) -> Element<'static, M> {
    // Centered icon-over-message card rather than a bare line of
    // text in the pane corner — the empty state is a whole-pane
    // situation, so let it own the pane like one.
    container(
        column![
            lucide_icons::Icon::FileQuestion
                .widget()
                .size(36.0)
                .style(muted_text_style),
            text(msg).size(crate::style::TEXT_HEADING).style(muted_text_style),
        ]
        .spacing(8)
        .align_x(Alignment::Center),
    )
    .width(Fill)
    .align_x(Alignment::Center)
    .padding(32)
    .style(crate::widgets::pane)
    .into()
}

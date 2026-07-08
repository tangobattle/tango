use crate::i18n::t;
use crate::selection::Loaded;
use crate::style::{self, TEXT_BODY, TEXT_CAPTION};
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
mod cover;
pub(crate) mod folder;
mod navi;
pub mod navicust;
pub(crate) mod patch_cards;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Cover,
    Navi,
    Navicust,
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

/// Left edge fade for the scrollable sub-tab strip: opaque pane plate at the
/// left edge dissolving to transparent inward, so a tab scrolled past the
/// start reads as "more to the left". Pure presentation — no event handlers,
/// so clicks and wheel-scroll fall through to the strip beneath.
fn tab_fade_left(theme: &iced::Theme) -> container::Style {
    edge_fade(theme, std::f32::consts::FRAC_PI_2)
}

/// Right edge fade for the scrollable sub-tab strip. It sits over the empty
/// plate when the tabs fit, so it stays invisible until tabs reach the edge.
fn tab_fade_right(theme: &iced::Theme) -> container::Style {
    edge_fade(theme, 3.0 * std::f32::consts::FRAC_PI_2)
}

/// A one-sided fade from the pane plate (at the `angle`-direction edge) to
/// transparent, for the tab strip's scroll-edge fades.
fn edge_fade(theme: &iced::Theme, angle: f32) -> container::Style {
    let plate = crate::widgets::plate_color(theme);
    let transparent = iced::Color { a: 0.0, ..plate };
    container::Style {
        background: Some(iced::Background::Gradient(iced::Gradient::Linear(
            iced::gradient::Linear::new(angle)
                .add_stop(0.0, plate)
                .add_stop(1.0, transparent),
        ))),
        ..Default::default()
    }
}

/// Scrollbar style for the sub-tab strip: invisible at rest (the edge fades are
/// the resting affordance) and revealing the standard chunky scrollbar only
/// while the strip is hovered or dragged.
fn tab_scrollbar(theme: &iced::Theme, status: iced::widget::scrollable::Status) -> iced::widget::scrollable::Style {
    let mut style = crate::widgets::chunky_scrollable(theme, status);
    if matches!(status, iced::widget::scrollable::Status::Active { .. }) {
        for rail in [&mut style.horizontal_rail, &mut style.vertical_rail] {
            rail.background = None;
            rail.scroller.background = iced::Background::Color(iced::Color::TRANSPARENT);
        }
    }
    style
}

pub fn available_tabs(save: &dyn Save, streamer_mode: bool) -> Vec<Tab> {
    let mut tabs = vec![];
    if streamer_mode {
        tabs.push(Tab::Cover);
    }
    // The equipped navi (emblem / name / HP / buster) is no longer a tab — it
    // lives in the persistent strip above the body (see [`view`]), so it's
    // always on screen regardless of the active section.
    if save.view_navicust().is_some() {
        tabs.push(Tab::Navicust);
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
        Tab::Cover => cover::render_cover(lang, loaded),
        Tab::Navi => navi::render_navi(lang, loaded),
        Tab::Navicust => navicust::render_navicust_tab(lang, loaded),
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
        Tab::Navicust => Icon::Puzzle,
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
    /// Id of the sub-tab strip scrollable, so [`Action::SelectTab`] can
    /// `snap_to` it (resetting its horizontal scroll to the start, in lockstep
    /// with the [`tab_scroll`] fade mirror) the same way it resets the body.
    tab_scroll_id: iced::widget::Id,
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
    /// Show/hide transition for the navi picker over the [tab strip + body]
    /// region. Driven in lockstep with `active_tab == Some(Tab::Navi)` as the
    /// change-navi card toggles it; the incoming side (the picker on open, the
    /// tab strip + body on dismiss) slides up into place — a plain vertical
    /// slide, matching every other screen/tab transition (no fade).
    pub navi_select: crate::anim::Transition,
    /// Horizontal scroll offset of the sub-tab strip (relative, 0..=1). Tracked
    /// so the strip's edge fades only appear on the side that has hidden tabs.
    tab_scroll: f32,
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
            tab_scroll_id: iced::widget::Id::unique(),
            editing: None,
            library_sort: folder::LibrarySort::Id,
            navicust_sort: navicust::NavicustSort::Id,
            patch_card56_sort: patch_cards::PatchCard56Sort::Id,
            auto_battle_data_sort: abd::AutoBattleDataSort::Id,
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
    pub fn enter_edit(&mut self, loaded: &Loaded) {
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
                // The navi picker is reached by parking `active_tab` on the
                // tab-less `Tab::Navi`; once editing ends there's nothing to
                // show for it, so fall back to the default tab.
                if self.active_tab == Some(Tab::Navi) {
                    self.active_tab = None;
                }
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
            // Toggle the navi picker. Opening points the body at it (the picker
            // slides up while the tab content drops); clicking the card again
            // while it's open closes it, dropping back to the tab the user came
            // from. The host opens the edit session on the way in — it needs
            // `&Loaded` to seed tag state, same as `EnterEdit`.
            Action::EnterEditNavi => {
                let now = iced::time::Instant::now();
                if self.active_tab == Some(Tab::Navi) {
                    self.active_tab = self.prev_tab;
                    self.navi_select.set(false, now);
                } else {
                    self.prev_tab = self.active_tab;
                    self.active_tab = Some(Tab::Navi);
                    self.navi_select.set(true, now);
                }
                iced::Task::none()
            }
            // Picking a navi closes the picker: drop back to the tab the user
            // was on when they opened it (still inside the edit session), the
            // picker dropping away while the tab strip + body rise back in. The
            // host stages the chosen navi via its own Effect.
            Action::SetNavi(_) => {
                if self.active_tab == Some(Tab::Navi) {
                    self.active_tab = self.prev_tab;
                    self.navi_select.set(false, iced::time::Instant::now());
                }
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
    /// somehow isn't already (it needs `&Loaded`, like [`Action::EnterEdit`]).
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
    // ----- Navi editor (only emitted when `editable` is set) -----
    /// Set the equipped navi to this index.
    SetNavi(usize),
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
///
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

    let now = iced::time::Instant::now();
    // Body entrance — restarted on sub-tab switches (sliding along the strip's
    // direction of travel), edit-mode toggles and game/save swaps (rising in
    // vertically).
    let enter = state.enter.progress(now);
    let enter_from = state.enter_from;
    let entered = move |el: Element<'a, Action>| crate::anim::slide_in_opt(el, enter, enter_from);
    // Tab-tail extras animate only when their content actually changed — a
    // sub-tab switch (horizontal enter). A game/save swap rises the body in, but
    // the extras are typically identical across saves, and re-animating them
    // there reads as a glitch.
    let tail_slide = enter.filter(|_| enter_from.x != 0.0);
    let extras_dx = if enter_from.x != 0.0 { enter_from.x } else { 24.0 };
    let extras_entered =
        move |el: Element<'a, Action>| crate::anim::slide_in_opt(el, tail_slide, iced::Vector::new(extras_dx, 0.0));
    // Edit-mode morph: the navi header's Edit / Play and the Save / Cancel pair
    // fade-through swap in both directions, so Edit visibly turns into Save /
    // Cancel and back.
    let (edit_side, edit_swap) = crate::anim::swap_phase(&state.edit_anim, now);
    let render_edit_buttons = editable && edit_side;
    // One global edit session covers the whole save (entered from the Edit button
    // in the navi header); `editing_session` is on whenever it's open and selects
    // each section's editable body below. It also suppresses the Play button
    // (single-player would fight the open session); one Save / Cancel commits /
    // discards every section at once.
    let editing_session = editable && state.editing.is_some();
    // The single Edit button covers the whole save: shown whenever *any* section
    // is editable, not per-tab. Once open, the user navigates tabs to edit each
    // section (and clicks the navi header to swap navi).
    let save_editable = editable && loaded.editability.any();

    // The save-level actions live at the navi header's right edge (not the tab
    // strip): Edit + Play in read mode, Save / Cancel while editing, swapping
    // between the two as one unit.
    let mut actions = row![].spacing(6).align_y(Alignment::Center);
    if render_edit_buttons {
        if inline_actions {
            actions = actions.push(edit_buttons(lang, loaded));
        }
    } else {
        if inline_actions && save_editable {
            actions = actions.push(widgets::labeled_icon_button(
                lucide_icons::Icon::Pencil,
                t!(lang, "save-edit"),
                Action::EnterEdit,
                [4.0, 10.0],
                widgets::neutral,
            ));
        }
        if let Some(enabled) = play_button {
            let label = row![lucide_icons::Icon::Play.widget(), text(t!(lang, "play-play"))]
                .spacing(6)
                .align_y(Alignment::Center);
            let mut btn = button(label).padding([4, 10]);
            if enabled {
                btn = btn.style(widgets::primary_button).on_press(Action::PlayClicked);
            } else {
                btn = btn.style(widgets::neutral);
            }
            actions = actions.push(btn);
        }
    }
    let mut actions_tail: Element<'a, Action> = actions.into();
    if let Some(phase) = edit_swap {
        actions_tail = crate::anim::swap_transform(
            actions_tail,
            phase,
            iced::Vector::new(32.0, 0.0),
            crate::widgets::plate_color,
        );
    }

    // The equipped navi (emblem / name / HP / buster) rides in a slim header
    // strip above the body on every tab — it used to be a tab of its own, but
    // it's a single row of stats, so a tab spent the whole body on it. The
    // save-level actions sit at its right edge. While editing (and the navi is
    // editable) the card itself becomes the change-navi affordance — clicking it
    // opens the picker as the body.
    let navi_edit = (editing_session && loaded.editability.navi).then_some(Action::EnterEditNavi);
    let navi_strip = loaded
        .save
        .view_navi()
        .is_some()
        .then(|| navi::render_navi_strip(lang, loaded, navi_edit, actions_tail));

    if available.is_empty() {
        // No section tabs (an unsupported / empty save) — still surface the navi
        // header (it carries Play) if there's a navi to show.
        let mut col = column![].spacing(style::PANE_GAP).width(Fill);
        if let Some(strip) = navi_strip {
            col = col.push(strip);
        }
        return col.push(placeholder(t!(lang, "save-empty"))).into();
    }
    let active = state
        .active_tab
        .filter(|t| available.contains(t))
        .unwrap_or(available[0]);

    // The global edit session selects each section's editable body below.
    let folder_editing = editing_session && loaded.editability.folder;
    let navicust_editing = editing_session && loaded.editability.navicust;
    let navi_editing = editing_session && loaded.editability.navi;
    let patch_cards_editing = editing_session && loaded.editability.patch_cards;
    let auto_battle_data_editing = editing_session && loaded.editability.auto_battle_data;

    // Tab strip: tabs left, the active tab's contextual extras (copy, folder
    // group toggle, copy-as-image) right — the save-level actions live in the
    // navi header now. We split into two rows so the tab list can wrap/scroll
    // without dragging the extras tail with it. The tail is a separate row,
    // sized to its content and capped to the tab button height so the strip's
    // overall height doesn't grow when the extras change.
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
            Tab::Navicust => t!(lang, "save-tab-navicust"),
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
    // Horizontally scrollable strip with a hidden scrollbar, so a long /
    // localized tab list scrolls instead of wrapping to a second line — the
    // edge fades below are the only scroll affordance.
    let tabs_scroll = scrollable(tabs_only)
        .id(state.tab_scroll_id.clone())
        .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new()))
        .on_scroll(|v| Action::TabScrolled(v.relative_offset().x))
        .style(tab_scrollbar)
        .width(Fill);
    const TAB_FADE_W: f32 = 24.0;
    // Left fade only once scrolled off the start; right fade until the end.
    let left_fade: Element<'a, Action> = if state.tab_scroll > 0.01 {
        container(Space::new())
            .width(Length::Fixed(TAB_FADE_W))
            .height(Fill)
            .style(tab_fade_left)
            .into()
    } else {
        Space::new().into()
    };
    let right_fade: Element<'a, Action> = if state.tab_scroll < 0.99 {
        container(Space::new())
            .width(Length::Fixed(TAB_FADE_W))
            .height(Fill)
            .style(tab_fade_right)
            .into()
    } else {
        Space::new().into()
    };
    let tabs_only = stack![
        tabs_scroll,
        row![left_fade, Space::new().width(Fill), right_fade].height(Fill),
    ];
    // The tail carries only the active tab's contextual extras now — the
    // save-level Edit / Play / Save-Cancel actions moved to the navi header.
    // Extras are read-mode only; entering edit fades them out under the same
    // swap phase the header's morph uses.
    let mut side = row![].spacing(6).align_y(Alignment::Center);
    if !render_edit_buttons && inline_actions {
        // Per-control entrances: a control carried over from the previous
        // sub-tab (the copy button lives on most tabs) stays anchored; only
        // controls that actually appeared slide in. Suppressed while the
        // edit-mode morph runs — the whole tail is moving then.
        let prev_kinds = state.prev_tab.map(|p| extra_kinds(p)).unwrap_or_default();
        for kind in extra_kinds(active) {
            let el = render_extra(lang, state, active, kind);
            let carried = enter_from.x != 0.0 && prev_kinds.contains(&kind);
            let el = if edit_swap.is_some() || carried {
                el
            } else {
                extras_entered(el)
            };
            side = side.push(el);
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

    // The navi picker shows over the [tab strip + body] region, reached via the
    // header's change-navi card (which parks `active_tab` on the tab-less
    // `Tab::Navi`). `navi_select` tracks it while the navi is editable; the
    // incoming side slides up into place on open/dismiss — a plain vertical
    // slide like every tab/screen transition. The equipped navi stays visible in
    // the header above throughout.
    let show_picker = navi_editing && state.navi_select.shown();
    let navi_sliding = navi_editing && state.navi_select.is_animating(now);

    // The region below the header. The navi picker, the in-place editors and the
    // Cover logo banner each claim the full available height; the read-only
    // section views hug their content inside a shrink-height scrollable so a
    // short tab doesn't stretch. `fill` carries that distinction to the column.
    let (region, mut fill): (Element<'a, Action>, bool) = if show_picker {
        (navi::render_navi_edit(lang, loaded), true)
    } else {
        let (body, body_fill): (Element<'a, Action>, bool) = if folder_editing && active == Tab::Folder {
            // The folder editor lays out two side-by-side panes, each with its
            // own scrollbar, and wants the full height — so it bypasses the
            // read-only views' shared shrink-height body scrollable.
            (folder::render_folder_edit(lang, loaded, state), true)
        } else if navicust_editing && active == Tab::Navicust {
            (navicust::render_navicust_edit(lang, loaded, state), true)
        } else if patch_cards_editing && active == Tab::PatchCards {
            (patch_cards::render_patch_cards_edit(lang, loaded, state), true)
        } else if auto_battle_data_editing && active == Tab::AutoBattleData {
            (abd::render_auto_battle_data_edit(lang, loaded, state), true)
        } else if active == Tab::Cover {
            // The Cover tab is a single full-height pane (logo banner).
            (cover::render_cover::<Action>(lang, loaded), true)
        } else {
            let opts = RenderOpts {
                folder_grouped: state.folder_grouped,
            };
            let body = render::<Action>(lang, active, loaded, opts);
            // Each render_* returns one-or-more pane-styled containers stacked
            // into an Element. We wrap that whole group in a shrink-height
            // scrollable so when its panes don't fill the available space the
            // column hugs them, and when they do the user can scroll past the
            // visible window. The per-instance id is what [`State::apply`] snaps
            // to the top on tab changes.
            let body_scrollable = scrollable(body)
                .id(state.body_scroll_id.clone())
                .style(crate::widgets::chunky_scrollable)
                .width(Fill);
            (body_scrollable.into(), false)
        };
        // The whole region (tab strip + body) slides as one while the picker
        // animates; the tab-switch slide is suppressed then (the navi slide owns
        // the motion), and the tab strip rides along instead of popping.
        let body = if navi_sliding { body } else { entered(body) };
        let mut tab_col = column![tab_pane, body].spacing(style::PANE_GAP).width(Fill);
        if body_fill {
            tab_col = tab_col.height(Fill);
        }
        (tab_col.into(), body_fill)
    };

    // Slide the incoming side up into place. While sliding, keep the region
    // full-height so it doesn't change the column's height mid-motion.
    let region = if navi_sliding {
        fill = true;
        let progress = state.navi_select.progress(now);
        // Entrance progress of the side actually on screen (the target): the
        // picker on open, the tab content on dismiss.
        let entrance = if state.navi_select.shown() {
            progress
        } else {
            1.0 - progress
        };
        crate::anim::slide_in(region, entrance, iced::Vector::new(0.0, 20.0))
    } else {
        region
    };

    // Assemble: persistent navi header (if any), then the body region.
    let mut col = column![].spacing(style::PANE_GAP).width(Fill);
    if let Some(strip) = navi_strip {
        col = col.push(strip);
    }
    col = col.push(region);
    if fill {
        col = col.height(Fill);
    }
    col.into()
}

/// The global edit mode's Save / Cancel pair, shown at the navi
/// header's right edge while edit mode is on (or sliding out). One
/// pair for the whole save: they commit / discard the edits on *all*
/// tabs at once. Save is gated on a legal folder when chips are
/// editable — a full 30 chips with no folder-limit violations (an
/// incomplete or over-limit folder can't be written over the
/// save); navicust / patch-card layouts are always valid to write.
fn edit_buttons<'a>(lang: &'a LanguageIdentifier, loaded: &'a Loaded) -> Element<'a, Action> {
    use crate::widgets;
    use lucide_icons::Icon;
    let can_save = !loaded.editability.folder || {
        let full = loaded.save.view_chips().is_none_or(|v| {
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
fn extra_kinds(tab: Tab) -> Vec<ExtraKind> {
    match tab {
        Tab::Folder => vec![ExtraKind::FolderGroup, ExtraKind::Copy],
        // The navi card copies as text only; the navicust grid also
        // copies as an image.
        Tab::Navi => vec![ExtraKind::Copy],
        Tab::Navicust => vec![ExtraKind::CopyImage, ExtraKind::Copy],
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

/// A save-view tab as TSV text for clipboard "copy as text", or `None` for
/// tabs without a text form. The Folder branch honors `opts.folder_grouped`.
pub fn tab_as_text(lang: &LanguageIdentifier, tab: Tab, loaded: &Loaded, opts: RenderOpts) -> Option<String> {
    match tab {
        Tab::Folder => folder::as_text(loaded, opts),
        Tab::PatchCards => patch_cards::as_text(loaded),
        Tab::AutoBattleData => abd::as_text(loaded),
        Tab::Navi => navi::navi_as_text(lang, loaded),
        Tab::Navicust => navicust::navicust_as_text(loaded),
        Tab::Cover => None,
    }
}

/// Render a save-view tab to an RGBA image for clipboard "copy as image".
/// Only Navi/NaviCust has an image form; `None` otherwise.
pub fn tab_as_image(tab: Tab, loaded: &Loaded) -> Option<image::RgbaImage> {
    match tab {
        Tab::Navicust => navicust::as_image(loaded),
        _ => None,
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

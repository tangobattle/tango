use crate::app::{TEXT_BODY, TEXT_CAPTION, TEXT_DISPLAY};
use crate::i18n::t;
use crate::selection::Loaded;
use crate::widgets::{muted_color, muted_text_style};
use iced::widget::{container, image as iced_image, scrollable, stack, text, tooltip, Image, Space};
use sweeten::widget::{button, column, pick_list, row, text_input};

/// Save view is read-only — every interactive bit (NCP hover, chip
/// hover) is handled by tooltip/canvas widgets that manage their own
/// state internally, so render fns never emit caller-visible messages.
/// The Element is generic over the embedder's Message type.
use iced::{Alignment, ContentFit, Element, Fill, Length};
use tango_dataview::rom::NavicustPartColor;
use tango_dataview::save::Save;
use unic_langid::LanguageIdentifier;

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

/// Sort order for the editor's chip-library (right) pane.
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

impl LibrarySort {
    fn label(self, lang: &LanguageIdentifier) -> String {
        match self {
            LibrarySort::Id => t!(lang, "folder-sort-id"),
            LibrarySort::Name => t!(lang, "folder-sort-name"),
            LibrarySort::Code => t!(lang, "folder-sort-code"),
            LibrarySort::Attack => t!(lang, "folder-sort-attack"),
            LibrarySort::Element => t!(lang, "folder-sort-element"),
            LibrarySort::Mb => t!(lang, "folder-sort-mb"),
        }
    }
}

/// A `LibrarySort` paired with its localized label, for the sort
/// pick_list — the picker renders options via `Display`, which can't
/// reach the language, so the label is resolved up front.
#[derive(Clone)]
struct LibrarySortChoice {
    sort: LibrarySort,
    label: String,
}

impl PartialEq for LibrarySortChoice {
    fn eq(&self, other: &Self) -> bool {
        self.sort == other.sort
    }
}

impl std::fmt::Display for LibrarySortChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// All selectable chips for `loaded` as `(id, name)`, in `sort` order.
/// Skips chips with no name or no valid codes — those can't be placed
/// in a folder. Ties fall back to id so the order is stable.
fn sorted_library_entries(loaded: &Loaded, sort: LibrarySort) -> Vec<(usize, String, tango_dataview::save::ChipCode)> {
    use tango_dataview::save::ChipCode;
    let assets = loaded.assets.as_ref();
    struct E {
        id: usize,
        name: String,
        code: ChipCode,
        code_rank: u8,
        atk: u32,
        elem: usize,
        mb: u8,
    }
    let mut rows: Vec<E> = Vec::new();
    for id in 0..assets.num_chips() {
        let Some(info) = assets.chip(id) else { continue };
        // Only real, folder-legal chips — skip dummy data-table entries
        // and unobtainable chips that aren't in the in-game Library.
        if !info.is_legal() {
            continue;
        }
        let Some(name) = info.name() else { continue };
        let (atk, elem, mb) = (info.attack_power(), info.element(), info.mb());
        // One row per valid code (e.g. Cannon A / Cannon B / Cannon *).
        for ch in info.codes() {
            let Some(code) = ChipCode::from_char(ch) else { continue };
            rows.push(E {
                id,
                name: name.clone(),
                code,
                code_rank: code as u8,
                atk,
                elem,
                mb,
            });
        }
    }
    // All ties fall back to (id, code) so the order stays stable.
    match sort {
        LibrarySort::Id => {}
        LibrarySort::Name => rows.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)).then(a.code_rank.cmp(&b.code_rank))),
        LibrarySort::Code => rows.sort_by(|a, b| a.code_rank.cmp(&b.code_rank).then(a.id.cmp(&b.id))),
        LibrarySort::Attack => rows.sort_by(|a, b| a.atk.cmp(&b.atk).then(a.id.cmp(&b.id)).then(a.code_rank.cmp(&b.code_rank))),
        LibrarySort::Element => rows.sort_by(|a, b| a.elem.cmp(&b.elem).then(a.id.cmp(&b.id)).then(a.code_rank.cmp(&b.code_rank))),
        LibrarySort::Mb => rows.sort_by(|a, b| a.mb.cmp(&b.mb).then(a.id.cmp(&b.id)).then(a.code_rank.cmp(&b.code_rank))),
    }
    rows.into_iter().map(|e| (e.id, e.name, e.code)).collect()
}

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
        Tab::Navi => render_navi(lang, loaded),
        Tab::Folder => render_folder(lang, loaded, opts.folder_grouped),
        Tab::PatchCards => render_patch_cards(lang, loaded),
        Tab::AutoBattleData => render_auto_battle_data(lang, loaded),
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
    /// Folder editor: `true` once the user hits Edit on the Folder
    /// tab. While set, the Folder body renders the editable layout
    /// instead of the read-only chip list.
    pub editing: bool,
    /// In-progress tag-chip selection (≤2 raw slot indexes). Seeded
    /// from the equipped folder's tag pair on entering edit mode; a
    /// committed pair is written to the save only when exactly two are
    /// selected (see [`State::toggle_tag`]).
    pub editing_tags: Vec<usize>,
    /// Filter text for the chip library (the editor's right-hand pane).
    pub library_filter: String,
    /// Sort order for the chip library pane.
    pub library_sort: LibrarySort,
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
            editing: false,
            editing_tags: Vec::new(),
            library_filter: String::new(),
            library_sort: LibrarySort::Id,
        }
    }

    /// Enter folder edit mode. Seeds [`Self::editing_tags`] from the
    /// equipped folder's current tag pair so the TAG toggles start in
    /// the right state. Needs `loaded` (the read view), so the play tab
    /// calls this rather than routing through [`Self::apply`].
    pub fn enter_edit(&mut self, loaded: &Loaded) {
        self.editing = true;
        self.library_filter.clear();

        // Seed the tag toggles from the equipped folder's tag pair, if
        // the game has tag chips and a pair is set.
        self.editing_tags = loaded
            .save
            .view_chips()
            .and_then(|v| {
                let folder = v.equipped_folder_index();
                v.tag_chip_indexes(folder)
            })
            .flatten()
            .map(|[a, b]| vec![a, b])
            .unwrap_or_default();
    }

    /// Toggle `slot` in the in-progress tag selection (capped at two).
    /// Returns the pair to commit to the save: `Some([a, b])` once two
    /// slots are selected, else `None` (which clears the tag pairing —
    /// a lone tag chip isn't a valid state in-game).
    pub fn toggle_tag(&mut self, slot: usize) -> Option<[usize; 2]> {
        if let Some(pos) = self.editing_tags.iter().position(|&s| s == slot) {
            self.editing_tags.remove(pos);
        } else if self.editing_tags.len() < 2 {
            self.editing_tags.push(slot);
        }
        match self.editing_tags.as_slice() {
            [a, b] => Some([*a, *b]),
            _ => None,
        }
    }

    /// Remap the in-progress tag selection when `removed_slot`'s chip is
    /// removed and the chips below it shift up one: drop that slot and
    /// shift any higher selected slots down, mirroring the save-side
    /// compaction.
    pub fn compact_tags(&mut self, removed_slot: usize) {
        self.editing_tags.retain(|&s| s != removed_slot);
        for s in self.editing_tags.iter_mut() {
            if *s > removed_slot {
                *s -= 1;
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
                self.active_tab = Some(*t);
                iced::widget::operation::snap_to(
                    self.body_scroll_id.clone(),
                    iced::widget::scrollable::RelativeOffset::START,
                )
            }
            Action::ToggleFolderGrouped(g) => {
                self.folder_grouped = *g;
                iced::Task::none()
            }
            // Save and Cancel both leave edit mode; the host runs the
            // commit/discard side effect separately.
            Action::SaveEdit | Action::CancelEdit => {
                self.editing = false;
                self.editing_tags.clear();
                self.library_filter.clear();
                iced::Task::none()
            }
            Action::LibraryFilterChanged(s) => {
                self.library_filter = s.clone();
                iced::Task::none()
            }
            Action::LibrarySortChanged(s) => {
                self.library_sort = *s;
                iced::Task::none()
            }
            // EnterEdit needs `&Loaded` (to seed tag state), and the
            // mutation actions become host Effects — all are driven by
            // the embedder (play tab), so they're no-ops here.
            Action::EnterEdit
            | Action::AddChip { .. }
            | Action::RemoveChip { .. }
            | Action::ClearFolder
            | Action::ToggleRegular { .. }
            | Action::ToggleTag { .. }
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
    LibrarySortChanged(LibrarySort),
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
    // True while the folder editor is open. Suppresses the Play
    // button (single-player would fight the open edit session) and
    // selects the editable Folder body below.
    let editing_session = editable && state.editing;

    // Tab strip: tabs left, extras+Play right. We split into two
    // rows so the tab list can wrap onto a second line without
    // dragging the extras/Play tail with it. The tail is a
    // separate row, sized to its content and capped to the tab
    // button height so the strip's overall height doesn't grow
    // when active-tab extras (folder group toggle, copy buttons)
    // change.
    const TAB_STRIP_HEIGHT: f32 = 31.0;
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
    let mut tail = row![].spacing(6).align_y(Alignment::Center);
    if inline_actions {
        if let Some(extras) = tab_extras(lang, active, state, loaded, editable) {
            tail = tail.push(extras);
        }
    }
    if let Some(enabled) = play_button.filter(|_| !editing_session) {
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
        tail = tail.push(btn);
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
    if editing_session && active == Tab::Folder {
        let editor = render_folder_edit(lang, loaded, state);
        return column![tab_pane, editor]
            .spacing(widgets::PANE_GAP)
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
    let body_scrollable = scrollable(body).id(state.body_scroll_id.clone()).width(Fill);
    column![tab_pane, body_scrollable]
        .spacing(widgets::PANE_GAP)
        .width(Fill)
        .into()
}

/// Per-tab extras (folder group-by toggle, copy button) shown on the
/// right of the tab strip. `None` = tab has no extras.
fn tab_extras<'a>(
    lang: &'a LanguageIdentifier,
    tab: Tab,
    state: &'a State,
    loaded: &'a Loaded,
    editable: bool,
) -> Option<Element<'a, Action>> {
    use crate::widgets;
    use lucide_icons::Icon;
    let copy_btn = |tab: Tab| -> Element<'a, Action> {
        widgets::icon_button(
            Icon::ClipboardCopy,
            t!(lang, "save-copy"),
            Action::CopyTab(tab),
            [4.0, 10.0],
        )
    };
    let copy_img_btn = |tab: Tab| -> Element<'a, Action> {
        widgets::icon_button(
            Icon::ImageDown,
            t!(lang, "save-copy-image"),
            Action::CopyTabImage(tab),
            [4.0, 10.0],
        )
    };
    match tab {
        Tab::Folder if state.editing => {
            // Edit mode: edits are staged live; Save writes them to the
            // .sav on disk, Cancel discards them. The group toggle /
            // copy don't apply here.
            //
            // Save is only allowed once the folder is full — a legal
            // folder is 30 chips, so an incomplete one can't be written
            // over the save. It's disabled otherwise.
            let folder_full = loaded.save.view_chips().map_or(false, |v| {
                let folder = v.equipped_folder_index();
                (0..30).all(|i| v.chip(folder, i).is_some())
            });
            Some(
                row![
                    widgets::labeled_icon_button(
                        Icon::X,
                        t!(lang, "folder-edit-cancel"),
                        Action::CancelEdit,
                        [4.0, 10.0],
                        widgets::neutral,
                    ),
                    widgets::labeled_icon_button_maybe(
                        Icon::Check,
                        t!(lang, "folder-edit-save"),
                        folder_full.then_some(Action::SaveEdit),
                        [4.0, 10.0],
                        widgets::primary_button,
                    ),
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center)
                .into(),
            )
        }
        Tab::Folder => {
            let mut r = row![
                iced::widget::checkbox(state.folder_grouped)
                    .label(t!(lang, "folder-group"))
                    .on_toggle(Action::ToggleFolderGrouped)
                    .size(TEXT_BODY)
                    .text_size(12)
                    .style(crate::widgets::chunky_checkbox),
                copy_btn(Tab::Folder),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center);
            // Only saves with a writable chip view (BN4/5/6) get the
            // Edit affordance; `chips_editable` is the cached
            // `view_chips_mut().is_some()` probe.
            if editable && loaded.chips_editable {
                r = r.push(widgets::labeled_icon_button(
                    Icon::Pencil,
                    t!(lang, "folder-edit"),
                    Action::EnterEdit,
                    [4.0, 10.0],
                    widgets::neutral,
                ));
            }
            Some(r.into())
        }
        Tab::PatchCards => Some(copy_btn(Tab::PatchCards)),
        Tab::AutoBattleData => Some(copy_btn(Tab::AutoBattleData)),
        Tab::Navi => {
            // Copy-as-image only emits anything for Navicust saves
            // (LinkNavi has no grid to render). Hide the button
            // outright on non-navicust navis instead of leaving a
            // dead affordance in the tab strip.
            let has_navicust = matches!(
                loaded.save.view_navi(),
                Some(tango_dataview::save::NaviView::Navicust(_))
            );
            let mut tail = row![].spacing(6).align_y(iced::Alignment::Center);
            if has_navicust {
                tail = tail.push(copy_img_btn(Tab::Navi));
            }
            tail = tail.push(copy_btn(Tab::Navi));
            Some(tail.into())
        }
        _ => None,
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
                (0..30).map(|i| chips_view.chip(folder_idx, i)).collect();
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
                let mut grouped_map: indexmap::IndexMap<Option<tango_dataview::save::Chip>, GroupedChip> =
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
    Some(crate::navicust::render(
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
        // Extra breathing room above/below the logo(s); standard
        // horizontal inset.
        .padding([crate::widgets::PANE_PADDING + 24.0, crate::widgets::PANE_PADDING])
        .style(crate::widgets::pane)
        .into()
}

// ---------- Folder ----------

#[derive(Default)]
struct GroupedChip {
    count: usize,
    is_regular: bool,
    has_tag1: bool,
    has_tag2: bool,
}

fn render_folder<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded, grouped: bool) -> Element<'static, M> {
    let Some(chips_view) = loaded.save.view_chips() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let folder_idx = chips_view.equipped_folder_index();
    // Read-only display treats "unsupported" and "unset" the same —
    // flatten the outer Option away.
    let regular_idx = chips_view.regular_chip_index(folder_idx).flatten();
    let tag_idxs = chips_view.tag_chip_indexes(folder_idx).flatten();
    let chips_have_mb = assets.chips_have_mb();

    // Pull the 30-chip folder.
    let mut chips: Vec<Option<tango_dataview::save::Chip>> = (0..30).map(|i| chips_view.chip(folder_idx, i)).collect();
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

    // Build display items: either grouped (collapsed by chip identity)
    // or per-slot (one row per slot, possibly empty).
    type Item = (Option<tango_dataview::save::Chip>, GroupedChip);
    let items: Vec<Item> = if grouped {
        let mut grouped_map: indexmap::IndexMap<Option<tango_dataview::save::Chip>, GroupedChip> =
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
        grouped_map.into_iter().collect()
    } else {
        chips
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let [t1, t2] = tag_idxs.map(|t| [t[0] == i, t[1] == i]).unwrap_or([false, false]);
                (
                    c,
                    GroupedChip {
                        count: 1,
                        is_regular: regular_display_idx == Some(i),
                        has_tag1: t1,
                        has_tag2: t2,
                    },
                )
            })
            .collect()
    };

    // No column header. The rows themselves carry enough visual info
    // (icon, name+code, element icon, ATK value, MB value) that a label
    // strip would be redundant — and labels are what make it read as a
    // spreadsheet. When ungrouped, skip empty slots so we don't waste
    // a full-height row on each "—".
    // Tight stack — rows already have their own padding + accent
    // stripe; extra column spacing here adds dead gaps that read
    // as "spreadsheet" rather than "chip list".
    let mut body = column![].spacing(1).padding(0);
    let total_visible = if grouped {
        items.len()
    } else {
        items.iter().filter(|(c, _)| c.is_some()).count()
    };
    let mut visible_idx = 0usize;
    for (chip, g) in &items {
        if !grouped && chip.is_none() {
            continue;
        }
        let chip_id = chip.as_ref().map(|c| c.id);
        let code = chip.as_ref().map(|c| c.code.to_string());
        let is_first = visible_idx == 0;
        let is_last = visible_idx + 1 == total_visible;
        body = body.push(chip_row(
            loaded,
            chip_id,
            code,
            g,
            grouped,
            chips_have_mb,
            visible_idx,
            is_first,
            is_last,
        ));
        visible_idx += 1;
    }

    let _ = grouped;
    // Rows are flush to the pane edges; the outer scrollable in
    // `view` handles vertical overflow once total content exceeds
    // the available height.
    container(body).width(Fill).style(crate::widgets::pane).into()
}

/// Editable folder view: the folder (left) beside the chip library
/// (right). The left pane lists the 30 raw slots — each filled slot can
/// be removed or marked REG/TAG; the right pane lists every selectable
/// chip with a button per valid code that adds it to the first empty
/// slot. Each pane scrolls independently.
fn render_folder_edit<'a>(lang: &'a LanguageIdentifier, loaded: &'a Loaded, state: &'a State) -> Element<'a, Action> {
    use crate::widgets;
    let Some(chips_view) = loaded.save.view_chips() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let folder_idx = chips_view.equipped_folder_index();
    // Outer Some = the game has the feature, so show its toggle.
    let reg = chips_view.regular_chip_index(folder_idx);
    let regular_supported = reg.is_some();
    let regular_idx = reg.flatten();
    let tag_supported = chips_view.tag_chip_indexes(folder_idx).is_some();

    // ----- Left pane: the folder -----
    let filled = (0..30).filter(|&i| chips_view.chip(folder_idx, i).is_some()).count();
    let mut folder_list = column![].spacing(1).padding(0);
    for slot in 0..30usize {
        let chip = chips_view.chip(folder_idx, slot);
        folder_list = folder_list.push(folder_slot_row(
            loaded,
            slot,
            chip,
            regular_idx == Some(slot),
            regular_supported,
            tag_supported,
            state.editing_tags.contains(&slot),
        ));
    }
    let clear_all = widgets::labeled_icon_button(
        lucide_icons::Icon::Trash2,
        t!(lang, "folder-edit-clear"),
        Action::ClearFolder,
        [5.0, 10.0],
        widgets::neutral,
    );
    let folder_header = container(
        row![
            text(t!(lang, "folder-edit-folder", count = filled as i64))
                .size(TEXT_BODY)
                .width(Fill),
            clear_all,
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding([8, 12]);
    let folder_pane = container(column![folder_header, scrollable(folder_list).height(Fill).width(Fill)])
        .width(Fill)
        .height(Fill)
        .style(widgets::pane);

    // ----- Right pane: the chip library -----
    let chips_have_mb = loaded.assets.chips_have_mb();
    let filter = state.library_filter.to_lowercase();
    let mut lib_list = column![].spacing(1).padding(0);
    let mut shown = 0usize;
    for (id, name, code) in sorted_library_entries(loaded, state.library_sort) {
        if !filter.is_empty() && !name.to_lowercase().contains(filter.as_str()) {
            continue;
        }
        lib_list = lib_list.push(library_entry_row(loaded, id, name, code, shown, chips_have_mb));
        shown += 1;
    }
    let filter_input = text_input(&t!(lang, "folder-edit-search"), &state.library_filter)
        .on_input(Action::LibraryFilterChanged)
        .padding([5, 10])
        .size(TEXT_BODY)
        .width(Fill)
        .style(widgets::chunky_text_input);
    let sort_options: Vec<LibrarySortChoice> = LibrarySort::ALL
        .iter()
        .map(|&sort| LibrarySortChoice {
            sort,
            label: sort.label(lang),
        })
        .collect();
    let sort_selected = sort_options.iter().find(|c| c.sort == state.library_sort).cloned();
    let sort_pick = pick_list(sort_options, sort_selected, |c: LibrarySortChoice| {
        Action::LibrarySortChanged(c.sort)
    })
    .padding([5, 10])
    .text_size(TEXT_BODY)
    .style(widgets::chunky_pick_list);
    let lib_header = container(
        row![
            text(t!(lang, "folder-edit-library")).size(TEXT_BODY),
            filter_input,
            text(t!(lang, "folder-edit-sort"))
                .size(TEXT_CAPTION)
                .style(muted_text_style),
            sort_pick,
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding([8, 12]);
    let library_pane = container(column![lib_header, scrollable(lib_list).height(Fill).width(Fill)])
        .width(Fill)
        .height(Fill)
        .style(widgets::pane);

    row![folder_pane, library_pane]
        .spacing(widgets::PANE_GAP)
        .width(Fill)
        .height(Fill)
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
    let description = loaded.assets.chip(id).and_then(|i| i.description());
    let image_handle = loaded.chip_images.get(id).cloned().flatten();
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
    container(r).width(Fill).style(crate::widgets::zebra_row(row_idx)).into()
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
/// chip's full stats (element / code / ATK / MB, like the read-only
/// list) plus Remove / REG / TAG controls (REG/TAG only where the game
/// supports them); empty slots show a muted placeholder.
fn folder_slot_row<'a>(
    loaded: &'a Loaded,
    slot: usize,
    chip: Option<tango_dataview::save::Chip>,
    is_regular: bool,
    regular_supported: bool,
    tag_supported: bool,
    is_tag: bool,
) -> Element<'a, Action> {
    use crate::widgets;
    use lucide_icons::Icon;
    let assets = loaded.assets.as_ref();
    let chips_have_mb = assets.chips_have_mb();
    let chip_id = chip.as_ref().map(|c| c.id);
    let info = chip_id.and_then(|id| assets.chip(id));
    let accent = class_accent(
        info.as_ref().map(|i| i.class()),
        info.as_ref().map(|i| i.dark()).unwrap_or(false),
    );

    let mut inner = row![chip_icon(loaded, chip_id)].spacing(8).align_y(Alignment::Center);
    match chip.as_ref() {
        Some(c) => {
            let name = info.as_ref().and_then(|i| i.name()).unwrap_or_else(|| "???".to_string());
            let [element, atk, mb] = chip_stat_cells(loaded, c.id, chips_have_mb);
            let code = container(text(c.code.to_string()).size(TEXT_BODY).font(iced::Font::MONOSPACE))
                .width(Length::Fixed(22.0))
                .align_x(iced::alignment::Horizontal::Right);
            inner = inner
                .push(text(name).size(TEXT_BODY).width(Fill))
                .push(element)
                .push(code)
                .push(atk)
                .push(mb);
            if regular_supported {
                inner = inner.push(edit_toggle(
                    "REG",
                    is_regular,
                    iced::Color::from_rgb8(0xff, 0x42, 0xa5),
                    Action::ToggleRegular { slot },
                ));
            }
            if tag_supported {
                inner = inner.push(edit_toggle(
                    "TAG",
                    is_tag,
                    iced::Color::from_rgb8(0x29, 0xa1, 0x21),
                    Action::ToggleTag { slot },
                ));
            }
            // Right arrow → remove this chip (back out to the library).
            inner = inner.push(
                button(Icon::ArrowRight.widget().size(TEXT_BODY))
                    .padding([3, 8])
                    .style(widgets::neutral)
                    .on_press(Action::RemoveChip { slot }),
            );
        }
        None => {
            inner = inner.push(text("—").size(TEXT_BODY).style(muted_text_style).width(Fill));
        }
    }
    with_chip_tooltip(
        loaded,
        chip_id,
        accent,
        edit_row_wrap(inner.padding([3, 12]).into(), accent, slot, None),
    )
}

/// One chip in the editor's right pane (the library). Shows the chip's
/// stats (element / ATK / MB, like the read-only list) with a button per
/// valid code; clicking a code adds it to the folder.
fn library_entry_row<'a>(
    loaded: &'a Loaded,
    chip_id: usize,
    name: String,
    code: tango_dataview::save::ChipCode,
    row_idx: usize,
    chips_have_mb: bool,
) -> Element<'a, Action> {
    use crate::widgets;
    use lucide_icons::Icon;
    let info = loaded.assets.chip(chip_id);
    let accent = class_accent(
        info.as_ref().map(|i| i.class()),
        info.as_ref().map(|i| i.dark()).unwrap_or(false),
    );
    let [element, atk, mb] = chip_stat_cells(loaded, chip_id, chips_have_mb);

    // Left arrow → add this chip+code into the folder (to its left). It
    // lives in the gutter left of the accent stripe (see edit_row_wrap).
    let add: Element<'a, Action> = button(Icon::ArrowLeft.widget().size(TEXT_BODY))
        .padding([3, 8])
        .style(widgets::neutral)
        .on_press(Action::AddChip { chip_id, code })
        .into();
    let code_cell = container(text(code.to_string()).size(TEXT_BODY).font(iced::Font::MONOSPACE))
        .width(Length::Fixed(22.0))
        .align_x(iced::alignment::Horizontal::Right);

    let inner = row![
        chip_icon(loaded, Some(chip_id)),
        text(name).size(TEXT_BODY).width(Fill),
        element,
        code_cell,
        atk,
        mb,
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .padding([3, 12]);
    with_chip_tooltip(
        loaded,
        Some(chip_id),
        accent,
        edit_row_wrap(inner.into(), accent, row_idx, Some(add)),
    )
}

/// Small toggle button used for the REG and TAG columns in the folder
/// editor: tinted in `on_color` when active, neutral (greyed) when not.
fn edit_toggle<'a>(label: &'static str, on: bool, on_color: iced::Color, msg: Action) -> Element<'a, Action> {
    let b = button(text(label).size(TEXT_CAPTION)).padding([4, 8]).on_press(msg);
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
    g: &GroupedChip,
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
        text("—")
            .size(TEXT_BODY)
            .color(iced::Color::from_rgb8(0x60, 0x60, 0x60))
            .into()
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
    let image_handle = loaded.chip_images.get(id).cloned().flatten();
    if description.is_none() && image_handle.is_none() {
        return card;
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
        card,
        container(tip).padding(8).style(chip_tooltip_style(accent)),
        tooltip::Position::FollowCursor,
    )
    .gap(8)
    .into()
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

/// Solid + plus colors for an NCP color, matching the navicust render.
fn ncp_colors(color: NavicustPartColor) -> (iced::Color, iced::Color) {
    use NavicustPartColor as N;
    match color {
        N::Red => (
            iced::Color::from_rgb8(0xde, 0x10, 0x00),
            iced::Color::from_rgb8(0xbd, 0x00, 0x00),
        ),
        N::Pink => (
            iced::Color::from_rgb8(0xde, 0x8c, 0xc6),
            iced::Color::from_rgb8(0xbd, 0x6b, 0xa5),
        ),
        N::Yellow => (
            iced::Color::from_rgb8(0xde, 0xde, 0x00),
            iced::Color::from_rgb8(0xbd, 0xbd, 0x00),
        ),
        N::Green => (
            iced::Color::from_rgb8(0x18, 0xc6, 0x00),
            iced::Color::from_rgb8(0x00, 0xa5, 0x00),
        ),
        N::Blue => (
            iced::Color::from_rgb8(0x29, 0x84, 0xde),
            iced::Color::from_rgb8(0x08, 0x60, 0xb8),
        ),
        N::White => (
            iced::Color::from_rgb8(0xde, 0xde, 0xde),
            iced::Color::from_rgb8(0xbd, 0xbd, 0xbd),
        ),
        N::Orange => (
            iced::Color::from_rgb8(0xde, 0x7b, 0x00),
            iced::Color::from_rgb8(0xbd, 0x5a, 0x00),
        ),
        N::Purple => (
            iced::Color::from_rgb8(0x94, 0x00, 0xce),
            iced::Color::from_rgb8(0x73, 0x00, 0xad),
        ),
        N::Gray => (
            iced::Color::from_rgb8(0x84, 0x84, 0x84),
            iced::Color::from_rgb8(0x63, 0x63, 0x63),
        ),
    }
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

// ---------- Navi ----------

fn render_navi<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    let Some(navi_view) = loaded.save.view_navi() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();

    match navi_view {
        tango_dataview::save::NaviView::LinkNavi(v) => {
            let navi_id = v.navi();
            let name = assets
                .navi(navi_id)
                .and_then(|n| n.name())
                .unwrap_or_else(|| format!("Navi #{navi_id}"));
            let emblem: Element<'static, M> = loaded
                .navi_emblems
                .get(&navi_id)
                .cloned()
                .map(|h| {
                    Image::new(h)
                        .width(Length::Fixed(64.0))
                        .height(Length::Fixed(64.0))
                        .filter_method(iced_image::FilterMethod::Nearest)
                        .content_fit(ContentFit::Contain)
                        .into()
                })
                .unwrap_or_else(|| Space::new().height(Length::Fixed(64.0)).into());
            container(
                column![emblem, text(name).size(TEXT_DISPLAY)]
                    .spacing(8)
                    .align_x(Alignment::Center),
            )
            .width(Fill)
            .align_x(Alignment::Center)
            .padding(crate::widgets::PANE_PADDING)
            .style(crate::widgets::pane)
            .into()
        }
        tango_dataview::save::NaviView::Navicust(v) => render_navicust(lang, loaded, v.as_ref()),
    }
}

fn render_navicust<M: 'static>(
    lang: &LanguageIdentifier,
    loaded: &Loaded,
    v: &dyn tango_dataview::save::NavicustView,
) -> Element<'static, M> {
    let assets = loaded.assets.as_ref();
    let [cols, rows_n] = v.size();

    // Big rendered grid (tiny-skia, cached at load time). Scale down to
    // ~440 px wide if larger (5×5 grids render around 360 wide native;
    // bigger grids get scaled). Wrapped in mouse_area so hovering over
    // Per-cell tooltip overlay: render the image as one layer of a
    // Stack and a column-of-rows-of-cell-sized empty widgets as the
    // second layer. Each cell that's covered by an installed part gets
    // its own tooltip wrapper, so iced's tooltip widget manages the
    // hover state internally — no NavicustHover message round-trip
    // needed.
    let grid_el: Element<'static, M> = match loaded.navicust_render.as_ref() {
        Some(nc) => {
            // `source_w/h` are now in DISPLAY coords (see selection.rs);
            // the underlying Handle is 2× that, and iced linear-
            // downsamples it for HiDPI crispness.
            let dw = nc.source_w as f32;
            let dh = nc.source_h as f32;
            let body_x = nc.body_origin_x;
            let body_y = nc.body_origin_y;
            let cell_size = nc.cell_size;
            let g_cols = nc.cols;
            let g_rows = nc.rows;

            let image: Element<'static, M> = Image::new(nc.handle.clone())
                .width(Length::Fixed(dw))
                .height(Length::Fixed(dh))
                // Handle is 2× source for HiDPI (see selection.rs
                // build_navicust_render). On a 2× display iced
                // presents at native device pixels = perfect; on
                // a 1× display iced linear-downsamples 2:1.
                .filter_method(iced_image::FilterMethod::Linear)
                .content_fit(ContentFit::Contain)
                .into();

            // Build the overlay: a fixed-size column of fixed-size rows
            // matching the grid. Each cell is either a no-op Space or
            // a tooltip-wrapped Space carrying the part's name + desc.
            let mut overlay_col = column![Space::new().height(Length::Fixed(body_y))];
            for row_idx in 0..g_rows {
                let mut cell_row = row![Space::new().width(Length::Fixed(body_x))];
                for col_idx in 0..g_cols {
                    let cell_idx = nc.cell_part_idx.get(row_idx * g_cols + col_idx).copied().flatten();
                    let info = cell_idx
                        .and_then(|pi| v.navicust_part(pi))
                        .and_then(|p| assets.navicust_part(p.id));
                    let cell: Element<'static, M> = if let Some(info) = info {
                        let name = info.name().unwrap_or_else(|| "?".to_string());
                        let mut tip_col = column![text(name).size(TEXT_BODY)].spacing(2);
                        if let Some(desc) = info.description() {
                            tip_col = tip_col.push(text(desc).size(TEXT_CAPTION));
                        }
                        let tip = container(tip_col).padding(8).style(tooltip_style);
                        let space = Space::new()
                            .width(Length::Fixed(cell_size))
                            .height(Length::Fixed(cell_size));
                        tooltip(space, tip, tooltip::Position::FollowCursor).gap(12).into()
                    } else {
                        Space::new()
                            .width(Length::Fixed(cell_size))
                            .height(Length::Fixed(cell_size))
                            .into()
                    };
                    cell_row = cell_row.push(cell);
                }
                overlay_col = overlay_col.push(cell_row);
            }

            let stacked = stack![image, overlay_col]
                .width(Length::Fixed(dw))
                .height(Length::Fixed(dh));
            // Flush against the pane — no shadow, no extra padding.
            // The image's corners are pre-masked in selection.rs to
            // match the pane's rounded corners. No Fill / centering
            // here either: that would propagate up and stretch the
            // whole pane across the tab.
            stacked.into()
        }
        None => text(t!(lang, "navicust-grid-size", cols = cols as i64, rows = rows_n as i64))
            .size(TEXT_CAPTION)
            .into(),
    };

    // Parts list: two columns — solid parts (left), plus parts (right) —
    // each colored by NCP color, with hover tooltip showing description.
    let mut solid_col = column![].spacing(4);
    let mut plus_col = column![].spacing(4);
    let mut installed_solid = 0;
    let mut installed_plus = 0;
    for i in 0..v.count() {
        let Some(part) = v.navicust_part(i) else {
            continue;
        };
        let Some(info) = assets.navicust_part(part.id) else {
            continue;
        };
        let part_name = info.name().unwrap_or_else(|| format!("#{}", part.id));
        let description = info.description();
        let is_solid = info.is_solid();
        let (solid_color, plus_color) = info.color().map(ncp_colors).unwrap_or((
            iced::Color::from_rgb8(0xbd, 0xbd, 0xbd),
            iced::Color::from_rgb8(0x88, 0x88, 0x88),
        ));
        let bg = if is_solid { solid_color } else { plus_color };
        let _ = i; // index no longer needed now that the list-highlight is gone
        let badge_el = colored_badge_sized(part_name, bg, iced::Color::BLACK, TEXT_BODY, [3.0, 8.0]);
        let badge_el: Element<'static, M> = if let Some(desc) = description {
            tooltip(
                badge_el,
                container(text(desc).size(TEXT_CAPTION)).padding(8).style(tooltip_style),
                tooltip::Position::FollowCursor,
            )
            .gap(8)
            .into()
        } else {
            badge_el
        };
        if is_solid {
            installed_solid += 1;
            solid_col = solid_col.push(badge_el);
        } else {
            installed_plus += 1;
            plus_col = plus_col.push(badge_el);
        }
    }
    // Single pane sized to its contents — no "(none installed)"
    // fallback; an empty navicust shows just the rounded image with
    // pane padding around it. `align_x(Center)` centers narrower rows
    // (style header, parts list) horizontally inside the column's
    // shrink-wrapped width without dragging in any Fill that would
    // stretch the pane across the tab.
    let mut content = column![].spacing(8).align_x(Alignment::Center);
    content = content.push(grid_el);
    if installed_solid + installed_plus > 0 {
        // No Fill anywhere here — Fill on a child propagates up
        // through the column, forcing the whole pane to span the tab.
        content = content.push(row![solid_col, plus_col].spacing(12));
    }

    let _ = (cols, rows_n, installed_solid, installed_plus);
    container(content)
        .padding(crate::widgets::PANE_PADDING)
        .style(crate::widgets::pane)
        .into()
}

// ---------- Patch cards ----------

fn render_patch_cards<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    let Some(view) = loaded.save.view_patch_cards() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();

    let mut list = column![].spacing(3).padding(0);
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
                let effects: Vec<_> = info.as_ref().map(|c| c.effects()).unwrap_or_default();

                let name_text = if card.enabled {
                    text(name).size(TEXT_BODY)
                } else {
                    text(name).size(TEXT_BODY).style(muted_text_style)
                };
                let name_col = column![name_text, text(format!("{mb}MB")).size(10).style(muted_text_style),].spacing(2);

                let mut ability_col = column![].spacing(2);
                for e in effects.iter().filter(|e| e.is_ability) {
                    ability_col = ability_col.push(effect_badge(e, card.enabled));
                }
                let mut bug_col = column![].spacing(2);
                for e in effects.iter().filter(|e| !e.is_ability) {
                    bug_col = bug_col.push(effect_badge(e, card.enabled));
                }

                let row = row![
                    text(format!("{:>2}", i + 1))
                        .size(TEXT_CAPTION)
                        .width(Length::Fixed(24.0)),
                    container(name_col).width(Length::Fill),
                    container(ability_col).width(Length::Fixed(180.0)),
                    container(bug_col).width(Length::Fixed(180.0)),
                ]
                .spacing(8)
                .align_y(Alignment::Start);
                list = list.push(container(row).padding([6, 10]).style(crate::widgets::zebra_row(i)));
            }
        }
        tango_dataview::save::PatchCardsView::PatchCard4s(v) => {
            for i in 0..6 {
                let card = v.patch_card(i);
                let info = card.as_ref().and_then(|c| assets.patch_card4(c.id));
                let label = match (card.as_ref(), info.as_ref()) {
                    (Some(c), Some(i)) if c.enabled => i.name().unwrap_or_else(|| format!("#{}", c.id)),
                    _ => "—".to_string(),
                };
                let effect = info.as_ref().and_then(|i| i.effect());
                let bug = info.as_ref().and_then(|i| i.bug());

                let mut details_col = column![].spacing(2);
                if let Some(e) = effect {
                    details_col = details_col.push(
                        text(e)
                            .size(TEXT_CAPTION)
                            .color(iced::Color::from_rgb8(0xff, 0xbd, 0x18)),
                    );
                }
                if let Some(b) = bug {
                    details_col = details_col.push(
                        text(b)
                            .size(TEXT_CAPTION)
                            .color(iced::Color::from_rgb8(0xb5, 0x5a, 0xde)),
                    );
                }

                let row = row![
                    text(format!("0{}", ['A', 'B', 'C', 'D', 'E', 'F'][i]))
                        .size(TEXT_CAPTION)
                        .width(Length::Fixed(22.0)),
                    text(label).size(TEXT_BODY).width(Length::Fill),
                    details_col,
                ]
                .spacing(8)
                .align_y(Alignment::Start);
                list = list.push(container(row).padding([6, 10]).style(crate::widgets::zebra_row(i)));
            }
        }
    }

    container(list).width(Fill).style(crate::widgets::pane).into()
}

// ---------- Auto Battle Data ----------

fn render_auto_battle_data<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    let Some(view) = loaded.save.view_auto_battle_data() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let mat = view.materialized();

    let chips_have_mb = assets.chips_have_mb();

    // ABD slots have no chip code and no REG/TAG indicators, so
    // pass `code=None` and a default-zeroed badge struct. Hover
    // preview comes for free from chip_row. Each section becomes
    // its own pane so the outer scrollable in `view` shows them
    // as distinct demarcated regions.
    let section = |title: String, slots: &[Option<usize>]| -> Element<'static, M> {
        let title_el = container(text(title).size(TEXT_BODY)).padding([8, 12]);
        let mut col = column![title_el, Space::new().height(4)].spacing(1);
        let empty_badges = GroupedChip::default();
        let last_idx = slots.len().saturating_sub(1);
        for (idx, id) in slots.iter().enumerate() {
            // is_first stays false — the title row sits above the chips,
            // so no chip row touches the pane's rounded top corners.
            col = col.push(chip_row(
                loaded,
                *id,
                None,
                &empty_badges,
                false,
                chips_have_mb,
                idx,
                false,
                idx == last_idx,
            ));
        }
        container(col).width(Fill).style(crate::widgets::pane).into()
    };

    column![
        section(
            t!(lang, "auto-battle-data-secondary-standard-chips"),
            mat.secondary_standard_chips(),
        ),
        section(t!(lang, "auto-battle-data-standard-chips"), mat.standard_chips(),),
        section(t!(lang, "auto-battle-data-mega-chips"), mat.mega_chips()),
        section(t!(lang, "auto-battle-data-giga-chip"), &[mat.giga_chip()]),
        section(t!(lang, "auto-battle-data-combos"), mat.combos()),
        section(t!(lang, "auto-battle-data-program-advance"), &[mat.program_advance()],),
    ]
    .spacing(crate::widgets::PANE_GAP)
    .width(Fill)
    .into()
}

fn placeholder<M: 'static>(msg: String) -> Element<'static, M> {
    container(text(msg).size(TEXT_BODY))
        .width(Fill)
        .padding(crate::widgets::PANE_PADDING)
        .style(crate::widgets::pane)
        .into()
}

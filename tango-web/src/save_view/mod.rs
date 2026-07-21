//! The save view: the desktop's `save_view` ported to Dioxus. The
//! persistent navi strip (emblem / name / stats + the save-level action
//! cluster) sits above a sub-tab strip (NaviCust / Folder / Patch Cards
//! / Auto Battle Data) and the active tab's body.
//!
//! Editing follows the desktop model exactly: one global edit session
//! covers the whole save — while it's open every editable tab shows its
//! editor and the navi card becomes the change-navi button; edits stage
//! into the in-memory save immediately (the read models render them
//! live) and one Save / Cancel commits them all to OPFS or reloads the
//! on-disk original. There is no undo stack — Cancel is the undo.
//!
//! The desktop's Cover tab is streamer-mode only and the web build has
//! no streamer mode, so it isn't ported.

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use unic_langid::LanguageIdentifier;

pub(crate) mod abd;
pub(crate) mod edit;
pub(crate) mod folder;
mod loaded;
mod navi;
pub(crate) mod navicust;
pub(crate) mod patch_cards;

pub use loaded::Loaded;

use crate::t;
use crate::ui::icons;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Navicust,
    Folder,
    PatchCards,
    AutoBattleData,
}

/// Which sub-tabs this save offers, in display order (the desktop's
/// `available_tabs`). The equipped navi is not a tab — it lives in the
/// persistent strip above the body.
pub fn available_tabs(save: &dyn tango_dataview::save::Save) -> Vec<Tab> {
    let mut tabs = vec![];
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

/// Shared handle to the loaded save. Cheap to clone into event handlers;
/// equality is identity (the same in-memory save), so staged edits — which
/// mutate through the `RefCell` — don't churn component props. Re-renders
/// after an edit ride [`SAVE_REV`] instead.
#[derive(Clone)]
pub struct SaveHandle(pub Rc<RefCell<Loaded>>);

impl PartialEq for SaveHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// Bumped after every staged in-memory edit; [`SaveView`] subscribes so
/// it re-reads the mutated save through the `RefCell`.
pub static SAVE_REV: GlobalSignal<u64> = Signal::global(|| 0);

/// Stage one edit into the in-memory save and wake every subscriber.
pub(crate) fn stage_edit(handle: &SaveHandle, e: edit::Edit) {
    {
        let mut l = handle.0.borrow_mut();
        edit::apply_edit(&mut l, e);
    }
    *SAVE_REV.write() += 1;
}

/// New index of an element originally at `i` after an ordered move that
/// takes the element at `from` and reinserts it at `to`. Used to keep
/// slot-indexed references (REG/TAG, staged tags) aligned with a reorder.
pub(crate) fn reorder_index(i: usize, from: usize, to: usize) -> usize {
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
/// edit ends (the desktop's `EditState`). Held as the `Option` payload of
/// the view's `editing` signal so one assignment clears it all.
#[derive(Clone, Default, PartialEq)]
pub(crate) struct EditUi {
    /// Folder editor: in-progress tag-chip selection (≤2 raw slot
    /// indexes). Seeded from the equipped folder's tag pair on entering
    /// edit mode; a committed pair is written to the save only when
    /// exactly two are selected.
    pub tags: Vec<usize>,
    /// Folder editor: chip library filter text.
    pub library_filter: String,
    /// Navicust editor: the part currently picked up from the palette,
    /// drawn as a ghost under the cursor.
    pub held_part: Option<navicust::HeldPart>,
    /// Navicust editor: per-part picker orientation (`id -> (rot,
    /// compressed)`). Missing id = default (rot 0, compressed).
    pub part_orient: std::collections::HashMap<usize, (u8, bool)>,
    /// Navicust editor: palette filter text.
    pub navicust_filter: String,
    /// BN5/BN6 patch-card editor: library filter text.
    pub patch_card56_filter: String,
    /// Auto-battle-data editor: chip library filter text.
    pub auto_battle_data_filter: String,
}

impl EditUi {
    /// The orientation a palette part is shown / picked up in.
    pub fn orient_of(&self, id: usize) -> (u8, bool) {
        self.part_orient.get(&id).copied().unwrap_or((0, true))
    }

    /// Toggle `slot` in the in-progress tag selection (capped at two).
    /// Returns the pair to commit to the save: `Some([a, b])` once two
    /// slots are selected, else `None` (which clears the tag pairing).
    pub fn toggle_tag(&mut self, slot: usize) -> Option<[usize; 2]> {
        if let Some(pos) = self.tags.iter().position(|&s| s == slot) {
            self.tags.remove(pos);
        } else if self.tags.len() < 2 {
            self.tags.push(slot);
        }
        match self.tags.as_slice() {
            [a, b] => Some([*a, *b]),
            _ => None,
        }
    }

    /// Remap the in-progress tag selection when `removed_slot`'s chip is
    /// removed and the chips below it shift up one.
    pub fn compact_tags(&mut self, removed_slot: usize) {
        self.tags.retain(|&s| s != removed_slot);
        for s in self.tags.iter_mut() {
            if *s > removed_slot {
                *s -= 1;
            }
        }
    }

    /// Remap the in-progress tag selection through a chip reorder.
    pub fn move_tags(&mut self, from: usize, to: usize) {
        for s in self.tags.iter_mut() {
            *s = reorder_index(*s, from, to);
        }
    }

    /// Shift the staged tag selection when a chip is added at the top:
    /// the run of chips above the first empty slot (`gap`) slides down.
    pub fn shift_tags_for_top_insert(&mut self, gap: usize) {
        for s in self.tags.iter_mut() {
            if *s < gap {
                *s += 1;
            }
        }
    }
}

/// Follow-cursor chip hover state, written by chip rows on mousemove and
/// read by the single [`ChipPopover`] layer — the browser stand-in for
/// the desktop's follow-cursor chip tooltip (a per-row CSS popover would
/// be clipped by the scroll container).
#[derive(Clone, PartialEq)]
pub(crate) struct ChipHover {
    pub chip_id: usize,
    pub accent: Option<&'static str>,
    pub x: f64,
    pub y: f64,
}

pub(crate) static CHIP_HOVER: GlobalSignal<Option<ChipHover>> = Signal::global(|| None);

fn tab_label(lang: &LanguageIdentifier, tab: Tab) -> String {
    match tab {
        Tab::Navicust => t!(lang, "save-tab-navicust"),
        Tab::Folder => t!(lang, "save-tab-folder"),
        Tab::PatchCards => t!(lang, "save-tab-patch-cards"),
        Tab::AutoBattleData => t!(lang, "save-tab-auto-battle-data"),
    }
}

fn tab_icon(tab: Tab) -> Element {
    match tab {
        Tab::Navicust => rsx! { icons::Puzzle {} },
        Tab::Folder => rsx! { icons::Files {} },
        Tab::PatchCards => rsx! { icons::CreditCard {} },
        Tab::AutoBattleData => rsx! { icons::Swords {} },
    }
}

/// A save-view tab as TSV text for clipboard "copy as text".
fn tab_as_text(tab: Tab, loaded: &Loaded, folder_grouped: bool) -> Option<String> {
    match tab {
        Tab::Folder => folder::as_text(loaded, folder_grouped),
        Tab::Navicust => navicust::navicust_as_text(loaded),
        Tab::PatchCards => patch_cards::as_text(loaded),
        Tab::AutoBattleData => abd::as_text(loaded),
    }
}

/// Centered icon-over-message card for a tab with nothing to show.
pub(crate) fn placeholder(msg: String) -> Element {
    rsx! {
        div { class: "pane save-placeholder",
            span { class: "ph-icon", icons::FileQuestion {} }
            span { class: "ph-msg", "{msg}" }
        }
    }
}

/// The filter box + sort picker strip shared by the editor library panes.
/// `options` are the pre-resolved sort labels; `selected` indexes them.
pub(crate) fn library_header(
    placeholder_text: String,
    filter_value: String,
    on_filter: EventHandler<String>,
    sort_label: String,
    options: Vec<String>,
    selected: usize,
    on_sort: EventHandler<usize>,
) -> Element {
    rsx! {
        div { class: "editor-header lib",
            input {
                class: "filter",
                r#type: "text",
                placeholder: "{placeholder_text}",
                value: "{filter_value}",
                oninput: move |evt: FormEvent| on_filter.call(evt.value()),
            }
            span { class: "sub", "{sort_label}" }
            select {
                onchange: move |evt: FormEvent| {
                    if let Ok(i) = evt.value().parse::<usize>() {
                        on_sort.call(i);
                    }
                },
                for (i, label) in options.iter().enumerate() {
                    option { value: "{i}", selected: i == selected, "{label}" }
                }
            }
        }
    }
}

/// The red "Clear all" button atop every editor pane.
pub(crate) fn clear_all_button(lang: &LanguageIdentifier, onclick: EventHandler<()>) -> Element {
    rsx! {
        button { class: "btn danger compact", onclick: move |_| onclick.call(()),
            icons::Trash2 {}
            {t!(lang, "save-edit-clear")}
        }
    }
}

/// Small toggle button for the REG / TAG / ON columns: tinted when
/// active, neutral when not; `None` renders it disabled.
pub(crate) fn edit_toggle_maybe(
    label: &'static str,
    on: bool,
    on_color: &'static str,
    msg: Option<EventHandler<()>>,
) -> Element {
    let style = if on {
        format!("background:{on_color};border-color:transparent;color:#fff")
    } else {
        String::new()
    };
    let disabled = msg.is_none();
    rsx! {
        button {
            class: "btn toggle",
            style: "{style}",
            disabled,
            onclick: move |_| {
                if let Some(m) = &msg {
                    m.call(());
                }
            },
            "{label}"
        }
    }
}

/// The "✕" button that removes a chip / patch-card from its slot.
pub(crate) fn remove_button(onclick: EventHandler<()>) -> Element {
    rsx! {
        button { class: "btn compact x", onclick: move |_| onclick.call(()), icons::X {} }
    }
}

/// Caption that turns danger-red when an editor budget is blown.
pub(crate) fn limit_caption(label: String, over: bool) -> Element {
    rsx! {
        span { class: if over { "sub over" } else { "sub" }, "{label}" }
    }
}

/// Wholesale save-view widget: navi strip (with the save-level actions
/// at its right edge), sub-tab strip with per-tab extras, and the body.
///
/// `play_enabled`:
///   * `None`        — no Play button in the strip.
///   * `Some(true)`  — Play button rendered and enabled.
///   * `Some(false)` — Play button rendered but disabled (e.g. while a
///     netplay lobby is open and single-player would conflict).
///
/// `editable`: only the Play tab passes `true` (the default) — other
/// embedders (the replay detail pane) never show the Edit affordance.
#[component]
pub fn SaveView(
    handle: SaveHandle,
    play_enabled: Option<bool>,
    on_play: Option<EventHandler<()>>,
    #[props(default = true)] editable: bool,
) -> Element {
    // Re-render after staged edits (they mutate through the RefCell).
    let _ = SAVE_REV.read();
    let lang = crate::i18n::LANG.read().clone();
    let crate::ui::Ctx { storage, .. } = crate::ui::use_ctx();
    let mut active_tab = use_signal(|| Option::<Tab>::None);
    let mut folder_grouped = use_signal(|| true);
    // Which tab's copy button is showing its "Copied!" flash.
    let mut copy_flash = use_signal(|| Option::<u8>::None);
    // The global edit session (the desktop's `State::editing`): while
    // `Some`, every editable tab shows its editor and one Save / Cancel
    // commits / discards them all.
    let mut editing = use_signal(|| Option::<EditUi>::None);
    // Whether the navi picker owns the body region (reached by clicking
    // the navi card while editing; the navi has no tab of its own).
    let mut navi_pick_open = use_signal(|| false);
    // Sort orders are persistent UI preferences, kept across sessions.
    let library_sort = use_signal(|| folder::LibrarySort::Id);
    let navicust_sort = use_signal(|| navicust::NavicustSort::Id);
    let patch_card56_sort = use_signal(|| patch_cards::PatchCard56Sort::Id);
    let abd_sort = use_signal(|| abd::AutoBattleDataSort::Id);

    // A different save swapped in under the view: drop any in-progress
    // edit (its staged state lived in the previous in-memory save).
    {
        let handle = handle.clone();
        use_effect(move || {
            let _ = &handle; // keyed on the handle prop identity
            editing.set(None);
            navi_pick_open.set(false);
        });
    }

    // Reset the body scroll on tab switches, like the desktop's
    // snap-to-top.
    use_effect(move || {
        let _ = active_tab.read();
        if let Some(el) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("save-body"))
        {
            el.set_scroll_top(0);
        }
    });

    let loaded_rc = handle.0.clone();
    let loaded = loaded_rc.borrow();
    let available = available_tabs(loaded.save.as_ref());
    let active = active_tab().filter(|t| available.contains(t)).or_else(|| available.first().copied());
    let has_navi_strip = loaded.save.view_navi().is_some();
    let editability = loaded.editability;
    let editing_session = editable && editing.read().is_some();
    let save_editable = editable && editability.any();

    // Save is gated on a legal folder when chips are editable: a full 30
    // chips with no folder-limit violations.
    let can_save = !editability.folder || {
        let full = loaded.save.view_chips().is_none_or(|v| {
            let folder = v.equipped_folder_index();
            (0..folder::MAX_FOLDER_CHIPS).all(|i| v.chip(folder, i).is_some())
        });
        full && folder::folder_limits_satisfied(&loaded)
    };

    // Enter the global edit session, seeding the tag toggles from the
    // equipped folder's current tag pair.
    let enter_edit = {
        let handle = handle.clone();
        move |_| {
            let tags = {
                let l = handle.0.borrow();
                l.save
                    .view_chips()
                    .and_then(|v| {
                        let folder = v.equipped_folder_index();
                        v.tag_chip_indexes(folder)
                    })
                    .flatten()
                    .map(|[a, b]| vec![a, b])
                    .unwrap_or_default()
            };
            editing.set(Some(EditUi {
                tags,
                ..Default::default()
            }));
        }
    };

    // Commit: checksum + serialize the staged save and write it back to
    // its OPFS file, then leave edit mode. The SAVES_REV bump makes the
    // loaded-save resource rebuild from the fresh on-disk bytes.
    let commit = {
        let handle = handle.clone();
        move |_| {
            let (file, sram) = {
                let mut l = handle.0.borrow_mut();
                l.save.rebuild_checksum();
                (l.save_file.clone(), l.save.to_sram_dump())
            };
            let storage = storage.read().clone().flatten();
            spawn(async move {
                let Some(storage) = storage else { return };
                match crate::storage::write(storage.saves(), &file, &sram).await {
                    Ok(()) => {
                        editing.set(None);
                        navi_pick_open.set(false);
                        *crate::runtime::SAVES_REV.write() += 1;
                    }
                    Err(e) => log::error!("save-edit commit {file}: {e}"),
                }
            });
        }
    };

    // Cancel: leave edit mode and force a reload of the on-disk
    // original, reverting every staged edit at once.
    let cancel = move |_| {
        editing.set(None);
        navi_pick_open.set(false);
        *crate::runtime::SAVES_REV.write() += 1;
    };

    // Copy-as-text for the active tab; flashes "Copied!" on the button.
    let on_copy = {
        let handle = handle.clone();
        move |tab: Tab| {
            let grouped = *folder_grouped.peek();
            let text = {
                let l = handle.0.borrow();
                tab_as_text(tab, &l, grouped)
            };
            let Some(text) = text else { return };
            spawn(async move {
                let Some(win) = web_sys::window() else { return };
                let p = win.navigator().clipboard().write_text(&text);
                if wasm_bindgen_futures::JsFuture::from(p).await.is_ok() {
                    copy_flash.set(Some(tab as u8));
                    gloo_timers::future::TimeoutFuture::new(1500).await;
                    if *copy_flash.peek() == Some(tab as u8) {
                        copy_flash.set(None);
                    }
                }
            });
        }
    };

    let show_picker = editing_session && editability.navi && navi_pick_open();

    rsx! {
        div { class: "save-view",
            // Persistent navi identity strip + the save-level actions.
            if has_navi_strip {
                div { class: "pane save-strip",
                    // While editing (and the navi is editable) the card
                    // itself becomes the change-navi button.
                    if editing_session && editability.navi {
                        button {
                            class: "btn ghost navi-card-btn",
                            onclick: move |_| navi_pick_open.set(!navi_pick_open()),
                            {navi::navi_card_content(&lang, &loaded, true)}
                        }
                    } else {
                        {navi::navi_card_content(&lang, &loaded, false)}
                    }
                    div { class: "grow" }
                    div { class: "strip-actions",
                        if editing_session {
                            button { class: "btn", onclick: cancel,
                                icons::X {}
                                {t!(&lang, "save-edit-cancel")}
                            }
                            button {
                                class: "btn primary",
                                disabled: !can_save,
                                onclick: commit,
                                icons::Check {}
                                {t!(&lang, "save-edit-save")}
                            }
                        } else {
                            if save_editable {
                                button { class: "btn", onclick: enter_edit,
                                    icons::Pencil {}
                                    {t!(&lang, "save-edit")}
                                }
                            }
                            if let Some(enabled) = play_enabled {
                                button {
                                    class: "btn primary",
                                    disabled: !enabled,
                                    onclick: move |_| {
                                        if let Some(h) = &on_play {
                                            h.call(());
                                        }
                                    },
                                    icons::Play {}
                                    {t!(&lang, "play-play")}
                                }
                            }
                        }
                    }
                }
            }
            if show_picker {
                // The navi picker claims the whole region below the strip.
                navi::NaviPicker { handle: handle.clone(), editing, open: navi_pick_open }
            } else if let Some(active) = active {
                // Sub-tab strip: tabs left, the active tab's extras right.
                div { class: "pane save-tabs",
                    div { class: "subtabs",
                        for tab in available.iter().copied() {
                            button {
                                class: if tab == active { "btn subtab active" } else { "btn subtab" },
                                onclick: move |_| {
                                    *CHIP_HOVER.write() = None;
                                    active_tab.set(Some(tab));
                                },
                                {tab_icon(tab)}
                                {tab_label(&lang, tab)}
                            }
                        }
                    }
                    // Extras are read-mode only, like the desktop.
                    if !editing_session {
                        div { class: "tab-extras",
                            if active == Tab::Folder {
                                label { class: "check",
                                    input {
                                        r#type: "checkbox",
                                        checked: folder_grouped(),
                                        onchange: move |evt: FormEvent| folder_grouped.set(evt.checked()),
                                    }
                                    {t!(&lang, "folder-group")}
                                }
                            }
                            button {
                                class: "btn subtle",
                                onclick: {
                                    let on_copy = on_copy.clone();
                                    move |_| on_copy(active)
                                },
                                icons::ClipboardCopy {}
                                if copy_flash() == Some(active as u8) {
                                    {t!(&lang, "copied")}
                                } else {
                                    {t!(&lang, "save-copy")}
                                }
                            }
                        }
                    }
                }
                // The active tab's body. The in-place editors claim the
                // full height with their own pane scrolls; the read views
                // share one scroll region.
                if editing_session && active == Tab::Folder && editability.folder {
                    folder::FolderEdit { handle: handle.clone(), editing, sort: library_sort }
                } else if editing_session && active == Tab::Navicust && editability.navicust {
                    navicust::NavicustEdit { handle: handle.clone(), editing, sort: navicust_sort }
                } else if editing_session && active == Tab::PatchCards && editability.patch_cards {
                    patch_cards::PatchCardsEdit { handle: handle.clone(), editing, sort: patch_card56_sort }
                } else if editing_session && active == Tab::AutoBattleData && editability.auto_battle_data {
                    abd::AbdEdit { handle: handle.clone(), editing, sort: abd_sort }
                } else {
                    div { class: "save-body", id: "save-body",
                        match active {
                            Tab::Folder => folder::render_folder(&lang, &loaded, folder_grouped()),
                            Tab::Navicust => navicust::render_navicust_tab(&lang, &loaded),
                            Tab::PatchCards => patch_cards::render_patch_cards(&lang, &loaded),
                            Tab::AutoBattleData => abd::render_auto_battle_data(&lang, &loaded),
                        }
                    }
                }
            } else {
                {placeholder(t!(&lang, "save-empty"))}
            }
            ChipPopover { handle: handle.clone() }
            navicust::NcpPopover { handle: handle.clone() }
        }
    }
}

/// The single follow-cursor chip popover layer: scaled artwork above the
/// chip's description, tinted to the chip's class accent.
#[component]
fn ChipPopover(handle: SaveHandle) -> Element {
    let hover = CHIP_HOVER.read().clone();
    let Some(h) = hover else { return rsx! {} };
    let l = handle.0.borrow();
    let info = l.assets.chip(h.chip_id);
    let description = info.as_ref().and_then(|i| i.description());
    // Program advances show description only — no standalone chip image.
    let is_pa = info
        .as_ref()
        .is_some_and(|i| i.class() == tango_dataview::rom::ChipClass::ProgramAdvance);
    let image = if is_pa {
        None
    } else {
        l.chip_images.get(h.chip_id).cloned().flatten()
    };
    if description.is_none() && image.is_none() {
        return rsx! {};
    }
    let bg = h.accent.unwrap_or("rgba(0, 0, 0, 0.85)");
    // Flip to the cursor's left near the viewport's right edge so the
    // popover never clips offscreen.
    let flip = web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|v| v.as_f64())
        .is_some_and(|w| h.x > w - 320.0);
    rsx! {
        div {
            class: if flip { "chip-pop flip" } else { "chip-pop" },
            style: "left:{h.x}px; top:{h.y}px; --pop-bg:{bg}",
            if let Some((w, img_h, url)) = image {
                img {
                    class: "pix",
                    width: "{w * 2}",
                    height: "{img_h * 2}",
                    src: "{url}",
                    alt: "",
                }
            }
            if let Some(desc) = description {
                p { "{desc}" }
            }
        }
    }
}

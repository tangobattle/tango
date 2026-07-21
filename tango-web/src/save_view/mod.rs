//! The save view: the desktop's `save_view` ported to Dioxus. The
//! persistent navi strip (emblem / name / stats + the save-level action
//! cluster) sits above a sub-tab strip (NaviCust / Folder / Patch Cards
//! / Auto Battle Data) and the active tab's body. Read views are pure
//! renders off [`Loaded`]; editors arrive with the save-edit port.
//!
//! The desktop's Cover tab is streamer-mode only and the web build has
//! no streamer mode, so it isn't ported.

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use unic_langid::LanguageIdentifier;

pub(crate) mod folder;
mod loaded;
mod navi;

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

/// A save-view tab as TSV text for clipboard "copy as text", or `None`
/// for tabs without a text form yet.
fn tab_as_text(_lang: &LanguageIdentifier, tab: Tab, loaded: &Loaded, folder_grouped: bool) -> Option<String> {
    match tab {
        Tab::Folder => folder::as_text(loaded, folder_grouped),
        // The remaining tabs' text forms arrive with their view ports.
        Tab::Navicust | Tab::PatchCards | Tab::AutoBattleData => None,
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

/// Wholesale save-view widget: navi strip (with the save-level actions
/// at its right edge), sub-tab strip with per-tab extras, and the body.
///
/// `play_enabled`:
///   * `None`        — no Play button in the strip.
///   * `Some(true)`  — Play button rendered and enabled.
///   * `Some(false)` — Play button rendered but disabled (e.g. while a
///     netplay lobby is open and single-player would conflict).
#[component]
pub fn SaveView(handle: SaveHandle, play_enabled: Option<bool>, on_play: EventHandler<()>) -> Element {
    // Re-render after staged edits (they mutate through the RefCell).
    let _ = SAVE_REV.read();
    let lang = crate::i18n::LANG.read().clone();
    let mut active_tab = use_signal(|| Option::<Tab>::None);
    let mut folder_grouped = use_signal(|| true);
    // Which tab's copy button is showing its "Copied!" flash.
    let mut copy_flash = use_signal(|| Option::<u8>::None);

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

    // Copy-as-text for the active tab; flashes "Copied!" on the button.
    let on_copy = {
        let handle = handle.clone();
        let lang = lang.clone();
        move |tab: Tab| {
            let grouped = *folder_grouped.peek();
            let text = {
                let l = handle.0.borrow();
                tab_as_text(&lang, tab, &l, grouped)
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

    rsx! {
        div { class: "save-view",
            // Persistent navi identity strip + the save-level actions.
            if has_navi_strip {
                div { class: "pane save-strip",
                    {navi::navi_card_content(&lang, &loaded, false)}
                    div { class: "grow" }
                    div { class: "strip-actions",
                        if let Some(enabled) = play_enabled {
                            button {
                                class: "btn primary",
                                disabled: !enabled,
                                onclick: move |_| on_play.call(()),
                                icons::Play {}
                                {t!(&lang, "play-play")}
                            }
                        }
                    }
                }
            }
            if let Some(active) = active {
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
                // The active tab's body, in its own scroll region.
                div { class: "save-body", id: "save-body",
                    match active {
                        Tab::Folder => folder::render_folder(&lang, &loaded, folder_grouped()),
                        // The remaining read views arrive with their ports.
                        Tab::Navicust | Tab::PatchCards | Tab::AutoBattleData => placeholder(t!(&lang, "save-empty")),
                    }
                }
            } else {
                {placeholder(t!(&lang, "save-empty"))}
            }
            ChipPopover { handle: handle.clone() }
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

//! The Patch Cards tab (the desktop's `save_view/patch_cards.rs`):
//! BN5/BN6 show the registered list (index · name+MB · ability badges ·
//! bug badges); BN4 shows the six fixed catalog slots (0A–0F) with each
//! installed card's "name — effect" line and its bug in purple.

use dioxus::prelude::*;
use unic_langid::LanguageIdentifier;

use super::edit::{Edit, PatchCard4Edit, PatchCard56Edit};
use super::{placeholder, stage_edit, EditUi, Loaded, SaveHandle};
use crate::t;
use crate::ui::icons;

/// BN4 catalog-slot labels (the "0A"–"0F" the game shows).
pub(crate) const PATCH_CARD4_SLOT_LABELS: [&str; 6] = ["0A", "0B", "0C", "0D", "0E", "0F"];

/// Total MB an enabled patch-card set may use in BN5/BN6.
pub const MAX_PATCH_CARD56_MB: u32 = 80;

pub(super) fn render_patch_cards(lang: &LanguageIdentifier, loaded: &Loaded) -> Element {
    let Some(view) = loaded.save.view_patch_cards() else {
        return placeholder(t!(lang, "save-empty"));
    };

    match view {
        tango_dataview::save::PatchCardsView::PatchCard56s(v) => {
            let rows = (0..v.count()).filter_map(|i| v.patch_card(i).map(|c| (i, c))).collect::<Vec<_>>();
            rsx! {
                div { class: "pane card-list",
                    for (i, card) in rows {
                        div { class: "card56-row",
                            span { class: "c-idx", "{i + 1:>2}" }
                            {patch_card56_cells(loaded, card.id, card.enabled)}
                        }
                    }
                }
            }
        }
        tango_dataview::save::PatchCardsView::PatchCard4s(v) => {
            rsx! {
                div { class: "pane card-list",
                    for (slot, slot_label) in PATCH_CARD4_SLOT_LABELS.iter().enumerate() {
                        div { class: "card4-row",
                            if let Some(card) = v.patch_card(slot) {
                                {patch_card4_cell(lang, loaded, slot_label, &card)}
                            } else {
                                div { class: "line",
                                    span { class: "slot-badge", "{slot_label}" }
                                    span { class: "muted", {t!(lang, "patch-card4-none")} }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The viewer-style cells for a BN5/BN6 card: name with MB stacked
/// beneath, then fixed-width ability and bug badge columns. Greyed (name
/// struck through) when disabled.
fn patch_card56_cells(loaded: &Loaded, id: usize, enabled: bool) -> Element {
    let info = loaded.assets.patch_card56(id);
    let name = info.as_ref().and_then(|c| c.name()).unwrap_or_else(|| format!("#{id}"));
    let mb = info.as_ref().map(|c| c.mb()).unwrap_or(0);
    let effects = info.as_ref().map(|c| c.effects()).unwrap_or_default();
    let mut abilities: Vec<Element> = Vec::new();
    let mut bugs: Vec<Element> = Vec::new();
    for e in &effects {
        let badge = effect_badge(e, enabled);
        if e.is_ability {
            abilities.push(badge);
        } else {
            bugs.push(badge);
        }
    }
    rsx! {
        div { class: "c-name-col",
            span { class: if enabled { "card-name" } else { "card-name off" }, "{name}" }
            span { class: "card-mb", "{mb}MB" }
        }
        div { class: "c-effects",
            {abilities.into_iter()}
        }
        div { class: "c-effects",
            {bugs.into_iter()}
        }
    }
}

/// A BN4 slot's installed card: slot badge + 3-digit catalog number +
/// "name — effect" (struck + muted when off), the bug line in purple
/// beneath.
fn patch_card4_cell(
    _lang: &LanguageIdentifier,
    loaded: &Loaded,
    slot_label: &str,
    card: &tango_dataview::save::PatchCard,
) -> Element {
    let info = loaded.assets.patch_card4(card.id);
    let name = info.as_ref().and_then(|i| i.name()).unwrap_or_else(|| format!("#{}", card.id));
    let effect = info.as_ref().map(|i| i.effect());
    let label = match effect {
        Some(effect) => format!("{name} — {}", patch_card4_effect_label(effect)),
        None => name,
    };
    let bug = info.as_ref().and_then(|i| patch_card4_bugs_label(i.bugs()));
    let number = format!("{:03}", card.id);
    let enabled = card.enabled;
    rsx! {
        div { class: "line",
            span { class: "slot-badge", "{slot_label}" }
            span { class: "card-number muted", "{number}" }
            span { class: if enabled { "card-name" } else { "card-name off" }, "{label}" }
        }
        if let Some(bug) = bug {
            div { class: "bug-line", "{bug}" }
        }
    }
}

fn effect_badge(e: &tango_dataview::rom::PatchCard56Effect, enabled: bool) -> Element {
    let name = e.name.clone().unwrap_or_else(|| "???".to_string());
    let bg = if enabled {
        if e.is_debuff {
            "#b55ade"
        } else {
            "#ffbd18"
        }
    } else {
        "#bdbdbd"
    };
    rsx! {
        span { class: "effect-badge", style: "background:{bg}", "{name}" }
    }
}

/// Human-readable label for a BN4 patch-card effect, decoded out of the
/// ROM. (B-shortcut chip params are shown raw for now — the shortcut →
/// chip-id table isn't mapped yet.)
pub(crate) fn patch_card4_effect_label(effect: tango_dataview::rom::PatchCard4Effect) -> String {
    use tango_dataview::rom::{
        PatchCard4Aura as A, PatchCard4Color as C, PatchCard4Effect as E, PatchCard4Panel as P,
        PatchCard4PetColor as PT, PatchCard4Soul as S,
    };
    match effect {
        E::None => "—".to_string(),
        E::PetMenu(c) => format!(
            "{} PET menu",
            match c {
                PT::Blue => "Blue",
                PT::Pink => "Pink",
                PT::Green => "Green",
                PT::Black => "Black",
            }
        ),
        E::MaxHp(n) => format!("Max HP +{n}"),
        E::BusterAttack(n) => format!("Buster Attack {}", n as u16 + 1),
        E::BButton(s) => format!("B Button {s:?}"),
        E::BCharge(s) => format!("B Charge {s:?}"),
        E::BLeft(s) => format!("B + ← {s:?}"),
        E::CustomSlots(n) => format!("Custom +{n}"),
        E::MegaFolder(n) => format!("Mega Chip +{n}"),
        E::GigaFolder(n) => format!("Giga Chip +{n}"),
        E::TripleSupporter => "Triple Supporter".to_string(),
        E::PanelStep(p) => format!(
            "{} Panel Step",
            match p {
                P::Broken => "Broken",
                P::Cracked => "Cracked",
                P::Metal => "Metal",
                P::Holy => "Holy",
            }
        ),
        E::FullSynchro => "Full Synchro".to_string(),
        E::Aura(a) => match a {
            A::Barrier100 => "Barrier 100",
            A::Barrier200 => "Barrier 200",
            A::LifeAura => "LifeAura",
        }
        .to_string(),
        E::Soul(s) => format!(
            "{} Soul",
            match s {
                S::Roll => "Roll",
                S::Guts => "Guts",
                S::Wind => "Wind",
                S::Search => "Search",
                S::Fire => "Fire",
                S::Thunder => "Thunder",
                S::Proto => "Proto",
                S::Number => "Number",
                S::Metal => "Metal",
                S::Junk => "Junk",
                S::Aqua => "Aqua",
                S::Wood => "Wood",
            }
        ),
        E::Color(c) => format!(
            "{} MegaMan",
            match c {
                C::Red => "Red",
                C::Yellow => "Yellow",
                C::White => "White",
                C::Green => "Green",
            }
        ),
        E::AllGuard => "All Guard".to_string(),
    }
}

/// Joined human-readable label for a card's bugs, or `None` if it has none.
pub(crate) fn patch_card4_bugs_label(bugs: &[tango_dataview::rom::PatchCard4Bug]) -> Option<String> {
    use tango_dataview::rom::PatchCard4Bug as B;
    if bugs.is_empty() {
        return None;
    }
    Some(
        bugs.iter()
            .map(|b| match b {
                B::Confused => "Start battle Confused",
                B::AutoMove => "Auto-move forward",
                B::Hp(_) => "HP Bug",
                B::CustomHP => "Custom HP Bug",
                B::CustomMinus1 => "Custom −1",
                B::PoisonPanelStep => "Poison Panel Step",
            })
            .collect::<Vec<_>>()
            .join(" & "),
    )
}

/// Sort order for the BN5/BN6 patch-card editor's library pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatchCard56Sort {
    Id,
    Name,
    Mb,
}

impl PatchCard56Sort {
    pub const ALL: [PatchCard56Sort; 3] = [PatchCard56Sort::Id, PatchCard56Sort::Name, PatchCard56Sort::Mb];

    fn label(self, lang: &LanguageIdentifier) -> String {
        match self {
            PatchCard56Sort::Id => t!(lang, "patch-card-sort-id"),
            PatchCard56Sort::Name => t!(lang, "patch-card-sort-name"),
            PatchCard56Sort::Mb => t!(lang, "patch-card-sort-mb"),
        }
    }
}

/// Every PatchCard56 the ROM defines, as `(id, name, mb)`, in `sort`
/// order. The caller applies the name filter and excludes ids already
/// registered.
fn sorted_patch_card56_library(loaded: &Loaded, sort: PatchCard56Sort) -> Vec<(usize, String, u8)> {
    let assets = loaded.assets.as_ref();
    let mut rows: Vec<(usize, String, u8)> = Vec::new();
    for id in 0..assets.num_patch_card56s() {
        let Some(info) = assets.patch_card56(id) else { continue };
        let name = info.name().unwrap_or_else(|| format!("#{id}"));
        rows.push((id, name, info.mb()));
    }
    match sort {
        PatchCard56Sort::Id => {}
        PatchCard56Sort::Name => rows.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0))),
        PatchCard56Sort::Mb => rows.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0))),
    }
    rows
}

/// Dispatch the Patch Cards tab's editor: BN5/BN6 (PatchCard56s) is a
/// variable MB-budgeted list; BN4 (PatchCard4s) is six fixed catalog
/// slots. Wholly separate editors — they only share the tab.
#[component]
pub(super) fn PatchCardsEdit(
    handle: SaveHandle,
    editing: Signal<Option<EditUi>>,
    sort: Signal<PatchCard56Sort>,
) -> Element {
    let is_56 = {
        let l = handle.0.borrow();
        let is_56 = matches!(
            l.save.view_patch_cards(),
            Some(tango_dataview::save::PatchCardsView::PatchCard56s(_))
        );
        is_56
    };
    if is_56 {
        rsx! {
            PatchCard56sEdit { handle, editing, sort }
        }
    } else {
        rsx! {
            PatchCard4sEdit { handle, editing }
        }
    }
}

/// The BN5/BN6 patch-card editor: registered list (left, drag to
/// reorder, ✕ to remove) beside the card library (right, click to
/// register). Every registered card is active; the MB budget is
/// enforced by disabling library rows that wouldn't fit.
#[component]
fn PatchCard56sEdit(handle: SaveHandle, editing: Signal<Option<EditUi>>, sort: Signal<PatchCard56Sort>) -> Element {
    let mut editing = editing;
    let mut sort = sort;
    let lang = crate::i18n::LANG.read().clone();
    let edit_ui = editing.read().clone().unwrap_or_default();
    let mut drag_from = use_signal(|| Option::<usize>::None);

    let loaded_rc = handle.0.clone();
    let loaded = loaded_rc.borrow();
    let Some(tango_dataview::save::PatchCardsView::PatchCard56s(v)) = loaded.save.view_patch_cards() else {
        return placeholder(t!(&lang, "save-empty"));
    };
    let count = v.count();
    let max = loaded.assets.num_patch_card56s();

    let card_mb = |id: usize| loaded.assets.patch_card56(id).map(|c| c.mb() as u32).unwrap_or(0);
    let cards: Vec<(usize, tango_dataview::save::PatchCard)> =
        (0..count).filter_map(|slot| v.patch_card(slot).map(|c| (slot, c))).collect();
    let in_list: std::collections::HashSet<usize> = cards.iter().map(|(_, c)| c.id).collect();
    let enabled_mb: u32 = cards
        .iter()
        .filter(|(_, c)| c.enabled)
        .map(|(_, c)| card_mb(c.id))
        .sum();

    // ----- Left pane: the registered list -----
    let mut list_rows: Vec<Element> = Vec::with_capacity(cards.len());
    for (slot, card) in &cards {
        let slot = *slot;
        let remove = {
            let handle = handle.clone();
            move |()| {
                stage_edit(&handle, Edit::PatchCard56s(PatchCard56Edit::RemoveCard { slot }));
            }
        };
        let on_drop = {
            let handle = handle.clone();
            move |evt: DragEvent| {
                evt.prevent_default();
                let Some(from) = drag_from.take() else { return };
                if from != slot {
                    stage_edit(&handle, Edit::PatchCard56s(PatchCard56Edit::MoveCard { from, to: slot }));
                }
            }
        };
        list_rows.push(rsx! {
            div {
                class: "card56-row edit",
                draggable: true,
                ondragstart: move |_| drag_from.set(Some(slot)),
                ondragover: move |evt: DragEvent| evt.prevent_default(),
                ondrop: on_drop,
                span { class: "grip", icons::GripVertical {} }
                span { class: "c-idx", "{slot + 1:>2}" }
                {patch_card56_cells(&loaded, card.id, card.enabled)}
                {super::remove_button(EventHandler::new(remove))}
            }
        });
    }

    let mb_label = t!(
        &lang,
        "patch-card-edit-mb",
        mb = enabled_mb as i64,
        limit = MAX_PATCH_CARD56_MB as i64
    );
    let count_label = t!(&lang, "patch-card-edit-count", count = count as i64);
    let clear_all = {
        let handle = handle.clone();
        move |()| {
            stage_edit(&handle, Edit::PatchCard56s(PatchCard56Edit::ClearAll));
        }
    };

    // ----- Right pane: the card library -----
    let filter = edit_ui.patch_card56_filter.to_lowercase();
    let list_full = count >= max;
    let mut lib_rows: Vec<Element> = Vec::new();
    for (id, name, mb) in sorted_patch_card56_library(&loaded, sort()) {
        if in_list.contains(&id) {
            continue;
        }
        if !filter.is_empty() && !name.to_lowercase().contains(filter.as_str()) {
            continue;
        }
        // Selectable only if there's room and it fits the MB budget.
        let selectable = !list_full && enabled_mb + mb as u32 <= MAX_PATCH_CARD56_MB;
        let add = {
            let handle = handle.clone();
            move |_| {
                if selectable {
                    stage_edit(&handle, Edit::PatchCard56s(PatchCard56Edit::AddCard { id }));
                }
            }
        };
        lib_rows.push(rsx! {
            div {
                class: if selectable { "card56-row lib" } else { "card56-row lib disabled" },
                onclick: add,
                {patch_card56_cells(&loaded, id, selectable)}
            }
        });
    }

    let sort_options: Vec<String> = PatchCard56Sort::ALL.iter().map(|s| s.label(&lang)).collect();
    let sort_selected = PatchCard56Sort::ALL.iter().position(|s| *s == sort()).unwrap_or(0);

    rsx! {
        div { class: "editor-panes",
            div { class: "pane editor-pane",
                div { class: "editor-header",
                    div { class: "line",
                        span { {t!(&lang, "save-tab-patch-cards")} }
                        span { class: "sub", "{count_label}" }
                        {super::limit_caption(mb_label, enabled_mb > MAX_PATCH_CARD56_MB)}
                        div { class: "grow" }
                        {super::clear_all_button(&lang, EventHandler::new(clear_all))}
                    }
                }
                div { class: "editor-scroll",
                    {list_rows.into_iter()}
                }
            }
            div { class: "pane editor-pane",
                {super::library_header(
                    t!(&lang, "patch-card-edit-search"),
                    edit_ui.patch_card56_filter.clone(),
                    EventHandler::new(move |v: String| {
                        editing.with_mut(|e| {
                            if let Some(e) = e.as_mut() {
                                e.patch_card56_filter = v;
                            }
                        });
                    }),
                    t!(&lang, "save-edit-sort"),
                    sort_options,
                    sort_selected,
                    EventHandler::new(move |i: usize| {
                        if let Some(s) = PatchCard56Sort::ALL.get(i) {
                            sort.set(*s);
                        }
                    }),
                )}
                div { class: "editor-scroll",
                    {lib_rows.into_iter()}
                }
            }
        }
    }
}

/// The BN4 patch-card editor: the six catalog slots (0A–0F) as one
/// form. Each slot has a dropdown of the cards that belong to it (plus
/// "None"), an ON toggle for the installed card, and its bug line in
/// purple beneath.
#[component]
fn PatchCard4sEdit(handle: SaveHandle, editing: Signal<Option<EditUi>>) -> Element {
    let _ = editing; // no scratch state — the slot form edits the save directly
    let lang = crate::i18n::LANG.read().clone();
    let loaded_rc = handle.0.clone();
    let loaded = loaded_rc.borrow();
    let Some(tango_dataview::save::PatchCardsView::PatchCard4s(v)) = loaded.save.view_patch_cards() else {
        return placeholder(t!(&lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();

    // Bucket every card id by the slot it belongs to, so each slot's
    // dropdown lists only its own cards.
    let mut by_slot: [Vec<usize>; PATCH_CARD4_SLOT_LABELS.len()] = std::array::from_fn(|_| Vec::new());
    for id in 0..assets.num_patch_card4s() {
        if let Some(info) = assets.patch_card4(id) {
            let s = info.slot() as usize;
            if let Some(bucket) = by_slot.get_mut(s) {
                bucket.push(id);
            }
        }
    }

    let mut rows: Vec<Element> = Vec::new();
    let mut filled = 0usize;
    for (slot, ids) in by_slot.iter().enumerate() {
        let installed = v.patch_card(slot);
        if installed.is_some() {
            filled += 1;
        }
        let selected_id = installed.as_ref().map(|c| c.id);
        let enabled = installed.as_ref().is_some_and(|c| c.enabled);
        let bug = installed
            .as_ref()
            .and_then(|c| assets.patch_card4(c.id))
            .and_then(|i| patch_card4_bugs_label(i.bugs()));
        let slot_label = PATCH_CARD4_SLOT_LABELS[slot];
        let on_pick = {
            let handle = handle.clone();
            move |val: String| {
                if val.is_empty() {
                    stage_edit(&handle, Edit::PatchCard4s(PatchCard4Edit::RemoveCard { slot }));
                } else if let Ok(id) = val.parse::<usize>() {
                    stage_edit(&handle, Edit::PatchCard4s(PatchCard4Edit::AddCard { id }));
                }
            }
        };
        let toggle = {
            let handle = handle.clone();
            move |()| {
                stage_edit(&handle, Edit::PatchCard4s(PatchCard4Edit::ToggleCard { slot }));
            }
        };
        // Dropdown labels fold the card's effect into the name — within
        // one slot several cards share a name and only the effect tells
        // them apart.
        let choices: Vec<(usize, String)> = ids
            .iter()
            .map(|&id| {
                let info = assets.patch_card4(id);
                let name = info.as_ref().and_then(|c| c.name()).unwrap_or_else(|| format!("#{id}"));
                let effect = info
                    .as_ref()
                    .map(|c| c.effect())
                    .unwrap_or(tango_dataview::rom::PatchCard4Effect::None);
                (id, format!("{id:03} {name} — {}", patch_card4_effect_label(effect)))
            })
            .collect();
        rows.push(rsx! {
            div { class: "card4-row edit",
                div { class: "line",
                    span { class: "slot-badge", "{slot_label}" }
                    crate::ui::widgets::Select {
                        class: "slot-pick",
                        value: selected_id.map(|id| id.to_string()).unwrap_or_default(),
                        options: std::iter::once(crate::ui::widgets::SelectOption::new(
                            "",
                            t!(&lang, "patch-card4-none"),
                        ))
                        .chain(
                            choices
                                .iter()
                                .map(|(id, label)| crate::ui::widgets::SelectOption::new(id.to_string(), label.clone())),
                        )
                        .collect::<Vec<_>>(),
                        onchange: on_pick,
                    }
                    {super::edit_toggle_maybe(
                        "ON",
                        enabled,
                        "#29a121",
                        installed.as_ref().map(|_| EventHandler::new(toggle)),
                    )}
                }
                if let Some(bug) = bug {
                    div { class: "bug-line", "{bug}" }
                }
            }
        });
    }

    let count_label = t!(&lang, "patch-card-edit-count", count = filled as i64);
    let clear_all = {
        let handle = handle.clone();
        move |()| {
            stage_edit(&handle, Edit::PatchCard4s(PatchCard4Edit::ClearAll));
        }
    };

    rsx! {
        div { class: "editor-panes",
            div { class: "pane editor-pane",
                div { class: "editor-header",
                    div { class: "line",
                        span { {t!(&lang, "save-tab-patch-cards")} }
                        span { class: "sub", "{count_label}" }
                        div { class: "grow" }
                        {super::clear_all_button(&lang, EventHandler::new(clear_all))}
                    }
                }
                div { class: "editor-scroll",
                    {rows.into_iter()}
                }
            }
        }
    }
}

/// The patch-cards tab as TSV text.
pub(crate) fn as_text(loaded: &Loaded) -> Option<String> {
    let assets = loaded.assets.as_ref();
    let view = loaded.save.view_patch_cards()?;
    let mut out = String::new();
    match view {
        tango_dataview::save::PatchCardsView::PatchCard56s(v) => {
            for i in 0..v.count() {
                let Some(card) = v.patch_card(i) else { continue };
                if !card.enabled {
                    continue;
                }
                let info = assets.patch_card56(card.id);
                let name = info
                    .as_ref()
                    .and_then(|c| c.name())
                    .unwrap_or_else(|| format!("#{}", card.id));
                let mb = info.as_ref().map(|c| c.mb()).unwrap_or(0);
                out.push_str(&format!("{name}\t{mb}MB\n"));
            }
        }
        tango_dataview::save::PatchCardsView::PatchCard4s(v) => {
            for i in 0..6 {
                let Some(card) = v.patch_card(i) else { continue };
                if !card.enabled {
                    continue;
                }
                let info = assets.patch_card4(card.id);
                let name = info
                    .as_ref()
                    .and_then(|c| c.name())
                    .unwrap_or_else(|| format!("#{}", card.id));
                out.push_str(&format!("0{}\t{name}\n", ['A', 'B', 'C', 'D', 'E', 'F'][i]));
            }
        }
    }
    Some(out)
}

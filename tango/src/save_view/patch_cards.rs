use super::*;
use sweeten::widget::{column, pick_list, row};

// ---------- Patch cards ----------

pub(super) fn render_patch_cards<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
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
                let [name_cell, ability_cell, bug_cell] =
                    patch_card56_cells::<M>(loaded, &name, mb, card.enabled, card.id);

                let row = row![
                    text(format!("{:>2}", i + 1))
                        .size(TEXT_CAPTION)
                        .width(Length::Fixed(24.0)),
                    name_cell,
                    ability_cell,
                    bug_cell,
                ]
                .spacing(8)
                .align_y(Alignment::Start);
                list = list.push(
                    container(row)
                        .padding(style::ROW_PADDING)
                        .style(crate::widgets::zebra_row(i)),
                );
            }
        }
        tango_dataview::save::PatchCardsView::PatchCard4s(v) => {
            // Mirror the BN4 editor's slot form: a slot badge + the card's
            // "name — effect" line, with the bug (if any) in purple beneath.
            for (slot, slot_label) in PATCH_CARD4_SLOT_LABELS.iter().enumerate() {
                let badge: Element<'static, M> =
                    container(text(*slot_label).size(TEXT_BODY).font(iced::Font::MONOSPACE))
                        .width(Length::Fixed(34.0))
                        .align_x(iced::alignment::Horizontal::Center)
                        .into();
                let cell: Element<'static, M> = match v.patch_card(slot) {
                    Some(card) => {
                        let info = assets.patch_card4(card.id);
                        let name = info
                            .as_ref()
                            .and_then(|i| i.name())
                            .unwrap_or_else(|| format!("#{}", card.id));
                        let effect = info.as_ref().map(|i| i.effect());
                        // 3-digit catalog number, then the "name — effect"
                        // line (name struck + everything muted when off).
                        let number = text(format!("{:03}", card.id))
                            .size(TEXT_BODY)
                            .font(iced::Font::MONOSPACE)
                            .style(muted_text_style);
                        let label = patch_card_name(
                            match effect {
                                Some(effect) => format!("{name} — {}", patch_card4_effect_label(effect)),
                                None => name,
                            },
                            card.enabled,
                        );
                        let mut col = column![row![badge, number, container(label).width(Length::Fill)]
                            .spacing(8)
                            .align_y(Alignment::Center)]
                        .spacing(2);
                        if let Some(bug) = info.as_ref().and_then(|i| patch_card4_bugs_label(i.bugs())) {
                            col = col.push(
                                row![
                                    Space::new().width(Length::Fixed(44.0)),
                                    text(bug)
                                        .size(TEXT_BODY)
                                        .color(iced::Color::from_rgb8(0xb5, 0x5a, 0xde)),
                                ]
                                .spacing(0),
                            );
                        }
                        col.into()
                    }
                    None => row![
                        badge,
                        text(t!(lang, "patch-card4-none"))
                            .size(TEXT_BODY)
                            .style(muted_text_style)
                            .width(Length::Fill),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .into(),
                };
                list = list.push(
                    container(cell)
                        .width(Fill)
                        .padding([8, 10])
                        .style(crate::widgets::zebra_row(slot)),
                );
            }
        }
    }

    container(list).width(Fill).style(crate::widgets::pane).into()
}

/// Every PatchCard56 the ROM defines, as `(id, name, mb)`, in `sort`
/// order. The caller applies the name filter and excludes ids already in
/// the registered list. Ties fall back to id for a stable order.
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

/// A patch-card name as an Element. Built as rich text so a disabled card's
/// name can be struck through (and muted) to read as inactive at a glance —
/// iced's strikethrough lives on rich-text spans. `on_link_click(never)`
/// pins the span's link type; these spans are never links.
fn patch_card_name<'a, M: 'a>(name: String, enabled: bool) -> Element<'a, M> {
    let mut el = iced::widget::rich_text([iced::widget::text::Span::new(name).strikethrough(!enabled)])
        .on_link_click(iced::never)
        .size(TEXT_BODY);
    if !enabled {
        el = el.style(muted_text_style);
    }
    el.into()
}

/// The viewer-style cells for a patch card: `[name+MB, abilities, bugs]`,
/// matching [`render_patch_cards`]'s column layout exactly (name with MB
/// stacked beneath, then a fixed-width ability column and bug column, each
/// a vertical stack of [`effect_badge`]s). Greyed when `enabled` is false.
/// Callers wrap these with a leading cell (index / add button) and, for the
/// registered list, trailing edit controls.
fn patch_card56_cells<'a, M: 'static>(
    loaded: &Loaded,
    name: &str,
    mb: u8,
    enabled: bool,
    id: usize,
) -> [Element<'a, M>; 3] {
    let effects = loaded.assets.patch_card56(id).map(|c| c.effects()).unwrap_or_default();
    let name_text = patch_card_name(name.to_string(), enabled);
    let name_col = column![name_text, text(format!("{mb}MB")).size(10).style(muted_text_style)].spacing(2);
    let mut ability_col = column![].spacing(2);
    for e in effects.iter().filter(|e| e.is_ability) {
        ability_col = ability_col.push(effect_badge::<M>(e, enabled));
    }
    let mut bug_col = column![].spacing(2);
    for e in effects.iter().filter(|e| !e.is_ability) {
        bug_col = bug_col.push(effect_badge::<M>(e, enabled));
    }
    [
        container(name_col).width(Length::Fill).into(),
        // Fixed-width ability / bug columns, matching the read-only viewer.
        container(ability_col).width(Length::Fixed(180.0)).into(),
        container(bug_col).width(Length::Fixed(180.0)).into(),
    ]
}

/// One registered patch card, laid out like a [`render_patch_cards`] row
/// (index · name+MB · abilities · bugs) with an enable toggle and an ✕
/// remove button appended. The name dims while the card is disabled.
/// `can_enable` is whether enabling this (currently disabled) card would
/// still fit the MB budget; it gates the ON toggle.
fn patch_card56_list_row<'a>(
    loaded: &'a Loaded,
    slot: usize,
    card: tango_dataview::save::PatchCard,
    can_enable: bool,
) -> Element<'a, Action> {
    let info = loaded.assets.patch_card56(card.id);
    let name = info
        .as_ref()
        .and_then(|c| c.name())
        .unwrap_or_else(|| format!("#{}", card.id));
    let mb = info.as_ref().map(|c| c.mb()).unwrap_or(0);
    let [name_cell, ability_cell, bug_cell] = patch_card56_cells(loaded, &name, mb, card.enabled, card.id);

    // Green "ON" toggle (matches the folder editor's TAG tint), then the
    // ✕ that backs the card out to the library. The toggle is disabled
    // when the card is off and enabling it would exceed the MB budget (an
    // already-on card can always be turned off).
    let toggle_msg = (card.enabled || can_enable).then_some(Action::TogglePatchCard56 { slot });
    let toggle = edit_toggle_maybe("ON", card.enabled, iced::Color::from_rgb8(0x29, 0xa1, 0x21), toggle_msg);
    let remove = remove_button(Action::RemovePatchCard56 { slot });

    let row = row![
        drag_handle(),
        text(format!("{:>2}", slot + 1))
            .size(TEXT_CAPTION)
            .width(Length::Fixed(24.0)),
        name_cell,
        ability_cell,
        bug_cell,
        toggle,
        remove,
    ]
    .spacing(8)
    .align_y(Alignment::Start);
    // Left padding trimmed (vs the usual 10) so the drag handle sits flush in
    // the gutter, matching the folder editor's grip.
    container(row)
        .padding(iced::Padding {
            top: 6.0,
            right: 10.0,
            bottom: 6.0,
            left: 6.0,
        })
        .style(crate::widgets::zebra_row(slot))
        .into()
}

/// One library card, laid out like a [`render_patch_cards`] row (index ·
/// name+MB · abilities · bugs). The whole row is a click-to-add button
/// (the palette affordance) that registers the card; effects show
/// enabled, since adding a card enables it when it fits. Disabled
/// (unclickable) when the list is full.
fn patch_card56_library_row<'a>(
    loaded: &'a Loaded,
    id: usize,
    name: String,
    mb: u8,
    row_idx: usize,
    list_full: bool,
) -> Element<'a, Action> {
    let [name_cell, ability_cell, bug_cell] = patch_card56_cells(loaded, &name, mb, true, id);

    let row = row![name_cell, ability_cell, bug_cell]
        .spacing(8)
        .align_y(Alignment::Start);
    // The entire row is the add control: clicking anywhere registers the
    // card. `list_item` paints the zebra base + hover highlight, so it
    // doubles as the palette's "click me" affordance.
    let mut b = button(row)
        .width(Fill)
        .padding(style::ROW_PADDING)
        .style(crate::widgets::list_item(false, row_idx));
    if !list_full {
        b = b.on_press(Action::AddPatchCard56 { id });
    }
    b.into()
}

/// Dispatch the Patch Cards tab's editor to the right implementation:
/// BN5/BN6 (PatchCard56s) is a variable MB-budgeted list; BN4
/// (PatchCard4s) is six fixed catalog slots. They're wholly separate
/// editors — the only thing they share is the tab.
pub(super) fn render_patch_cards_edit<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a Loaded,
    state: &'a State,
) -> Element<'a, Action> {
    match loaded.save.view_patch_cards() {
        Some(tango_dataview::save::PatchCardsView::PatchCard56s(_)) => render_patch_card56s_edit(lang, loaded, state),
        Some(tango_dataview::save::PatchCardsView::PatchCard4s(_)) => render_patch_card4s_edit(lang, loaded, state),
        None => placeholder(t!(lang, "save-empty")),
    }
}

/// The BN5/BN6 patch-card editor: a two-pane layout (registered list left,
/// card library right) whose rows match the read-only viewer (index ·
/// name+MB stacked · ability column · bug column), with edit controls
/// appended — an enable toggle + remove on the list, an add button on the
/// library. Edits stage live in the loaded save and are written to disk
/// only on Save.
fn render_patch_card56s_edit<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a Loaded,
    state: &'a State,
) -> Element<'a, Action> {
    // Only reached while editing, so the EditState is present.
    let Some(edit) = state.editing.as_ref() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let Some(tango_dataview::save::PatchCardsView::PatchCard56s(v)) = loaded.save.view_patch_cards() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let count = v.count();
    let max = loaded.assets.num_patch_card56s();

    // ----- Left pane: the registered list -----
    // MB of each card (0 for the "no card" id / unknown), so the budget
    // and per-row gating are computed from one source.
    let card_mb = |id: usize| loaded.assets.patch_card56(id).map(|c| c.mb() as u32).unwrap_or(0);
    let cards: Vec<(usize, tango_dataview::save::PatchCard)> = (0..count)
        .filter_map(|slot| v.patch_card(slot).map(|c| (slot, c)))
        .collect();
    let in_list: std::collections::HashSet<usize> = cards.iter().map(|(_, c)| c.id).collect();
    let enabled_mb: u32 = cards
        .iter()
        .filter(|(_, c)| c.enabled)
        .map(|(_, c)| card_mb(c.id))
        .sum();

    let mut list_rows: Vec<Element<'a, Action>> = Vec::with_capacity(cards.len());
    for (slot, card) in &cards {
        // A disabled card can be enabled only if it still fits the budget;
        // an enabled card can always be turned off.
        let can_enable = enabled_mb + card_mb(card.id) <= MAX_PATCH_CARD56_MB;
        list_rows.push(patch_card56_list_row(loaded, *slot, card.clone(), can_enable));
    }
    // Draggable list: grab a card row and drop it to reorder the registered
    // order (dense list, so any drop is a valid ordered move).
    let list_col = sweeten::widget::Column::from_vec(list_rows)
        .width(Fill)
        .spacing(3)
        .style(reorder_drag_style)
        .on_drag(Action::ReorderPatchCard56s);
    // MB total turns red if it somehow exceeds the limit (e.g. a save
    // imported over-budget); the editor itself never lets it go over.
    let mb_text = limit_caption(
        t!(
            lang,
            "patch-card-edit-mb",
            mb = enabled_mb as i64,
            limit = MAX_PATCH_CARD56_MB
        ),
        enabled_mb > MAX_PATCH_CARD56_MB,
    );
    let count_caption = text(t!(lang, "patch-card-edit-count", count = count as i64))
        .size(TEXT_CAPTION)
        .style(muted_text_style);
    let list_header = editor_header(
        lang,
        t!(lang, "save-tab-patch-cards"),
        vec![count_caption.into(), mb_text.into()],
        Action::ClearPatchCard56s,
    );
    let list_pane = editor_pane(list_header, list_col);

    // ----- Right pane: the card library -----
    let filter = edit.patch_card56_filter.to_lowercase();
    let list_full = count >= max;
    let mut lib_col = column![].spacing(3).padding(0);
    let mut shown = 0usize;
    for (id, name, mb) in sorted_patch_card56_library(loaded, state.patch_card56_sort) {
        if in_list.contains(&id) {
            continue;
        }
        if !filter.is_empty() && !name.to_lowercase().contains(filter.as_str()) {
            continue;
        }
        lib_col = lib_col.push(patch_card56_library_row(loaded, id, name, mb, shown, list_full));
        shown += 1;
    }
    let lib_header = library_header(
        lang,
        t!(lang, "patch-card-edit-search"),
        &edit.patch_card56_filter,
        Action::PatchCard56FilterChanged,
        &PatchCard56Sort::ALL,
        state.patch_card56_sort,
        PatchCard56Sort::label,
        Action::PatchCard56SortChanged,
    );
    editor_panes(list_pane, editor_pane(lib_header, lib_col))
}

// ---------- BN4 patch cards ----------

/// BN4 catalog-slot labels (the "0A"–"0F" the game shows). A BN4 patch
/// card belongs to exactly one of these six slots, and a slot holds at most
/// one card — so the editor is a per-slot picker, not the BN5/BN6 list.
const PATCH_CARD4_SLOT_LABELS: [&str; 6] = ["0A", "0B", "0C", "0D", "0E", "0F"];

/// One slot's row in the BN4 editor: the slot label, a dropdown of every
/// card that belongs to this slot (plus "None" to empty it), an ON/off
/// toggle for the installed card, and — since a card's downside isn't in
/// the dropdown label — its bug line in purple underneath.
fn patch_card4_slot_row<'a>(
    loaded: &'a Loaded,
    slot: usize,
    installed: Option<tango_dataview::save::PatchCard>,
    choices: Vec<PatchCard4Choice>,
) -> Element<'a, Action> {
    use crate::widgets;
    let badge = container(
        text(PATCH_CARD4_SLOT_LABELS[slot])
            .size(TEXT_BODY)
            .font(iced::Font::MONOSPACE),
    )
    .width(Length::Fixed(34.0))
    .align_x(iced::alignment::Horizontal::Center);

    let selected_id = installed.as_ref().map(|c| c.id);
    let selected = choices.iter().find(|c| c.id == selected_id).cloned();
    let picker = pick_list(choices, selected, move |c: PatchCard4Choice| match c.id {
        Some(id) => Action::AddPatchCard4 { id },
        None => Action::RemovePatchCard4 { slot },
    })
    .width(Fill)
    .padding(style::CONTROL_PADDING)
    .text_size(TEXT_BODY)
    .style(widgets::chunky_pick_list);

    // The ON toggle shows on every row (so the column stays aligned); an
    // empty slot has nothing to enable, so it renders disabled (greyed,
    // unclickable). Green matches the other editors' "on" tint.
    let toggle = edit_toggle_maybe(
        "ON",
        installed.as_ref().is_some_and(|c| c.enabled),
        iced::Color::from_rgb8(0x29, 0xa1, 0x21),
        installed.as_ref().map(|_| Action::TogglePatchCard4 { slot }),
    );
    let top = row![badge, picker, toggle].spacing(10).align_y(Alignment::Center);

    let mut cell = column![top].spacing(2);
    // Bug line for the installed card, aligned under the dropdown (past the
    // slot badge). The effect is already in the dropdown label; the bug is
    // the downside the user should still see at a glance.
    if let Some(bug) = installed
        .as_ref()
        .and_then(|c| loaded.assets.patch_card4(c.id))
        .and_then(|i| patch_card4_bugs_label(i.bugs()))
    {
        cell = cell.push(
            row![
                Space::new().width(Length::Fixed(44.0)),
                text(bug)
                    .size(TEXT_BODY)
                    .color(iced::Color::from_rgb8(0xb5, 0x5a, 0xde)),
            ]
            .spacing(0),
        );
    }
    container(cell)
        .width(Fill)
        .padding([8, 10])
        .style(crate::widgets::zebra_row(slot))
        .into()
}

/// The BN4 patch-card editor: the six catalog slots (0A–0F) as a single
/// form. Each slot has a dropdown of the cards that belong to it (plus
/// "None"), so the model is "pick one card per slot" — matching the in-game
/// Mod Card screen — rather than the BN5/BN6 collection-from-a-library.
/// There's no MB budget. Edits stage live in the loaded save and are
/// written to disk only on Save.
fn render_patch_card4s_edit<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a Loaded,
    state: &'a State,
) -> Element<'a, Action> {
    use crate::widgets;
    // Only reached while editing, so the EditState is present.
    if state.editing.is_none() {
        return placeholder(t!(lang, "save-empty"));
    }
    let Some(tango_dataview::save::PatchCardsView::PatchCard4s(v)) = loaded.save.view_patch_cards() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();

    // Bucket every card id by the slot it belongs to (one pass), so each
    // slot's dropdown lists only its own cards.
    let mut by_slot: [Vec<usize>; PATCH_CARD4_SLOT_LABELS.len()] = std::array::from_fn(|_| Vec::new());
    for id in 0..assets.num_patch_card4s() {
        if let Some(info) = assets.patch_card4(id) {
            let s = info.slot() as usize;
            if let Some(bucket) = by_slot.get_mut(s) {
                bucket.push(id);
            }
        }
    }

    let mut rows = column![].spacing(3).padding(0);
    let mut filled = 0usize;
    for (slot, ids) in by_slot.iter().enumerate() {
        let installed = v.patch_card(slot);
        if installed.is_some() {
            filled += 1;
        }
        let mut choices = vec![PatchCard4Choice::none(lang)];
        choices.extend(ids.iter().map(|&id| PatchCard4Choice::card(loaded, id)));
        rows = rows.push(patch_card4_slot_row(loaded, slot, installed, choices));
    }

    let count_caption = text(t!(lang, "patch-card-edit-count", count = filled as i64))
        .size(TEXT_CAPTION)
        .style(muted_text_style);
    let header = editor_header(
        lang,
        t!(lang, "save-tab-patch-cards"),
        vec![count_caption.into()],
        Action::ClearPatchCard4s,
    );

    container(column![
        header,
        scrollable(rows)
            .style(crate::widgets::chunky_scrollable)
            .height(Fill)
            .width(Fill)
    ])
    .width(Fill)
    .height(Fill)
    .style(widgets::pane)
    .into()
}

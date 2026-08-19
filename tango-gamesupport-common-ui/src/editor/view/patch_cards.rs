use super::*;
use sweeten::widget::{column, row};

// ---------- Patch cards ----------

/// The BN5/BN6 patch-card list, read-only. BN4's six-slot Mod Card form
/// lives in the bn4 UI crate — the only thing the two share is the tab.
pub fn render_patch_cards56<M: 'static>(lang: &LanguageIdentifier, loaded: &OpenSave) -> Element<'static, M> {
    let Some(v) = loaded.save.view_patch_card56s() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let enabled_mb: u32 = (0..v.count())
        .filter_map(|i| v.patch_card(i))
        .filter(|card| card.enabled)
        .map(|card| assets.patch_card56(card.id).map(|c| c.mb() as u32).unwrap_or(0))
        .sum();
    let overflow_slots = crate::dataview::build::patch_card56_overflow_slots(loaded.save.as_ref(), assets);

    let mut list = column![].spacing(3).padding(0);
    for i in 0..v.count() {
        let Some(card) = v.patch_card(i) else { continue };
        let info = assets.patch_card56(card.id);
        let name = info
            .as_ref()
            .and_then(|c| c.name())
            .unwrap_or_else(|| format!("#{}", card.id));
        let mb = info.as_ref().map(|c| c.mb()).unwrap_or(0);
        let danger = overflow_slots.contains(&i);
        let issue = danger.then(|| patch_card56_issue(lang, mb, enabled_mb));
        let [name_cell, param_cell, ability_cell] =
            patch_card56_cells::<M>(loaded, &name, mb, card.enabled, danger, card.id);

        let row = row![
            text(format!("{:>2}", i + 1))
                .size(TEXT_CAPTION)
                .width(Length::Fixed(24.0)),
            name_cell,
            param_cell,
            ability_cell,
        ]
        .spacing(8)
        .align_y(Alignment::Start);
        let row: Element<'static, M> =
            container(row)
                .padding(style::ROW_PADDING)
                .style(move |theme: &iced::Theme| {
                    let mut style = crate::widgets::zebra_row(i)(theme);
                    if danger {
                        style.text_color = Some(theme.palette().danger);
                    }
                    style
                })
                .into();
        list = list.push(folder::detail_popover_with_issue(row, None, None, None, issue));
    }

    container(list).width(Fill).style(crate::widgets::pane).into()
}

/// The compact warning attached directly to an over-budget card row.
fn patch_card56_issue(lang: &LanguageIdentifier, mb: u8, used: u32) -> String {
    crate::build::patch_card_slot_warning(lang, mb, used, MAX_PATCH_CARD56_MB)
}

/// Every PatchCard56 the ROM defines, as `(id, name, mb)`, in `sort`
/// order. The caller applies the name filter and excludes ids already in
/// the registered list. Ties fall back to id for a stable order.
fn sorted_patch_card56_library(loaded: &OpenSave, sort: PatchCard56Sort) -> Vec<(usize, String, u8)> {
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
pub fn patch_card_name<'a, M: 'a>(name: String, enabled: bool) -> Element<'a, M> {
    patch_card_name_maybe_danger(name, enabled, false)
}

fn patch_card_name_maybe_danger<'a, M: 'a>(name: String, enabled: bool, danger: bool) -> Element<'a, M> {
    let mut el = iced::widget::rich_text([iced::widget::text::Span::new(name).strikethrough(!enabled)])
        .on_link_click(iced::never)
        .size(TEXT_BODY);
    if !enabled {
        el = el.style(muted_text_style);
    } else if danger {
        el = el.style(crate::widgets::danger_text_style);
    }
    el.into()
}

/// The viewer-style cells for a patch card: `[name+MB, parameters,
/// abilities]`, matching [`render_patch_cards56`]'s column layout exactly
/// (name with MB stacked beneath, then a fixed-width parameter column and
/// ability column, each a vertical stack of [`effect_badge`]s). The game's
/// own mod-card screen orders them パラメータ then アビリティ, so we do
/// too. Greyed when `enabled` is false. Callers wrap these with a leading
/// cell (index / add button) and, for the registered list, trailing edit
/// controls.
fn patch_card56_cells<'a, M: 'static>(
    loaded: &OpenSave,
    name: &str,
    mb: u8,
    enabled: bool,
    danger: bool,
    id: usize,
) -> [Element<'a, M>; 3] {
    let effects = loaded.assets.patch_card56(id).map(|c| c.effects()).unwrap_or_default();
    let name_text = patch_card_name_maybe_danger(name.to_string(), enabled, danger);
    let mb_text = text(format!("{mb}MB")).size(10).style(move |theme: &iced::Theme| {
        if danger {
            crate::widgets::danger_text_style(theme)
        } else {
            muted_text_style(theme)
        }
    });
    let name_col = column![name_text, mb_text].spacing(2);
    let mut param_col = column![].spacing(2);
    for e in effects.iter().filter(|e| !e.is_ability) {
        param_col = param_col.push(effect_badge::<M>(e, enabled));
    }
    let mut ability_col = column![].spacing(2);
    for e in effects.iter().filter(|e| e.is_ability) {
        ability_col = ability_col.push(effect_badge::<M>(e, enabled));
    }
    [
        container(name_col).width(Length::Fill).into(),
        // Fixed-width parameter / ability columns, matching the read-only
        // viewer. The badges inside fill them, so their edges line up down
        // the list instead of going ragged with the effect names.
        container(param_col).width(Length::Fixed(BADGE_COLUMN_WIDTH)).into(),
        container(ability_col).width(Length::Fixed(BADGE_COLUMN_WIDTH)).into(),
    ]
}

/// One registered patch card, laid out like a [`render_patch_cards56`] row
/// (index · name+MB · parameters · abilities) with an ✕ remove button appended.
/// Every newly registered card is active — there's no enable/disable toggle —
/// so the list is simply the set of equipped cards. An over-budget set is an
/// editor error that blocks Save, and its enabled rows render danger-red.
fn patch_card56_list_row<'a>(
    lang: &LanguageIdentifier,
    loaded: &'a OpenSave,
    slot: usize,
    card: crate::dataview::save::PatchCard,
    enabled_mb: u32,
    danger: bool,
) -> Element<'a, Action> {
    let info = loaded.assets.patch_card56(card.id);
    let name = info
        .as_ref()
        .and_then(|c| c.name())
        .unwrap_or_else(|| format!("#{}", card.id));
    let mb = info.as_ref().map(|c| c.mb()).unwrap_or(0);
    let issue = danger.then(|| patch_card56_issue(lang, mb, enabled_mb));
    let [name_cell, param_cell, ability_cell] = patch_card56_cells(loaded, &name, mb, card.enabled, danger, card.id);

    // Just the ✕ that backs the card out to the library.
    let remove = remove_button(Action::RemovePatchCard56 { slot });

    let row = row![
        drag_handle(),
        text(format!("{:>2}", slot + 1))
            .size(TEXT_CAPTION)
            .width(Length::Fixed(24.0)),
        name_cell,
        param_cell,
        ability_cell,
        remove,
    ]
    .spacing(8)
    .align_y(Alignment::Start);
    // Left padding trimmed (vs the usual 10) so the drag handle sits flush in
    // the gutter, matching the folder editor's grip.
    let row = container(row)
        .padding(iced::Padding {
            top: 6.0,
            right: 10.0,
            bottom: 6.0,
            left: 6.0,
        })
        .style(move |theme: &iced::Theme| {
            let mut style = crate::widgets::zebra_row(slot)(theme);
            if danger {
                style.text_color = Some(theme.palette().danger);
            }
            style
        })
        .into();
    folder::detail_popover_with_issue(row, None, None, None, issue)
}

/// One library card, laid out like a [`render_patch_cards56`] row (index ·
/// name+MB · parameters · abilities). The whole row is a click-to-add button (the
/// palette affordance) that registers the card. A full list renders the row
/// greyed and unclickable. An addition that would exceed the MB budget remains
/// clickable but renders danger-red; Save stays disabled until the error is
/// resolved.
fn patch_card56_library_row<'a>(
    lang: &LanguageIdentifier,
    loaded: &'a OpenSave,
    id: usize,
    name: String,
    mb: u8,
    row_idx: usize,
    addable: bool,
    enabled_mb_after_add: u32,
) -> Element<'a, Action> {
    let issue = (addable && enabled_mb_after_add > MAX_PATCH_CARD56_MB)
        .then(|| patch_card56_issue(lang, mb, enabled_mb_after_add));
    let danger = issue.is_some();
    let [name_cell, param_cell, ability_cell] = patch_card56_cells(loaded, &name, mb, addable, danger, id);

    let row = row![name_cell, param_cell, ability_cell]
        .spacing(8)
        .align_y(Alignment::Start);
    // The entire row is the add control: clicking anywhere registers the
    // card. `list_item` paints the zebra base + hover highlight, so it
    // doubles as the palette's "click me" affordance.
    let b = button(row).width(Fill).padding(style::ROW_PADDING);
    let mut b = if danger {
        b.style(crate::widgets::danger_text_list_item(row_idx))
    } else {
        b.style(crate::widgets::list_item(false, row_idx))
    };
    if addable {
        b = b.on_press(Action::AddPatchCard56 { id });
    }
    folder::detail_popover_with_issue(b.into(), None, None, None, issue)
}

/// The BN5/BN6 patch-card editor: a two-pane layout (registered list left,
/// card library right) whose rows match the read-only viewer (index ·
/// name+MB stacked · ability column · bug column), with edit controls
/// appended — an enable toggle + remove on the list, an add button on the
/// library. Edits stage live in the loaded save and are written to disk
/// only on Save.
pub fn render_patch_cards56_edit<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a OpenSave,
    state: &'a State,
) -> Element<'a, Action> {
    // Only reached while editing, so the EditState is present.
    let Some(edit) = state.editing.as_ref() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let Some(v) = loaded.save.view_patch_card56s() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let count = v.count();
    let max = loaded.assets.num_patch_card56s();

    // ----- Left pane: the registered list -----
    // MB of each card (0 for the "no card" id / unknown), so the budget
    // and per-row gating are computed from one source.
    let card_mb = |id: usize| loaded.assets.patch_card56(id).map(|c| c.mb() as u32).unwrap_or(0);
    let cards: Vec<(usize, crate::dataview::save::PatchCard)> = (0..count)
        .filter_map(|slot| v.patch_card(slot).map(|c| (slot, c)))
        .collect();
    let in_list: std::collections::HashSet<usize> = cards.iter().map(|(_, c)| c.id).collect();
    let enabled_mb: u32 = cards
        .iter()
        .filter(|(_, c)| c.enabled)
        .map(|(_, c)| card_mb(c.id))
        .sum();

    let mut list_rows: Vec<Element<'a, Action>> = Vec::with_capacity(cards.len());
    let over_mb_limit = enabled_mb > MAX_PATCH_CARD56_MB;
    let overflow_slots =
        crate::dataview::build::patch_card56_overflow_slots(loaded.save.as_ref(), loaded.assets.as_ref());
    for (slot, card) in &cards {
        list_rows.push(patch_card56_list_row(
            lang,
            loaded,
            *slot,
            card.clone(),
            enabled_mb,
            overflow_slots.contains(slot),
        ));
    }
    // Draggable list: grab a card row and drop it to reorder the registered
    // order (dense list, so any drop is a valid ordered move).
    let list_col = sweeten::widget::Column::from_vec(list_rows)
        .width(Fill)
        .spacing(3)
        .style(reorder_drag_style)
        .on_drag(Action::ReorderPatchCard56s);
    // The MB budget is save-blocking: its total and contributing enabled card
    // rows turn red when exceeded, though cards remain addable while editing.
    let mb_text = limit_caption(
        t!(
            lang,
            "patch-card-edit-mb",
            mb = enabled_mb as i64,
            limit = MAX_PATCH_CARD56_MB
        ),
        over_mb_limit,
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
        // Only the hard list capacity disables adding. Like folder-limit
        // violations, a choice that would exceed the MB budget stays clickable
        // and turns danger-red.
        let enabled_mb_after_add = enabled_mb + mb as u32;
        lib_col = lib_col.push(patch_card56_library_row(
            lang,
            loaded,
            id,
            name,
            mb,
            shown,
            !list_full,
            enabled_mb_after_add,
        ));
        shown += 1;
    }
    let lib_header = library_header(
        lang,
        t!(lang, "patch-card-edit-search"),
        &edit.patch_card56_filter,
        Action::PatchCard56FilterChanged,
        &PatchCard56Sort::ALL,
        state.patch_card56_sort,
        patch_card56_sort_label,
        Action::PatchCard56SortChanged,
    );
    editor_panes(list_pane, editor_pane(lib_header, lib_col))
}

/// Advisory total MB budget across enabled PatchCard56s. Over-budget sets and
/// would-be additions render danger-red but are allowed.
pub use crate::model::rules::MAX_PATCH_CARD56_MB;

/// Localized label for a [`PatchCard56Sort`] picker entry (see
/// `folder::library_sort_label` for why it's a free function).
pub fn patch_card56_sort_label(sort: PatchCard56Sort, lang: &LanguageIdentifier) -> String {
    match sort {
        PatchCard56Sort::Id => t!(lang, "patch-card-sort-id"),
        PatchCard56Sort::Name => t!(lang, "patch-card-sort-name"),
        PatchCard56Sort::Mb => t!(lang, "patch-card-sort-mb"),
    }
}

/// The BN5/BN6 patch-cards tab as TSV text.
pub fn as_text56(loaded: &OpenSave) -> Option<String> {
    let assets = loaded.assets.as_ref();
    let v = loaded.save.view_patch_card56s()?;
    let mut out = String::new();
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
        out.push_str(&format!("{name}\t{mb}MB\n",));
    }
    Some(out)
}

fn effect_badge<M: 'static>(e: &crate::dataview::rom::PatchCard56Effect, enabled: bool) -> Element<'static, M> {
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

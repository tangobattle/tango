use super::*;
use sweeten::widget::{column, row};

pub(super) fn render_folder<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded, grouped: bool) -> Element<'static, M> {
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
    let mut chips: Vec<Option<tango_dataview::save::Chip>> =
        (0..MAX_FOLDER_CHIPS).map(|i| chips_view.chip(folder_idx, i)).collect();
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
/// slot. Each pane scrolls independently. The equipped navi's
/// [`tango_dataview::save::FolderLimits`] (mega/giga caps, per-chip copy
/// cap, Regular/Tag memory) are surfaced in the folder header and enforced
/// by greying out library chips / REG / TAG toggles that would break them.
pub(super) fn render_folder_edit<'a>(lang: &'a LanguageIdentifier, loaded: &'a Loaded, state: &'a State) -> Element<'a, Action> {
    // Only reached while editing, so the EditState is present.
    let Some(edit) = state.editing.as_ref() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let Some(chips_view) = loaded.save.view_chips() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let folder_idx = chips_view.equipped_folder_index();
    // Outer Some = the game has the feature, so show its toggle.
    let reg = chips_view.regular_chip_index(folder_idx);
    let regular_supported = reg.is_some();
    let regular_idx = reg.flatten();
    let tag_supported = chips_view.tag_chip_indexes(folder_idx).is_some();

    // Folder-construction limits for the equipped navi (mega/giga class
    // caps, per-chip copy cap, Regular/Tag memory budgets). `None` for
    // games that don't define them — those stay unrestricted.
    let assets = loaded.assets.as_ref();
    let limits = loaded.save.folder_limits(assets);
    let usage = FolderUsage::scan(loaded, folder_idx);
    // If exactly one Tag chip is picked, a second can only join if the
    // pair's combined MB fits Tag memory; capture the partner's MB so each
    // slot can test its own addition.
    let tag_partner_mb: Option<u32> = match edit.tags.as_slice() {
        [only] => chips_view
            .chip(folder_idx, *only)
            .and_then(|c| assets.chip(c.id))
            .map(|c| c.mb() as u32),
        _ => None,
    };

    // ----- Left pane: the folder -----
    let filled = (0..MAX_FOLDER_CHIPS)
        .filter(|&i| chips_view.chip(folder_idx, i).is_some())
        .count();
    let mut folder_rows: Vec<Element<'a, Action>> = Vec::with_capacity(MAX_FOLDER_CHIPS);
    for slot in 0..MAX_FOLDER_CHIPS {
        let chip = chips_view.chip(folder_idx, slot);
        let is_regular = regular_idx == Some(slot);
        let is_tag = edit.tags.contains(&slot);
        // This slot's chip MB, for the Regular / Tag memory gates.
        let this_mb = chip.as_ref().and_then(|c| assets.chip(c.id)).map(|c| c.mb());
        // A chip can be made Regular only if its MB fits Regular memory;
        // clearing the current Regular is always allowed.
        let reg_allowed = match limits.reg_memory {
            Some(cap) => is_regular || this_mb.map_or(true, |mb| mb <= cap),
            None => true,
        };
        // It can join the Tag pair only if it fits Tag memory on its own
        // (a chip bigger than the whole budget can never be tagged) and,
        // once a partner is picked, the pair's combined MB still fits.
        // Deselecting is always allowed.
        let tag_allowed = match limits.tag_memory {
            Some(budget) => {
                is_tag || {
                    let this = this_mb.map(|m| m as u32).unwrap_or(0);
                    this <= budget && tag_partner_mb.map_or(true, |partner| partner + this <= budget)
                }
            }
            None => true,
        };
        folder_rows.push(folder_slot_row(
            loaded,
            slot,
            chip,
            is_regular,
            regular_supported,
            tag_supported,
            is_tag,
            reg_allowed,
            tag_allowed,
        ));
    }
    // Draggable list: grab a chip row and drop it to reorder. The handler
    // ignores drops involving an empty slot, so only chips move (no dragging a
    // gap, no dropping into one).
    // `width(Fill)` is required because the rows contain `Fill` cells — unlike
    // iced's `column!`, sweeten's `from_vec` defaults to `Shrink` and won't
    // adapt to Fill children (they'd collapse to zero width, hiding the rows).
    let folder_list = sweeten::widget::Column::from_vec(folder_rows)
        .width(Fill)
        .spacing(1)
        .style(reorder_drag_style)
        .on_drag(Action::ReorderChips);
    let clear_all = clear_all_button(lang, Action::ClearFolder);
    // "Folder" label, then a smaller count that turns red while the
    // folder is short of the 30 chips a legal folder needs.
    let count = limit_caption(
        t!(
            lang,
            "folder-edit-count",
            count = filled as i64,
            limit = MAX_FOLDER_CHIPS
        ),
        filled < MAX_FOLDER_CHIPS,
    );
    let header_row = row![
        text(t!(lang, "folder-edit-folder")).size(TEXT_BODY),
        count,
        Space::new().width(Fill),
        clear_all,
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    // Second line (only for navis with folder limits): mega/dark/giga usage vs
    // their caps (red when over) plus the Regular/Tag memory budgets.
    let stats_row = {
        let mut r = row![].spacing(12).align_y(Alignment::Center);
        // Per-class usage vs cap, red when over. Labels are resolved
        // up front so every `t!` key stays a literal.
        let class_stats = [
            limits.navi_limit.map(|l| {
                (
                    t!(lang, "folder-edit-navi", used = usage.navi as i64, limit = l as i64),
                    usage.navi > l,
                )
            }),
            limits.mega_limit.map(|l| {
                (
                    t!(lang, "folder-edit-mega", used = usage.mega as i64, limit = l as i64),
                    usage.mega > l,
                )
            }),
            limits.giga_limit.map(|l| {
                (
                    t!(lang, "folder-edit-giga", used = usage.giga as i64, limit = l as i64),
                    usage.giga > l,
                )
            }),
            limits.dark_limit.map(|l| {
                (
                    t!(lang, "folder-edit-dark", used = usage.dark as i64, limit = l as i64),
                    usage.dark > l,
                )
            }),
        ];
        for (label, over) in class_stats.into_iter().flatten() {
            r = r.push(limit_caption(label, over));
        }
        if let Some(reg) = limits.reg_memory {
            r = r.push(
                text(t!(lang, "folder-edit-reg-memory", mb = reg as i64))
                    .size(TEXT_CAPTION)
                    .style(muted_text_style),
            );
        }
        if let Some(tag) = limits.tag_memory {
            r = r.push(
                text(t!(lang, "folder-edit-tag-memory", mb = tag as i64))
                    .size(TEXT_CAPTION)
                    .style(muted_text_style),
            );
        }
        r
    };
    let header_col = column![header_row, stats_row].spacing(4);
    let folder_header = container(header_col).width(Fill).padding(style::HEADER_PADDING);
    let folder_pane = editor_pane(folder_header, folder_list);

    // ----- Right pane: the chip library -----
    let chips_have_mb = loaded.assets.chips_have_mb();
    let filter = edit.library_filter.to_lowercase();
    let mut lib_list = column![].spacing(1).padding(0);
    let mut shown = 0usize;
    for (id, name, code) in sorted_library_entries(loaded, state.library_sort) {
        if !filter.is_empty() && !name.to_lowercase().contains(filter.as_str()) {
            continue;
        }
        // Disabled when the folder is full or adding this chip would break
        // the navi's mega/giga/copy limits.
        let addable = filled < MAX_FOLDER_CHIPS && usage.can_add(loaded, id, &limits);
        lib_list = lib_list.push(library_entry_row(loaded, id, name, code, shown, chips_have_mb, addable));
        shown += 1;
    }
    let lib_header = library_header(
        lang,
        t!(lang, "folder-edit-search"),
        &edit.library_filter,
        Action::LibraryFilterChanged,
        &LibrarySort::ALL,
        state.library_sort,
        LibrarySort::label,
        Action::LibrarySortChanged,
    );
    editor_panes(folder_pane, editor_pane(lib_header, lib_list))
}

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
    reg_allowed: bool,
    tag_allowed: bool,
) -> Element<'a, Action> {
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
            let name = info
                .as_ref()
                .and_then(|i| i.name())
                .unwrap_or_else(|| "???".to_string());
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
                // Greyed out (no message) when the chip's MB won't fit
                // Regular memory; see render_folder_edit.
                inner = inner.push(edit_toggle_maybe(
                    "REG",
                    is_regular,
                    iced::Color::from_rgb8(0xff, 0x42, 0xa5),
                    reg_allowed.then_some(Action::ToggleRegular { slot }),
                ));
            }
            if tag_supported {
                // Greyed out when joining the Tag pair would bust Tag memory.
                inner = inner.push(edit_toggle_maybe(
                    "TAG",
                    is_tag,
                    iced::Color::from_rgb8(0x29, 0xa1, 0x21),
                    tag_allowed.then_some(Action::ToggleTag { slot }),
                ));
            }
            // ✕ → remove this chip (back out to the library).
            inner = inner.push(remove_button(Action::RemoveChip { slot }));
        }
        None => {
            inner = inner.push(text("—").size(TEXT_BODY).style(muted_text_style).width(Fill));
        }
    }
    // Drag handle in the far-left gutter (left of the accent stripe) on filled
    // rows; empty slots get a same-width spacer so the stripes stay aligned and
    // aren't draggable anyway.
    let leading: Option<Element<'a, Action>> = Some(if chip.is_some() {
        drag_handle()
    } else {
        Space::new().width(Length::Fixed(16.0)).into()
    });
    // Tooltip wraps only the chip content — not the leading grip gutter — so
    // hovering the drag handle doesn't pop the chip card.
    let tipped = with_chip_tooltip(loaded, chip_id, accent, inner.padding([3, 12]).into());
    edit_row_wrap(tipped, accent, slot, leading)
}

/// One chip+code in the editor's right pane (the library / palette).
/// Shows the chip's stats (element / code / ATK / MB, like the read-only
/// list). The whole row is a click-to-add button that drops this
/// chip+code into the folder; it's disabled (`addable == false`) when the
/// folder is full or adding the chip would break the navi's folder limits.
fn library_entry_row<'a>(
    loaded: &'a Loaded,
    chip_id: usize,
    name: String,
    code: tango_dataview::save::ChipCode,
    row_idx: usize,
    chips_have_mb: bool,
    addable: bool,
) -> Element<'a, Action> {
    use crate::widgets;
    let info = loaded.assets.chip(chip_id);
    let accent = class_accent(
        info.as_ref().map(|i| i.class()),
        info.as_ref().map(|i| i.dark()).unwrap_or(false),
    );
    let [element, atk, mb] = chip_stat_cells(loaded, chip_id, chips_have_mb);

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
    .align_y(Alignment::Center);

    // The whole row is the add control: clicking anywhere drops this
    // chip+code into the folder. The class-accent stripe rides inside the
    // button as its leading column (flush left — the button carries no
    // padding of its own), so `list_item`'s zebra base paints the full
    // width behind it and the gutter stays tinted even when a chip has no
    // accent. Same composition as `edit_row_wrap` / `card_wrap`, so the
    // library row isn't a bespoke wrapper. Disabled when not addable (no
    // empty slot, or it would break the navi's folder limits). ChipCode is Copy.
    let stripe: Element<'a, Action> = container(Space::new())
        .width(Length::Fixed(6.0))
        .height(Length::Fill)
        .style(move |_t: &iced::Theme| container::Style {
            background: accent.map(iced::Background::Color),
            ..Default::default()
        })
        .into();
    let content = row![stripe, container(inner).width(Fill).padding([3, 12])]
        .height(Length::Shrink)
        .align_y(Alignment::Center);
    let mut body = button(content)
        .width(Fill)
        .padding(0)
        .style(widgets::list_item(false, row_idx));
    if addable {
        body = body.on_press(Action::AddChip { chip_id, code });
    }
    // Un-addable chips (folder full, or adding would break a folder limit)
    // read as disabled: a translucent wash in the pane's background colour
    // over the whole non-pressable row. The Stack takes the button's size,
    // so the wash covers it exactly.
    let row_el: Element<'a, Action> = if addable {
        body.into()
    } else {
        stack![
            body,
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(|theme: &iced::Theme| container::Style {
                    background: Some(iced::Background::Color(iced::Color {
                        a: 0.6,
                        ..theme.palette().background
                    })),
                    ..Default::default()
                }),
        ]
        .into()
    };
    with_chip_tooltip(loaded, Some(chip_id), accent, row_el)
}

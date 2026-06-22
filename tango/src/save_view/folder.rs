use super::*;
use sweeten::widget::{column, row};

pub(super) fn render_folder<M: 'static>(
    lang: &LanguageIdentifier,
    loaded: &Loaded,
    grouped: bool,
) -> Element<'static, M> {
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

    // Pull the 30-chip folder. The Regular chip is stored at its real
    // grid slot in every game that has one, so `regular_idx` already is
    // its display position.
    let chips: Vec<Option<tango_dataview::save::Chip>> =
        (0..MAX_FOLDER_CHIPS).map(|i| chips_view.chip(folder_idx, i)).collect();

    // Build display items: either grouped (collapsed by chip identity)
    // or per-slot (one row per slot, possibly empty).
    type Item = (Option<tango_dataview::save::Chip>, GroupedChip);
    let items: Vec<Item> = if grouped {
        let mut grouped_map: indexmap::IndexMap<Option<tango_dataview::save::Chip>, GroupedChip> =
            indexmap::IndexMap::new();
        for (i, chip) in chips.iter().enumerate() {
            let g = grouped_map.entry(chip.clone()).or_default();
            g.count += 1;
            if regular_idx == Some(i) {
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
                        is_regular: regular_idx == Some(i),
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
pub(super) fn render_folder_edit<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a Loaded,
    state: &'a State,
) -> Element<'a, Action> {
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
        // A chip can be made Regular only if its MB fits Regular memory and
        // it isn't already a Tag chip (a chip can't be both); clearing the
        // current Regular is always allowed.
        let reg_fits = match limits.reg_memory {
            Some(cap) => this_mb.map_or(true, |mb| mb <= cap),
            None => true,
        };
        let reg_allowed = is_regular || (!is_tag && reg_fits);
        // It can join the Tag pair only if it fits Tag memory on its own
        // (a chip bigger than the whole budget can never be tagged), the
        // pair's combined MB still fits once a partner is picked, and it
        // isn't the Regular chip. Deselecting is always allowed.
        let tag_fits = match limits.tag_memory {
            Some(budget) => {
                let this = this_mb.map(|m| m as u32).unwrap_or(0);
                this <= budget && tag_partner_mb.map_or(true, |partner| partner + this <= budget)
            }
            None => true,
        };
        let tag_allowed = is_tag || (!is_regular && tag_fits);
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

/// Sort order for the editor's chip-library (right) pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibrarySort {
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

fn sorted_library_entries(loaded: &Loaded, sort: LibrarySort) -> Vec<(usize, String, tango_dataview::save::ChipCode)> {
    use tango_dataview::save::ChipCode;
    let assets = loaded.assets.as_ref();
    let chips_view = loaded.save.view_chips();
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
        let Some(name) = info.name() else { continue };
        let (atk, elem, mb) = (info.attack_power(), info.element(), info.mb());
        // One row per valid code (e.g. Cannon A / Cannon B / Cannon *),
        // but only for codes the player owns (pack count > 0). `variant`
        // is the code's index within the chip's code list — the index the
        // pack table is keyed by. Ids past the pack table (Program
        // Advances, etc.) return `None` and are dropped. The editor only
        // renders for games with a pack, so a missing count means "not
        // owned", not "unsupported".
        for (variant, ch) in info.codes().into_iter().enumerate() {
            let Some(code) = ChipCode::from_char(ch) else { continue };
            let owned = chips_view
                .as_ref()
                .and_then(|v| v.pack_count(id, variant))
                .map_or(false, |c| c > 0);
            if !owned {
                continue;
            }
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
        LibrarySort::Name => rows.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then(a.id.cmp(&b.id))
                .then(a.code_rank.cmp(&b.code_rank))
        }),
        LibrarySort::Code => rows.sort_by(|a, b| a.code_rank.cmp(&b.code_rank).then(a.id.cmp(&b.id))),
        LibrarySort::Attack => rows.sort_by(|a, b| {
            a.atk
                .cmp(&b.atk)
                .then(a.id.cmp(&b.id))
                .then(a.code_rank.cmp(&b.code_rank))
        }),
        LibrarySort::Element => rows.sort_by(|a, b| {
            a.elem
                .cmp(&b.elem)
                .then(a.id.cmp(&b.id))
                .then(a.code_rank.cmp(&b.code_rank))
        }),
        LibrarySort::Mb => rows.sort_by(|a, b| {
            a.mb.cmp(&b.mb)
                .then(a.id.cmp(&b.id))
                .then(a.code_rank.cmp(&b.code_rank))
        }),
    }
    rows.into_iter().map(|e| (e.id, e.name, e.code)).collect()
}

/// Mega/Giga class usage and per-chip copies in one folder, used to honor
/// the equipped navi's [`tango_dataview::save::FolderLimits`] in both the
/// editor UI (greying out un-addable library chips) and the apply path
/// ([`crate::app`]'s `apply_chip_edit`). Built by scanning the folder's 30
/// slots; cheap enough to rebuild per edit / per frame.
pub struct FolderUsage {
    pub navi: usize,
    pub mega: usize,
    pub giga: usize,
    pub dark: usize,
    /// Copies installed per chip id (codes collapsed — the copy cap is
    /// per chip, not per code).
    pub copies: std::collections::HashMap<usize, usize>,
}

impl FolderUsage {
    /// Tally the equipped folder's 30 slots.
    pub fn scan(loaded: &Loaded, folder_idx: usize) -> Self {
        use tango_dataview::rom::ChipClass;
        let assets = loaded.assets.as_ref();
        let mut navi = 0;
        let mut mega = 0;
        let mut giga = 0;
        let mut dark = 0;
        let mut copies: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        if let Some(view) = loaded.save.view_chips() {
            for slot in 0..MAX_FOLDER_CHIPS {
                let Some(c) = view.chip(folder_idx, slot) else { continue };
                *copies.entry(c.id).or_insert(0) += 1;
                let Some(chip) = assets.chip(c.id) else {
                    continue;
                };
                if chip.dark() {
                    dark += 1;
                    continue;
                }
                match chip.class() {
                    ChipClass::Navi => navi += 1,
                    ChipClass::Mega => mega += 1,
                    ChipClass::Giga => giga += 1,
                    _ => {}
                }
            }
        }
        Self {
            navi,
            mega,
            giga,
            dark,
            copies,
        }
    }

    /// Whether one more copy of `chip_id` fits under `limits` — the
    /// per-chip copy cap plus the mega/giga class cap. The folder-full
    /// (30-slot) check is separate. Unknown chips aren't blocked.
    pub fn can_add(&self, loaded: &Loaded, chip_id: usize, limits: &tango_dataview::save::FolderLimits) -> bool {
        use tango_dataview::rom::ChipClass;
        let Some(info) = loaded.assets.chip(chip_id) else {
            return true;
        };
        if self.copies.get(&chip_id).copied().unwrap_or(0) >= (limits.max_copies)(info.as_ref()) {
            return false;
        }
        if info.dark() {
            return limits.dark_limit.map(|limit| self.dark < limit).unwrap_or(true);
        }
        match info.class() {
            ChipClass::Navi => limits.navi_limit.map(|limit| self.navi < limit).unwrap_or(true),
            ChipClass::Mega => limits.mega_limit.map(|limit| self.mega < limit).unwrap_or(true),
            ChipClass::Giga => limits.giga_limit.map(|limit| self.giga < limit).unwrap_or(true),
            _ => true,
        }
    }
}

/// Whether the equipped folder satisfies the navi's
/// [`tango_dataview::save::FolderLimits`] — the mega/giga class caps, the
/// per-chip copy cap, and Regular/Tag memory. `true` when the game defines
/// no limits. Gates Save: the folder pane blocks *adding* a violation, but
/// cross-tab edits can still leave an already-built folder illegal (e.g.
/// pulling a MegFldr part on the Navi tab lowers the mega cap under the
/// chips already in the folder), and a save edited elsewhere may arrive
/// over a limit.
pub(crate) fn folder_limits_satisfied(loaded: &Loaded) -> bool {
    let Some(view) = loaded.save.view_chips() else {
        return true;
    };
    let folder_idx = view.equipped_folder_index();
    let limits = loaded.save.folder_limits(&*loaded.assets);
    let usage = FolderUsage::scan(loaded, folder_idx);
    if limits.navi_limit.map(|limit| usage.navi > limit).unwrap_or(false)
        || limits.mega_limit.map(|limit| usage.mega > limit).unwrap_or(false)
        || limits.giga_limit.map(|limit| usage.giga > limit).unwrap_or(false)
        || limits.dark_limit.map(|limit| usage.dark > limit).unwrap_or(false)
    {
        return false;
    }
    // Per-chip copy cap.
    for (&id, &count) in &usage.copies {
        if let Some(chip) = loaded.assets.chip(id) {
            if count > (limits.max_copies)(chip.as_ref()) {
                return false;
            }
        }
    }
    let mb_of = |slot: usize| {
        view.chip(folder_idx, slot)
            .and_then(|c| loaded.assets.chip(c.id))
            .map_or(0u32, |c| c.mb() as u32)
    };
    // The Regular chip must fit Regular memory.
    if let Some(cap) = limits.reg_memory {
        if let Some(Some(reg)) = view.regular_chip_index(folder_idx) {
            if mb_of(reg) > cap as u32 {
                return false;
            }
        }
    }
    // The Tag pair's combined MB must fit Tag memory.
    if let Some(budget) = limits.tag_memory {
        if let Some(Some([a, b])) = view.tag_chip_indexes(folder_idx) {
            if mb_of(a) + mb_of(b) > budget {
                return false;
            }
        }
    }
    true
}

/// Number of chip slots in an equipped folder.
pub const MAX_FOLDER_CHIPS: usize = 30;

#[derive(Default)]
pub(crate) struct GroupedChip {
    pub(crate) count: usize,
    pub(crate) is_regular: bool,
    pub(crate) has_tag1: bool,
    pub(crate) has_tag2: bool,
}

// `code = None` skips the code badge (Auto Battle Data slots
// have a chip id but no code). `show_count_cell` toggles the
// leading "N×" column — on for the folder's grouped mode, off
// for ABD.
pub(crate) fn chip_row<M: 'static>(
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

/// Accent color for the left edge of a chip row. None = no accent (the
/// row reads as a default chip with no class adornment).
pub(crate) fn class_accent(class: Option<tango_dataview::rom::ChipClass>, dark: bool) -> Option<iced::Color> {
    if dark {
        return Some(iced::Color::from_rgb8(0x4a, 0x55, 0x82));
    }
    match class {
        Some(tango_dataview::rom::ChipClass::Mega) => Some(iced::Color::from_rgb8(0x52, 0x84, 0x9c)),
        Some(tango_dataview::rom::ChipClass::Giga) => Some(iced::Color::from_rgb8(0xc4, 0x52, 0x84)),
        _ => None,
    }
}

/// 28×28 chip icon. Empty (`None`) renders a same-sized spacer so empty
/// rows keep the same height as filled ones.
pub(crate) fn chip_icon<'a>(loaded: &'a Loaded, chip_id: Option<usize>) -> Element<'a, Action> {
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

/// Build the chip popover — scaled artwork above its description — and wrap
/// `inner` with it as a follow-cursor tooltip. Returns `inner` unchanged when
/// the chip has neither artwork nor a description. `accent` tints the popover
/// background to match the chip's class stripe.
pub(crate) fn chip_popover<'a, M: 'a>(
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
pub(crate) fn with_chip_tooltip<'a>(
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

/// Element-icon / ATK / MB stat cells shared by both editor panes,
/// matching the read-only chip list's columns. The MB cell collapses to
/// nothing when the game doesn't use MB.
pub(crate) fn chip_stat_cells<'a>(loaded: &'a Loaded, chip_id: usize, chips_have_mb: bool) -> [Element<'a, Action>; 3] {
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

pub(crate) fn chip_tooltip_style(accent: Option<iced::Color>) -> impl Fn(&iced::Theme) -> container::Style {
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

/// The folder tab as TSV text for clipboard "copy as text".
pub(crate) fn as_text(loaded: &Loaded, opts: RenderOpts) -> Option<String> {
    let assets = loaded.assets.as_ref();
    let chips_view = loaded.save.view_chips()?;
    let folder_idx = chips_view.equipped_folder_index();
    // Read-only display treats "unsupported" and "unset" the same.
    let regular_idx = chips_view.regular_chip_index(folder_idx).flatten();
    let tag_idxs = chips_view.tag_chip_indexes(folder_idx).flatten();

    let chips: Vec<Option<tango_dataview::save::Chip>> =
        (0..MAX_FOLDER_CHIPS).map(|i| chips_view.chip(folder_idx, i)).collect();

    let mut out = String::new();
    if opts.folder_grouped {
        let mut grouped_map: indexmap::IndexMap<Option<tango_dataview::save::Chip>, GroupedChip> =
            indexmap::IndexMap::new();
        for (i, chip) in chips.iter().enumerate() {
            let g = grouped_map.entry(chip.clone()).or_default();
            g.count += 1;
            if regular_idx == Some(i) {
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
            if regular_idx == Some(i) {
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

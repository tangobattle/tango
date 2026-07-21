//! The Folder tab: the equipped folder's 30 chips, grouped by identity
//! or one row per slot (the desktop's `save_view/folder.rs` read view).

use dioxus::prelude::*;
use unic_langid::LanguageIdentifier;

use super::edit::{ChipEdit, Edit};
use super::{placeholder, stage_edit, ChipHover, EditUi, Loaded, SaveHandle, CHIP_HOVER};
use crate::t;
use crate::ui::icons;

/// Number of chip slots in an equipped folder.
pub const MAX_FOLDER_CHIPS: usize = 30;

#[derive(Default)]
pub(crate) struct GroupedChip {
    pub(crate) count: usize,
    pub(crate) is_regular: bool,
    pub(crate) has_tag1: bool,
    pub(crate) has_tag2: bool,
}

pub(super) fn render_folder(lang: &LanguageIdentifier, loaded: &Loaded, grouped: bool) -> Element {
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

    let chips: Vec<Option<tango_dataview::save::Chip>> =
        (0..MAX_FOLDER_CHIPS).map(|i| chips_view.chip(folder_idx, i)).collect();

    // Build display items: either grouped (collapsed by chip identity)
    // or per-slot (one row per filled slot).
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

    // When ungrouped, skip empty slots so we don't waste a full-height
    // row on each "—" (matching the desktop's read view).
    rsx! {
        div { class: "pane chip-list",
            for (chip, g) in items.iter().filter(|(c, _)| grouped || c.is_some()) {
                {chip_row(
                    loaded,
                    chip.as_ref().map(|c| c.id),
                    chip.as_ref().map(|c| c.code.to_string()),
                    g,
                    grouped,
                    chips_have_mb,
                )}
            }
        }
    }
}

/// One chip row: `N×` count (grouped mode), 28px icon, name + REG/TAG
/// badges, 28px element icon, right-aligned code / ATK / MB columns, all
/// on a zebra wash behind a class-accent stripe. `code = None` skips the
/// code cell (Auto Battle Data slots have a chip id but no code);
/// `show_count_cell` toggles the leading count column.
pub(crate) fn chip_row(
    loaded: &Loaded,
    chip_id: Option<usize>,
    code: Option<String>,
    g: &GroupedChip,
    show_count_cell: bool,
    chips_have_mb: bool,
) -> Element {
    let info = chip_id.and_then(|id| loaded.assets.chip(id));
    let chip_class = info.as_ref().map(|i| i.class());
    let dark = info.as_ref().map(|i| i.dark()).unwrap_or(false);
    let accent = class_accent(chip_class, dark);
    let is_empty_slot = chip_id.is_none();

    let icon = chip_id.and_then(|id| loaded.chip_icons.get(id).cloned().flatten());
    let element_icon = info
        .as_ref()
        .map(|i| i.element())
        .and_then(|id| loaded.element_icons.get(&id).cloned());
    let name_text = info.as_ref().and_then(|i| i.name()).unwrap_or_else(|| "???".to_string());
    let power = info.as_ref().map(|i| i.attack_power()).unwrap_or(0);
    let mb = info.as_ref().map(|i| i.mb()).unwrap_or(0);
    let count_is_one = g.count == 1;
    let stripe_style = accent.map(|a| format!("background:{a}")).unwrap_or_default();
    // The popover only pops for chips with hover content, mirroring the
    // desktop's chip tooltip.
    let hoverable = chip_id.is_some();

    rsx! {
        div {
            class: "chip-row-wrap",
            onmousemove: move |evt| {
                if let Some(id) = chip_id.filter(|_| hoverable) {
                    let p = evt.client_coordinates();
                    *CHIP_HOVER.write() = Some(ChipHover {
                        chip_id: id,
                        accent,
                        x: p.x,
                        y: p.y,
                    });
                }
            },
            onmouseleave: move |_| {
                *CHIP_HOVER.write() = None;
            },
            div { class: "stripe", style: "{stripe_style}" }
            div { class: "chip-cells",
                if show_count_cell {
                    span { class: if count_is_one { "c-count muted" } else { "c-count" }, "{g.count}×" }
                }
                if let Some(url) = icon {
                    img { class: "c-icon pix", src: "{url}", alt: "" }
                } else {
                    span { class: "c-icon" }
                }
                div { class: "c-name",
                    if is_empty_slot {
                        span { class: "muted", "—" }
                    } else {
                        span { "{name_text}" }
                    }
                    if g.is_regular {
                        span { class: "chip-flag reg", "REG" }
                    }
                    for _ in 0..(g.has_tag1 as usize + g.has_tag2 as usize) {
                        span { class: "chip-flag tag", "TAG" }
                    }
                }
                if let Some(url) = element_icon {
                    img { class: "c-elem pix", src: "{url}", alt: "" }
                } else {
                    span { class: "c-elem" }
                }
                if let Some(code) = code.filter(|s| !s.is_empty()) {
                    span { class: "c-code", "{code}" }
                }
                span { class: "c-atk", if power > 0 { "{power}" } }
                if chips_have_mb {
                    span { class: "c-mb", if mb > 0 { "{mb}MB" } }
                }
            }
        }
    }
}

/// Accent color for the left edge of a chip row. `None` = no accent (the
/// row reads as a default chip with no class adornment). Colors match the
/// desktop's `class_accent`.
pub(crate) fn class_accent(class: Option<tango_dataview::rom::ChipClass>, dark: bool) -> Option<&'static str> {
    if dark {
        return Some("#4a5582");
    }
    match class {
        Some(tango_dataview::rom::ChipClass::Mega) => Some("#52849c"),
        Some(tango_dataview::rom::ChipClass::Giga) => Some("#c45284"),
        _ => None,
    }
}

/// Mega/Giga class usage and per-chip copies in one folder, used to
/// honor the equipped navi's `FolderLimits` in both the editor UI
/// (greying out un-addable library chips) and the apply path.
pub(crate) struct FolderUsage {
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
    /// per-chip copy cap plus the class caps. The folder-full (30-slot)
    /// check is separate. Unknown chips aren't blocked.
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

/// Whether the equipped folder satisfies the navi's `FolderLimits`.
/// `true` when the game defines no limits. Gates Save — cross-tab edits
/// can leave an already-built folder illegal (e.g. pulling a MegFldr
/// part lowers the mega cap under the chips already in the folder).
pub(crate) fn folder_limits_satisfied(loaded: &Loaded) -> bool {
    let Some(view) = loaded.save.view_chips() else {
        return true;
    };
    let folder_idx = view.equipped_folder_index();
    let limits = loaded
        .save
        .view_navi()
        .map(|nv| nv.folder_limits(&*loaded.assets))
        .unwrap_or_default();
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

    pub(crate) fn label(self, lang: &LanguageIdentifier) -> String {
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

/// Every owned chip×code in the library pane, in `sort` order (one row
/// per owned code variant — `pack_count > 0`). Ties fall back to
/// (id, code) so the order stays stable.
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
        for (variant, ch) in info.codes().into_iter().enumerate() {
            let Some(code) = ChipCode::from_char(ch) else { continue };
            let owned = chips_view
                .as_ref()
                .and_then(|v| v.pack_count(id, variant))
                .is_some_and(|c| c > 0);
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

/// Element-icon / [code] / ATK / MB stat cells shared by the editor
/// panes, in the read-only chip list's column order.
pub(super) fn stat_cells(loaded: &Loaded, chip_id: usize, code: Option<String>, chips_have_mb: bool) -> Element {
    let info = loaded.assets.chip(chip_id);
    let element = info
        .as_ref()
        .map(|i| i.element())
        .and_then(|id| loaded.element_icons.get(&id).cloned());
    let power = info.as_ref().map(|i| i.attack_power()).unwrap_or(0);
    let mb = info.as_ref().map(|i| i.mb()).unwrap_or(0);
    rsx! {
        if let Some(url) = element {
            img { class: "c-elem pix", src: "{url}", alt: "" }
        } else {
            span { class: "c-elem" }
        }
        if let Some(code) = code {
            span { class: "c-code", "{code}" }
        }
        span { class: "c-atk", if power > 0 { "{power}" } }
        if chips_have_mb {
            span { class: "c-mb", if mb > 0 { "{mb}MB" } }
        }
    }
}

/// The editable Folder tab: the folder's 30 raw slots (left) beside the
/// owned-chip library (right). Chips reorder by dragging rows; REG/TAG
/// toggle where the game supports them, greyed when the chip's MB won't
/// fit the memory budgets; library rows click-to-add, greyed when the
/// folder is full or the navi's limits would break.
#[component]
pub(super) fn FolderEdit(handle: SaveHandle, editing: Signal<Option<EditUi>>, sort: Signal<LibrarySort>) -> Element {
    let mut editing = editing;
    let mut sort = sort;
    let lang = crate::i18n::LANG.read().clone();
    let edit_ui = editing.read().clone().unwrap_or_default();
    // The slot a drag started from, while one is in flight.
    let mut drag_from = use_signal(|| Option::<usize>::None);

    let loaded_rc = handle.0.clone();
    let loaded = loaded_rc.borrow();
    let Some(chips_view) = loaded.save.view_chips() else {
        return placeholder(t!(&lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let folder_idx = chips_view.equipped_folder_index();
    // Outer Some = the game has the feature, so show its toggle.
    let reg = chips_view.regular_chip_index(folder_idx);
    let regular_supported = reg.is_some();
    let regular_idx = reg.flatten();
    let tag_supported = chips_view.tag_chip_indexes(folder_idx).is_some();
    let chips_have_mb = assets.chips_have_mb();

    let limits = loaded
        .save
        .view_navi()
        .map(|nv| nv.folder_limits(assets))
        .unwrap_or_default();
    let usage = FolderUsage::scan(&loaded, folder_idx);
    // If exactly one Tag chip is picked, a second can only join if the
    // pair's combined MB fits Tag memory.
    let tag_partner_mb: Option<u32> = match edit_ui.tags.as_slice() {
        [only] => chips_view
            .chip(folder_idx, *only)
            .and_then(|c| assets.chip(c.id))
            .map(|c| c.mb() as u32),
        _ => None,
    };

    let filled = (0..MAX_FOLDER_CHIPS)
        .filter(|&i| chips_view.chip(folder_idx, i).is_some())
        .count();
    let filled_flags: Vec<bool> = (0..MAX_FOLDER_CHIPS)
        .map(|i| chips_view.chip(folder_idx, i).is_some())
        .collect();

    // A completed drop: resolve like the desktop — dropping onto an
    // empty slot lands right after the last chip, never leaving a gap.
    let on_drop = {
        let handle = handle.clone();
        let filled_flags = filled_flags.clone();
        move |target: usize| {
            let Some(from) = drag_from.take() else { return };
            if !filled_flags.get(from).copied().unwrap_or(false) {
                return;
            }
            let to = if filled_flags.get(target).copied().unwrap_or(false) {
                target
            } else {
                match filled_flags.iter().rposition(|&f| f) {
                    Some(last) => last,
                    None => return,
                }
            };
            if from == to {
                return;
            }
            editing.with_mut(|e| {
                if let Some(e) = e.as_mut() {
                    e.move_tags(from, to);
                }
            });
            stage_edit(&handle, Edit::Chips(ChipEdit::MoveChip { from, to }));
        }
    };

    // ----- Left pane: the folder's 30 slots -----
    let mut slot_rows: Vec<Element> = Vec::with_capacity(MAX_FOLDER_CHIPS);
    for slot in 0..MAX_FOLDER_CHIPS {
        let chip = chips_view.chip(folder_idx, slot);
        let is_regular = regular_idx == Some(slot);
        let is_tag = edit_ui.tags.contains(&slot);
        let this_mb = chip.as_ref().and_then(|c| assets.chip(c.id)).map(|c| c.mb());
        // A chip can be made Regular only if its MB fits Regular memory
        // and it isn't already a Tag chip; clearing is always allowed.
        let reg_fits = match limits.reg_memory {
            Some(cap) => this_mb.is_none_or(|mb| mb <= cap),
            None => true,
        };
        let reg_allowed = is_regular || (!is_tag && reg_fits);
        // It can join the Tag pair only if it fits Tag memory on its own,
        // the pair's combined MB still fits once a partner is picked, and
        // it isn't the Regular chip. Deselecting is always allowed.
        let tag_fits = match limits.tag_memory {
            Some(budget) => {
                let this = this_mb.map(|m| m as u32).unwrap_or(0);
                this <= budget && tag_partner_mb.is_none_or(|partner| partner + this <= budget)
            }
            None => true,
        };
        let tag_allowed = is_tag || (!is_regular && tag_fits && edit_ui.tags.len() < 2);

        let info = chip.as_ref().and_then(|c| assets.chip(c.id));
        let accent = class_accent(
            info.as_ref().map(|i| i.class()),
            info.as_ref().map(|i| i.dark()).unwrap_or(false),
        );
        let stripe_style = accent.map(|a| format!("background:{a}")).unwrap_or_default();
        let chip_id = chip.as_ref().map(|c| c.id);
        let code = chip.as_ref().map(|c| c.code.to_string()).unwrap_or_default();
        let name = info.as_ref().and_then(|i| i.name()).unwrap_or_else(|| "???".to_string());
        let icon = chip_id.and_then(|id| loaded.chip_icons.get(id).cloned().flatten());
        let has_chip = chip.is_some();
        let mut on_drop = on_drop.clone();

        let toggle_reg = {
            let handle = handle.clone();
            move |()| {
                stage_edit(&handle, Edit::Chips(ChipEdit::ToggleRegular { slot }));
            }
        };
        let toggle_tag = {
            let handle = handle.clone();
            move |()| {
                let mut pair = None;
                editing.with_mut(|e| {
                    if let Some(e) = e.as_mut() {
                        pair = e.toggle_tag(slot);
                    }
                });
                stage_edit(&handle, Edit::Chips(ChipEdit::SetTags(pair)));
            }
        };
        let remove = {
            let handle = handle.clone();
            move |()| {
                editing.with_mut(|e| {
                    if let Some(e) = e.as_mut() {
                        e.compact_tags(slot);
                    }
                });
                stage_edit(&handle, Edit::Chips(ChipEdit::RemoveChip { slot }));
            }
        };

        slot_rows.push(rsx! {
            div {
                class: "chip-row-wrap edit",
                draggable: has_chip,
                ondragstart: move |_| drag_from.set(Some(slot)),
                ondragover: move |evt: DragEvent| evt.prevent_default(),
                ondrop: move |evt: DragEvent| {
                    evt.prevent_default();
                    on_drop(slot);
                },
                onmousemove: move |evt| {
                    if let Some(id) = chip_id {
                        let p = evt.client_coordinates();
                        *CHIP_HOVER.write() = Some(ChipHover {
                            chip_id: id,
                            accent,
                            x: p.x,
                            y: p.y,
                        });
                    }
                },
                onmouseleave: move |_| {
                    *CHIP_HOVER.write() = None;
                },
                span { class: "grip", if has_chip { icons::GripVertical {} } }
                div { class: "stripe", style: "{stripe_style}" }
                div { class: "chip-cells",
                    if let Some(url) = icon {
                        img { class: "c-icon pix", src: "{url}", alt: "" }
                    } else {
                        span { class: "c-icon" }
                    }
                    if has_chip {
                        div { class: "c-name",
                            span { "{name}" }
                        }
                        {stat_cells(&loaded, chip_id.unwrap_or_default(), Some(code.clone()), chips_have_mb)}
                        if regular_supported {
                            {super::edit_toggle_maybe("REG", is_regular, "#ff42a5", reg_allowed.then(|| EventHandler::new(toggle_reg)))}
                        }
                        if tag_supported {
                            {super::edit_toggle_maybe("TAG", is_tag, "#29a121", tag_allowed.then(|| EventHandler::new(toggle_tag)))}
                        }
                        {super::remove_button(EventHandler::new(remove))}
                    } else {
                        div { class: "c-name",
                            span { class: "muted", "—" }
                        }
                    }
                }
            }
        });
    }

    let count_label = t!(
        &lang,
        "folder-edit-count",
        count = filled as i64,
        limit = MAX_FOLDER_CHIPS as i64
    );
    let clear_folder = {
        let handle = handle.clone();
        move |()| {
            editing.with_mut(|e| {
                if let Some(e) = e.as_mut() {
                    e.tags.clear();
                }
            });
            stage_edit(&handle, Edit::Chips(ChipEdit::ClearFolder));
        }
    };

    // Per-class usage vs cap (red when over) + the Reg/Tag memory
    // budgets, on the folder header's second line.
    let mut stats: Vec<Element> = Vec::new();
    if let Some(l) = limits.navi_limit {
        stats.push(super::limit_caption(
            t!(&lang, "folder-edit-navi", used = usage.navi as i64, limit = l as i64),
            usage.navi > l,
        ));
    }
    if let Some(l) = limits.mega_limit {
        stats.push(super::limit_caption(
            t!(&lang, "folder-edit-mega", used = usage.mega as i64, limit = l as i64),
            usage.mega > l,
        ));
    }
    if let Some(l) = limits.giga_limit {
        stats.push(super::limit_caption(
            t!(&lang, "folder-edit-giga", used = usage.giga as i64, limit = l as i64),
            usage.giga > l,
        ));
    }
    if let Some(l) = limits.dark_limit {
        stats.push(super::limit_caption(
            t!(&lang, "folder-edit-dark", used = usage.dark as i64, limit = l as i64),
            usage.dark > l,
        ));
    }
    if let Some(reg) = limits.reg_memory {
        stats.push(rsx! {
            span { class: "sub", {t!(&lang, "folder-edit-reg-memory", mb = reg as i64)} }
        });
    }
    if let Some(tag) = limits.tag_memory {
        stats.push(rsx! {
            span { class: "sub", {t!(&lang, "folder-edit-tag-memory", mb = tag as i64)} }
        });
    }

    // ----- Right pane: the chip library -----
    let filter = edit_ui.library_filter.to_lowercase();
    let mut lib_rows: Vec<Element> = Vec::new();
    for (id, name, code) in sorted_library_entries(&loaded, sort()) {
        if !filter.is_empty() && !name.to_lowercase().contains(filter.as_str()) {
            continue;
        }
        let addable = filled < MAX_FOLDER_CHIPS && usage.can_add(&loaded, id, &limits);
        let info = loaded.assets.chip(id);
        let accent = class_accent(
            info.as_ref().map(|i| i.class()),
            info.as_ref().map(|i| i.dark()).unwrap_or(false),
        );
        let stripe_style = accent.map(|a| format!("background:{a}")).unwrap_or_default();
        let icon = loaded.chip_icons.get(id).cloned().flatten();
        let code_str = code.to_string();
        let add = {
            let handle = handle.clone();
            move |_| {
                if !addable {
                    return;
                }
                // New chips insert at the top, sliding the run above the
                // first empty slot down — shift the staged TAGs to match.
                let gap = {
                    let l = handle.0.borrow();
                    l.save.view_chips().and_then(|v| {
                        let fi = v.equipped_folder_index();
                        (0..MAX_FOLDER_CHIPS).find(|&i| v.chip(fi, i).is_none())
                    })
                };
                if let Some(gap) = gap {
                    editing.with_mut(|e| {
                        if let Some(e) = e.as_mut() {
                            e.shift_tags_for_top_insert(gap);
                        }
                    });
                }
                stage_edit(&handle, Edit::Chips(ChipEdit::AddChip { chip_id: id, code }));
            }
        };
        lib_rows.push(rsx! {
            div {
                class: if addable { "chip-row-wrap lib" } else { "chip-row-wrap lib disabled" },
                onclick: add,
                onmousemove: move |evt| {
                    let p = evt.client_coordinates();
                    *CHIP_HOVER.write() = Some(ChipHover {
                        chip_id: id,
                        accent,
                        x: p.x,
                        y: p.y,
                    });
                },
                onmouseleave: move |_| {
                    *CHIP_HOVER.write() = None;
                },
                div { class: "stripe", style: "{stripe_style}" }
                div { class: "chip-cells",
                    if let Some(url) = icon {
                        img { class: "c-icon pix", src: "{url}", alt: "" }
                    } else {
                        span { class: "c-icon" }
                    }
                    div { class: "c-name",
                        span { "{name}" }
                    }
                    {stat_cells(&loaded, id, Some(code_str.clone()), chips_have_mb)}
                }
            }
        });
    }

    let sort_options: Vec<String> = LibrarySort::ALL.iter().map(|s| s.label(&lang)).collect();
    let sort_selected = LibrarySort::ALL.iter().position(|s| *s == sort()).unwrap_or(0);

    rsx! {
        div { class: "editor-panes",
            div { class: "pane editor-pane",
                div { class: "editor-header",
                    div { class: "line",
                        span { {t!(&lang, "folder-edit-folder")} }
                        {super::limit_caption(count_label, filled < MAX_FOLDER_CHIPS)}
                        div { class: "grow" }
                        {super::clear_all_button(&lang, EventHandler::new(clear_folder))}
                    }
                    if !stats.is_empty() {
                        div { class: "line stats",
                            {stats.into_iter()}
                        }
                    }
                }
                div { class: "editor-scroll",
                    {slot_rows.into_iter()}
                }
            }
            div { class: "pane editor-pane",
                {super::library_header(
                    t!(&lang, "folder-edit-search"),
                    edit_ui.library_filter.clone(),
                    EventHandler::new(move |v: String| {
                        editing.with_mut(|e| {
                            if let Some(e) = e.as_mut() {
                                e.library_filter = v;
                            }
                        });
                    }),
                    t!(&lang, "save-edit-sort"),
                    sort_options,
                    sort_selected,
                    EventHandler::new(move |i: usize| {
                        if let Some(s) = LibrarySort::ALL.get(i) {
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

/// The folder tab as TSV text for clipboard "copy as text".
pub(crate) fn as_text(loaded: &Loaded, grouped: bool) -> Option<String> {
    let assets = loaded.assets.as_ref();
    let chips_view = loaded.save.view_chips()?;
    let folder_idx = chips_view.equipped_folder_index();
    let regular_idx = chips_view.regular_chip_index(folder_idx).flatten();
    let tag_idxs = chips_view.tag_chip_indexes(folder_idx).flatten();

    let chips: Vec<Option<tango_dataview::save::Chip>> =
        (0..MAX_FOLDER_CHIPS).map(|i| chips_view.chip(folder_idx, i)).collect();

    let mut out = String::new();
    if grouped {
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
            let mut suffix = vec![];
            if g.is_regular {
                suffix.push("[REG]");
            }
            suffix.extend(std::iter::repeat_n("[TAG]", g.has_tag1 as usize + g.has_tag2 as usize));
            if !suffix.is_empty() {
                out.push('\t');
                out.push_str(&suffix.join(""));
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
            let mut suffix = vec![];
            if regular_idx == Some(i) {
                suffix.push("[REG]");
            }
            if let Some(ti) = tag_idxs {
                if ti.contains(&i) {
                    suffix.push("[TAG]");
                }
            }
            if !suffix.is_empty() {
                out.push('\t');
                out.push_str(&suffix.join(""));
            }
            out.push('\n');
        }
    }
    Some(out)
}

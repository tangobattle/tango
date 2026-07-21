//! The Folder tab: the equipped folder's 30 chips, grouped by identity
//! or one row per slot (the desktop's `save_view/folder.rs` read view).

use dioxus::prelude::*;
use unic_langid::LanguageIdentifier;

use super::{placeholder, ChipHover, Loaded, CHIP_HOVER};
use crate::t;

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
            div { class: "chip-row",
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
                        span { class: "badge reg", "REG" }
                    }
                    for _ in 0..(g.has_tag1 as usize + g.has_tag2 as usize) {
                        span { class: "badge tag", "TAG" }
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

//! The Auto Battle Data tab (the desktop's `save_view/abd.rs`): the six
//! deck sections — Standard (secondary) / Standard / Mega / Giga /
//! Combos / Program advance — each its own pane of grouped chip rows.

use dioxus::prelude::*;
use unic_langid::LanguageIdentifier;

use super::edit::{AutoBattleDataEdit, Edit};
use super::folder::{chip_row, stat_cells, GroupedChip};
use super::{placeholder, stage_edit, ChipHover, EditUi, Loaded, SaveHandle, CHIP_HOVER};
use crate::t;

/// Use counts are stored as `u16` in the save, so the numeric fields
/// clamp entries to this ceiling.
const MAX_ABD_USE_COUNT: usize = u16::MAX as usize;

/// The six deck sections in display order, as `(title, runs)` where each
/// run is a `(chip, slots)` pair. Shared read model for the viewer and
/// (later) the editor's live preview.
pub(super) fn abd_grouped_sections(
    lang: &LanguageIdentifier,
    grouped: &tango_dataview::auto_battle_data::GroupedAutoBattleData,
) -> Vec<(String, Vec<(Option<usize>, usize)>)> {
    vec![
        (
            t!(lang, "auto-battle-data-secondary-standard-chips"),
            grouped.secondary_standard_chips.clone(),
        ),
        (
            t!(lang, "auto-battle-data-standard-chips"),
            grouped.standard_chips.clone(),
        ),
        (t!(lang, "auto-battle-data-mega-chips"), grouped.mega_chips.clone()),
        (t!(lang, "auto-battle-data-giga-chip"), grouped.giga_chip.clone()),
        (t!(lang, "auto-battle-data-combos"), grouped.combos.clone()),
        (
            t!(lang, "auto-battle-data-program-advance"),
            grouped.program_advance.clone(),
        ),
    ]
}

/// One deck section: title row + a grouped chip row per run. Unfilled
/// runs still render as empty "—" rows so the section keeps its shape.
pub(super) fn abd_section(
    loaded: &Loaded,
    title: &str,
    runs: &[(Option<usize>, usize)],
    chips_have_mb: bool,
) -> Element {
    rsx! {
        div { class: "pane chip-list abd-section",
            div { class: "abd-title", "{title}" }
            for (id, count) in runs.iter() {
                {chip_row(
                    loaded,
                    *id,
                    None,
                    &GroupedChip {
                        count: *count,
                        ..GroupedChip::default()
                    },
                    true,
                    chips_have_mb,
                )}
            }
        }
    }
}

pub(super) fn render_auto_battle_data(lang: &LanguageIdentifier, loaded: &Loaded) -> Element {
    let Some(view) = loaded.save.view_auto_battle_data() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let chips_have_mb = assets.chips_have_mb();

    // Grouped form of the deck, computed from the per-chip use counts: a
    // chip that fills several deck slots becomes one "N× chip" row.
    let grouped = tango_dataview::auto_battle_data::GroupedAutoBattleData::materialize(view.as_ref(), assets);

    rsx! {
        div { class: "abd-sections",
            for (title, runs) in abd_grouped_sections(lang, &grouped) {
                {abd_section(loaded, &title, &runs, chips_have_mb)}
            }
        }
    }
}

/// Sort order for the auto-battle-data editor's chip library pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoBattleDataSort {
    Id,
    Name,
    Used,
}

impl AutoBattleDataSort {
    pub const ALL: [AutoBattleDataSort; 3] = [
        AutoBattleDataSort::Id,
        AutoBattleDataSort::Name,
        AutoBattleDataSort::Used,
    ];

    fn label(self, lang: &LanguageIdentifier) -> String {
        match self {
            AutoBattleDataSort::Id => t!(lang, "folder-sort-id"),
            AutoBattleDataSort::Name => t!(lang, "folder-sort-name"),
            AutoBattleDataSort::Used => t!(lang, "auto-battle-data-edit-used"),
        }
    }
}

/// The chips offered by the editor's library: program advances (always
/// available to the deck) plus every other chip the player holds in
/// their pack. Stable sorts (Id / Name) keep a row in place while its
/// count fields are edited; Used reorders as counts change.
fn sorted_auto_battle_data_chips(loaded: &Loaded, sort: AutoBattleDataSort, filter: &str) -> Vec<usize> {
    use tango_dataview::rom::ChipClass as CC;
    let assets = loaded.assets.as_ref();
    let view = loaded.save.view_auto_battle_data();
    let chips_view = loaded.save.view_chips();
    let filter = filter.to_lowercase();
    struct E {
        id: usize,
        name: String,
        used: usize,
    }
    let mut rows: Vec<E> = Vec::new();
    for id in 0..assets.num_chips() {
        let Some(info) = assets.chip(id) else { continue };
        let class = info.class();
        let is_pa = class == CC::ProgramAdvance;
        if !is_pa && !matches!(class, CC::Standard | CC::Mega | CC::Giga) {
            continue;
        }
        let Some(name) = info.name() else { continue };
        if name.trim().is_empty() {
            continue;
        }
        if !is_pa {
            let in_pack = (0..info.codes().len()).any(|variant| {
                chips_view
                    .as_ref()
                    .and_then(|v| v.pack_count(id, variant))
                    .is_some_and(|c| c > 0)
            });
            if !in_pack {
                continue;
            }
        }
        if !filter.is_empty() && !name.to_lowercase().contains(filter.as_str()) {
            continue;
        }
        let used = view.as_ref().and_then(|v| v.chip_use_count(id)).unwrap_or(0);
        rows.push(E { id, name, used });
    }
    match sort {
        AutoBattleDataSort::Id => {}
        AutoBattleDataSort::Name => rows.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id))),
        AutoBattleDataSort::Used => rows.sort_by(|a, b| b.used.cmp(&a.used).then(a.id.cmp(&b.id))),
    }
    rows.into_iter().map(|e| e.id).collect()
}

/// The auto-battle-data editor: live deck preview (left) beside the
/// chip library with editable Used / Sec. use counts (right). The deck
/// derives from the counts, so each edit restages them and rebuilds the
/// materialized deck — the preview updates live.
#[component]
pub(super) fn AbdEdit(handle: SaveHandle, editing: Signal<Option<EditUi>>, sort: Signal<AutoBattleDataSort>) -> Element {
    let mut editing = editing;
    let mut sort = sort;
    let lang = crate::i18n::LANG.read().clone();
    let edit_ui = editing.read().clone().unwrap_or_default();

    let loaded_rc = handle.0.clone();
    let loaded = loaded_rc.borrow();
    let Some(view) = loaded.save.view_auto_battle_data() else {
        return placeholder(t!(&lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let chips_have_mb = assets.chips_have_mb();

    // ----- Left pane: the live deck, grouped like the read-only viewer -----
    let grouped = tango_dataview::auto_battle_data::GroupedAutoBattleData::materialize(view.as_ref(), assets);
    let sections = abd_grouped_sections(&lang, &grouped);
    // Distinct chips currently contributing to the deck.
    let distinct = sections
        .iter()
        .flat_map(|(_, runs)| runs.iter())
        .filter_map(|(id, _)| *id)
        .collect::<std::collections::HashSet<_>>()
        .len();
    let count_label = t!(&lang, "auto-battle-data-edit-count", count = distinct as i64);
    let clear_all = {
        let handle = handle.clone();
        move |()| {
            stage_edit(&handle, Edit::AutoBattleData(AutoBattleDataEdit::ClearAll));
        }
    };

    // ----- Right pane: the chip library with editable use counts -----
    let used_label = t!(&lang, "auto-battle-data-edit-used");
    let sec_label = t!(&lang, "auto-battle-data-edit-secondary");
    let mut lib_rows: Vec<Element> = Vec::new();
    for id in sorted_auto_battle_data_chips(&loaded, sort(), &edit_ui.auto_battle_data_filter) {
        // Secondary use count only feeds the secondary-standard section,
        // so only Standard chips get a Sec. field.
        let is_standard = assets
            .chip(id)
            .map(|i| matches!(i.class(), tango_dataview::rom::ChipClass::Standard))
            .unwrap_or(false);
        let used = view.chip_use_count(id).unwrap_or(0);
        let secondary = is_standard.then(|| view.secondary_chip_use_count(id).unwrap_or(0));
        let info = loaded.assets.chip(id);
        let name = info.as_ref().and_then(|i| i.name()).unwrap_or_else(|| format!("#{id}"));
        let accent = super::folder::class_accent(
            info.as_ref().map(|i| i.class()),
            info.as_ref().map(|i| i.dark()).unwrap_or(false),
        );
        let stripe_style = accent.map(|a| format!("background:{a}")).unwrap_or_default();
        let icon = loaded.chip_icons.get(id).cloned().flatten();
        let on_used = {
            let handle = handle.clone();
            move |evt: FormEvent| {
                let count = parse_count(&evt.value());
                stage_edit(&handle, Edit::AutoBattleData(AutoBattleDataEdit::SetUseCount { id, count }));
            }
        };
        let on_sec = {
            let handle = handle.clone();
            move |evt: FormEvent| {
                let count = parse_count(&evt.value());
                stage_edit(
                    &handle,
                    Edit::AutoBattleData(AutoBattleDataEdit::SetSecondaryUseCount { id, count }),
                );
            }
        };
        lib_rows.push(rsx! {
            div {
                class: "chip-row-wrap abd-lib",
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
                    {stat_cells(&loaded, id, None, chips_have_mb)}
                    span { class: "abd-count",
                        span { class: "sub", "{used_label}" }
                        input {
                            r#type: "text",
                            inputmode: "numeric",
                            value: "{used}",
                            oninput: on_used,
                        }
                    }
                    span { class: "abd-count",
                        if let Some(sec) = secondary {
                            span { class: "sub", "{sec_label}" }
                            input {
                                r#type: "text",
                                inputmode: "numeric",
                                value: "{sec}",
                                oninput: on_sec,
                            }
                        }
                    }
                }
            }
        });
    }

    let sort_options: Vec<String> = AutoBattleDataSort::ALL.iter().map(|s| s.label(&lang)).collect();
    let sort_selected = AutoBattleDataSort::ALL.iter().position(|s| *s == sort()).unwrap_or(0);

    rsx! {
        div { class: "editor-panes",
            div { class: "pane editor-pane",
                div { class: "editor-header",
                    div { class: "line",
                        span { {t!(&lang, "save-tab-auto-battle-data")} }
                        span { class: "sub", "{count_label}" }
                        div { class: "grow" }
                        {super::clear_all_button(&lang, EventHandler::new(clear_all))}
                    }
                }
                div { class: "editor-scroll",
                    for (title, runs) in sections.iter() {
                        div { class: "abd-title", "{title}" }
                        for (id, count) in runs.iter() {
                            {chip_row(
                                &loaded,
                                *id,
                                None,
                                &GroupedChip {
                                    count: *count,
                                    ..GroupedChip::default()
                                },
                                true,
                                chips_have_mb,
                            )}
                        }
                    }
                }
            }
            div { class: "pane editor-pane",
                {super::library_header(
                    t!(&lang, "folder-edit-search"),
                    edit_ui.auto_battle_data_filter.clone(),
                    EventHandler::new(move |v: String| {
                        editing.with_mut(|e| {
                            if let Some(e) = e.as_mut() {
                                e.auto_battle_data_filter = v;
                            }
                        });
                    }),
                    t!(&lang, "save-edit-sort"),
                    sort_options,
                    sort_selected,
                    EventHandler::new(move |i: usize| {
                        if let Some(s) = AutoBattleDataSort::ALL.get(i) {
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

/// Digits-only parse of a use-count field, clamped to the u16 the save
/// stores.
fn parse_count(t: &str) -> usize {
    let digits: String = t.chars().filter(|c| c.is_ascii_digit()).take(5).collect();
    digits.parse::<usize>().unwrap_or(0).min(MAX_ABD_USE_COUNT)
}

/// The auto-battle-data tab as text.
pub(crate) fn as_text(loaded: &Loaded) -> Option<String> {
    let assets = loaded.assets.as_ref();
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

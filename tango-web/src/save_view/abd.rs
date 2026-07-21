//! The Auto Battle Data tab (the desktop's `save_view/abd.rs`): the six
//! deck sections — Standard (secondary) / Standard / Mega / Giga /
//! Combos / Program advance — each its own pane of grouped chip rows.

use dioxus::prelude::*;
use unic_langid::LanguageIdentifier;

use super::folder::{chip_row, GroupedChip};
use super::{placeholder, Loaded};
use crate::t;

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

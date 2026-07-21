//! The Patch Cards tab (the desktop's `save_view/patch_cards.rs`):
//! BN5/BN6 show the registered list (index · name+MB · ability badges ·
//! bug badges); BN4 shows the six fixed catalog slots (0A–0F) with each
//! installed card's "name — effect" line and its bug in purple.

use dioxus::prelude::*;
use unic_langid::LanguageIdentifier;

use super::{placeholder, Loaded};
use crate::t;

/// BN4 catalog-slot labels (the "0A"–"0F" the game shows).
pub(crate) const PATCH_CARD4_SLOT_LABELS: [&str; 6] = ["0A", "0B", "0C", "0D", "0E", "0F"];

/// Total MB an enabled patch-card set may use in BN5/BN6.
#[allow(dead_code)] // the patch-card editor (next phase)
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

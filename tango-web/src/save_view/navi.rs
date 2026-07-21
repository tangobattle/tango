//! The persistent navi identity strip shown above the tab body (the
//! desktop's `save_view/navi.rs`): the equipped navi's emblem / name /
//! stats card on the left, the save-level action cluster on the right.

use dioxus::prelude::*;
use unic_langid::LanguageIdentifier;

use super::Loaded;
use crate::t;

/// The navi card's inner content: emblem on the left, the navi's name
/// stacked over its stats on the right — base max HP and, where the game
/// exposes them (BN6), the MegaBuster levels. The navi-less games (BN1–4)
/// drop the emblem/name and just show the base HP. With `editing_hint`
/// set, a small pencil sits by the name to signal the card is the
/// change-navi button.
pub(super) fn navi_card_content(lang: &LanguageIdentifier, loaded: &Loaded, editing_hint: bool) -> Element {
    let assets = loaded.assets.as_ref();
    // Every game has a player navi with a base max HP. Games with a
    // link-navi roster (BN5/BN6/EXE4.5) also report which navi is equipped;
    // the rest (BN1–4) have no navi to pick.
    let navi = loaded.save.view_navi();
    let navi_id = navi.as_ref().map(|nv| nv.navi());
    let base_max_hp = navi.as_ref().map(|nv| nv.max_hp(assets));
    let buster = navi.as_ref().and_then(|nv| nv.buster_stats(assets));

    // Only the games with a real navi roster get an emblem + name. BN1–4
    // report a placeholder navi the ROM has no entry for.
    let roster_navi = navi_id.filter(|&id| assets.navi(id).is_some());

    if let Some(navi_id) = roster_navi {
        let name = assets
            .navi(navi_id)
            .and_then(|n| n.name())
            .unwrap_or_else(|| format!("Navi #{navi_id}"));
        let emblem = loaded.navi_emblems.get(&navi_id).cloned();
        rsx! {
            div { class: "navi-card",
                if let Some(url) = emblem {
                    img { class: "emblem pix", src: "{url}", alt: "" }
                } else {
                    span { class: "emblem" }
                }
                div { class: "info",
                    div { class: "name-row",
                        span { class: "name", "{name}" }
                        if editing_hint {
                            span { class: "edit-hint", crate::ui::icons::Pencil {} }
                        }
                    }
                    div { class: "stats",
                        if let Some(hp) = base_max_hp {
                            StatInline { label: t!(lang, "navi-base-hp"), value: hp.to_string() }
                        }
                        if let Some(b) = buster {
                            div { class: "buster",
                                StatInline { label: t!(lang, "navi-buster-attack"), value: b.attack.to_string() }
                                StatInline { label: t!(lang, "navi-buster-rapid"), value: b.speed.to_string() }
                                StatInline { label: t!(lang, "navi-buster-charge"), value: b.charge.to_string() }
                            }
                        }
                    }
                }
            }
        }
    } else {
        rsx! {
            div { class: "navi-card bare",
                if let Some(hp) = base_max_hp {
                    StatInline { label: t!(lang, "navi-base-hp"), value: hp.to_string() }
                }
            }
        }
    }
}

/// One stat as a tight inline pair: a muted label with its value flush
/// beside it.
#[component]
fn StatInline(label: String, value: String) -> Element {
    rsx! {
        span { class: "stat-inline",
            span { class: "stat-label", "{label}" }
            span { class: "stat-value", "{value}" }
        }
    }
}

/// The navi strip as text: the equipped navi's name (for games with a
/// link-navi roster) and its base max HP.
#[allow(dead_code)] // joins tab_as_text when the strip gets a copy affordance
pub(crate) fn navi_as_text(lang: &LanguageIdentifier, loaded: &Loaded) -> Option<String> {
    let assets = loaded.assets.as_ref();
    let navi = loaded.save.view_navi()?;
    let mut out = String::new();
    if assets.navi(navi.navi()).is_some() {
        let name = assets
            .navi(navi.navi())
            .and_then(|n| n.name())
            .unwrap_or_else(|| format!("#{}", navi.navi()));
        out.push_str(&name);
        out.push('\n');
    }
    out.push_str(&t!(lang, "navi-base-hp"));
    out.push('\t');
    out.push_str(&navi.max_hp(assets).to_string());
    Some(out)
}

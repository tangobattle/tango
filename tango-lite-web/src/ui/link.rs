//! The link screen: dial a code, then the lobby.
//!
//! Both phases are one screen because they are one flow, and because the
//! interesting state — is the opponent there, do we agree on what we're
//! playing, is either of us ready — reads the same either side of the
//! connection landing.

use dioxus::prelude::*;

use crate::link::{PhaseView, Snapshot, Verdict};
use crate::loadout::Loadout;

#[component]
pub fn Link(
    snapshot: ReadSignal<Snapshot>,
    loadout: ReadSignal<Loadout>,
    nickname: Signal<String>,
    code: Signal<String>,
) -> Element {
    let snapshot = snapshot();
    rsx! {
        div { class: "pane",
            match snapshot.phase {
                PhaseView::Idle | PhaseView::Failed => rsx! {
                    Dial { snapshot: snapshot.clone(), loadout, nickname, code }
                },
                PhaseView::Connecting { waiting_for_opponent } => rsx! {
                    Waiting { snapshot: snapshot.clone(), waiting_for_opponent }
                },
                PhaseView::Negotiating => rsx! {
                    Waiting { snapshot: snapshot.clone(), waiting_for_opponent: false }
                },
                PhaseView::Lobby => rsx! {
                    Lobby { snapshot: snapshot.clone(), loadout }
                },
            }
        }
    }
}

#[component]
fn Dial(snapshot: Snapshot, loadout: ReadSignal<Loadout>, nickname: Signal<String>, code: Signal<String>) -> Element {
    let ready_to_dial = loadout().is_playable() && !code().trim().is_empty();

    rsx! {
        if let Some(error) = snapshot.error.as_ref() {
            div { class: "card",
                h2 { "Last attempt" }
                div { class: "error", "{error}" }
            }
        }
        div { class: "card",
            h2 { "Link code" }
            div { class: "muted",
                "Both players type the same code. Whoever gets there first waits."
            }
            input {
                r#type: "text",
                autocapitalize: "none",
                autocomplete: "off",
                spellcheck: "false",
                placeholder: "e.g. yellow-mettaur-42",
                value: "{code}",
                oninput: move |event| {
                    let value = event.value();
                    crate::storage::prefs::set("code", &value);
                    code.set(value);
                },
            }
            button {
                class: "btn small",
                onclick: move |_| {
                    let generated = crate::link::random_code();
                    crate::storage::prefs::set("code", &generated);
                    code.set(generated);
                },
                "Suggest one"
            }
        }
        div { class: "card",
            h2 { "Nickname" }
            input {
                r#type: "text",
                autocomplete: "off",
                placeholder: "Shown to your opponent",
                value: "{nickname}",
                oninput: move |event| {
                    let value = event.value();
                    crate::storage::prefs::set("nickname", &value);
                    nickname.set(value);
                },
            }
        }
        if !loadout().is_playable() {
            div { class: "card",
                div { class: "muted", "Pick a game and a save on the Library tab first." }
            }
        }
        button {
            class: "btn primary wide",
            disabled: !ready_to_dial,
            onclick: move |_| {
                // Connecting is the user gesture that gets to build the
                // audio graph, so the context is unsuspended and the
                // worklet module is loaded well before the match starts.
                spawn(async move {
                    let _ = crate::audio::sink().await;
                });
                crate::link::set_loadout(loadout());
                crate::link::connect(code().trim().to_string(), nickname());
            },
            "Connect"
        }
    }
}

#[component]
fn Waiting(snapshot: Snapshot, waiting_for_opponent: bool) -> Element {
    let message = if waiting_for_opponent {
        "Waiting for your opponent to join…"
    } else {
        "Connecting…"
    };
    rsx! {
        div { class: "card",
            h2 { "{snapshot.link_code}" }
            div { class: "spinner" }
            div { class: "muted", style: "text-align:center", "{message}" }
        }
        button {
            class: "btn wide danger",
            onclick: move |_| crate::link::disconnect(),
            "Cancel"
        }
    }
}

#[component]
fn Lobby(snapshot: Snapshot, loadout: ReadSignal<Loadout>) -> Element {
    let compatible = snapshot.verdict == Some(Verdict::Compatible);
    let mine = loadout();
    let my_game = mine
        .game
        .map(crate::ui::game_label)
        .unwrap_or_else(|| "No game".to_string());
    let my_patch = mine
        .patch
        .as_ref()
        .map(|(name, version)| format!("{name} {version}"))
        .unwrap_or_else(|| "Unpatched".to_string());

    rsx! {
        if snapshot.starting {
            div { class: "overlay",
                div { class: "box",
                    div { class: "spinner" }
                    div { "Starting match…" }
                    // Priming the pair is seconds of emulation with no
                    // thread to hide it on, so the page really does stop
                    // responding for a moment. Saying so beats looking
                    // broken.
                    div { class: "muted", "Both games are booting to the link screen. This takes a few seconds." }
                }
            }
        }

        div { class: "card",
            h2 { "{snapshot.link_code}" }
            div { class: "versus",
                div { class: "side",
                    span { class: "name", "You" }
                    span { class: "meta muted", "{my_game}" }
                    span { class: "meta muted", "{my_patch}" }
                    if snapshot.local_ready {
                        span { class: "pill on", "Ready" }
                    } else {
                        span { class: "pill", "Not ready" }
                    }
                }
                span { class: "vs", "VS" }
                div { class: "side them",
                    span { class: "name",
                        {snapshot.opponent.clone().unwrap_or_else(|| "Waiting…".to_string())}
                    }
                    span { class: "meta muted",
                        {snapshot.opponent_game.clone().unwrap_or_default()}
                    }
                    if snapshot.remote_ready {
                        span { class: "pill on", "Ready" }
                    } else {
                        span { class: "pill", "Not ready" }
                    }
                }
            }
            div { class: "row muted",
                if let Some(ping) = snapshot.latency_ms {
                    span { "{ping} ms" }
                }
                match snapshot.relayed {
                    Some(true) => rsx! { span { "relayed" } },
                    Some(false) => rsx! { span { "direct" } },
                    None => rsx! {},
                }
            }
        }

        MatchTypes { loadout, selected: snapshot.match_type }

        div { class: "card",
            if let Some(verdict) = snapshot.verdict.as_ref() {
                VerdictLine { verdict: verdict.clone() }
            }
            button {
                class: if snapshot.local_ready { "btn wide" } else { "btn primary wide" },
                disabled: !compatible && !snapshot.local_ready,
                onclick: move |_| crate::link::set_ready(!snapshot.local_ready),
                if snapshot.local_ready { "Cancel ready" } else { "Ready" }
            }
        }

        button {
            class: "btn wide danger",
            onclick: move |_| crate::link::disconnect(),
            "Leave"
        }
    }
}

#[component]
fn VerdictLine(verdict: Verdict) -> Element {
    let (class, text) = match &verdict {
        Verdict::Compatible => ("muted", "Ready when you are.".to_string()),
        Verdict::MissingGame => ("muted", "Waiting for both sides to pick a game.".to_string()),
        // A ROM is the one thing the app can't go and get for you.
        Verdict::MissingRom => (
            "error",
            "You don't have your opponent's ROM. Both sides are simulated locally, so you need a copy of their game."
                .to_string(),
        ),
        Verdict::Fetching { name } => ("muted", format!("Downloading {name}…")),
        Verdict::DifferentVersions => (
            "error",
            "Different game or patch versions — you can't play across them.".to_string(),
        ),
        Verdict::SimVersionTooOld => (
            "error",
            "This game's netplay changed since your opponent's version of Tango — they need to update.".to_string(),
        ),
        Verdict::SimVersionTooNew => (
            "error",
            "This game's netplay changed since your version of Tango — you need to update.".to_string(),
        ),
        Verdict::DifferentMatchTypes => ("error", "You've picked different match types.".to_string()),
    };
    rsx! { div { class: "{class}", "{text}" } }
}

/// Both sides must agree on the match type, so it lives here rather than
/// on the library screen: it's part of the negotiation, not part of the
/// loadout.
#[component]
fn MatchTypes(loadout: ReadSignal<Loadout>, selected: (u8, u8)) -> Element {
    let Some(game) = loadout().game else {
        return rsx! {};
    };
    // Entry `i` is how many subtypes mode `i` has — e.g. BN6 is `[1, 1]`.
    let options: Vec<(u8, u8)> = game
        .family
        .match_types
        .iter()
        .enumerate()
        .flat_map(|(mode, subtypes)| (0..*subtypes).map(move |sub| (mode as u8, sub as u8)))
        .collect();
    if options.len() < 2 {
        return rsx! {};
    }

    rsx! {
        div { class: "card",
            h2 { "Match type" }
            div { class: "list",
                for (mode , subtype) in options {
                    button {
                        key: "{mode}-{subtype}",
                        class: "item",
                        "aria-selected": "{selected == (mode, subtype)}",
                        onclick: move |_| crate::link::set_match_type((mode, subtype)),
                        span { class: "grow title", "{crate::lang::match_type_name(game, mode, subtype)}" }
                    }
                }
            }
        }
    }
}

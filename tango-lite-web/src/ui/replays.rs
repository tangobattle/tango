//! The replays screen: what you've recorded, and what you can watch.
//!
//! Matches record themselves (see [`crate::recording`]), so this is
//! mostly a list. Import is here because a recording made on the
//! desktop — or sent by an opponent — plays back identically; export is
//! here because a recording that can only ever be watched on the phone
//! that made it is half a feature.

use dioxus::prelude::*;

use crate::library::ReplayEntry;

#[component]
pub fn Replays(revision: ReadSignal<u64>, onerror: EventHandler<String>) -> Element {
    let _ = revision();
    let entries = crate::library::replays();

    rsx! {
        div { class: "pane",
            if let Some(state) = crate::export::state() {
                ExportCard { state }
            }
            div { class: "card",
                h2 { "Recordings" }
                if entries.is_empty() {
                    div { class: "empty",
                        "Nothing recorded yet. Every link battle you play is saved here."
                    }
                } else {
                    div { class: "list",
                        for entry in entries {
                            Row { key: "{entry.path.display()}", entry, onerror }
                        }
                    }
                }
                crate::ui::FilePicker {
                    label: "Import a replay".to_string(),
                    onpick: move |(name, bytes): (String, Vec<u8>)| async move {
                        if !crate::library::import_replay(&name, &bytes).await {
                            onerror.call(format!("{name} isn't a replay this build can read."));
                        }
                    },
                }
            }
            div { class: "card",
                div { class: "muted",
                    "Playback re-simulates the match, so it needs both players' ROMs and the patches they were using."
                }
            }
        }
    }
}

/// The in-flight render. A phone can't do this in the background — the
/// tab has to stay open — so it says so, and gives you a way out.
#[component]
fn ExportCard(state: crate::export::State) -> Element {
    use crate::export::State;
    rsx! {
        div { class: "card",
            h2 { "Exporting video" }
            match &state {
                State::Rendering { done, total } => {
                    let percent = if *total == 0 { 0 } else { done * 100 / total };
                    rsx! {
                        div { class: "bar", div { style: "width: {percent}%" } }
                        div { class: "muted", "{percent}% — keep this tab open." }
                    }
                }
                State::Flushing => rsx! {
                    div { class: "bar", div { style: "width: 100%" } }
                    div { class: "muted", "Finishing the file…" }
                },
                State::Failed(message) => rsx! {
                    div { class: "error", "{message}" }
                },
            }
            if matches!(state, State::Failed(_)) {
                button { class: "btn small", onclick: move |_| crate::export::clear(), "Dismiss" }
            } else {
                button { class: "btn small danger", onclick: move |_| crate::export::cancel(), "Cancel" }
            }
        }
    }
}

#[component]
fn Row(entry: ReplayEntry, onerror: EventHandler<String>) -> Element {
    let (mine, theirs) = entry.sides.clone();
    let versus = match (mine.is_empty(), theirs.is_empty()) {
        (false, false) => format!("{mine} vs {theirs}"),
        (true, false) => format!("vs {theirs}"),
        _ => entry.name.clone(),
    };
    let path = entry.path.clone();

    rsx! {
        div { class: "item",
            button {
                class: "stack bare",
                onclick: {
                    let path = path.clone();
                    move |_| {
                        let path = path.clone();
                        async move {
                            if let Err(e) = crate::playback::open(path).await {
                                onerror.call(e);
                            }
                        }
                    }
                },
                span { class: "title", "{versus}" }
                span { class: "meta", "{entry.game}" }
                span { class: "meta", "{stamp(entry.ts)} · {kib(entry.bytes)}" }
            }
            button {
                class: "btn small",
                onclick: {
                    let (path, name) = (path.clone(), entry.name.clone());
                    move |_| {
                        if let Some(bytes) = crate::library::replay_bytes(&path) {
                            crate::ui::download(&bytes, &format!("{name}.{}", tango_replay::EXTENSION));
                        }
                    }
                },
                "Save"
            }
            button {
                class: "btn small",
                disabled: crate::export::is_running(),
                onclick: {
                    let (path, name) = (path.clone(), entry.name.clone());
                    move |_| crate::export::run(path.clone(), name.clone())
                },
                "Video"
            }
        }
    }
}

/// The match clock, as a local date and time. `chrono` is already in
/// the graph (the recorder stamps names with it), and it is the only
/// thing here that knows what a timezone is.
fn stamp(ts: u64) -> String {
    use chrono::TimeZone as _;
    match chrono::Local.timestamp_millis_opt(ts as i64).single() {
        Some(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
        None => String::new(),
    }
}

fn kib(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else {
        format!("{} KB", bytes / 1024)
    }
}

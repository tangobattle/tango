//! The library screen: what you own, and what you're taking into a
//! match.
//!
//! One screen rather than the desktop's tabs, because on a phone the
//! whole decision is "which game, which save, which patch" and splitting
//! three lists across three tabs would cost more taps than it saves.

use dioxus::prelude::*;

use tango_library::rom::GameRef;

use crate::loadout::Loadout;
use crate::ui::{game_label, save_label, FilePicker};

/// The library is read through [`crate::library::with`], not through
/// signals — so nothing here re-renders on its own when a scan lands.
/// The revision counter is what makes it reactive, and it has to reach
/// every card that reads the library: a child whose props are unchanged
/// is memoized and simply not re-rendered, however stale what it drew is.
#[component]
pub fn Library(loadout: Signal<Loadout>, revision: ReadSignal<u64>, onplay: EventHandler<()>) -> Element {
    let revision = revision();
    let owned = crate::library::owned_games();
    let picked = loadout().game;

    rsx! {
        div { class: "pane",
            div { class: "card",
                h2 { "Games" }
                if owned.is_empty() {
                    div { class: "empty",
                        "No ROMs yet. Add a Battle Network ROM to get started — it stays on this device."
                    }
                } else {
                    div { class: "list",
                        for game in owned {
                            GameRow {
                                key: "{game.family_and_variant().0}-{game.family_and_variant().1}",
                                game,
                                selected: picked == Some(game),
                                onpick: move |_| {
                                    let mut next = Loadout { game: Some(game), ..Default::default() };
                                    next.reconcile();
                                    loadout.set(next);
                                },
                            }
                        }
                    }
                }
                FilePicker {
                    label: "Add ROM".to_string(),
                    accept: ".gba,.bin".to_string(),
                    onpick: move |(name, bytes): (String, Vec<u8>)| async move {
                        match crate::library::import_rom(&name, &bytes).await {
                            Some(game) => {
                                let mut next = Loadout { game: Some(game), ..Default::default() };
                                next.reconcile();
                                loadout.set(next);
                            }
                            // Either an unsupported game or a bad dump —
                            // and a bad dump has to be refused, because
                            // it desyncs a match rather than failing.
                            None => log::warn!("{name}: not a supported ROM"),
                        }
                    },
                }
            }

            if let Some(game) = picked {
                SaveCard { game, loadout, revision }
                PatchCard { game, loadout, revision }
                button {
                    class: "btn primary wide",
                    disabled: !loadout().is_playable(),
                    onclick: move |_| onplay.call(()),
                    "Play"
                }
                StorageCard { game, loadout, revision }
            }
        }
    }
}

/// What the library is costing, and the way back out of it. Worth a card
/// of its own on a phone: a ROM is 16 MB, and the browser will evict the
/// whole origin rather than negotiate.
#[component]
fn StorageCard(game: GameRef, loadout: Signal<Loadout>, revision: u64) -> Element {
    let mut confirming = use_signal(|| false);
    let megabytes = crate::library::bytes_used() as f64 / (1024.0 * 1024.0);

    rsx! {
        div { class: "card",
            h2 { "Storage" }
            div { class: "muted", "{megabytes:.1} MB on this device." }
            if confirming() {
                div { class: "muted", "Delete {game_label(game)} and all of its saves?" }
                div { class: "row",
                    button {
                        class: "btn danger grow",
                        onclick: move |_| async move {
                            crate::library::delete_game(game).await;
                            loadout.set(Loadout::default());
                        },
                        "Delete"
                    }
                    button {
                        class: "btn grow",
                        onclick: move |_| confirming.set(false),
                        "Keep"
                    }
                }
            } else {
                button {
                    class: "btn small danger",
                    onclick: move |_| confirming.set(true),
                    "Remove {game_label(game)}"
                }
            }
        }
    }
}

#[component]
fn GameRow(game: GameRef, selected: bool, onpick: EventHandler<()>) -> Element {
    let region = match game.region() {
        tango_library::game::Region::US => "US",
        tango_library::game::Region::JP => "JP",
    };
    rsx! {
        button {
            class: "item",
            "aria-selected": "{selected}",
            onclick: move |_| onpick.call(()),
            div { class: "stack",
                span { class: "title", "{game_label(game)}" }
                span { class: "meta", "{region} · {String::from_utf8_lossy(game.rom_code)}" }
            }
        }
    }
}

#[component]
fn SaveCard(game: GameRef, loadout: Signal<Loadout>, revision: u64) -> Element {
    let saves = crate::library::with(|library| {
        library
            .saves
            .read()
            .get(&game)
            .map(|saves| saves.iter().map(|s| s.path.clone()).collect::<Vec<_>>())
            .unwrap_or_default()
    })
    .unwrap_or_default();
    let picked = loadout().save_path;
    let has_template = !game.save_templates.is_empty();

    rsx! {
        div { class: "card",
            h2 { "Save" }
            if saves.is_empty() {
                div { class: "empty", "No save for this game yet." }
            } else {
                div { class: "list",
                    for path in saves {
                        // A row rather than one big button: it carries a
                        // delete action, and a button inside a button is
                        // not a thing.
                        div {
                            key: "{path.display()}",
                            class: "item",
                            "aria-selected": "{picked.as_ref() == Some(&path)}",
                            button {
                                class: "grow title bare",
                                onclick: {
                                    let path = path.clone();
                                    move |_| loadout.write().save_path = Some(path.clone())
                                },
                                "{save_label(&path)}"
                            }
                            button {
                                class: "btn small danger",
                                onclick: {
                                    let path = path.clone();
                                    move |_| {
                                        let path = path.clone();
                                        async move {
                                            crate::library::delete_file(path).await;
                                            loadout.write().reconcile();
                                        }
                                    }
                                },
                                "Delete"
                            }
                        }
                    }
                }
            }
            div { class: "row",
                FilePicker {
                    label: "Add save".to_string(),
                    accept: ".sav,.srm,.bin".to_string(),
                    onpick: move |(name, bytes): (String, Vec<u8>)| async move {
                        if crate::library::import_save(&name, &bytes).await {
                            loadout.write().reconcile();
                        } else {
                            log::warn!("{name}: no game recognises this save");
                        }
                    },
                }
                if has_template {
                    button {
                        class: "btn",
                        onclick: move |_| async move {
                            // A starter save so a first-time player can
                            // get into a link battle without hunting one
                            // down on a phone.
                            if crate::library::create_starter_save(game).await {
                                loadout.write().reconcile();
                            }
                        },
                        "New save"
                    }
                }
            }
        }
    }
}

#[component]
fn PatchCard(game: GameRef, loadout: Signal<Loadout>, revision: u64) -> Element {
    let available = crate::library::patches_for(game);
    let picked = loadout().patch;
    let download = crate::library::download_progress();

    rsx! {
        div { class: "card",
            h2 { "Patch" }
            div { class: "list",
                button {
                    class: "item",
                    "aria-selected": "{picked.is_none()}",
                    onclick: move |_| loadout.write().patch = None,
                    div { class: "stack",
                        span { class: "title", "Unpatched" }
                        span { class: "meta", "The game as it shipped" }
                    }
                }
                for (name, version, installed) in available {
                    PatchRow {
                        key: "{name}-{version}",
                        name: name.clone(),
                        version: version.clone(),
                        installed,
                        selected: picked.as_ref() == Some(&(name.clone(), version.clone())),
                        loadout,
                    }
                }
            }
            if let Some(progress) = download {
                div { class: "bar",
                    div {
                        style: "width: {percent(progress)}%",
                    }
                }
            }
            div { class: "muted",
                "Both players must be on the same patch and version. If your opponent brings one you don't have, it's fetched automatically."
            }
            button {
                class: "btn small",
                onclick: move |_| async move {
                    if let Err(e) = crate::library::fetch_index().await {
                        log::warn!("patch index refresh failed: {e}");
                    }
                },
                "Refresh list"
            }
        }
    }
}

fn percent(progress: tango_library::patch::Progress) -> u64 {
    if progress.total == 0 {
        return 0;
    }
    (progress.downloaded * 100 / progress.total).min(100)
}

#[component]
fn PatchRow(
    name: String,
    version: semver::Version,
    installed: bool,
    selected: bool,
    loadout: Signal<Loadout>,
) -> Element {
    let title = crate::library::with(|library| library.patches.read().title(&name).map(str::to_string))
        .flatten()
        .unwrap_or_else(|| name.clone());

    rsx! {
        div { class: "item", "aria-selected": "{selected}",
            button {
                class: "stack bare",
                // Selecting an uninstalled patch would advertise a
                // package we can't apply, so the row installs first and
                // the pick follows.
                onclick: {
                    let (name, version) = (name.clone(), version.clone());
                    move |_| {
                        let (name, version) = (name.clone(), version.clone());
                        async move {
                            if !installed {
                                if let Err(e) = crate::library::install_patch(name.clone(), version.clone()).await {
                                    log::warn!("install {name} {version}: {e}");
                                    return;
                                }
                            }
                            loadout.write().patch = Some((name, version));
                        }
                    }
                },
                span { class: "title", "{title}" }
                span { class: "meta", "{version}" }
            }
            if installed {
                button {
                    class: "btn small danger",
                    onclick: {
                        let (name, version) = (name.clone(), version.clone());
                        move |_| {
                            let (name, version) = (name.clone(), version.clone());
                            async move {
                                crate::library::uninstall_patch(name, version).await;
                                // The pick may have been this package;
                                // reconcile drops it rather than leaving
                                // a play button that fails on press.
                                loadout.write().reconcile();
                            }
                        }
                    },
                    "Remove"
                }
            } else {
                span { class: "pill", "Get" }
            }
        }
    }
}

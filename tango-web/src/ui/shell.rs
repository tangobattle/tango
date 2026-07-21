//! The root component: global state wiring (config, runtime, OPFS
//! resources), the screen router (Welcome / Session / tab shell), and
//! the top nav bar — mirroring the desktop shell (`app/mod.rs`): logo,
//! Play + Replays pills, spacer, Patches + Settings icon tabs. The
//! nickname is set on the Welcome screen and edited in Settings, never
//! in the chrome.

use dioxus::html::HasFileData;
use dioxus::prelude::*;

use super::{icons, play, session_view, settings, use_ctx, welcome, Ctx, Tab};
use crate::config::Config;
use crate::library::Library;
use crate::runtime::{Runtime, SESSION_EPOCH};
use crate::storage::Storage;

const STYLE: Asset = asset!("/assets/style.css");

#[component]
pub fn App() -> Element {
    let config = use_hook(|| Signal::new(Config::load()));
    let runtime = use_hook(Runtime::install);
    // Bumped to rescan the library after imports/deletes.
    let library_rev = use_signal(|| 0u64);
    // The last picks are remembered across loads: the game, and its
    // last-picked save (pruned later if the file is gone — the
    // save-pane listing is the judge).
    let selected_game = use_signal(|| config.peek().last_game.clone());
    let selected_save = use_signal(|| {
        let c = config.peek();
        c.last_game.as_ref().and_then(|slug| c.last_saves.get(slug).cloned())
    });

    let storage = use_resource(|| async {
        match Storage::open().await {
            Ok(s) => Some(s),
            Err(e) => {
                log::error!("OPFS unavailable: {e}");
                None
            }
        }
    });

    let library = use_resource(move || {
        let _ = library_rev.read();
        let storage = storage.read().clone();
        async move {
            match storage.flatten() {
                Some(s) => Some(Library::scan(&s).await),
                None => None,
            }
        }
    });

    use_context_provider(|| Ctx {
        runtime: runtime.clone(),
        config,
        library_rev,
        storage,
        library,
        selected_game,
        selected_save,
    });

    // The runtime persists SRAM into OPFS.
    {
        let runtime = runtime.clone();
        use_effect(move || {
            if let Some(Some(storage)) = storage.read().clone() {
                runtime.borrow_mut().set_storage(storage);
            }
        });
    }

    // A "(fresh save)" session that persisted SRAM created a real
    // save file — move the picker onto it, and remember it as the
    // game's pick, so the next Play continues it instead of booting
    // fresh again. Watched from the always-mounted root on session
    // epochs (close, swap): the session view's own teardown proved
    // an unreliable place to write signals from.
    {
        let runtime = runtime.clone();
        let mut selected_save = selected_save;
        let mut config = config;
        let selected_game = selected_game;
        use_effect(move || {
            let _ = SESSION_EPOCH.read();
            if let Some(target) = runtime.borrow_mut().take_persisted_save() {
                if selected_save.peek().is_none() {
                    selected_save.set(Some(target.file.clone()));
                    let slug = selected_game.peek().clone();
                    if let Some(slug) = slug {
                        config.with_mut(|c| {
                            c.last_saves.insert(slug, target.file);
                        });
                    }
                }
            }
        });
    }

    // Persist every config edit; the screens just mutate the signal.
    use_effect(move || config.read().save());

    // Keep the runtime fed with the settings it consumes: the master
    // volume and the input mapping (which otherwise stays at default).
    {
        let runtime = runtime.clone();
        use_effect(move || {
            let c = config.read();
            let mut rt = runtime.borrow_mut();
            rt.set_volume(c.volume);
            rt.mapping = c.mapping.clone();
        });
    }

    // Screen routing, mirroring the desktop's ScreenKey: Welcome until
    // a nickname exists, the session view while a game runs (or its
    // end is undismissed), the tab shell otherwise.
    let in_session = {
        let _ = SESSION_EPOCH.read();
        let rt = runtime.borrow();
        rt.shared().is_some() || rt.last_end().is_some()
    };
    let needs_welcome = config.read().nick.trim().is_empty();

    rsx! {
        document::Stylesheet { href: STYLE }
        // App-frame viewport: no pinch zoom, edge-to-edge on notched
        // screens, browser chrome tinted to match.
        document::Meta {
            name: "viewport",
            // maximum-scale=1 stops iOS Safari's zoom-into-focused-field
            // jump without oversizing fonts.
            content: "width=device-width, initial-scale=1, maximum-scale=1, viewport-fit=cover, user-scalable=no",
        }
        document::Meta { name: "theme-color", content: "#0e1011" }
        if in_session {
            session_view::SessionView {}
        } else if needs_welcome {
            welcome::WelcomeScreen {}
        } else {
            Shell {}
        }
    }
}

#[component]
fn Shell() -> Element {
    let Ctx {
        storage,
        mut library_rev,
        ..
    } = use_ctx();
    let mut tab = use_signal(Tab::default);
    let current = tab();
    // True while a file drag hovers the content area (the drop cue).
    let mut drop_hover = use_signal(|| false);

    rsx! {
        document::Title { "Tango" }
        div {
            class: "shell",
            // A stray file drop must not navigate away from the app
            // (imports go through the pickers or the content area).
            ondragover: move |evt| evt.prevent_default(),
            ondrop: move |evt| evt.prevent_default(),
            header { class: "topbar",
                div { class: "brand",
                    h1 { "TANGO" }
                }
                nav { class: "tabs",
                    button {
                        class: "btn tab",
                        class: if current == Tab::Play { "active" },
                        onclick: move |_| tab.set(Tab::Play),
                        icons::Gamepad2 {}
                        "Play"
                    }
                    button {
                        class: "btn tab",
                        class: if current == Tab::Replays { "active" },
                        onclick: move |_| tab.set(Tab::Replays),
                        icons::Film {}
                        "Replays"
                    }
                }
                div { class: "spacer" }
                nav { class: "tabs",
                    button {
                        class: "btn tab icon-only",
                        class: if current == Tab::Patches { "active" },
                        title: "Patches",
                        onclick: move |_| tab.set(Tab::Patches),
                        icons::Puzzle {}
                    }
                    button {
                        class: "btn tab icon-only",
                        class: if current == Tab::Settings { "active" },
                        title: "Settings",
                        onclick: move |_| tab.set(Tab::Settings),
                        icons::Settings {}
                    }
                }
            }
            // The whole content area is one drop target: dropped files
            // import wherever they land, sorted by extension.
            main {
                class: if current == Tab::Settings { "settings-main" },
                class: if drop_hover() { "dropping" },
                ondragover: move |evt: DragEvent| {
                    evt.prevent_default();
                    if !*drop_hover.peek() {
                        drop_hover.set(true);
                    }
                },
                ondragleave: move |_| {
                    if *drop_hover.peek() {
                        drop_hover.set(false);
                    }
                },
                ondrop: move |evt: DragEvent| {
                    evt.prevent_default();
                    drop_hover.set(false);
                    let storage = storage.read().clone().flatten();
                    let files = evt.files();
                    async move {
                        let Some(storage) = storage else { return };
                        let counts = crate::web::import_files(&storage, files).await;
                        play::note_import(&counts);
                        *library_rev.write() += 1;
                        *crate::runtime::SAVES_REV.write() += 1;
                    }
                },
                match current {
                    Tab::Play => rsx! { play::PlayScreen {} },
                    Tab::Replays => rsx! { ComingSoon {
                        title: "Replays",
                        note: "Replay recording arrives with the netplay port; replays recorded here will open in the desktop client too.",
                    } },
                    Tab::Patches => rsx! { ComingSoon {
                        title: "Patches",
                        note: "Patch import lands after crossplay; unpatched games netplay-match the desktop client already.",
                    } },
                    Tab::Settings => rsx! { settings::SettingsScreen {} },
                }
            }
        }
    }
}

/// Empty-state card for tabs whose feature lands in a later milestone —
/// the tab structure mirrors the desktop shell from day one.
#[component]
fn ComingSoon(title: &'static str, note: &'static str) -> Element {
    rsx! {
        section { class: "panel", style: "align-self: center; margin: auto; max-width: 420px;",
            h2 { "{title}" }
            p { class: "sub", "{note}" }
        }
    }
}

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
use crate::t;
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
    let selected_family = use_signal(|| config.peek().last_game.clone());
    let selected_save = use_signal(|| {
        let c = config.peek();
        c.last_game.as_ref().and_then(|fam| c.last_saves.get(fam).cloned())
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

    let patches = use_resource(move || {
        let _ = super::patches_tab::PATCHES_REV.read();
        let storage = storage.read().clone();
        async move {
            match storage.flatten() {
                Some(s) => crate::patches::scan(&s).await,
                None => Vec::new(),
            }
        }
    });

    use_context_provider(|| Ctx {
        runtime: runtime.clone(),
        config,
        library_rev,
        storage,
        library,
        patches,
        selected_family,
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
        let selected_family = selected_family;
        use_effect(move || {
            let _ = SESSION_EPOCH.read();
            if let Some(target) = runtime.borrow_mut().take_persisted_save() {
                let pick = selected_save.peek().clone();
                // Only adopt when the picker still sits on a fresh row
                // (a real file pick already names its own file).
                if pick.is_none() || pick.as_deref().is_some_and(|p| p.starts_with("//fresh/")) {
                    selected_save.set(Some(target.file.clone()));
                    let family = selected_family.peek().clone();
                    if let Some(family) = family {
                        config.with_mut(|c| {
                            c.last_saves.insert(family, target.file);
                        });
                    }
                }
            }
        });
    }

    // The lobby handshake completed: boot the PvP session from the
    // deposited handoff (both ROMs resolved from the library). Guarded
    // on the handoff actually being present so re-renders can't
    // double-boot.
    {
        let runtime = runtime.clone();
        use_effect(move || {
            if !matches!(&*crate::netplay::PHASE.read(), crate::netplay::PhaseView::Starting) {
                return;
            }
            if crate::netplay::PRE_MATCH.with(|s| s.borrow().is_none()) {
                return;
            }
            let storage = storage.peek().clone().flatten();
            let lib = library.peek().clone().flatten();
            let runtime = runtime.clone();
            spawn(async move {
                let (Some(storage), Some(lib)) = (storage, lib) else {
                    *crate::netplay::PHASE.write() = crate::netplay::PhaseView::Failed {
                        error: "storage unavailable".to_string(),
                    };
                    return;
                };
                match crate::session::pvp::boot_from_handoff(runtime, storage, lib).await {
                    Ok(()) => {
                        *crate::netplay::PHASE.write() = crate::netplay::PhaseView::Idle;
                    }
                    Err(e) => {
                        log::error!("pvp boot: {e:#}");
                        *crate::netplay::PHASE.write() = crate::netplay::PhaseView::Failed {
                            error: format!("{e:#}"),
                        };
                    }
                }
            });
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
    let lang = crate::i18n::LANG.read().clone();
    let patches_title = t!(&lang, "tab-patches");
    let settings_title = t!(&lang, "tab-settings");
    let _ = &patches_title;
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
                        {t!(&lang, "tab-play")}
                    }
                    button {
                        class: "btn tab",
                        class: if current == Tab::Replays { "active" },
                        onclick: move |_| tab.set(Tab::Replays),
                        icons::Film {}
                        {t!(&lang, "tab-replays")}
                    }
                }
                div { class: "spacer" }
                nav { class: "tabs",
                    button {
                        class: "btn tab icon-only",
                        class: if current == Tab::Patches { "active" },
                        title: "{patches_title}",
                        onclick: move |_| tab.set(Tab::Patches),
                        icons::Puzzle {}
                    }
                    button {
                        class: "btn tab icon-only",
                        class: if current == Tab::Settings { "active" },
                        title: "{settings_title}",
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
                    Tab::Replays => rsx! { super::replays::ReplaysScreen {} },
                    Tab::Patches => rsx! { super::patches_tab::PatchesScreen {} },
                    Tab::Settings => rsx! { settings::SettingsScreen {} },
                }
            }
        }
    }
}



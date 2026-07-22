//! The root component: global state wiring (config, runtime, OPFS
//! resources), the screen router (Welcome / Session / tab shell), and
//! the top nav bar — mirroring the desktop shell (`app/mod.rs`): logo,
//! Play + Replays pills, spacer, Patches + Settings icon tabs. The
//! nickname is set on the Welcome screen and edited in Settings, never
//! in the chrome.

use dioxus::html::HasFileData;
use dioxus::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;

use super::{icons, patches_tab, play, session_view, settings, use_ctx, welcome, Ctx, Tab};
use crate::config::Config;
use crate::t;
use crate::library::Library;
use crate::runtime::{Runtime, SESSION_EPOCH};
use crate::storage::Storage;

#[cfg(target_arch = "wasm32")]
const STYLE: Asset = asset!("/assets/style.css");
/// The desktop's standalone logo mark (`tango/src/icon.png`), shown at
/// the nav strip's left edge like the desktop top bar.
#[cfg(target_arch = "wasm32")]
const LOGO: Asset = asset!("/assets/icon.png");

/// Native asset delivery: no dx bundle exists under plain `cargo run`,
/// so the stylesheet inlines as a `<style>` element and the logo
/// embeds as a data URL (see `host::png_data_url`).
#[cfg(not(target_arch = "wasm32"))]
const STYLE_TEXT: &str = include_str!("../../assets/style.css");

/// The logo's `src`, per target.
fn logo_src() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        LOGO.to_string()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        static URL: OnceLock<String> = OnceLock::new();
        URL.get_or_init(|| crate::host::png_data_url(include_bytes!("../../assets/icon.png")))
            .clone()
    }
}

#[component]
pub fn App() -> Element {
    let config = use_hook(|| Signal::new(Config::load()));
    // The language signal's first-ever access must happen inside the
    // Dioxus runtime (a GlobalSignal lazily initializes against the
    // current runtime — touching it before launch panics).
    use_hook(|| crate::i18n::init(config.peek().language.as_deref()));
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

    // The desktop's 15-minute patch autoupdater, browser flavor: a
    // background re-sync loop gated on the config toggle each round.
    use_hook(move || {
        spawn(async move {
            loop {
                crate::compat::sleep_ms(15 * 60 * 1000).await;
                if !config.peek().enable_patch_autoupdate {
                    continue;
                }
                let Some(Some(storage)) = storage.peek().clone() else {
                    continue;
                };
                let repo = config.peek().patch_repo.clone();
                match crate::patches::sync(&storage, &repo).await {
                    Ok(n) if n > 0 => *patches_tab::PATCHES_REV.write() += 1,
                    Ok(_) => {}
                    Err(e) => log::warn!("patch autoupdate: {e:#}"),
                }
            }
        });
    });

    // The theme + accent drive the chrome: `data-theme` selects the
    // palette override block in the stylesheet, and the accent custom
    // props follow the theme's own accent values — the web analog of
    // the desktop's theme_for. Selection gold stays constant
    // (--select-ink); the ink-vs-white flip on accent plates follows
    // the desktop's on_accent luma rule.
    #[cfg(target_arch = "wasm32")]
    use_effect(move || {
        let (accent, theme) = {
            let c = config.read();
            (c.accent, c.theme)
        };
        let Some(root) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
            .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
        else {
            return;
        };
        let _ = root.set_attribute(
            "data-theme",
            match theme {
                crate::config::Theme::Dark => "dark",
                crate::config::Theme::Light => "light",
            },
        );
        let (r, g, b) = accent.rgb(theme);
        let style = root.style();
        let _ = style.set_property("--accent", &format!("rgb({r},{g},{b})"));
        let _ = style.set_property("--accent-weak", &format!("rgba({r},{g},{b},0.14)"));
        let _ = style.set_property("--accent-hair", &format!("rgba({r},{g},{b},0.38)"));
        let luma = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0;
        let _ = style.set_property("--accent-ink", if luma > 0.6 { "#0a2012" } else { "#ffffff" });
    });

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

    let screen = rsx! {
        if in_session {
            session_view::SessionView {}
        } else if needs_welcome {
            welcome::WelcomeScreen {}
        } else {
            Shell {}
        }
    };

    #[cfg(target_arch = "wasm32")]
    return rsx! {
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
        {screen}
    };

    // Native: no DOM to hang `data-theme`/inline custom props on, so
    // the theme + accent go through an injected style element instead
    // (the light-palette block is lifted out of the stylesheet itself,
    // so the values can't drift). Keyboard events reach the app
    // through a focused wrapper — there are no document-level
    // listeners to install.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let theme_css = {
            let c = config.read();
            native_theme_css(c.accent, c.theme)
        };
        let rt_down = runtime.clone();
        let rt_up = runtime.clone();
        rsx! {
            // The base sheet first, the theme overrides second — the
            // later style element wins at equal specificity, which is
            // exactly what the lifted light-palette block needs.
            document::Style { {STYLE_TEXT} }
            document::Style { {theme_css} }
            div {
                class: "native-root",
                style: "display: contents;",
                tabindex: "0",
                autofocus: true,
                onkeydown: move |evt: KeyboardEvent| {
                    let code = evt.data().code().to_string();
                    if crate::runtime::native_key_event(&rt_down, &code, true) {
                        evt.prevent_default();
                    }
                },
                onkeyup: move |evt: KeyboardEvent| {
                    let code = evt.data().code().to_string();
                    crate::runtime::native_key_event(&rt_up, &code, false);
                },
                {screen}
            }
        }
    }
}

/// The native theme style block: accent custom props at `:root`, plus —
/// for the light theme — every `:root[data-theme="light"]`-prefixed
/// rule from the stylesheet re-emitted without the un-settable
/// attribute selector (the bare block lands on `:root`, scoped ones
/// keep their descendant selectors).
#[cfg(not(target_arch = "wasm32"))]
fn native_theme_css(accent: crate::config::Accent, theme: crate::config::Theme) -> String {
    let (r, g, b) = accent.rgb(theme);
    let luma = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0;
    let accent_ink = if luma > 0.6 { "#0a2012" } else { "#ffffff" };
    let mut css = format!(
        ":root {{ --accent: rgb({r},{g},{b}); --accent-weak: rgba({r},{g},{b},0.14); \
         --accent-hair: rgba({r},{g},{b},0.38); --accent-ink: {accent_ink}; }}\n"
    );
    if theme == crate::config::Theme::Light {
        const SHEET: &str = include_str!("../../assets/style.css");
        const PREFIX: &str = ":root[data-theme=\"light\"]";
        let mut rest = SHEET;
        while let Some(at) = rest.find(PREFIX) {
            let after = &rest[at + PREFIX.len()..];
            let Some(brace) = after.find('{') else { break };
            let selector_tail = after[..brace].trim();
            let body = &after[brace + 1..];
            let Some(end) = body.find('}') else { break };
            let rules = &body[..end];
            if selector_tail.is_empty() {
                css.push_str(&format!(":root {{{rules}}}\n"));
            } else {
                css.push_str(&format!("{selector_tail} {{{rules}}}\n"));
            }
            rest = &body[end + 1..];
        }
    }
    css
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
                    img { class: "logo", src: logo_src(), alt: "Tango" }
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
                        let counts = crate::host::import_files(&storage, files).await;
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



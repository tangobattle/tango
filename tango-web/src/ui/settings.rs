//! The Settings tab, shaped like the desktop's (`tabs/settings.rs`):
//! a left pill sidebar of sections over a scrollable pane of
//! option rows (label left, control hugging right). The input section
//! draws the stylized GBA console with live binding capture (keyboard
//! via the document listener, gamepad via the pump).

use dioxus::prelude::*;

use super::{icons, use_ctx, Ctx};
use crate::t;
use crate::platform::input::{self, DescribeKind, MappedKey};
use crate::runtime::{CAPTURED, CAPTURE_TARGET};

/// The desktop's section list in its order, plus Diagnostics — the
/// determinism probe and desync tooling live there.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Section {
    #[default]
    General,
    Graphics,
    Audio,
    Input,
    Netplay,
    Diagnostics,
    About,
}

#[component]
pub fn SettingsScreen() -> Element {
    let mut section = use_signal(Section::default);
    let current = section();
    let lang = crate::i18n::LANG.read().clone();

    rsx! {
        nav { class: "pane settings-nav",
            for (s, label) in [
                (Section::General, t!(&lang, "settings-section-general")),
                (Section::Graphics, t!(&lang, "settings-section-graphics")),
                (Section::Audio, t!(&lang, "settings-section-audio")),
                (Section::Input, t!(&lang, "settings-section-input")),
                (Section::Netplay, t!(&lang, "settings-section-netplay")),
                (Section::Diagnostics, t!(&lang, "web-diagnostics")),
                (Section::About, t!(&lang, "settings-section-about")),
            ] {
                button {
                    class: "btn tab",
                    class: if current == s { "active" },
                    onclick: move |_| section.set(s),
                    "{label}"
                }
            }
        }
        div { class: "settings-pane",
            match current {
                Section::General => rsx! { GeneralSection {} },
                Section::Graphics => rsx! { GraphicsSection {} },
                Section::Audio => rsx! { AudioSection {} },
                Section::Input => rsx! { InputSection {} },
                Section::Netplay => rsx! { NetplaySection {} },
                Section::Diagnostics => rsx! { super::diag::DiagnosticsSection {} },
                Section::About => rsx! { AboutSection {} },
            }
        }
    }
}

#[component]
fn GeneralSection() -> Element {
    let Ctx { mut config, .. } = use_ctx();
    let nick = config.read().nick.clone();
    let lang = crate::i18n::LANG.read().clone();
    rsx! {
        section { class: "pane",
            h2 { {t!(&lang, "settings-section-general")} }
            div { class: "option-row",
                label { {t!(&lang, "settings-nickname")} }
                input {
                    r#type: "text",
                    value: "{nick}",
                    spellcheck: "false",
                    autocomplete: "off",
                    maxlength: "32",
                    oninput: move |evt: FormEvent| {
                        config.with_mut(|c| c.nick = evt.value())
                    },
                }
            }
            div { class: "option-row",
                label { {t!(&lang, "settings-language")} }
                select {
                    onchange: move |evt: FormEvent| {
                        let v = evt.value();
                        if let Ok(id) = v.parse::<unic_langid::LanguageIdentifier>() {
                            config.with_mut(|c| c.language = Some(v.clone()));
                            *crate::i18n::LANG.write() = id;
                        }
                    },
                    for id in crate::i18n::SUPPORTED_LANGS {
                        option {
                            value: "{id}",
                            selected: *id == lang,
                            {crate::i18n::language_label(id)}
                        }
                    }
                }
            }
            div { class: "option-row",
                label { {t!(&lang, "settings-streamer-mode")} }
                input {
                    r#type: "checkbox",
                    checked: config.read().streamer_mode,
                    onchange: move |evt: FormEvent| {
                        config.with_mut(|c| c.streamer_mode = evt.checked())
                    },
                }
            }
            div { class: "option-row",
                label { {t!(&lang, "settings-theme")} }
                select {
                    onchange: move |evt: FormEvent| {
                        if let Ok(i) = evt.value().parse::<usize>() {
                            if let Some(t) = crate::config::Theme::ALL.get(i) {
                                config.with_mut(|c| c.theme = *t);
                            }
                        }
                    },
                    {
                        let theme = config.read().theme;
                        let label = |t: crate::config::Theme| match t {
                            crate::config::Theme::Dark => t!(&lang, "settings-theme-dark"),
                            crate::config::Theme::Light => t!(&lang, "settings-theme-light"),
                        };
                        rsx! {
                            for (i, t) in crate::config::Theme::ALL.iter().enumerate() {
                                option { value: "{i}", selected: *t == theme, {label(*t)} }
                            }
                        }
                    }
                }
            }
            div { class: "option-row",
                label { {t!(&lang, "settings-accent")} }
                select {
                    onchange: move |evt: FormEvent| {
                        if let Ok(i) = evt.value().parse::<usize>() {
                            if let Some(a) = crate::config::Accent::ALL.get(i) {
                                config.with_mut(|c| c.accent = *a);
                            }
                        }
                    },
                    {
                        let accent = config.read().accent;
                        let label = |a: crate::config::Accent| match a {
                            crate::config::Accent::TangoGreen => t!(&lang, "settings-accent-tango-green"),
                            crate::config::Accent::MegaManBlue => t!(&lang, "settings-accent-megaman-blue"),
                            crate::config::Accent::ProtoManRed => t!(&lang, "settings-accent-protoman-red"),
                            crate::config::Accent::RollPink => t!(&lang, "settings-accent-roll-pink"),
                            crate::config::Accent::GutsManYellow => t!(&lang, "settings-accent-gutsman-yellow"),
                            crate::config::Accent::BassPurple => t!(&lang, "settings-accent-bass-purple"),
                        };
                        rsx! {
                            for (i, a) in crate::config::Accent::ALL.iter().enumerate() {
                                option { value: "{i}", selected: *a == accent, {label(*a)} }
                            }
                        }
                    }
                }
            }
            div { class: "option-row",
                label { {t!(&lang, "settings-patch-repo")} }
                input {
                    r#type: "text",
                    class: "wide",
                    value: "{config.read().patch_repo}",
                    spellcheck: "false",
                    autocomplete: "off",
                    oninput: move |evt: FormEvent| {
                        config.with_mut(|c| c.patch_repo = evt.value())
                    },
                }
            }
            div { class: "option-row",
                label { {t!(&lang, "settings-enable-patch-autoupdate")} }
                input {
                    r#type: "checkbox",
                    checked: config.read().enable_patch_autoupdate,
                    onchange: move |evt: FormEvent| {
                        config.with_mut(|c| c.enable_patch_autoupdate = evt.checked())
                    },
                }
            }
        }
    }
}

#[component]
fn GraphicsSection() -> Element {
    let Ctx { mut config, .. } = use_ctx();
    let lang = crate::i18n::LANG.read().clone();
    let integer_scaling = config.read().integer_scaling;
    let video_filter = config.read().video_filter.clone();
    rsx! {
        section { class: "pane",
            h2 { "Emulator" }
            div { class: "option-row",
                label { "Integer scaling" }
                input {
                    r#type: "checkbox",
                    checked: integer_scaling,
                    onchange: move |evt: FormEvent| {
                        config.with_mut(|c| c.integer_scaling = evt.checked())
                    },
                }
            }
            div { class: "option-row",
                label { {t!(&lang, "settings-video-filter")} }
                select {
                    onchange: move |evt: FormEvent| {
                        config.with_mut(|c| c.video_filter = evt.value());
                    },
                    for (id, name) in crate::platform::video::FILTERS.iter() {
                        option {
                            value: "{id}",
                            selected: video_filter == *id,
                            "{name}"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn AudioSection() -> Element {
    let Ctx { mut config, .. } = use_ctx();
    let volume_pct = (config.read().volume * 100.0).round() as u32;
    let lang = crate::i18n::LANG.read().clone();
    rsx! {
        section { class: "pane",
            h2 { {t!(&lang, "settings-section-audio")} }
            div { class: "option-row",
                label { {t!(&lang, "settings-volume")} }
                input {
                    r#type: "range",
                    min: "0",
                    max: "100",
                    value: "{volume_pct}",
                    oninput: move |evt: FormEvent| {
                        if let Ok(v) = evt.value().parse::<f32>() {
                            config.with_mut(|c| c.volume = (v / 100.0).clamp(0.0, 1.0));
                        }
                    },
                }
                span { class: "status", "{volume_pct}%" }
            }
            div { class: "option-row",
                label { {t!(&lang, "settings-disable-bgm-in-pvp")} }
                input {
                    r#type: "checkbox",
                    checked: config.read().mute_bgm_in_pvp,
                    onchange: move |evt: FormEvent| {
                        config.with_mut(|c| c.mute_bgm_in_pvp = evt.checked())
                    },
                }
            }
        }
    }
}

/// Settings → Netplay: the matchmaking endpoint, the opponent-drawer
/// auto-open, and the relay policy — the desktop's section. Endpoint /
/// relay changes take effect on the next connection.
#[component]
fn NetplaySection() -> Element {
    let Ctx { mut config, .. } = use_ctx();
    let endpoint = config.read().matchmaking_endpoint.clone().unwrap_or_default();
    let use_relay = config.read().use_relay;
    let lang = crate::i18n::LANG.read().clone();
    let relay_label = |r: crate::config::UseRelay| match r {
        crate::config::UseRelay::Auto => t!(&lang, "settings-use-relay-auto"),
        crate::config::UseRelay::Always => t!(&lang, "settings-use-relay-always"),
        crate::config::UseRelay::Never => t!(&lang, "settings-use-relay-never"),
    };
    rsx! {
        section { class: "pane",
            h2 { {t!(&lang, "settings-section-netplay")} }
            div { class: "option-row",
                label { {t!(&lang, "settings-matchmaking-endpoint")} }
                input {
                    r#type: "text",
                    class: "wide",
                    placeholder: crate::config::DEFAULT_MATCHMAKING,
                    value: "{endpoint}",
                    spellcheck: "false",
                    autocomplete: "off",
                    oninput: move |evt: FormEvent| {
                        let v = evt.value();
                        config.with_mut(|c| {
                            c.matchmaking_endpoint = (!v.trim().is_empty()).then_some(v);
                        });
                    },
                }
            }
            div { class: "option-row",
                label { {t!(&lang, "settings-show-opponent-setup")} }
                input {
                    r#type: "checkbox",
                    checked: config.read().show_opponent_setup,
                    onchange: move |evt: FormEvent| {
                        config.with_mut(|c| c.show_opponent_setup = evt.checked())
                    },
                }
            }
            div { class: "option-row",
                label { {t!(&lang, "settings-use-relay")} }
                select {
                    onchange: move |evt: FormEvent| {
                        if let Ok(i) = evt.value().parse::<usize>() {
                            if let Some(r) = crate::config::UseRelay::ALL.get(i) {
                                config.with_mut(|c| c.use_relay = *r);
                            }
                        }
                    },
                    for (i, r) in crate::config::UseRelay::ALL.iter().enumerate() {
                        option { value: "{i}", selected: *r == use_relay, {relay_label(*r)} }
                    }
                }
            }
        }
    }
}

#[component]
fn InputSection() -> Element {
    let Ctx { mut config, .. } = use_ctx();

    // Apply captured bindings. The Config is the source of truth; the
    // shell's sync effect mirrors it into the runtime's mapping.
    use_effect(move || {
        let Some((key, physical)) = CAPTURED.read().clone() else {
            return;
        };
        *CAPTURED.write() = None;
        config.with_mut(|c| {
            let slot = c.mapping.slot_mut(key);
            if !slot.contains(&physical) {
                slot.push(physical);
            }
        });
    });

    // Leaving the section cancels any pending capture.
    use_drop(|| {
        *CAPTURE_TARGET.write() = None;
        *CAPTURED.write() = None;
    });

    let capture_target = *CAPTURE_TARGET.read();

    // The control whose bindings the detail row is editing.
    let selected = use_signal(|| Option::<MappedKey>::None);
    let sel = *selected.read();
    let sel_chips: Vec<(DescribeKind, String)> = sel
        .map(|k| config.read().mapping.slot(k).iter().map(input::describe).collect())
        .unwrap_or_default();

    rsx! {
        // Hidden on touch screens (CSS decides): play happens on the
        // on-screen controls there, so there's nothing to bind.
        section { class: "pane input-bindings",
            h2 { "Input bindings" }
            // The console plate: controls sit where they do on the
            // machine — shoulders up top, d-pad left, A/B right,
            // Start/Select at the bottom. Every control wears a tiny
            // one-line hint of its first binding so the geometry never
            // shifts; the full binding list edits in the detail row
            // below the plate.
            div { class: "gba",
                div { class: "gba-l", BindControl { mapped: MappedKey::L, label: "L", shape: "shoulder", selected } }
                div { class: "gba-r", BindControl { mapped: MappedKey::R, label: "R", shape: "shoulder", selected } }
                div { class: "gba-dpad",
                    div { class: "dp-up", BindControl { mapped: MappedKey::Up, label: "▲", shape: "pad", selected } }
                    div { class: "dp-left", BindControl { mapped: MappedKey::Left, label: "◀", shape: "pad", selected } }
                    div { class: "dp-right", BindControl { mapped: MappedKey::Right, label: "▶", shape: "pad", selected } }
                    div { class: "dp-down", BindControl { mapped: MappedKey::Down, label: "▼", shape: "pad", selected } }
                }
                div { class: "gba-screen", span { "Tango" } }
                div { class: "gba-face",
                    div { class: "face-a", BindControl { mapped: MappedKey::A, label: "A", shape: "round", selected } }
                    div { class: "face-b", BindControl { mapped: MappedKey::B, label: "B", shape: "round", selected } }
                }
                div { class: "gba-pills",
                    BindControl { mapped: MappedKey::Select, label: "select", shape: "pill", selected }
                    BindControl { mapped: MappedKey::Start, label: "start", shape: "pill", selected }
                }
            }
            // Not a console control; it rides below the plate.
            div { class: "gba-extra",
                BindControl { mapped: MappedKey::SpeedUp, label: "fast-forward", shape: "pill", selected }
            }
            // The selected control's bindings, editable. One reserved
            // row — the prompt swapping in and out must not shift the
            // buttons below.
            div { class: "bind-detail",
                if let Some(key) = sel {
                    span { class: "bind-detail-label", "{key_label(key)}" }
                    div { class: "chips",
                        for (index , (kind , chip_label)) in sel_chips.into_iter().enumerate() {
                            button {
                                class: "chip",
                                title: "Remove this binding",
                                onclick: move |_| {
                                    config.with_mut(|c| {
                                        let slot = c.mapping.slot_mut(key);
                                        if index < slot.len() {
                                            slot.remove(index);
                                        }
                                    });
                                },
                                if kind == DescribeKind::Keyboard {
                                    icons::Keyboard {}
                                } else {
                                    icons::Gamepad2 {}
                                }
                                span { "{chip_label}" }
                                icons::X {}
                            }
                        }
                    }
                    if capture_target == Some(key) {
                        span { class: "sub", {t!(&crate::i18n::LANG.read().clone(), "settings-input-press-key")} }
                    }
                } else {
                    span { class: "sub", {t!(&crate::i18n::LANG.read().clone(), "settings-input-select-hint")} }
                }
            }
            button {
                class: "btn",
                onclick: move |_| config.with_mut(|c| c.mapping = Default::default()),
                {t!(&crate::i18n::LANG.read().clone(), "settings-input-reset")}
            }
        }
    }
}

/// The credits roll. External links must open in a new tab —
/// in-place navigation would tear down the running app.
#[component]
fn AboutSection() -> Element {
    // The desktop's About: emblem, then "# Tango {version}" + the
    // repo's CREDITS.md as rendered markdown (English-only, like the
    // desktop). Parsed once — the content is static. Web extras (mGBA /
    // Dioxus attribution) ride at the end.
    static ABOUT_HTML: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        let md = format!(
            "# Tango {VERSION}\n{}",
            include_str!("../../../CREDITS.md")
        );
        let parser = pulldown_cmark::Parser::new(&md).filter(|ev| {
            !matches!(
                ev,
                pulldown_cmark::Event::Html(_) | pulldown_cmark::Event::InlineHtml(_)
            )
        });
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, parser);
        // In-app navigation would unload the wasm app — every credits
        // link opens a new tab instead.
        html.replace("<a href=", "<a target=\"_blank\" rel=\"noopener\" href=")
    });
    rsx! {
        section { class: "pane credits",
            img { class: "emblem-banner", src: EMBLEM, alt: "Tango" }
            div { class: "md", dangerous_inner_html: "{ABOUT_HTML.as_str()}" }
            hr {}
            p { class: "sub",
                "Tango (web) v{VERSION} · "
                Ext { href: "https://mgba.io", label: "mGBA" }
                " (MPL-2.0) · "
                Ext { href: "https://dioxuslabs.com", label: "Dioxus" }
            }
        }
    }
}

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The desktop About page's banner (`tango/src/emblem.png`).
const EMBLEM: Asset = asset!("/assets/emblem.png");

/// A credits link out of the app: new tab, no opener.
#[component]
fn Ext(href: &'static str, label: &'static str) -> Element {
    rsx! {
        a { href, target: "_blank", rel: "noopener", "{label}" }
    }
}

/// The detail row's name for a control.
fn key_label(key: MappedKey) -> &'static str {
    match key {
        MappedKey::Up => "Up",
        MappedKey::Down => "Down",
        MappedKey::Left => "Left",
        MappedKey::Right => "Right",
        MappedKey::A => "A",
        MappedKey::B => "B",
        MappedKey::L => "L",
        MappedKey::R => "R",
        MappedKey::Start => "Start",
        MappedKey::Select => "Select",
        MappedKey::SpeedUp => "Fast-forward",
    }
}

/// One console control: the physical-looking button selects it into
/// the detail row and arms capture (clicking again cancels). The tiny
/// hint under it is the first binding, so the plate reads at a glance
/// without its geometry moving. `shape` picks the silhouette (`round`,
/// `pad`, `shoulder`, `pill`).
#[component]
fn BindControl(
    mapped: MappedKey,
    label: &'static str,
    shape: &'static str,
    selected: Signal<Option<MappedKey>>,
) -> Element {
    let Ctx { config, .. } = use_ctx();
    let mut selected = selected;
    let capturing = *CAPTURE_TARGET.read() == Some(mapped);
    let is_selected = *selected.read() == Some(mapped);
    let hint = {
        let cfg = config.read();
        let slot = cfg.mapping.slot(mapped);
        match slot.split_first() {
            None => "—".to_string(),
            Some((first, rest)) => {
                let (_, label) = input::describe(first);
                if rest.is_empty() {
                    label
                } else {
                    format!("{label} +{}", rest.len())
                }
            }
        }
    };

    rsx! {
        div { class: "gba-bind",
            button {
                class: "gba-btn {shape}",
                class: if capturing { "capturing" },
                class: if is_selected { "selected" },
                title: "Rebind {key_label(mapped)}",
                onclick: move |_| {
                    selected.set(Some(mapped));
                    if *CAPTURE_TARGET.peek() == Some(mapped) {
                        *CAPTURE_TARGET.write() = None;
                    } else {
                        *CAPTURED.write() = None;
                        *CAPTURE_TARGET.write() = Some(mapped);
                    }
                },
                "{label}"
            }
            span { class: "bind-hint", "{hint}" }
        }
    }
}

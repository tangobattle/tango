//! The Welcome screen (chrome-less, over the backdrop), laid out like
//! the desktop's first-run card: title row with the language picker at
//! its right, then the two gated steps — add ROMs (the web's Import
//! button standing in for the desktop's open-folder/rescan pair) and
//! pick a nickname — with Continue armed once both are done. This is
//! the only place the nickname is first set; afterwards it lives in
//! Settings → General.

use dioxus::prelude::*;

use super::{icons, play, use_ctx, Ctx};
use crate::t;

/// A step's header: its done-state icon beside the 18px label, the
/// desktop's `step_header`.
#[component]
fn StepHeader(done: bool, label: String) -> Element {
    rsx! {
        div { class: "step-header",
            if done {
                span { class: "step-icon done", icons::Check {} }
            } else {
                span { class: "step-icon", icons::RefreshCw {} }
            }
            span { class: "step-label", "{label}" }
        }
    }
}

#[component]
pub fn WelcomeScreen() -> Element {
    let Ctx {
        mut config,
        storage,
        mut library_rev,
        library,
        ..
    } = use_ctx();
    let mut nick = use_signal(String::new);
    let lang = crate::i18n::LANG.read().clone();

    let games = library
        .read()
        .clone()
        .flatten()
        .map(|l| l.entries.len())
        .unwrap_or(0);
    let has_roms = games > 0;
    let has_nick = !nick.read().trim().is_empty();
    let ready = has_roms && has_nick;

    let on_continue = move |_| {
        let name = nick.peek().trim().to_string();
        if !name.is_empty() {
            config.with_mut(|c| c.nick = name);
        }
    };

    rsx! {
        document::Title { "Welcome — Tango" }
        div { class: "welcome",
            div { class: "panel",
                div { class: "title-row",
                    h1 { {t!(&lang, "welcome-title")} }
                    div { class: "grow" }
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
                p { class: "sub", {t!(&lang, "welcome-subtitle")} }
                div { class: "step",
                    StepHeader { done: has_roms, label: t!(&lang, "welcome-step-roms") }
                    p { class: "sub", {t!(&lang, "web-import-privacy")} }
                    div { class: "actions",
                        label { class: "btn file-btn",
                            icons::Upload {}
                            {t!(&lang, "web-import")}
                            input {
                                r#type: "file",
                                multiple: true,
                                // octet-stream: iOS greys out unknown
                                // extensions without a recognized UTI —
                                // see the play tab's ImportButton.
                                accept: ".gba,.srl,.sav,application/octet-stream",
                                onchange: move |evt: FormEvent| {
                                    let storage = storage.read().clone().flatten();
                                    let files = evt.files();
                                    crate::web::reset_file_input(&evt);
                                    async move {
                                        let Some(storage) = storage else { return };
                                        let counts =
                                            crate::web::import_files(&storage, files).await;
                                        play::note_import(&counts);
                                        *library_rev.write() += 1;
                                    }
                                },
                            }
                        }
                    }
                    if let Some(f) = play::IMPORT_FLASH.read().clone() {
                        p { class: "sub", play::FlashText { flash: f } }
                    }
                    if has_roms {
                        p { class: "sub flash ok", {t!(&lang, "welcome-step-roms-detected", count = games as i64)} }
                    }
                }
                div { class: "step",
                    StepHeader { done: has_nick, label: t!(&lang, "welcome-step-nickname") }
                    p { class: "sub", {t!(&lang, "welcome-step-nickname-description")} }
                    input {
                        class: "nickname",
                        r#type: "text",
                        placeholder: t!(&lang, "settings-nickname"),
                        spellcheck: "false",
                        autocomplete: "off",
                        maxlength: "32",
                        value: "{nick}",
                        oninput: move |evt: FormEvent| nick.set(evt.value()),
                        onkeydown: move |evt: KeyboardEvent| {
                            if evt.key() == Key::Enter {
                                let name = nick.peek().trim().to_string();
                                if !name.is_empty() && has_roms {
                                    config.with_mut(|c| c.nick = name);
                                }
                            }
                        },
                    }
                    if !has_roms {
                        p { class: "sub", {t!(&lang, "welcome-roms-needed")} }
                    }
                }
                div { class: "actions end",
                    button {
                        class: "btn primary",
                        disabled: !ready,
                        onclick: on_continue,
                        icons::Check {}
                        {t!(&lang, "welcome-continue")}
                    }
                }
            }
        }
    }
}

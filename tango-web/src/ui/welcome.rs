//! The Welcome screen (chrome-less, over the backdrop), mirroring the
//! desktop's first-run flow: import ROMs, pick a nickname, Continue.
//! This is the only place the nickname is first set; afterwards it
//! lives in Settings → General.

use dioxus::prelude::*;

use super::{icons, play, use_ctx, Ctx};

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

    let games = library
        .read()
        .clone()
        .flatten()
        .map(|l| l.entries.len())
        .unwrap_or(0);
    let ready = games > 0 && !nick.read().trim().is_empty();

    rsx! {
        document::Title { "Welcome — Tango" }
        div { class: "welcome",
            div { class: "panel",
                h1 { "TANGO" }
                p { class: "sub",
                    "Battle Network netplay, in your browser. Crossplays with the desktop client."
                }
                div { class: "step",
                    span { class: "caption", "1 · Your games" }
                    label { class: "btn file-btn",
                        icons::Upload {}
                        if games > 0 {
                            "{games} game(s) imported — add more…"
                        } else {
                            "Import ROMs (.gba)…"
                        }
                        input {
                            r#type: "file",
                            multiple: true,
                            accept: ".gba,.srl,.sav",
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
                    if let Some(f) = play::IMPORT_FLASH.read().clone() {
                        p { class: "sub", play::FlashText { flash: f } }
                    }
                    p { class: "sub",
                        "Files are copied into private browser storage and never leave this device."
                    }
                }
                div { class: "step",
                    span { class: "caption", "2 · Your name" }
                    input {
                        r#type: "text",
                        placeholder: "nickname",
                        spellcheck: "false",
                        autocomplete: "off",
                        maxlength: "32",
                        value: "{nick}",
                        oninput: move |evt: FormEvent| nick.set(evt.value()),
                    }
                }
                button {
                    class: "btn primary",
                    disabled: !ready,
                    onclick: move |_| {
                        let name = nick.peek().trim().to_string();
                        if !name.is_empty() {
                            config.with_mut(|c| c.nick = name);
                        }
                    },
                    icons::Check {}
                    "Continue"
                }
            }
        }
    }
}

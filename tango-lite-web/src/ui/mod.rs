//! The screens, and the bits they share.

pub mod library;
pub mod link;
pub mod play;
pub mod touch;

use dioxus::prelude::*;

use tango_library::rom::GameRef;

/// How a game names itself. The families ship Fluent translations for
/// their real titles, but a lite build has no localizer — and
/// `family`/`variant` is the vocabulary the wire protocol, the storage
/// keys and the compatibility check all already speak, so the label
/// stays in it rather than inventing a second one the user would have to
/// map back.
pub fn game_label(game: GameRef) -> String {
    let (family, variant) = game.family_and_variant();
    game_label_parts(family, variant)
}

pub fn game_label_parts(family: &str, variant: u8) -> String {
    let base = family.to_ascii_uppercase();
    if variant == 0 {
        base
    } else {
        format!("{base} · {variant}")
    }
}

/// The name a save file shows as: what the user called it, minus the
/// game prefix the importer added.
pub fn save_label(path: &std::path::Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Read the bytes out of a file input, if one was picked. Async because
/// reading a `File` is; the caller is always inside a spawned task
/// anyway, since it goes on to write to storage.
pub async fn read_picked_file(event: &Event<FormData>) -> Option<(String, Vec<u8>)> {
    let file = event.files().into_iter().next()?;
    match file.read_bytes().await {
        Ok(bytes) => Some((file.name(), bytes.to_vec())),
        Err(e) => {
            log::warn!("reading {}: {e}", file.name());
            None
        }
    }
}

/// A labelled file picker. The native control is unusable on a phone, so
/// it sits invisibly on top of a normal-looking button and lends it its
/// click behaviour — which is the only way to open the picker at all.
#[component]
pub fn FilePicker(label: String, accept: String, onpick: EventHandler<(String, Vec<u8>)>) -> Element {
    rsx! {
        label { class: "btn file",
            "{label}"
            input {
                r#type: "file",
                accept: "{accept}",
                onchange: move |event| async move {
                    if let Some(picked) = read_picked_file(&event).await {
                        onpick.call(picked);
                    }
                },
            }
        }
    }
}

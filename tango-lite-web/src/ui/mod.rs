//! The screens, and the bits they share.

pub mod library;
pub mod link;
pub mod play;
pub mod replays;
pub mod touch;

use dioxus::prelude::*;

use tango_library::rom::GameRef;

/// How a game names itself: its real title, in the viewer's language.
/// See [`crate::lang`] — the families ship the Fluent fragments and
/// tango-library resolves them, so this is a re-export with a shorter
/// name rather than a policy of its own.
pub fn game_label(game: GameRef) -> String {
    crate::lang::game_name(game)
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

/// Hand a stored file to the browser's downloads.
///
/// The only way out of origin-private storage: a blob URL behind a
/// synthetic anchor click. Worth having — a recording that can only be
/// watched on the device that made it is half a feature.
pub fn download(bytes: &[u8], filename: &str) {
    use wasm_bindgen::JsCast as _;

    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes).into());
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/octet-stream");
    let Ok(blob) = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options) else {
        return;
    };
    let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else {
        return;
    };
    if let Ok(anchor) = document.create_element("a").and_then(|e| {
        e.dyn_into::<web_sys::HtmlAnchorElement>()
            .map_err(|_| wasm_bindgen::JsValue::NULL)
    }) {
        anchor.set_href(&url);
        anchor.set_download(filename);
        anchor.click();
    }
    // The download holds its own reference; ours is done.
    let _ = web_sys::Url::revoke_object_url(&url);
}

/// A labelled file picker. The native control is unusable on a phone, so
/// it sits invisibly on top of a normal-looking button and lends it its
/// click behaviour — which is the only way to open the picker at all.
///
/// # No `accept`
///
/// It would be natural to filter by extension here, and it is actively
/// harmful. iOS resolves `accept` to UTIs, and not one of the
/// extensions this app deals in has one — `.gba`, `.srl`, `.sav`,
/// `.srm`, `.tangoreplay` are all unknown to it — so the filter matches
/// nothing and Files greys out every file the user came to pick. A
/// picker that cannot pick is worse than an unfiltered one.
///
/// Nothing is lost by dropping it, because the extension was never what
/// identified these files: a ROM is recognized by its header and CRC32,
/// a save by which game can parse it, a replay by decoding it. A file
/// picked here is checked for what it actually is, and told so if it
/// isn't.
#[component]
pub fn FilePicker(label: String, onpick: EventHandler<(String, Vec<u8>)>) -> Element {
    rsx! {
        label { class: "btn file",
            "{label}"
            input {
                r#type: "file",
                onchange: move |event| async move {
                    if let Some(picked) = read_picked_file(&event).await {
                        onpick.call(picked);
                    }
                },
            }
        }
    }
}

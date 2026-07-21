//! Where an export's muxed bytes stream to: a
//! `FileSystemWritableFileStream` — either the user's own file via
//! `showSaveFilePicker` (Chromium; the export never materializes in
//! memory or OPFS at all) or, where the picker doesn't exist, a temp
//! file in OPFS that's handed to the downloader and deleted afterwards.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{FileSystemFileHandle, FileSystemWritableFileStream};

fn js_err(e: JsValue) -> anyhow::Error {
    anyhow::anyhow!("{e:?}")
}

#[wasm_bindgen]
extern "C" {
    /// `window.showSaveFilePicker` — not in web-sys's stable surface.
    #[wasm_bindgen(js_name = showSaveFilePicker, js_namespace = window, catch)]
    fn show_save_file_picker(options: &js_sys::Object) -> Result<js_sys::Promise, JsValue>;
}

/// Whether this browser offers the save-file picker.
pub fn save_picker_available() -> bool {
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("showSaveFilePicker"))
        .map(|v| v.is_function())
        .unwrap_or(false)
}

/// Ask the user where to save. `Ok(None)` = they dismissed the picker
/// (a quiet cancel, not an error). Must be called while the triggering
/// click's user activation is still live.
pub async fn pick_save_file(suggested_name: &str) -> anyhow::Result<Option<FileSystemFileHandle>> {
    let accept = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &accept,
        &JsValue::from_str("video/webm"),
        &js_sys::Array::of1(&JsValue::from_str(".webm")),
    );
    let ty = super::webcodecs::obj(&[
        ("description", JsValue::from_str("WebM video")),
        ("accept", accept.into()),
    ]);
    let options = super::webcodecs::obj(&[
        ("suggestedName", JsValue::from_str(suggested_name)),
        ("types", js_sys::Array::of1(&ty).into()),
    ]);
    let promise = show_save_file_picker(&options).map_err(js_err)?;
    match JsFuture::from(promise).await {
        Ok(handle) => Ok(Some(
            handle
                .dyn_into()
                .map_err(|_| anyhow::anyhow!("picker returned a non-file handle"))?,
        )),
        // AbortError = the user closed the picker.
        Err(e) => {
            let name = js_sys::Reflect::get(&e, &JsValue::from_str("name"))
                .ok()
                .and_then(|n| n.as_string());
            if name.as_deref() == Some("AbortError") {
                Ok(None)
            } else {
                Err(js_err(e))
            }
        }
    }
}

/// A streaming sink over a writable file stream, tracking the forward
/// position so `patch` can seek back and return.
pub struct FileSink {
    stream: FileSystemWritableFileStream,
    pos: u64,
}

impl FileSink {
    pub async fn open(handle: &FileSystemFileHandle) -> anyhow::Result<FileSink> {
        let stream: FileSystemWritableFileStream = JsFuture::from(handle.create_writable())
            .await
            .map_err(js_err)?
            .dyn_into()
            .map_err(|_| anyhow::anyhow!("expected a writable stream"))?;
        Ok(FileSink { stream, pos: 0 })
    }
}

impl super::webm::Sink for FileSink {
    async fn write(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        // Owned exact-sized buffer, never a `&[u8]` view — see
        // `storage::write` for the iOS WebKit view-vs-buffer hazard.
        let data = js_sys::Uint8Array::from(bytes);
        JsFuture::from(self.stream.write_with_js_u8_array(&data).map_err(js_err)?)
            .await
            .map_err(js_err)?;
        self.pos += bytes.len() as u64;
        Ok(())
    }

    async fn patch(&mut self, position: u64, bytes: &[u8]) -> anyhow::Result<()> {
        JsFuture::from(self.stream.seek_with_f64(position as f64).map_err(js_err)?)
            .await
            .map_err(js_err)?;
        let data = js_sys::Uint8Array::from(bytes);
        JsFuture::from(self.stream.write_with_js_u8_array(&data).map_err(js_err)?)
            .await
            .map_err(js_err)?;
        // Return to the stream's end so later writes append.
        JsFuture::from(self.stream.seek_with_f64(self.pos as f64).map_err(js_err)?)
            .await
            .map_err(js_err)?;
        Ok(())
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        JsFuture::from(self.stream.close()).await.map_err(js_err)?;
        Ok(())
    }
}

//! Target-neutral facade over the bootstrap/platform-glue helpers the
//! UI screens call: gesture-gated boot + audio, file import, byte
//! downloads. Each name resolves to `crate::web` or `crate::native`;
//! the import pass itself is shared (only the how-to-read-a-picked-file
//! differs).

mod import;
pub use import::{import_files, ImportCounts};

#[cfg(target_arch = "wasm32")]
pub use crate::web::{boot, download_bytes, ensure_audio, reset_file_input};

#[cfg(not(target_arch = "wasm32"))]
pub use crate::native::{boot, download_bytes, ensure_audio, reset_file_input};

/// The viewport's CSS width, where the platform can say — popover
/// edge-flip decisions fall back to never-flip without it (Blitz has
/// no viewport query in 0.7.9).
pub fn viewport_width() -> Option<f64> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window().and_then(|w| w.inner_width().ok()).and_then(|v| v.as_f64())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// Put `text` on the clipboard; true on success (the UI's copied-flash
/// cue keys off it).
#[cfg(target_arch = "wasm32")]
pub async fn copy_text(text: &str) -> bool {
    let Some(win) = web_sys::window() else {
        return false;
    };
    let p = win.navigator().clipboard().write_text(text);
    wasm_bindgen_futures::JsFuture::from(p).await.is_ok()
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn copy_text(text: &str) -> bool {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text.to_owned()))
        .is_ok()
}

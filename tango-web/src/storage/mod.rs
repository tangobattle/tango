//! Browser storage: OPFS (the Origin Private File System) for ROMs,
//! saves, and replays — flat `roms/`, `saves/`, and `replays/`
//! directories, matching the desktop client's layout (saves aren't
//! namespaced per game; which game a save belongs to is detected from
//! its content, same as the desktop scanner) — plus localStorage for
//! the small sync config blob. No IndexedDB anywhere: OPFS needs no
//! permissions or persisted handles and works in every major browser;
//! the trade is that files are *imported* (copied in) rather than read
//! in place, with export as a download.

use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetDirectoryOptions,
    FileSystemGetFileOptions, FileSystemWritableFileStream,
};

#[wasm_bindgen]
extern "C" {
    /// `FileSystemDirectoryHandle.values()` — the async iterator
    /// web-sys doesn't generate.
    #[wasm_bindgen(extends = FileSystemDirectoryHandle)]
    type DirectoryHandleExt;

    #[wasm_bindgen(method)]
    fn values(this: &DirectoryHandleExt) -> js_sys::AsyncIterator;
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct StorageError(String);

impl From<JsValue> for StorageError {
    fn from(v: JsValue) -> Self {
        StorageError(format!("{v:?}"))
    }
}

fn err(msg: &str) -> StorageError {
    StorageError(msg.to_owned())
}

/// The app's OPFS root. Cheap to clone (JS handles inside).
#[derive(Clone)]
pub struct Storage {
    roms: FileSystemDirectoryHandle,
    saves: FileSystemDirectoryHandle,
    replays: FileSystemDirectoryHandle,
    patches: FileSystemDirectoryHandle,
}

impl Storage {
    pub async fn open() -> Result<Storage, StorageError> {
        let navigator = web_sys::window().ok_or_else(|| err("no window"))?.navigator();
        let root: FileSystemDirectoryHandle = JsFuture::from(navigator.storage().get_directory())
            .await?
            .dyn_into()
            .map_err(|_| err("getDirectory returned a non-directory"))?;
        // Best-effort: ask the browser not to evict us under pressure.
        let _ = navigator.storage().persist();
        let roms = subdir(&root, "roms").await?;
        let saves = subdir(&root, "saves").await?;
        let replays = subdir(&root, "replays").await?;
        let patches = subdir(&root, "patches").await?;
        Ok(Storage {
            roms,
            saves,
            replays,
            patches,
        })
    }

    pub fn roms(&self) -> &FileSystemDirectoryHandle {
        &self.roms
    }

    pub fn replays(&self) -> &FileSystemDirectoryHandle {
        &self.replays
    }

    /// The synced patch tree (`patches/<name>/v<version>/…`), the
    /// desktop's on-disk layout.
    pub fn patches(&self) -> &FileSystemDirectoryHandle {
        &self.patches
    }

    /// The flat save directory `saves/` — which game each file belongs
    /// to is detected from its content, like the desktop scanner.
    pub fn saves(&self) -> &FileSystemDirectoryHandle {
        &self.saves
    }

}

async fn subdir(
    parent: &FileSystemDirectoryHandle,
    name: &str,
) -> Result<FileSystemDirectoryHandle, StorageError> {
    let opts = FileSystemGetDirectoryOptions::new();
    opts.set_create(true);
    JsFuture::from(parent.get_directory_handle_with_options(name, &opts))
        .await?
        .dyn_into()
        .map_err(|_| err("expected a directory handle"))
}

/// List a directory's subdirectories (name, handle), sorted by name.
pub async fn list_dirs(
    dir: &FileSystemDirectoryHandle,
) -> Result<Vec<(String, FileSystemDirectoryHandle)>, StorageError> {
    let iter = dir.unchecked_ref::<DirectoryHandleExt>().values();
    let mut out = Vec::new();
    loop {
        let next = JsFuture::from(iter.next().map_err(StorageError::from)?).await?;
        let done = js_sys::Reflect::get(&next, &"done".into())?
            .as_bool()
            .unwrap_or(true);
        if done {
            break;
        }
        let value = js_sys::Reflect::get(&next, &"value".into())?;
        if let Ok(d) = value.dyn_into::<FileSystemDirectoryHandle>() {
            out.push((d.name(), d));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// List a directory's plain files (name, handle), sorted by name.
pub async fn list_files(
    dir: &FileSystemDirectoryHandle,
) -> Result<Vec<(String, FileSystemFileHandle)>, StorageError> {
    let iter = dir.unchecked_ref::<DirectoryHandleExt>().values();
    let mut out = Vec::new();
    loop {
        let next = JsFuture::from(iter.next().map_err(StorageError::from)?).await?;
        let done = js_sys::Reflect::get(&next, &"done".into())?
            .as_bool()
            .unwrap_or(true);
        if done {
            break;
        }
        let value = js_sys::Reflect::get(&next, &"value".into())?;
        if let Ok(file) = value.dyn_into::<FileSystemFileHandle>() {
            out.push((file.name(), file));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Read a file's bytes by name; `Ok(None)` when it doesn't exist.
pub async fn read(
    dir: &FileSystemDirectoryHandle,
    name: &str,
) -> Result<Option<Vec<u8>>, StorageError> {
    let handle = match JsFuture::from(dir.get_file_handle(name)).await {
        Ok(h) => h
            .dyn_into::<FileSystemFileHandle>()
            .map_err(|_| err("expected a file handle"))?,
        Err(_) => return Ok(None), // NotFoundError
    };
    Ok(Some(read_handle(&handle).await?))
}

/// Read a file handle's bytes.
pub async fn read_handle(handle: &FileSystemFileHandle) -> Result<Vec<u8>, StorageError> {
    let file: web_sys::File = JsFuture::from(handle.get_file())
        .await?
        .dyn_into()
        .map_err(|_| err("expected a File"))?;
    let buf = JsFuture::from(file.array_buffer()).await?;
    Ok(Uint8Array::new(&buf).to_vec())
}

/// Create-or-truncate `name` with `bytes`.
pub async fn write(
    dir: &FileSystemDirectoryHandle,
    name: &str,
    bytes: &[u8],
) -> Result<(), StorageError> {
    let opts = FileSystemGetFileOptions::new();
    opts.set_create(true);
    let handle: FileSystemFileHandle =
        JsFuture::from(dir.get_file_handle_with_options(name, &opts))
            .await?
            .dyn_into()
            .map_err(|_| err("expected a file handle"))?;
    let stream: FileSystemWritableFileStream = JsFuture::from(handle.create_writable())
        .await?
        .dyn_into()
        .map_err(|_| err("expected a writable stream"))?;
    // Hand write() an owned, exact-sized buffer, never a `&[u8]` view:
    // web-sys passes a slice as a Uint8Array view into all of wasm
    // linear memory, and iOS WebKit has written the view's whole
    // backing buffer — tens of MiB of heap — instead of the view,
    // which is how imports came out enormous. (A view is also unsound
    // against mid-write heap growth, which detaches it.)
    let data = Uint8Array::from(bytes);
    JsFuture::from(
        stream
            .write_with_js_u8_array(&data)
            .map_err(StorageError::from)?,
    )
    .await?;
    JsFuture::from(stream.close()).await?;
    // Trust, but verify: a browser that wrote some other size must
    // fail the import loudly, not seed the library with a corrupt
    // file. (The old content is already gone either way — the write
    // truncates — so deleting the bad copy loses nothing.)
    let file: web_sys::File = JsFuture::from(handle.get_file())
        .await?
        .dyn_into()
        .map_err(|_| err("expected a File"))?;
    let written = file.size() as u64;
    if written != bytes.len() as u64 {
        let _ = JsFuture::from(dir.remove_entry(name)).await;
        return Err(err(&format!(
            "browser wrote {written} bytes instead of {}",
            bytes.len()
        )));
    }
    Ok(())
}

pub async fn delete(dir: &FileSystemDirectoryHandle, name: &str) -> Result<(), StorageError> {
    JsFuture::from(dir.remove_entry(name)).await?;
    Ok(())
}

/// Rename within a directory as copy-then-delete (`FileSystemHandle.
/// move()` isn't available everywhere yet). Refuses to clobber an
/// existing file.
pub async fn rename(
    dir: &FileSystemDirectoryHandle,
    from: &str,
    to: &str,
) -> Result<(), StorageError> {
    if from == to {
        return Ok(());
    }
    if read(dir, to).await?.is_some() {
        return Err(err("a file with that name already exists"));
    }
    let bytes = read(dir, from)
        .await?
        .ok_or_else(|| err("the file disappeared"))?;
    write(dir, to, &bytes).await?;
    delete(dir, from).await
}

//! [`tango_library::Storage`], in a browser.
//!
//! # Why this is a memory image with a persistence mirror
//!
//! The trait is deliberately synchronous — `apply_patch` and every
//! session-construction path read through it, and making them async
//! would buy nothing. The backend that can honour that natively is
//! OPFS's `createSyncAccessHandle()` — but that is `[Exposed=DedicatedWorker]`,
//! and this app is main-thread-only: the core, the frame pump, the audio
//! callback and the UI all live there, because a browser has no threads
//! to give a lite build without cross-origin isolation. On the main
//! thread OPFS is as asynchronous as anything else and offers nothing
//! IndexedDB doesn't.
//!
//! So the files live in memory and the reads and writes are real reads
//! and writes of that image, which is what makes them synchronous.
//! Persistence is a mirror: [`Files::load`] pulls the whole store in at
//! startup, and every mutation queues the affected path to be written
//! back. That is affordable because of what is actually stored — some
//! ROMs, some saves, a handful of patch packages, a config file — and
//! because the desktop's ROM scanner holds every ROM in memory anyway.
//!
//! If emulation ever moves into a worker, OPFS becomes the better
//! backend (sync access handles, no copy, byte ranges) and this module
//! is the only thing that changes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use tango_library::storage::{Entry, ListFuture, Listing, ReadSeek, Storage};

/// The file image plus its mirror. Cheap to clone — every clone is the
/// same store.
#[derive(Clone)]
pub struct Files {
    inner: Rc<RefCell<HashMap<PathBuf, Vec<u8>>>>,
}

impl Files {
    /// Read the whole persisted store into memory. Everything after this
    /// is synchronous.
    pub async fn load() -> Self {
        let mut inner = HashMap::new();
        for key in idb::keys().await.unwrap_or_default() {
            match idb::get(&key).await {
                Ok(Some(bytes)) => {
                    inner.insert(PathBuf::from(&key), bytes);
                }
                Ok(None) => {}
                Err(e) => log::warn!("storage: load {key} failed: {e:?}"),
            }
        }
        log::info!("storage: {} files loaded", inner.len());
        Self {
            inner: Rc::new(RefCell::new(inner)),
        }
    }

    /// Total bytes held, for the settings screen's "how much am I using"
    /// line. The browser's own quota accounting is asynchronous and
    /// origin-wide; this is the part we put there.
    pub fn bytes_used(&self) -> u64 {
        self.inner.borrow().values().map(|v| v.len() as u64).sum()
    }

    /// Push one path's current state to the mirror. Fire-and-forget: a
    /// failed write costs persistence across a reload, never the running
    /// session, so it is logged rather than surfaced.
    fn persist(&self, path: &Path) {
        let key = key(path);
        let value = self.inner.borrow().get(path).cloned();
        wasm_bindgen_futures::spawn_local(async move {
            let result = match value {
                Some(bytes) => idb::put(&key, &bytes).await,
                None => idb::delete(&key).await,
            };
            if let Err(e) = result {
                log::warn!("storage: persisting {key} failed: {e:?}");
            }
        });
    }
}

/// Paths are the keys. They are always the ones this app built (rooted
/// at `/`, forward slashes), so the round trip is lossless.
fn key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn not_found(path: &Path) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::NotFound, format!("{} not found", path.display()))
}

impl Storage for Files {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.inner.borrow().get(path).cloned().ok_or_else(|| not_found(path))
    }

    fn open(&self, path: &Path) -> std::io::Result<Box<dyn ReadSeek>> {
        // The whole point of `open` upstream is to read a header without
        // pulling a large file into memory. Here the file is already in
        // memory, so a cursor over a copy is the same thing.
        Ok(Box::new(std::io::Cursor::new(self.read(path)?)))
    }

    fn write(&self, path: &Path, data: &[u8]) -> std::io::Result<()> {
        // No directories to create: a flat path→bytes map has no empty
        // parents to miss, and `list` derives directories from the keys.
        self.inner.borrow_mut().insert(path.to_path_buf(), data.to_vec());
        self.persist(path);
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        if self.inner.borrow_mut().remove(path).is_none() {
            return Err(not_found(path));
        }
        self.persist(path);
        Ok(())
    }

    fn create_dir_all(&self, _path: &Path) -> std::io::Result<()> {
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        // Replacing, as the trait requires — `write_atomic` depends on
        // it. Within one synchronous call there is nothing to interleave,
        // so the memory image never sees a half-renamed state; the two
        // mirror writes that follow can land in either order, and the
        // worst case is a stale duplicate that the next load ignores.
        let bytes = self.inner.borrow_mut().remove(from).ok_or_else(|| not_found(from))?;
        self.inner.borrow_mut().insert(to.to_path_buf(), bytes);
        self.persist(from);
        self.persist(to);
        Ok(())
    }

    fn is_file(&self, path: &Path) -> bool {
        self.inner.borrow().contains_key(path)
    }

    fn list<'a>(&'a self, roots: &'a [PathBuf]) -> ListFuture<'a> {
        // Async in the trait because `FileSystemDirectoryHandle` has no
        // synchronous iteration; over a map there is nothing to await,
        // and the future is ready on first poll.
        let entries = self
            .inner
            .borrow()
            .iter()
            .filter(|(path, _)| roots.iter().any(|root| path.starts_with(root)))
            .map(|(path, bytes)| Entry {
                path: path.clone(),
                len: bytes.len() as u64,
                // No modification times: nothing here is edited outside
                // this app, so the listing's other fields are enough of a
                // rescan fingerprint on their own.
                modified: None,
            })
            .collect();
        Box::pin(std::future::ready(Listing::new(entries)))
    }
}

/// The mirror: a single IndexedDB object store of path → bytes.
mod idb {
    use super::*;

    const DB_NAME: &str = "tango-lite";
    const STORE: &str = "files";

    pub async fn keys() -> Result<Vec<String>, JsValue> {
        let req = db()
            .await?
            .transaction_with_str(STORE)?
            .object_store(STORE)?
            .get_all_keys()?;
        Ok(js_sys::Array::from(&settle(&req).await?)
            .iter()
            .filter_map(|k| k.as_string())
            .collect())
    }

    pub async fn get(key: &str) -> Result<Option<Vec<u8>>, JsValue> {
        let req = db()
            .await?
            .transaction_with_str(STORE)?
            .object_store(STORE)?
            .get(&JsValue::from_str(key))?;
        let value = settle(&req).await?;
        if value.is_undefined() || value.is_null() {
            return Ok(None);
        }
        Ok(Some(value.unchecked_into::<js_sys::Uint8Array>().to_vec()))
    }

    pub async fn put(key: &str, bytes: &[u8]) -> Result<(), JsValue> {
        let req = db()
            .await?
            .transaction_with_str_and_mode(STORE, web_sys::IdbTransactionMode::Readwrite)?
            .object_store(STORE)?
            .put_with_key(&js_sys::Uint8Array::from(bytes), &JsValue::from_str(key))?;
        settle(&req).await?;
        Ok(())
    }

    pub async fn delete(key: &str) -> Result<(), JsValue> {
        let req = db()
            .await?
            .transaction_with_str_and_mode(STORE, web_sys::IdbTransactionMode::Readwrite)?
            .object_store(STORE)?
            .delete(&JsValue::from_str(key))?;
        settle(&req).await?;
        Ok(())
    }

    thread_local! {
        /// One connection for the page. Opening per operation works, but
        /// each open is its own trip through the version check, and the
        /// save autosave calls this on a timer.
        static CONNECTION: RefCell<Option<web_sys::IdbDatabase>> = const { RefCell::new(None) };
    }

    async fn db() -> Result<web_sys::IdbDatabase, JsValue> {
        if let Some(db) = CONNECTION.with(|c| c.borrow().clone()) {
            return Ok(db);
        }
        let factory = web_sys::window()
            .ok_or_else(|| JsValue::from_str("no window"))?
            .indexed_db()?
            .ok_or_else(|| JsValue::from_str("indexedDB unavailable"))?;
        let req = factory.open_with_u32(DB_NAME, 1)?;
        // First open on this origin: create the one object store. Runs
        // inside the version-change transaction, before `settle` resolves.
        let upgrade = Closure::<dyn FnMut(web_sys::Event)>::new(|e: web_sys::Event| {
            let Some(target) = e.target() else { return };
            let req: web_sys::IdbOpenDbRequest = target.unchecked_into();
            if let Ok(db) = req.result() {
                let _ = db.unchecked_into::<web_sys::IdbDatabase>().create_object_store(STORE);
            }
        });
        req.set_onupgradeneeded(Some(upgrade.as_ref().unchecked_ref()));
        let result = settle(&req).await;
        drop(upgrade);
        let db: web_sys::IdbDatabase = result?.unchecked_into();
        CONNECTION.with(|c| *c.borrow_mut() = Some(db.clone()));
        Ok(db)
    }

    /// Await an `IDBRequest`: resolve with its `result`, reject with its
    /// `error`. Both handlers go on before the event loop turns again,
    /// which is what makes it race-free.
    async fn settle(req: &web_sys::IdbRequest) -> Result<JsValue, JsValue> {
        let (tx, rx) = futures::channel::oneshot::channel::<Result<JsValue, JsValue>>();
        let tx = Rc::new(RefCell::new(Some(tx)));

        let (ok_tx, ok_req) = (tx.clone(), req.clone());
        let onsuccess = Closure::once(move |_: web_sys::Event| {
            if let Some(tx) = ok_tx.borrow_mut().take() {
                let _ = tx.send(ok_req.result());
            }
        });
        let (err_tx, err_req) = (tx.clone(), req.clone());
        let onerror = Closure::once(move |_: web_sys::Event| {
            if let Some(tx) = err_tx.borrow_mut().take() {
                let e = err_req
                    .error()
                    .ok()
                    .flatten()
                    .map(JsValue::from)
                    .unwrap_or_else(|| JsValue::from_str("idb request failed"));
                let _ = tx.send(Err(e));
            }
        });
        req.set_onsuccess(Some(onsuccess.as_ref().unchecked_ref()));
        req.set_onerror(Some(onerror.as_ref().unchecked_ref()));

        let outcome = rx.await;
        // Held until here on purpose: dropping a Closure invalidates the
        // JS function it handed out, and the request only fired just now.
        req.set_onsuccess(None);
        req.set_onerror(None);
        drop((onsuccess, onerror));
        outcome.map_err(|_| JsValue::from_str("idb request dropped"))?
    }
}

/// Small key/value bits that aren't files — the last-used link code and
/// nickname. localStorage is the right size for these and reads
/// synchronously, so the first render doesn't wait on anything.
pub mod prefs {
    fn store() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok().flatten()
    }

    pub fn get(key: &str) -> Option<String> {
        store()?.get_item(&format!("tango-lite.{key}")).ok().flatten()
    }

    pub fn set(key: &str, value: &str) {
        if let Some(s) = store() {
            let _ = s.set_item(&format!("tango-lite.{key}"), value);
        }
    }
}

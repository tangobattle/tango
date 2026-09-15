//! The filesystem seam.
//!
//! Every library path is accessed through [`Storage`]. Native adapters use
//! the filesystem; the browser uses a synchronous memory image that mirrors
//! writes to IndexedDB. This keeps ROM/save parsing and patch application
//! synchronous on both hosts.
//!
//! Directory enumeration returns a future because a backing store may need
//! asynchronous I/O. Scanners receive its completed [`Listing`] and perform
//! parsing separately, without requiring an async API for every read.

use crate::marker::{BoxFuture, WasmNotSend, WasmNotSync};
use std::path::{Path, PathBuf};

/// Future returned by [`Storage::list`]. Boxed so `Storage` stays object
/// safe; `Send` off wasm, where the backing JS promises aren't.
pub type ListFuture<'a> = BoxFuture<'a, Listing>;

/// A snapshot of what is in a set of directories: every file, with its
/// size and modification time.
///
/// This is both the input to a scan and its change-detection
/// fingerprint. Equality is what gates a rescan: an unchanged listing
/// means re-reading would reproduce the current items. Gathering one
/// enumerates but reads nothing, so it is far cheaper than the scan it
/// gates — and because the scan works from the same snapshot, a rescan
/// walks the tree once rather than once to fingerprint and again to
/// read.
#[derive(PartialEq, Eq, Default, Clone, Debug)]
pub struct Listing(Vec<Entry>);

impl Listing {
    pub fn new(mut entries: Vec<Entry>) -> Self {
        // Don't trust enumeration order to be stable between walks —
        // equality is the rescan gate.
        entries.sort_unstable();
        Listing(entries)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A seekable reader, as returned by [`Storage::open`]. The supertrait
/// carries the conditional `Send` through to `dyn ReadSeek`, the same
/// way [`crate::marker::WasmNotSendFuture`] does for boxed futures.
pub trait ReadSeek: std::io::Read + std::io::Seek + WasmNotSend {}
impl<T: std::io::Read + std::io::Seek + WasmNotSend + ?Sized> ReadSeek for T {}

/// One file turned up by [`Storage::list`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub len: u64,
    /// Modification time in milliseconds since the Unix epoch, when the
    /// backend tracks one. A plain integer also works with browser timestamps.
    pub modified: Option<u64>,
}

/// A store of files addressed by path. Paths are ordinary `Path`s — on a
/// browser backend they are `/`-separated keys into its file store.
///
/// Implementations report failures as `std::io::Error`; a missing file
/// must be `ErrorKind::NotFound`, since callers branch on it.
pub trait Storage: WasmNotSend + WasmNotSync + 'static {
    /// Read a whole file.
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>>;
    /// Open a file for random access. Scans that only need a header —
    /// the replay index reads one out of files that run to megabytes —
    /// go through this rather than pulling whole files into memory.
    fn open(&self, path: &Path) -> std::io::Result<Box<dyn ReadSeek>>;
    /// Create or replace a file, creating parent directories as needed.
    fn write(&self, path: &Path, data: &[u8]) -> std::io::Result<()>;
    fn remove_file(&self, path: &Path) -> std::io::Result<()>;
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()>;
    /// Replace `to` with `from`. Must overwrite an existing `to`.
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()>;
    fn is_file(&self, path: &Path) -> bool;

    /// Every file at or under each of `roots`, recursively, as one
    /// [`Listing`]. Roots that do not exist contribute nothing rather
    /// than failing — the content directories are all created lazily,
    /// and a per-root error is not worth failing a whole rescan over.
    ///
    /// Backends may enumerate asynchronously. Scans parse the returned
    /// snapshot synchronously; native hosts run that work off the UI thread.
    fn list<'a>(&'a self, roots: &'a [PathBuf]) -> ListFuture<'a>;
}

/// Read a file, mapping `NotFound` to `None` — the shape nearly every
/// caller here wants, since an absent index/config/etag is normal.
pub fn read_opt(storage: &dyn Storage, path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match storage.read(path) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Write through a sibling temporary and rename into place, so an
/// interrupted write can't leave a truncated file where a valid one was.
/// The backend's `rename` must replace an existing destination.
pub fn write_atomic(storage: &dyn Storage, path: &Path, data: &[u8]) -> std::io::Result<()> {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Err(std::io::Error::other("no file name"));
    };
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    storage.write(&tmp, data)?;
    match storage.rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = storage.remove_file(&tmp);
            Err(e)
        }
    }
}

/// `std::fs`-backed [`Storage`]: the native frontend's implementation.
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
mod std_impl {
    use super::{ListFuture, Listing, ReadSeek, Storage};
    use std::path::{Path, PathBuf};

    /// The whole real filesystem, paths taken as absolute. Stateless, so
    /// the library can hold it as a `&'static dyn Storage`.
    pub struct StdStorage;

    impl Storage for StdStorage {
        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            std::fs::read(path)
        }

        fn open(&self, path: &Path) -> std::io::Result<Box<dyn ReadSeek>> {
            Ok(Box::new(std::io::BufReader::new(std::fs::File::open(path)?)))
        }

        fn write(&self, path: &Path, data: &[u8]) -> std::io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, data)
        }

        fn remove_file(&self, path: &Path) -> std::io::Result<()> {
            std::fs::remove_file(path)
        }

        fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
            std::fs::create_dir_all(path)
        }

        fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
            std::fs::rename(from, to)
        }

        fn is_file(&self, path: &Path) -> bool {
            path.is_file()
        }

        fn list<'a>(&'a self, roots: &'a [PathBuf]) -> ListFuture<'a> {
            // Nothing here actually awaits: walkdir is synchronous, and
            // the future is ready on first poll. The signature exists
            // for backends that can't say the same.
            Box::pin(std::future::ready(Listing::new(super::walk(roots))))
        }
    }
}

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub use std_impl::StdStorage;

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
fn walk(roots: &[std::path::PathBuf]) -> Vec<Entry> {
    let mut out = vec![];
    for root in roots {
        for entry in walkdir::WalkDir::new(root) {
            let Ok(entry) = entry else {
                continue;
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let (len, modified) = entry
                .metadata()
                .map(|m| (m.len(), m.modified().ok().and_then(epoch_millis)))
                .unwrap_or((0, None));
            out.push(Entry {
                path: entry.into_path(),
                len,
                modified,
            });
        }
    }
    out
}

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
fn epoch_millis(t: std::time::SystemTime) -> Option<u64> {
    t.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
}

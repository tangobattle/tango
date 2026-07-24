//! The filesystem seam.
//!
//! Every path the library reads or writes goes through [`Storage`] so a
//! frontend can supply the backing store: `std::fs` natively,
//! [OPFS](https://developer.mozilla.org/en-US/docs/Web/API/File_System_API/Origin_private_file_system)
//! in a browser build.
//!
//! # Why the file operations are synchronous and only `list` is not
//!
//! OPFS exposes `createSyncAccessHandle()`, which gives genuinely
//! synchronous reads and writes — but *only inside a dedicated Worker*.
//! That is where a browser build has to put the emulator anyway (the
//! session drive loops are threads), so the library running beside it
//! can be synchronous too. Keeping it that way matters: it is what lets
//! `patch::apply_patch` and the save/ROM loads stay sync, instead of
//! turning every session-construction path async to no benefit.
//!
//! Directory *enumeration* has no synchronous form — `FileSystemDirectoryHandle`
//! is async-iterated whatever thread you are on — so [`Storage::list`]
//! alone returns a future. That is the one thing the scanners already do
//! off the UI thread, so it costs nothing.

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
    /// backend tracks one. A plain integer rather than `SystemTime`
    /// because that is the only form OPFS offers (`File.lastModified`),
    /// and because `SystemTime` arithmetic is a trap on wasm32.
    pub modified: Option<u64>,
}

/// A store of files addressed by path. Paths are ordinary `Path`s — on a
/// browser backend they are just `/`-separated keys into the OPFS
/// directory tree, which is why nothing here uses `OsStr`-only APIs.
///
/// Implementations report failures as `std::io::Error`; a missing file
/// must be `ErrorKind::NotFound`, since callers branch on it.
pub trait Storage: WasmNotSend + WasmNotSync + 'static {
    /// Read a whole file.
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>>;
    /// Open a file for random access. Scans that only need a header —
    /// the replay index reads one out of files that run to megabytes —
    /// go through this rather than pulling whole files into memory.
    /// OPFS supports it: `FileSystemSyncAccessHandle::read` takes an
    /// offset.
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
    /// The only operation here that is async, and the reason is
    /// external: `FileSystemDirectoryHandle` iterates asynchronously
    /// whatever thread you are on, so a browser backend has no
    /// synchronous form to offer. Everything downstream — the scans
    /// themselves — works from the returned snapshot and stays
    /// synchronous, which is what keeps a rescan a plain blocking call
    /// rather than an async pipeline.
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
/// `rename` is required to be replacing, which both `std::fs` and OPFS's
/// move-with-overwrite give us.
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

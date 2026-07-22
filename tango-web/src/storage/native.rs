//! Native storage: the desktop client's directory layout, scanned in
//! place. The data dir is `~/Documents/Tango` — the same tree the
//! desktop app uses, so both clients see one library — with `roms/`,
//! `saves/`, `replays/`, `patches/` beneath it. Stats sidecars go to
//! this app's own cache dir (they're derived data; kept out of the
//! desktop's cache so the two clients' formats can drift safely).
//!
//! API mirrors the web backend: directory/file *handles* are path
//! newtypes, [`list_files`] walks recursively like the desktop
//! scanners (a file's `name` is its forward-slash relative path, which
//! [`read`]/[`write`]/[`delete`] resolve back), [`list_dirs`] lists
//! immediate subdirectories (the patch tree's shape). I/O is plain
//! sync `std::fs` inside async fns — these run on the UI thread, and
//! the library-scan volumes involved are the same ones the desktop
//! does on its blocking pool; if startup jank ever shows, lift the
//! scans onto a background thread.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct StorageError(String);

impl From<std::io::Error> for StorageError {
    fn from(e: std::io::Error) -> Self {
        StorageError(e.to_string())
    }
}

fn err(msg: &str) -> StorageError {
    StorageError(msg.to_owned())
}

/// A directory handle: just a path. `name`/`path` mirror the web
/// handle's surface for callers that want them.
#[derive(Debug, Clone)]
pub struct DirHandle(PathBuf);

#[allow(dead_code)]
impl DirHandle {
    pub fn name(&self) -> String {
        self.0
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// A (created) subdirectory handle.
    pub fn child(&self, name: &str) -> Result<DirHandle, StorageError> {
        let p = self.0.join(name);
        std::fs::create_dir_all(&p)?;
        Ok(DirHandle(p))
    }
}

/// A file handle: the file's path plus the relative name it was listed
/// under (forward-slash separated below the listed directory).
#[derive(Debug, Clone)]
pub struct FileHandle {
    path: PathBuf,
    #[allow(dead_code)]
    name: String,
}

#[allow(dead_code)]
impl FileHandle {
    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The app's storage root(s). Cheap to clone.
#[derive(Clone)]
pub struct Storage {
    /// Kept for web-API parity (the web exporter's temp lives at the
    /// root; the native exporter uses real temp dirs).
    #[allow(dead_code)]
    root: DirHandle,
    roms: DirHandle,
    saves: DirHandle,
    replays: DirHandle,
    patches: DirHandle,
    stats: DirHandle,
}

/// `~/Documents/Tango`, the desktop client's default data dir.
fn data_dir() -> PathBuf {
    directories_next::UserDirs::new()
        .and_then(|d| d.document_dir().map(|p| p.to_path_buf()))
        .or_else(|| directories_next::UserDirs::new().map(|d| d.home_dir().to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Tango")
}

fn cache_dir() -> PathBuf {
    directories_next::ProjectDirs::from("net", "n1gp", "tango-web")
        .map(|d| d.cache_dir().to_path_buf())
        .unwrap_or_else(|| data_dir().join("cache"))
}

impl Storage {
    pub async fn open() -> Result<Storage, StorageError> {
        let root = data_dir();
        let subdir = |name: &str| -> Result<DirHandle, StorageError> {
            let p = root.join(name);
            std::fs::create_dir_all(&p)?;
            Ok(DirHandle(p))
        };
        let roms = subdir("roms")?;
        let saves = subdir("saves")?;
        let replays = subdir("replays")?;
        let patches = subdir("patches")?;
        let stats = cache_dir().join("stats");
        std::fs::create_dir_all(&stats)?;
        std::fs::create_dir_all(&root)?;
        Ok(Storage {
            root: DirHandle(root),
            roms,
            saves,
            replays,
            patches,
            stats: DirHandle(stats),
        })
    }

    /// The data dir itself — scratch space outside the scanned
    /// subdirectories.
    #[allow(dead_code)]
    pub fn root(&self) -> &DirHandle {
        &self.root
    }

    pub fn roms(&self) -> &DirHandle {
        &self.roms
    }

    pub fn replays(&self) -> &DirHandle {
        &self.replays
    }

    /// The synced patch tree (`patches/<name>/v<version>/…`), the
    /// desktop's on-disk layout.
    pub fn patches(&self) -> &DirHandle {
        &self.patches
    }

    /// The save directory `saves/` — which game each file belongs to
    /// is detected from its content, like the desktop scanner.
    pub fn saves(&self) -> &DirHandle {
        &self.saves
    }

    /// Match-stats sidecars (`<replay stem>.stats`). Format-versioned:
    /// readers reject stale versions and recompute.
    pub fn stats(&self) -> &DirHandle {
        &self.stats
    }
}

/// List a directory's immediate subdirectories (name, handle), sorted
/// by name.
pub async fn list_dirs(dir: &DirHandle) -> Result<Vec<(String, DirHandle)>, StorageError> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir.0)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            out.push((
                entry.file_name().to_string_lossy().into_owned(),
                DirHandle(entry.path()),
            ));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// List a directory's files recursively (relative name, handle),
/// sorted by name — the desktop scanners walk subdirectories, and a
/// shared `~/Documents/Tango` tree may be organized that way.
pub async fn list_files(dir: &DirHandle) -> Result<Vec<(String, FileHandle)>, StorageError> {
    fn walk(
        root: &Path,
        dir: &Path,
        out: &mut Vec<(String, FileHandle)>,
    ) -> Result<(), StorageError> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            let path = entry.path();
            if ty.is_dir() {
                walk(root, &path, out)?;
            } else if ty.is_file() {
                let name = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((
                    name.clone(),
                    FileHandle { path, name },
                ));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(&dir.0, &dir.0, &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn resolve(dir: &DirHandle, name: &str) -> Result<PathBuf, StorageError> {
    // Relative names come back out of `list_files`; refuse anything
    // that would escape the directory.
    if name.split('/').any(|c| c == ".." || c.is_empty()) || name.contains('\\') {
        return Err(err(&format!("bad file name: {name}")));
    }
    Ok(dir.0.join(name))
}

/// Read a file's bytes by (relative) name; `Ok(None)` when it doesn't
/// exist.
pub async fn read(dir: &DirHandle, name: &str) -> Result<Option<Vec<u8>>, StorageError> {
    let path = resolve(dir, name)?;
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Read a file handle's bytes.
pub async fn read_handle(handle: &FileHandle) -> Result<Vec<u8>, StorageError> {
    Ok(std::fs::read(&handle.path)?)
}

/// Create-or-truncate `name` with `bytes`. Parent directories are
/// created as needed (relative names may point into subdirectories).
pub async fn write(dir: &DirHandle, name: &str, bytes: &[u8]) -> Result<(), StorageError> {
    let path = resolve(dir, name)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, bytes)?;
    Ok(())
}

pub async fn delete(dir: &DirHandle, name: &str) -> Result<(), StorageError> {
    let path = resolve(dir, name)?;
    std::fs::remove_file(&path)?;
    Ok(())
}

/// Rename within a directory. Refuses to clobber an existing file.
pub async fn rename(dir: &DirHandle, from: &str, to: &str) -> Result<(), StorageError> {
    if from == to {
        return Ok(());
    }
    let from_path = resolve(dir, from)?;
    let to_path = resolve(dir, to)?;
    if to_path.exists() {
        return Err(err("a file with that name already exists"));
    }
    if let Some(parent) = to_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&from_path, &to_path)?;
    Ok(())
}

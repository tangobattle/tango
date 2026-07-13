//! Battle Network Legacy Collection (Steam) discovery + asset access.
//!
//! Both BNLC volumes ship the games + shared chrome assets under
//! `<app_dir>/exe/data/`. Per-game ROMs live in `exeN.dat` zips; the
//! shared assets (backgrounds, etc.) live in `exe.dat`.
//!
//! [`Bnlc`] locates a volume's install via Steam and keeps the shared
//! `exe.dat` zip open for the process lifetime, so repeated reads
//! (backgrounds + future asset pulls) don't re-probe Steam or re-open
//! the archive. Use [`get`] to access the per-volume cached instance.

use std::io::Read;
use std::path::PathBuf;
use std::sync::LazyLock;

use std::sync::Mutex;

/// Which BNLC volume — Vol 1 (BN1-3) or Vol 2 (BN4-6). Defined in
/// `tango-gamesupport` so per-game `BackgroundRef`s can name their volume
/// without depending on the GUI crate; re-exported here since the runtime
/// discovery below is GUI-side.
pub use tango_gamesupport::Volume;

/// Open BNLC volume install + cached shared-archive handle. Held in
/// a process-lifetime `LazyLock` per volume so each shared-file read
/// reuses the same `ZipArchive`.
pub struct Bnlc {
    volume: Volume,
    app_dir: PathBuf,
    /// `exe.dat` archive kept open for the lifetime of this Bnlc.
    /// `ZipArchive::by_name` seeks the underlying file, so we need
    /// exclusive access for every read.
    shared: Mutex<zip::ZipArchive<std::fs::File>>,
}

impl Bnlc {
    /// Try to open a BNLC volume. Returns `None` when Steam isn't
    /// installed, the volume isn't owned/installed, or the shared
    /// archive can't be opened. Prefer [`get`] — it caches the
    /// result for the process lifetime.
    pub fn open(volume: Volume) -> Option<Self> {
        let app_dir = locate_app_dir(volume)?;
        let archive_path = app_dir.join("exe").join("data").join("exe.dat");
        let file = std::fs::File::open(&archive_path)
            .inspect_err(|e| log::debug!("bnlc {volume:?}: open {}: {e}", archive_path.display()))
            .ok()?;
        let za = zip::ZipArchive::new(file)
            .inspect_err(|e| log::warn!("bnlc {volume:?}: open zip {}: {e}", archive_path.display()))
            .ok()?;
        Some(Bnlc {
            volume,
            app_dir,
            shared: Mutex::new(za),
        })
    }

    /// Paths of the per-game `exeN.dat` archives in the volume's
    /// `<root>/exe/data/`. Excludes the shared `exe.dat`. Returned
    /// as a Vec rather than an iterator so the underlying ReadDir
    /// closes immediately.
    pub fn rom_archives(&self) -> Vec<PathBuf> {
        let data_path = self.app_dir.join("exe").join("data");
        let read_dir = match std::fs::read_dir(&data_path) {
            Ok(rd) => rd,
            Err(e) => {
                log::warn!("bnlc {:?}: read {}: {e}", self.volume, data_path.display());
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for entry in read_dir.flatten() {
            let p = entry.path();
            let Some(file_name) = p.file_name() else { continue };
            if file_name != std::ffi::OsStr::new("exe.dat")
                && file_name.to_string_lossy().starts_with("exe")
                && p.extension() == Some(std::ffi::OsStr::new("dat"))
            {
                out.push(p);
            }
        }
        out
    }

    /// Read a single file out of the cached shared `exe.dat`.
    /// Returns `None` on missing entry / IO error.
    pub fn read_shared_file(&self, path_in_zip: &str) -> Option<Vec<u8>> {
        let mut za = self.shared.lock().unwrap();
        let mut entry = za
            .by_name(path_in_zip)
            .inspect_err(|e| log::warn!("bnlc {:?}: read {path_in_zip}: {e}", self.volume))
            .ok()?;
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut buf)
            .inspect_err(|e| log::warn!("bnlc {:?}: read {path_in_zip}: {e}", self.volume))
            .ok()?;
        Some(buf)
    }
}

/// Process-lifetime cached [`Bnlc`] for a volume. The first call
/// probes Steam + opens the shared archive; subsequent calls return
/// the same `&'static Bnlc`. `None` whenever the volume isn't
/// installed.
pub fn get(volume: Volume) -> Option<&'static Bnlc> {
    static VOL1: LazyLock<Option<Bnlc>> = LazyLock::new(|| Bnlc::open(Volume::Vol1));
    static VOL2: LazyLock<Option<Bnlc>> = LazyLock::new(|| Bnlc::open(Volume::Vol2));
    match volume {
        Volume::Vol1 => VOL1.as_ref(),
        Volume::Vol2 => VOL2.as_ref(),
    }
}

fn locate_app_dir(volume: Volume) -> Option<PathBuf> {
    let steamdir = steamlocate::SteamDir::locate()
        .inspect_err(|err| log::debug!("steam not located: {err:?}"))
        .ok()?;
    let (app, lib) = steamdir.find_app(volume.steam_app_id()).ok().flatten()?;
    Some(lib.resolve_app_dir(&app))
}

//! Battle Network Legacy Collection (Steam) discovery, copied from
//! `tango/src/bnlc.rs` and trimmed to ROM-archive discovery (the shared
//! `exe.dat` asset access comes over when backgrounds/assets land).

use std::path::PathBuf;
use std::sync::LazyLock;

/// Which BNLC volume — Vol 1 (BN1-3) or Vol 2 (BN4-6).
pub use tango_gamesupport::Volume;

/// Located BNLC volume install.
pub struct Bnlc {
    volume: Volume,
    app_dir: PathBuf,
}

impl Bnlc {
    /// Try to locate a BNLC volume. Returns `None` when Steam isn't
    /// installed or the volume isn't owned/installed. Prefer [`get`] —
    /// it caches the result for the process lifetime.
    pub fn open(volume: Volume) -> Option<Self> {
        let app_dir = locate_app_dir(volume)?;
        Some(Bnlc { volume, app_dir })
    }

    /// Paths of the per-game `exeN.dat` archives in the volume's
    /// `<root>/exe/data/`. Excludes the shared `exe.dat`.
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
}

/// Process-lifetime cached [`Bnlc`] for a volume. `None` whenever the
/// volume isn't installed.
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

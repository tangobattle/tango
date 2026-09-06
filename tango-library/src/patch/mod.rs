//! Patch catalogue, installation, and ROM patching.
//!
//! Installed `.tangopatch` packages and the cached `index.json` are
//! merged by [`Catalog`]. Fetching the index does not download packages;
//! callers install individual versions through [`download`]. All I/O
//! uses the frontend's [`Storage`] and [`crate::http::Http`] implementations.

use crate::http;
use crate::rom::GameRef;
use crate::storage::Storage;
use std::path::{Path, PathBuf};
use tango_patch::Package;

mod catalog;
mod download;

pub use catalog::{display_authors, scan, Catalog, Patch, PatchMap, Scanner, Version, VersionInfo};
pub use download::{download, fetch_index, uninstall, Download, Downloads, Outcome, Progress, VersionKey};

#[cfg(all(test, feature = "native", not(target_arch = "wasm32")))]
mod tests;

/// Everything that can go wrong reading, fetching, or applying a patch.
///
/// The `tango_patch::Error` sources are boxed: that type nests (its
/// `At` variant wraps another one), so inlining it would make every
/// `Result` in this module pay for the largest failure case.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Http(#[from] http::Error),
    #[error(transparent)]
    Package(Box<tango_patch::Error>),
    #[error("bad patch name: {0}")]
    BadName(String),
    #[error("{name} {version}: {source}")]
    NotWhatTheIndexPromised {
        name: String,
        version: semver::Version,
        #[source]
        source: Box<tango_patch::Error>,
    },
    #[error("{name} {version}: package contains {actual} instead")]
    WrongPackage {
        name: String,
        version: semver::Version,
        /// The `name version` the package actually declares. Kept
        /// pre-formatted: a second `semver::Version` here would make
        /// every `Result` in this module wider than the payload it
        /// usually carries.
        actual: String,
    },
    #[error("bad bps patch: {0}")]
    BpsDecode(#[from] bps::DecodeError),
    #[error("could not apply bps patch: {0}")]
    BpsApply(#[from] bps::ApplyError),
}

impl From<tango_patch::Error> for Error {
    fn from(e: tango_patch::Error) -> Self {
        Error::Package(Box::new(e))
    }
}

fn validate_name(name: &str) -> Result<(), Error> {
    tango_patch::validate_name(name).map_err(|e| Error::BadName(e.to_string()))
}

/// Where a given patch version lives (or would live) on disk.
pub fn package_path(patches_path: &Path, name: &str, version: &semver::Version) -> PathBuf {
    patches_path.join(format!("{name}-{version}.{}", tango_patch::EXTENSION))
}

/// `<patches>/index.json` — the cached repo catalogue.
pub fn index_path(patches_path: &Path) -> PathBuf {
    patches_path.join(tango_patch::index::FILE_NAME)
}

/// Everything a scan reads, for the change-detection fingerprint — the
/// packages and the cached index all live in the one directory.
pub fn scan_roots(patches_path: &Path) -> Vec<PathBuf> {
    vec![patches_path.to_path_buf()]
}

/// Read the BPS for `game` out of the installed package and apply it to
/// `rom`, returning the patched image.
///
/// Synchronous on purpose: every session construction path applies a
/// patch, and making this async would turn all of them async for no
/// gain. See [`crate::storage`] for why that is affordable.
pub fn apply_patch(
    storage: &dyn Storage,
    rom: &[u8],
    game: GameRef,
    patches_path: &Path,
    patch_name: &str,
    patch_version: &semver::Version,
) -> Result<Vec<u8>, Error> {
    // Names are validated on the way in (`tango_patch::validate_name`
    // forbids separators), so a name can't escape the directory.
    validate_name(patch_name)?;

    let path = package_path(patches_path, patch_name, patch_version);
    let (rom_code, revision) = game.rom_code_and_revision();
    let target = tango_patch::RomTarget::new(*rom_code, revision);
    let raw = Package::read(storage.open(&path)?)?.bps(target)?;
    Ok(bps::Patch::decode(&raw)?.apply(rom)?)
}

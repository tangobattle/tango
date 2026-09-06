//! Conditional index fetches and validated package installation.

use super::{index_path, package_path, validate_name, Error};
use crate::http::{self, Fetch, Http};
use crate::storage::{self, Storage};
use std::path::Path;
use tango_patch::Package;

/// A patch version — the unit everything installs, removes and renders.
pub type VersionKey = (String, semver::Version);

/// Progress of one package download, in bytes.
#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
}

/// What a download is doing.
///
/// Owned by `App` rather than by the patches tab: four different things
/// start downloads — the patches tab, a lobby peer's patch, a replay's
/// patch, and the play tab's picker — and two tabs render them.
#[derive(Debug, Clone, Copy)]
pub enum Download {
    Running(Progress),
    /// Kept after a failure so the UI can say so instead of going quiet.
    /// Replaced when the download is retried.
    Failed,
}

impl Download {
    /// Whole percent, once the server has told us how big the package
    /// is. `None` while that's still unknown, or after a failure.
    pub fn percent(&self) -> Option<u64> {
        match self {
            Download::Running(p) if p.total > 0 => Some(p.downloaded * 100 / p.total),
            _ => None,
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self, Download::Running(_))
    }

    /// Completed fraction for a progress bar, `None` until the server
    /// has told us how big the package is (and after a failure).
    pub fn fraction(&self) -> Option<f32> {
        match self {
            Download::Running(p) if p.total > 0 => Some(p.downloaded as f32 / p.total as f32),
            _ => None,
        }
    }
}

pub type Downloads = std::collections::HashMap<VersionKey, Download>;

/// Fetch the repo index, writing it to storage when it changed.
///
/// Returns `true` if the stored copy was replaced (the caller should
/// rescan). Conditional on the stored ETag, so the common case — the
/// repo hasn't published anything since we last looked — is a 304 with
/// no body, which is what lets this poll on a timer without being rude.
pub async fn fetch_index(
    http: &dyn Http,
    storage: &dyn Storage,
    url: &str,
    patches_path: &Path,
) -> Result<bool, Error> {
    storage.create_dir_all(patches_path)?;
    let path = index_path(patches_path);
    let etag_path = patches_path.join("index.etag");

    // Only send the validator if we still have the body it describes.
    let etag = if storage.is_file(&path) {
        storage::read_opt(storage, &etag_path)?.map(|raw| String::from_utf8_lossy(&raw).trim().to_owned())
    } else {
        None
    };

    let full_url = format!("{}/{}", url.trim_end_matches('/'), tango_patch::index::FILE_NAME);
    let fetched = http
        .get(http::Request {
            url: &full_url,
            if_none_match: etag.as_deref(),
            max_len: None,
            on_progress: None,
        })
        .await?;

    let (etag, raw) = match fetched {
        Fetch::NotModified | Fetch::Cancelled => return Ok(false),
        Fetch::Body { etag, body } => (etag, body),
    };

    // Parse before writing: a corrupt or future-format index shouldn't
    // clobber the usable one we already have.
    let index = tango_patch::Index::parse(&String::from_utf8_lossy(&raw))?;
    storage::write_atomic(storage, &path, &raw)?;
    match etag {
        Some(etag) => storage.write(&etag_path, etag.as_bytes())?,
        None => {
            let _ = storage.remove_file(&etag_path);
        }
    }
    log::info!(
        "patch index: {} versions of {} patches",
        index.len(),
        index.patches.len()
    );
    Ok(true)
}

/// How a download ended, short of an error. The installed path isn't
/// carried back — [`package_path`] derives it, and nothing needs it.
#[derive(Debug)]
pub enum Outcome {
    Installed,
    /// `progress` asked to stop. Nothing was written.
    Cancelled,
}

/// Download one patch version into the patches directory.
///
/// The package is verified against the index's hash *before* anything is
/// written, so a failed, truncated, or cancelled download can't leave a
/// half-written package that the next scan would treat as installed.
/// The index fixes the exact byte count, which is passed down as the
/// transfer's hard cap.
///
/// `progress` returns false to cancel.
pub async fn download(
    http: &dyn Http,
    storage: &dyn Storage,
    url: &str,
    patches_path: &Path,
    name: &str,
    version: &semver::Version,
    entry: &tango_patch::index::Entry,
    progress: impl Fn(Progress) -> bool + crate::marker::WasmNotSend + crate::marker::WasmNotSync,
) -> Result<Outcome, Error> {
    validate_name(name)?;
    storage.create_dir_all(patches_path)?;

    let full_url = format!("{}/{}", url.trim_end_matches('/'), entry.path);
    let on_progress = move |downloaded, total| progress(Progress { downloaded, total });
    let fetched = http
        .get(http::Request {
            url: &full_url,
            if_none_match: None,
            max_len: Some(entry.size),
            on_progress: Some(&on_progress),
        })
        .await?;

    let raw = match fetched {
        Fetch::Cancelled => return Ok(Outcome::Cancelled),
        // Nothing sends a validator here, so a 304 would be the server
        // misbehaving; treat it as an empty body and let verify reject it.
        Fetch::NotModified => Vec::new(),
        Fetch::Body { body, .. } => body,
    };

    verify(&raw, entry, name, version)?;

    storage::write_atomic(storage, &package_path(patches_path, name, version), &raw)?;
    log::info!("installed {name} {version} ({} bytes)", raw.len());
    Ok(Outcome::Installed)
}

/// A downloaded package must be what the index promised, and must be
/// what it says it is. The hash check is also what makes serving
/// packages from a mirror or a CDN cache safe.
fn verify(raw: &[u8], entry: &tango_patch::index::Entry, name: &str, version: &semver::Version) -> Result<(), Error> {
    entry.verify(raw).map_err(|source| Error::NotWhatTheIndexPromised {
        name: name.to_owned(),
        version: version.clone(),
        source: Box::new(source),
    })?;
    let manifest = Package::read(std::io::Cursor::new(raw))?.manifest().clone();
    if manifest.name != name || &manifest.version != version {
        return Err(Error::WrongPackage {
            name: name.to_owned(),
            version: version.clone(),
            actual: format!("{} {}", manifest.name, manifest.version),
        });
    }
    Ok(())
}

/// Delete an installed package. The next scan drops it from the catalog;
/// the index still lists it, so it can be reinstalled.
pub fn uninstall(
    storage: &dyn Storage,
    patches_path: &Path,
    name: &str,
    version: &semver::Version,
) -> Result<(), Error> {
    validate_name(name)?;
    storage.remove_file(&package_path(patches_path, name, version))?;
    Ok(())
}

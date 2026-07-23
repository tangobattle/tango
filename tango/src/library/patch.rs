//! Installed patch packages and the cached repo index.
//!
//! Patches live on disk as `.tangopatch` packages (see the `tango-patch`
//! crate) in `<data>/patches/`, one file per patch version. Alongside
//! them sits `index.json`, a cached copy of the repo's catalogue.
//!
//! # Nothing is mirrored
//!
//! The index is metadata only — tens of KiB for an entire repo — and it
//! carries everything the UI lists plus everything netplay compatibility
//! resolution needs. So the app polls just that, and fetches a package
//! only when something actually calls for it: the player installs it, a
//! peer turns up using it, or a replay needs it to re-simulate. The old
//! format made the client sha256 every file in the repo and download all
//! of them (hundreds of MiB) before it could tell you what a patch was.

use crate::library::rom::GameRef;
use crate::library::scanner;
use futures::StreamExt;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tango_patch::{Compatibility, Package, Tag};
use tokio::io::AsyncWriteExt;

/// One installed patch version — everything read out of its package at
/// scan time. The BPS payloads stay in the file and are read on demand
/// (see [`apply_patch_from_disk`]); a scan only touches metadata, the
/// README, and any save templates.
pub struct Version {
    /// The `.tangopatch` this came from.
    pub path: PathBuf,
    pub netplay: Compatibility,
    pub rom_overrides: tango_patch::Overrides,
    pub supported_games: HashSet<GameRef>,
    /// Per-game save templates the patch ships. Keyed by template name
    /// (empty string = the default template); values are owned Save
    /// trait objects ready to be serialized via `to_sram_dump`.
    pub save_templates: HashMap<GameRef, BTreeMap<String, Box<dyn tango_dataview::save::Save + Send + Sync>>>,
    pub readme: Option<String>,
}

/// An installed patch: its versions plus the metadata of the newest one.
/// Metadata is per-version in the package format (a patch can be
/// retitled or change hands between releases), and the newest version is
/// what a list row should describe.
pub struct Patch {
    pub title: String,
    /// Raw `Name <addr@example.com>` strings, exactly as the manifest
    /// has them — the same form the index carries, so both sources
    /// render identically once passed through [`display_authors`].
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub source: Option<String>,
    pub versions: BTreeMap<semver::Version, Arc<Version>>,
}

pub type PatchMap = BTreeMap<String, Arc<Patch>>;
pub type Scanner = scanner::Scanner<Catalog>;

/// What patches exist, from both directions: what's installed here and
/// what the repo offers. The two overlap freely — a patch can be
/// installed and indexed, installed only (sideloaded), or indexed only
/// (not downloaded yet).
#[derive(Default)]
pub struct Catalog {
    pub installed: PatchMap,
    /// Last index we fetched. Empty before the first successful fetch,
    /// and kept across restarts so the app can browse offline.
    pub index: tango_patch::Index,
}

/// One version as the UI sees it, whether or not it's on disk.
pub struct VersionInfo<'a> {
    pub installed: Option<&'a Arc<Version>>,
    pub indexed: Option<&'a tango_patch::index::Entry>,
}

impl VersionInfo<'_> {
    pub fn is_installed(&self) -> bool {
        self.installed.is_some()
    }

    pub fn netplay(&self) -> Option<&Compatibility> {
        self.installed.map(|v| &v.netplay).or(self.indexed.map(|e| &e.netplay))
    }

    /// Package size in bytes, when the repo told us.
    pub fn size(&self) -> Option<u64> {
        self.indexed.map(|e| e.size)
    }
}

impl Catalog {
    /// Every patch name, installed or merely offered.
    pub fn names(&self) -> BTreeSet<&str> {
        self.installed
            .keys()
            .map(|s| s.as_str())
            .chain(self.index.patches.keys().map(|s| s.as_str()))
            .collect()
    }

    pub fn is_installed(&self, name: &str, version: &semver::Version) -> bool {
        self.version(name, version).is_some()
    }

    pub fn version(&self, name: &str, version: &semver::Version) -> Option<&Arc<Version>> {
        self.installed.get(name)?.versions.get(version)
    }

    /// The index entry for a version, if the repo offers it.
    pub fn entry(&self, name: &str, version: &semver::Version) -> Option<&tango_patch::index::Entry> {
        self.index.get(name, version)
    }

    /// Display metadata for a patch: the installed newest version's, or
    /// the index's if none is installed.
    pub fn title(&self, name: &str) -> Option<&str> {
        self.installed
            .get(name)
            .map(|p| p.title.as_str())
            .or_else(|| self.index.latest(name).map(|(_, e)| e.title.as_str()))
    }

    /// Every known version of `name`, oldest first.
    pub fn versions(&self, name: &str) -> BTreeMap<semver::Version, VersionInfo<'_>> {
        let mut out: BTreeMap<semver::Version, VersionInfo<'_>> = BTreeMap::new();
        if let Some(patch) = self.installed.get(name) {
            for (v, version) in &patch.versions {
                out.insert(
                    v.clone(),
                    VersionInfo {
                        installed: Some(version),
                        indexed: None,
                    },
                );
            }
        }
        if let Some(entries) = self.index.patches.get(name) {
            for (v, entry) in entries {
                out.entry(v.clone())
                    .or_insert(VersionInfo {
                        installed: None,
                        indexed: None,
                    })
                    .indexed = Some(entry);
            }
        }
        out
    }

    /// A version's netplay declaration, preferring the installed package
    /// (it's the thing that would actually run) over the index.
    pub fn compatibility(&self, name: &str, version: &semver::Version) -> Option<&Compatibility> {
        self.version(name, version)
            .map(|v| &v.netplay)
            .or_else(|| self.entry(name, version).map(|e| &e.netplay))
    }

    /// Resolve the netplay identity of a `(game, patch)` pair. `None`
    /// when the patch is one we've never heard of, installed or indexed
    /// — the peer may be running something sideloaded, which we can't
    /// vouch for either way.
    ///
    /// Resolving from the index matters: it's what lets a peer's patch
    /// be judged compatible *before* it's downloaded.
    pub fn tag(&self, game: GameRef, patch: Option<(&str, &semver::Version)>) -> Option<Tag> {
        let family = game.family_and_variant().0;
        match patch {
            None => Some(Tag::vanilla(family)),
            Some((name, version)) => Some(Tag::patched(family, name, version, self.compatibility(name, version)?)),
        }
    }

    /// Games a version supports. Falls back to the index's list so the
    /// Play tab can offer a patch that isn't downloaded yet.
    pub fn supported_games(&self, name: &str, version: &semver::Version) -> HashSet<GameRef> {
        if let Some(v) = self.version(name, version) {
            return v.supported_games.clone();
        }
        self.entry(name, version)
            .map(|e| e.games.iter().filter_map(|t| game_for(*t)).collect())
            .unwrap_or_default()
    }

    /// Newest version of `name` supporting `game` (any version when
    /// `game` is `None`), across installed *and* indexed versions.
    pub fn newest_version(&self, name: &str, game: Option<GameRef>) -> Option<semver::Version> {
        self.versions(name).into_keys().rfind(|v| match game {
            Some(g) => self.supported_games(name, v).contains(&g),
            None => true,
        })
    }
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

fn game_for(target: tango_patch::RomTarget) -> Option<GameRef> {
    crate::library::game::find_by_rom_info(&target.code, target.revision)
}

/// Read the installed packages and the cached index.
pub fn scan(patches_path: &Path) -> std::io::Result<Catalog> {
    let index = match std::fs::read_to_string(index_path(patches_path)) {
        Ok(raw) => match tango_patch::Index::parse(&raw) {
            Ok(index) => index,
            Err(e) => {
                log::warn!("cached patch index is unusable, ignoring it: {e}");
                tango_patch::Index::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => tango_patch::Index::default(),
        Err(e) => return Err(e),
    };

    // Newest version wins for the patch-level display metadata, so
    // collect per version first and fold afterwards.
    let mut versions: BTreeMap<String, BTreeMap<semver::Version, (Arc<Version>, tango_patch::Manifest)>> =
        BTreeMap::new();
    let read_dir = match std::fs::read_dir(patches_path) {
        Ok(read_dir) => read_dir,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Catalog {
                installed: PatchMap::new(),
                index,
            })
        }
        Err(e) => return Err(e),
    };

    for entry in read_dir {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(e) => {
                log::warn!("patch scan: {e}");
                continue;
            }
        };
        if path.extension().is_none_or(|e| e != tango_patch::EXTENSION) {
            continue;
        }
        match read_package(&path) {
            Ok((manifest, version)) => {
                versions
                    .entry(manifest.name.clone())
                    .or_default()
                    .insert(manifest.version.clone(), (Arc::new(version), manifest));
            }
            Err(e) => log::warn!("{}: {e}", path.display()),
        }
    }

    let installed = versions
        .into_iter()
        .filter_map(|(name, versions)| {
            // Newest version supplies the patch's display metadata.
            let (_, newest) = versions.values().next_back()?;
            let patch = Patch {
                title: newest.title.clone(),
                authors: newest.authors.clone(),
                license: newest.license.clone(),
                source: newest.source.clone(),
                versions: versions.into_iter().map(|(v, (version, _))| (v, version)).collect(),
            };
            Some((name, Arc::new(patch)))
        })
        .collect();

    Ok(Catalog { installed, index })
}

/// Read one package into a [`Version`] and its manifest.
fn read_package(path: &Path) -> anyhow::Result<(tango_patch::Manifest, Version)> {
    let mut package = Package::open(path)?;
    let manifest = package.manifest().clone();

    // A package can be named for a game this build doesn't support (no
    // gamesupport feature, or a rom we don't know); that just means
    // fewer supported games, not a bad package.
    let mut supported_games = HashSet::new();
    let mut save_templates: HashMap<GameRef, BTreeMap<String, Box<dyn tango_dataview::save::Save + Send + Sync>>> =
        HashMap::new();
    for target in package.targets().collect::<Vec<_>>() {
        let Some(game) = game_for(target) else {
            continue;
        };
        supported_games.insert(game);
        for template in package.save_templates(target).map(|s| s.to_owned()).collect::<Vec<_>>() {
            let raw = package.save_template(target, &template)?;
            match game.parse_save(&raw) {
                Ok(save) => {
                    save_templates.entry(game).or_default().insert(template, save);
                }
                Err(e) => log::warn!(
                    "{}: {target} template {template:?} is not a valid save: {e}",
                    path.display()
                ),
            }
        }
    }

    let version = Version {
        path: path.to_path_buf(),
        netplay: manifest.netplay.clone(),
        rom_overrides: manifest.rom_overrides.clone(),
        supported_games,
        save_templates,
        readme: package.readme()?,
    };
    Ok((manifest, version))
}

/// `Name <addr@example.com>` → `Name`, falling back to the address when
/// there's no display name and to the raw string when it doesn't parse.
///
/// Applied at render time rather than at scan time: an installed patch's
/// authors come from its package and a non-installed one's from the
/// index, and reducing only the former made a patch appear to change
/// authors the moment it finished downloading.
pub fn display_authors(authors: &[String]) -> Vec<String> {
    authors
        .iter()
        .map(|s| match mailparse::addrparse(s) {
            Ok(addrs) => addrs
                .iter()
                .filter_map(|addr| match addr {
                    mailparse::MailAddr::Single(info) => {
                        Some(info.display_name.clone().unwrap_or_else(|| info.addr.clone()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(", "),
            Err(_) => s.clone(),
        })
        .collect()
}

/// Read the BPS for `game` out of the installed package and apply it to
/// `rom`, returning the patched image.
pub fn apply_patch_from_disk(
    rom: &[u8],
    game: GameRef,
    patches_path: &Path,
    patch_name: &str,
    patch_version: &semver::Version,
) -> anyhow::Result<Vec<u8>> {
    // Names are validated on the way in (`tango_patch::validate_name`
    // forbids separators), so a name can't escape the directory.
    tango_patch::validate_name(patch_name).map_err(|e| anyhow::anyhow!("bad patch name: {e}"))?;

    let path = package_path(patches_path, patch_name, patch_version);
    let (rom_code, revision) = game.rom_code_and_revision();
    let target = tango_patch::RomTarget::new(*rom_code, revision);
    let raw = Package::open(&path)?.bps(target)?;
    Ok(bps::Patch::decode(&raw)?.apply(rom)?)
}

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
}

pub type Downloads = std::collections::HashMap<VersionKey, Download>;

const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

/// Fetch the repo index, writing it to disk when it changed.
///
/// Returns `true` if the on-disk copy was replaced (the caller should
/// rescan). Conditional on the stored ETag, so the common case — the
/// repo hasn't published anything since we last looked — is a 304 with
/// no body, which is what lets this poll on a timer without being rude.
pub async fn fetch_index(url: &str, patches_path: &Path) -> anyhow::Result<bool> {
    std::fs::create_dir_all(patches_path)?;
    let path = index_path(patches_path);
    let etag_path = patches_path.join("index.etag");

    let mut request = client()
        .get(format!(
            "{}/{}",
            url.trim_end_matches('/'),
            tango_patch::index::FILE_NAME
        ))
        .header("User-Agent", "tango")
        .timeout(TIMEOUT);
    // Only send the validator if we still have the body it describes.
    if path.is_file() {
        if let Ok(etag) = std::fs::read_to_string(&etag_path) {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag.trim());
        }
    }

    let response = request.send().await?.error_for_status()?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(false);
    }
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());
    let raw = response.text().await?;

    // Parse before writing: a corrupt or future-format index shouldn't
    // clobber the usable one we already have.
    let index = tango_patch::Index::parse(&raw)?;
    std::fs::write(&path, &raw)?;
    match etag {
        Some(etag) => std::fs::write(&etag_path, etag)?,
        None => {
            let _ = std::fs::remove_file(&etag_path);
        }
    }
    log::info!(
        "patch index: {} versions of {} patches",
        index.len(),
        index.patches.len()
    );
    Ok(true)
}

/// Download one patch version into the patches directory.
///
/// The package is verified against the index's hash and written through
/// a temporary file, so a failed or truncated download can't leave a
/// half-written package that the next scan would treat as installed.
pub async fn download(
    url: &str,
    patches_path: &Path,
    name: &str,
    version: &semver::Version,
    entry: &tango_patch::index::Entry,
    progress: impl Fn(Progress),
) -> anyhow::Result<PathBuf> {
    tango_patch::validate_name(name).map_err(|e| anyhow::anyhow!("bad patch name: {e}"))?;
    std::fs::create_dir_all(patches_path)?;

    let response = client()
        .get(format!("{}/{}", url.trim_end_matches('/'), entry.path))
        .header("User-Agent", "tango")
        .timeout(TIMEOUT)
        .send()
        .await?
        .error_for_status()?;

    let total = response.content_length().unwrap_or(entry.size);
    let temp = patches_path.join(format!(".{name}-{version}.part"));
    let mut file = tokio::fs::File::create(&temp).await?;
    let mut raw = Vec::with_capacity(entry.size as usize);
    let mut stream = response.bytes_stream();
    progress(Progress { downloaded: 0, total });
    while let Some(chunk) = tokio::time::timeout(TIMEOUT, stream.next()).await? {
        let chunk = chunk?;
        // The index fixes the exact byte count; a server sending more than
        // it promised is misbehaving, and buffering it unbounded would let
        // it exhaust memory and disk before the post-download hash check
        // ever runs. Stop as soon as it overruns.
        if raw.len() as u64 + chunk.len() as u64 > entry.size {
            drop(file);
            let _ = tokio::fs::remove_file(&temp).await;
            anyhow::bail!(
                "{name} {version}: download exceeds the {} bytes the index promised",
                entry.size
            );
        }
        file.write_all(&chunk).await?;
        raw.extend_from_slice(&chunk);
        progress(Progress {
            downloaded: raw.len() as u64,
            total,
        });
    }
    file.flush().await?;
    drop(file);

    let verified = verify(&raw, entry, name, version);
    if verified.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    verified?;

    let path = package_path(patches_path, name, version);
    std::fs::rename(&temp, &path)?;
    log::info!("installed {name} {version} ({} bytes)", raw.len());
    Ok(path)
}

/// A downloaded package must be what the index promised, and must be
/// what it says it is. The hash check is also what makes serving
/// packages from a mirror or a CDN cache safe.
fn verify(raw: &[u8], entry: &tango_patch::index::Entry, name: &str, version: &semver::Version) -> anyhow::Result<()> {
    entry
        .verify(raw)
        .map_err(|e| anyhow::anyhow!("{name} {version}: {e}"))?;
    let manifest = Package::read(std::io::Cursor::new(raw))?.manifest().clone();
    if manifest.name != name || &manifest.version != version {
        anyhow::bail!(
            "{name} {version}: package contains {} {} instead",
            manifest.name,
            manifest.version
        );
    }
    Ok(())
}

/// Delete an installed package. The next scan drops it from the catalog;
/// the index still lists it, so it can be reinstalled.
pub fn uninstall(patches_path: &Path, name: &str, version: &semver::Version) -> anyhow::Result<()> {
    tango_patch::validate_name(name).map_err(|e| anyhow::anyhow!("bad patch name: {e}"))?;
    std::fs::remove_file(package_path(patches_path, name, version))?;
    Ok(())
}

/// Background index refresher.
///
/// Under the old format this re-hashed every file in the patch directory
/// and downloaded whatever differed; now it re-fetches one small
/// conditional GET, so it costs a 304 in the steady state.
pub struct Autoupdater {
    patches_path: PathBuf,
    patch_repo: String,
    patches_scanner: Scanner,
    cancellation_token: Option<tokio_util::sync::CancellationToken>,
}

impl Autoupdater {
    /// Fast enough to notice a new patch within the hour, slow enough
    /// not to hammer the repo.
    const INTERVAL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

    pub fn new(patches_path: PathBuf, patch_repo: String, patches_scanner: Scanner) -> Self {
        Self {
            patches_path,
            patch_repo,
            patches_scanner,
            cancellation_token: None,
        }
    }

    /// Start the background loop. Idempotent.
    pub fn start(&mut self) {
        if self.cancellation_token.is_some() {
            return;
        }
        log::info!("starting patch index autoupdater (every {:?})", Self::INTERVAL);
        let token = tokio_util::sync::CancellationToken::new();
        let scanner = self.patches_scanner.clone();
        let patches_path = self.patches_path.clone();
        let patch_repo = if self.patch_repo.is_empty() {
            crate::config::DEFAULT_PATCH_REPO.to_string()
        } else {
            self.patch_repo.clone()
        };
        tokio::task::spawn({
            let token = token.clone();
            async move {
                loop {
                    match fetch_index(&patch_repo, &patches_path).await {
                        // Only a changed index is worth a rescan.
                        Ok(true) => {
                            let path = patches_path.clone();
                            let scanner = scanner.clone();
                            let _ = tokio::task::spawn_blocking(move || {
                                scanner.rescan(|| scan(&path).ok());
                            })
                            .await;
                        }
                        Ok(false) => {}
                        Err(e) => log::error!("patch index autoupdate failed: {e:?}"),
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(Self::INTERVAL) => {}
                        _ = token.cancelled() => break,
                    }
                }
                log::info!("stopped patch index autoupdater");
            }
        });
        self.cancellation_token = Some(token);
    }

    pub fn stop(&mut self) {
        if let Some(token) = self.cancellation_token.take() {
            token.cancel();
        }
    }
}

impl Drop for Autoupdater {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tango_patch::bundle::Builder;

    /// A scratch data directory that cleans up on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("tango-patch-scan-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn manifest(name: &str, version: &str, netplay: &str) -> tango_patch::Manifest {
        tango_patch::Manifest::parse(&format!(
            r#"
format = 1
name = "{name}"
version = "{version}"
title = "Test {name}"
authors = ["Someone <someone@example.com>"]
netplay = "{netplay}"
"#
        ))
        .unwrap()
    }

    /// Install a package patching BR6E_00 (BN6 Falzar), which every
    /// gamesupport-enabled build knows.
    fn install(root: &Path, name: &str, version: &str, netplay: &str) {
        let mut builder = Builder::new(manifest(name, version, netplay));
        builder.set_readme("# hello");
        builder.add_rom("BR6E_00".parse().unwrap(), b"not really a bps".to_vec());
        builder.write_file(root).unwrap();
    }

    /// An index offering `name` at `version` without it being installed.
    fn index_with(root: &Path, entries: &[(&str, &str, &str)]) {
        let mut index = tango_patch::Index::default();
        for (name, version, netplay) in entries {
            index.patches.entry((*name).to_owned()).or_default().insert(
                version.parse().unwrap(),
                tango_patch::index::Entry {
                    title: format!("Test {name}"),
                    authors: vec!["Someone <someone@example.com>".into()],
                    license: None,
                    source: None,
                    netplay: netplay.parse().unwrap(),
                    games: vec!["BR6E_00".parse().unwrap()],
                    path: format!("{name}/{name}-{version}.tangopatch"),
                    size: 1234,
                    sha256: "0".repeat(64),
                    readme: None,
                },
            );
        }
        std::fs::write(index_path(root), index.to_json().unwrap()).unwrap();
    }

    fn bn6_falzar() -> GameRef {
        crate::library::game::find_by_rom_info(b"BR6E", 0).expect("gamesupport-bn6 must be enabled for this test")
    }

    fn v(s: &str) -> semver::Version {
        s.parse().unwrap()
    }

    #[test]
    fn scans_installed_packages() {
        let dir = TempDir::new();
        install(&dir.0, "bn6_test", "1.0.0", "group:testing");

        let catalog = scan(&dir.0).unwrap();
        assert_eq!(catalog.installed.len(), 1);
        assert_eq!(catalog.title("bn6_test"), Some("Test bn6_test"));
        // mailparse reduces the address to its display name.
        // Kept raw; the display reduction happens at render time so
        // that installing a patch can't change how its authors read.
        assert_eq!(
            catalog.installed["bn6_test"].authors,
            vec!["Someone <someone@example.com>"]
        );
        assert_eq!(display_authors(&catalog.installed["bn6_test"].authors), vec!["Someone"]);

        let version = catalog.version("bn6_test", &v("1.0.0")).unwrap();
        assert_eq!(version.netplay, Compatibility::Group("testing".into()));
        assert_eq!(version.readme.as_deref(), Some("# hello"));
        assert!(version.supported_games.contains(&bn6_falzar()));
        assert!(catalog.is_installed("bn6_test", &v("1.0.0")));
    }

    #[test]
    fn an_empty_or_absent_patches_dir_is_not_an_error() {
        let dir = TempDir::new();
        assert!(scan(&dir.0).unwrap().installed.is_empty());
        std::fs::create_dir_all(&dir.0).unwrap();
        assert!(scan(&dir.0).unwrap().installed.is_empty());
    }

    #[test]
    fn the_catalog_merges_installed_and_offered() {
        let dir = TempDir::new();
        install(&dir.0, "bn6_test", "1.0.0", "group:testing");
        index_with(
            &dir.0,
            &[
                // A newer version of something we have, plus something
                // we don't have at all.
                ("bn6_test", "2.0.0", "group:testing"),
                ("bn6_other", "1.0.0", "vanilla"),
            ],
        );

        let catalog = scan(&dir.0).unwrap();
        assert_eq!(
            catalog.names().into_iter().collect::<Vec<_>>(),
            vec!["bn6_other", "bn6_test"]
        );

        let versions = catalog.versions("bn6_test");
        assert_eq!(versions.len(), 2);
        assert!(versions[&v("1.0.0")].is_installed());
        assert!(!versions[&v("2.0.0")].is_installed());
        assert_eq!(versions[&v("2.0.0")].size(), Some(1234));

        // Not installed, but still browsable and resolvable.
        assert_eq!(catalog.title("bn6_other"), Some("Test bn6_other"));
        assert!(catalog
            .supported_games("bn6_other", &v("1.0.0"))
            .contains(&bn6_falzar()));
        assert!(!catalog.is_installed("bn6_other", &v("1.0.0")));
    }

    #[test]
    fn tags_resolve_from_the_index_before_anything_is_downloaded() {
        let dir = TempDir::new();
        index_with(
            &dir.0,
            &[("bn6_cosmetic", "1.0.0", "vanilla"), ("bn6_mod", "1.0.0", "isolated")],
        );
        let catalog = scan(&dir.0).unwrap();
        let game = bn6_falzar();

        // This is what lets the lobby judge a peer's patch it has never
        // seen: a cosmetic patch plays the unpatched game...
        assert_eq!(
            catalog.tag(game, Some(("bn6_cosmetic", &v("1.0.0")))),
            catalog.tag(game, None)
        );
        // ... and a gameplay patch does not.
        assert_ne!(
            catalog.tag(game, Some(("bn6_mod", &v("1.0.0")))),
            catalog.tag(game, None)
        );
        // A patch nobody has heard of can't be vouched for either way.
        assert_eq!(catalog.tag(game, Some(("bn6_unknown", &v("1.0.0")))), None);
    }

    #[test]
    fn the_index_and_the_package_agree_on_author_form() {
        let dir = TempDir::new();
        install(&dir.0, "bn6_test", "1.0.0", "isolated");
        index_with(&dir.0, &[("bn6_test", "1.0.0", "isolated")]);
        let catalog = scan(&dir.0).unwrap();

        let installed = &catalog.installed["bn6_test"].authors;
        let indexed = &catalog.entry("bn6_test", &v("1.0.0")).unwrap().authors;
        // Both sides hold the same raw form, so the single reduction the
        // UI runs can't make a patch look like it changed hands the
        // moment it finishes downloading.
        assert_eq!(installed, indexed);
        assert_eq!(display_authors(installed), display_authors(indexed));
        assert_eq!(display_authors(installed), vec!["Someone"]);
    }

    #[test]
    fn an_installed_package_outranks_the_index() {
        // The installed package is the thing that would actually run, so
        // a stale or wrong index entry must not decide compatibility.
        let dir = TempDir::new();
        install(&dir.0, "bn6_test", "1.0.0", "isolated");
        index_with(&dir.0, &[("bn6_test", "1.0.0", "vanilla")]);

        let catalog = scan(&dir.0).unwrap();
        assert_eq!(
            catalog.compatibility("bn6_test", &v("1.0.0")),
            Some(&Compatibility::Isolated)
        );
        assert_ne!(
            catalog.tag(bn6_falzar(), Some(("bn6_test", &v("1.0.0")))),
            catalog.tag(bn6_falzar(), None)
        );
    }

    #[test]
    fn newest_version_spans_installed_and_offered() {
        let dir = TempDir::new();
        install(&dir.0, "bn6_test", "1.0.0", "isolated");
        index_with(&dir.0, &[("bn6_test", "1.2.0", "isolated")]);
        let catalog = scan(&dir.0).unwrap();
        assert_eq!(catalog.newest_version("bn6_test", None), Some(v("1.2.0")));
        assert_eq!(catalog.newest_version("bn6_test", Some(bn6_falzar())), Some(v("1.2.0")));
        assert_eq!(catalog.newest_version("nonexistent", None), None);
    }

    #[test]
    fn a_corrupt_package_is_skipped_not_fatal() {
        let dir = TempDir::new();
        install(&dir.0, "bn6_test", "1.0.0", "isolated");
        std::fs::write(
            dir.0.join(format!("junk-1.0.0.{}", tango_patch::EXTENSION)),
            b"not a zip",
        )
        .unwrap();
        // A file someone dropped in by hand shouldn't cost them the
        // rest of their patches.
        let catalog = scan(&dir.0).unwrap();
        assert_eq!(catalog.installed.len(), 1);
        assert!(catalog.is_installed("bn6_test", &v("1.0.0")));
    }

    /// The smallest HTTP server that can stand in for a patch repo:
    /// serves files under a root, with the ETag handling `fetch_index`
    /// depends on. Testing against a real socket is the point — URL
    /// joining, conditional requests, and streamed bodies are exactly
    /// what a hand-rolled fake would paper over.
    async fn serve(root: PathBuf) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let root = root.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = vec![0u8; 8192];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let path = request
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/")
                        .trim_start_matches('/')
                        .to_owned();
                    let if_none_match = request
                        .lines()
                        .find_map(|l| l.strip_prefix("if-none-match: ").or(l.strip_prefix("If-None-Match: ")))
                        .map(|s| s.trim().to_owned());

                    let response = match std::fs::read(root.join(&path)) {
                        Ok(body) => {
                            let etag = format!("\"{}\"", tango_patch::sha256_hex(&body));
                            if if_none_match.as_deref() == Some(etag.as_str()) {
                                b"HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\n\r\n".to_vec()
                            } else {
                                let mut out = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: {etag}\r\n\r\n",
                                    body.len()
                                )
                                .into_bytes();
                                out.extend_from_slice(&body);
                                out
                            }
                        }
                        Err(_) => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
                    };
                    let _ = socket.write_all(&response).await;
                    let _ = socket.flush().await;
                });
            }
        });
        format!("http://{addr}")
    }

    /// Build a repo of packages plus its index, the way the bundler's
    /// `pack` + `index` would.
    fn build_repo(root: &Path, packages: &[(&str, &str, &str)]) {
        for (name, version, netplay) in packages {
            let mut builder = Builder::new(manifest(name, version, netplay));
            builder.set_readme(format!("# {name} {version}"));
            builder.add_rom(
                "BR6E_00".parse().unwrap(),
                format!("bps for {name} {version}").into_bytes(),
            );
            builder.write_file(&root.join(name)).unwrap();
        }
        let index = tango_patch::Index::build(root, true).unwrap();
        std::fs::write(root.join(tango_patch::index::FILE_NAME), index.to_json().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn fetches_the_index_then_installs_only_what_is_asked_for() {
        let repo = TempDir::new();
        build_repo(
            &repo.0,
            &[("bn6_one", "1.0.0", "vanilla"), ("bn6_two", "2.0.0", "group:pair")],
        );
        let url = serve(repo.0.clone()).await;
        let data = TempDir::new();

        // First fetch pulls the index; the second is a 304.
        assert!(fetch_index(&url, &data.0).await.unwrap(), "first fetch should store");
        assert!(!fetch_index(&url, &data.0).await.unwrap(), "second fetch should 304");

        // The whole repo is browsable, with nothing downloaded.
        let catalog = scan(&data.0).unwrap();
        assert!(catalog.installed.is_empty());
        assert_eq!(
            catalog.names().into_iter().collect::<Vec<_>>(),
            vec!["bn6_one", "bn6_two"]
        );
        assert_eq!(catalog.title("bn6_two"), Some("Test bn6_two"));
        assert_eq!(
            std::fs::read_dir(&data.0)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|x| x == tango_patch::EXTENSION))
                .count(),
            0,
            "nothing should have been downloaded"
        );

        // Install one of them.
        let version = v("2.0.0");
        let entry = catalog.entry("bn6_two", &version).unwrap().clone();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_w = seen.clone();
        download(&url, &data.0, "bn6_two", &version, &entry, move |p| {
            seen_w.lock().unwrap().push(p.downloaded)
        })
        .await
        .unwrap();
        assert!(
            seen.lock().unwrap().last().copied() == Some(entry.size),
            "progress should end at the full size: {:?}",
            seen.lock().unwrap()
        );

        let catalog = scan(&data.0).unwrap();
        assert!(catalog.is_installed("bn6_two", &version));
        assert!(!catalog.is_installed("bn6_one", &v("1.0.0")), "only the asked-for one");
        assert_eq!(
            catalog.version("bn6_two", &version).unwrap().readme.as_deref(),
            Some("# bn6_two 2.0.0")
        );
        assert_eq!(
            catalog.compatibility("bn6_two", &version),
            Some(&Compatibility::Group("pair".into()))
        );
    }

    #[tokio::test]
    async fn a_package_that_is_not_what_the_index_promised_is_rejected() {
        let repo = TempDir::new();
        build_repo(&repo.0, &[("bn6_one", "1.0.0", "vanilla")]);
        let url = serve(repo.0.clone()).await;
        let data = TempDir::new();
        fetch_index(&url, &data.0).await.unwrap();

        let catalog = scan(&data.0).unwrap();
        let version = v("1.0.0");
        let mut entry = catalog.entry("bn6_one", &version).unwrap().clone();
        entry.sha256 = "0".repeat(64);

        let err = download(&url, &data.0, "bn6_one", &version, &entry, |_| {})
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("hash mismatch"), "{err}");
        // Nothing half-written left behind: not the package, and not
        // the temporary it streamed into.
        assert!(!package_path(&data.0, "bn6_one", &version).exists());
        // Only the index and its validator; no package, and no
        // half-written temporary.
        let mut left: Vec<String> = std::fs::read_dir(&data.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, vec!["index.etag", "index.json"], "no leftovers");
        assert!(scan(&data.0).unwrap().installed.is_empty());
    }

    #[tokio::test]
    async fn a_failed_fetch_leaves_the_cached_index_usable() {
        let repo = TempDir::new();
        build_repo(&repo.0, &[("bn6_one", "1.0.0", "vanilla")]);
        let url = serve(repo.0.clone()).await;
        let data = TempDir::new();
        fetch_index(&url, &data.0).await.unwrap();

        // Repo goes away (or serves garbage) — we keep browsing what we
        // last saw, which is what makes the app work offline.
        std::fs::write(repo.0.join(tango_patch::index::FILE_NAME), "not json at all").unwrap();
        assert!(fetch_index(&url, &data.0).await.is_err());
        let catalog = scan(&data.0).unwrap();
        assert_eq!(catalog.names().len(), 1);
        assert!(catalog.entry("bn6_one", &v("1.0.0")).is_some());
    }

    #[test]
    fn a_corrupt_index_leaves_the_installed_patches_alone() {
        let dir = TempDir::new();
        install(&dir.0, "bn6_test", "1.0.0", "isolated");
        std::fs::write(index_path(&dir.0), "{ this is not json").unwrap();
        let catalog = scan(&dir.0).unwrap();
        assert!(catalog.index.is_empty());
        assert!(catalog.is_installed("bn6_test", &v("1.0.0")));
    }
}

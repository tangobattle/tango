//! Patches, browser flavor: sync the same patch repo the desktop
//! client uses (`{repo}/index.json` manifest + per-file GETs, the
//! `tango_filesync` protocol) into OPFS `patches/`, scan the synced
//! tree's `info.toml`s, and apply `.bps` patches at boot. Metadata
//! shapes mirror the desktop's `library/patch.rs`; the on-disk layout
//! is identical (`patches/<name>/v<version>/<CODE>_<REV>.bps`), so the
//! netplay-compatibility rules line up exactly.
//!
//! The repo host must allow cross-origin GETs — a browser can't fetch
//! it otherwise (the desktop has no such constraint).

use std::collections::{BTreeMap, HashMap};

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::FileSystemDirectoryHandle;

use crate::library::{self, GameRef};
use crate::storage::{self, Storage};

/// The `tango_filesync` manifest, mirrored locally (the crate itself
/// is tokio-bound; the schema is tiny and stable).
pub type Entries = HashMap<String, Entry>;

#[derive(serde::Deserialize)]
#[serde(untagged)]
pub enum Entry {
    Directory(Entries),
    File(#[serde(with = "serde_hex::SerHex::<serde_hex::Strict>")] [u8; 32]),
}

/// `info.toml`, the desktop's schema.
#[derive(serde::Deserialize)]
struct Metadata {
    pub patch: PatchMetadata,
    pub versions: HashMap<String, VersionMetadata>,
}

#[derive(serde::Deserialize)]
struct PatchMetadata {
    pub title: String,
}

#[derive(serde::Deserialize, Default)]
struct VersionMetadata {
    pub netplay_compatibility: String,
}

/// One synced patch, scanned from OPFS.
#[derive(Clone, PartialEq)]
pub struct Patch {
    pub name: String,
    pub title: String,
    /// version → (netplay compatibility tag, games with a .bps).
    pub versions: BTreeMap<semver::Version, PatchVersion>,
}

#[derive(Clone, PartialEq)]
pub struct PatchVersion {
    pub netplay_compatibility: String,
    pub supported: Vec<GameRef>,
}

async fn fetch_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("no window"))?;
    let resp = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| anyhow::anyhow!("fetch {url}: {e:?}"))?;
    let resp: web_sys::Response = resp
        .dyn_into()
        .map_err(|_| anyhow::anyhow!("fetch returned a non-Response"))?;
    if !resp.ok() {
        anyhow::bail!("fetch {url}: HTTP {}", resp.status());
    }
    let buf = JsFuture::from(
        resp.array_buffer()
            .map_err(|e| anyhow::anyhow!("array_buffer: {e:?}"))?,
    )
    .await
    .map_err(|e| anyhow::anyhow!("read body: {e:?}"))?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

/// Resolve (creating) a nested directory under `root`.
async fn dir_at(
    root: &FileSystemDirectoryHandle,
    components: &[&str],
) -> anyhow::Result<FileSystemDirectoryHandle> {
    let mut cur = root.clone();
    for c in components {
        let opts = web_sys::FileSystemGetDirectoryOptions::new();
        opts.set_create(true);
        cur = JsFuture::from(cur.get_directory_handle_with_options(c, &opts))
            .await
            .map_err(|e| anyhow::anyhow!("subdir {c}: {e:?}"))?
            .dyn_into()
            .map_err(|_| anyhow::anyhow!("expected a directory"))?;
    }
    Ok(cur)
}

/// One file of the sync: skip when the local copy's SHA-256 matches.
async fn sync_file(
    repo: &str,
    root: &FileSystemDirectoryHandle,
    path: &[&str],
    hash: &[u8; 32],
) -> anyhow::Result<bool> {
    use sha2::Digest;
    let (dir_parts, file_name) = path.split_at(path.len() - 1);
    let dir = dir_at(root, dir_parts).await?;
    if let Ok(Some(existing)) = storage::read(&dir, file_name[0]).await {
        let digest: [u8; 32] = sha2::Sha256::digest(&existing).into();
        if digest == *hash {
            return Ok(false);
        }
    }
    let url = format!("{repo}/{}", path.join("/"));
    let bytes = fetch_bytes(&url).await?;
    storage::write(&dir, file_name[0], &bytes)
        .await
        .map_err(|e| anyhow::anyhow!("write {}: {e}", path.join("/")))?;
    Ok(true)
}

fn walk<'a>(
    entries: &'a Entries,
    prefix: &mut Vec<&'a str>,
    out: &mut Vec<(Vec<&'a str>, &'a [u8; 32])>,
) {
    for (name, entry) in entries {
        prefix.push(name.as_str());
        match entry {
            Entry::Directory(children) => walk(children, prefix, out),
            Entry::File(hash) => out.push((prefix.clone(), hash)),
        }
        prefix.pop();
    }
}

/// Sync the repo into OPFS `patches/`. Returns how many files were
/// fetched (0 = already current).
pub async fn sync(storage: &Storage, repo: &str) -> anyhow::Result<usize> {
    let repo = repo.trim_end_matches('/');
    let index = fetch_bytes(&format!("{repo}/index.json")).await?;
    let entries: Entries = serde_json::from_slice(&index)
        .map_err(|e| anyhow::anyhow!("bad index.json: {e}"))?;

    let root = storage.patches().clone();
    let mut files = Vec::new();
    let mut prefix = Vec::new();
    walk(&entries, &mut prefix, &mut files);

    let mut fetched = 0;
    for (path, hash) in files {
        if sync_file(repo, &root, &path, hash).await? {
            fetched += 1;
        }
    }
    log::info!("patch sync: {fetched} file(s) updated");
    Ok(fetched)
}

/// Scan OPFS `patches/` into the patch list, the web analog of the
/// desktop's `patch::scan`: each subdirectory's `info.toml` names the
/// versions; each `v<version>/` directory's `.bps` file names say
/// which games it supports.
pub async fn scan(storage: &Storage) -> Vec<Patch> {
    let root = storage.patches().clone();
    let Ok(dirs) = storage::list_dirs(&root).await else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, dir) in dirs {
        let Ok(Some(raw)) = storage::read(&dir, "info.toml").await else {
            continue;
        };
        let Ok(info) = toml::from_str::<Metadata>(&String::from_utf8_lossy(&raw)) else {
            log::warn!("patch {name}: bad info.toml");
            continue;
        };
        let mut versions = BTreeMap::new();
        for (v, ver) in info.versions {
            let Ok(sv) = semver::Version::parse(&v) else {
                continue;
            };
            // Which games this version ships a .bps for.
            let mut supported = Vec::new();
            if let Ok(vdir) = dir_at(&dir, &[&format!("v{sv}")]).await {
                if let Ok(files) = storage::list_files(&vdir).await {
                    for (file, _) in files {
                        let Some(stem) = file.strip_suffix(".bps") else {
                            continue;
                        };
                        let Some((code, rev)) = stem.rsplit_once('_') else {
                            continue;
                        };
                        let (Ok(code), Ok(rev)) =
                            (<[u8; 4]>::try_from(code.as_bytes()), rev.parse::<u8>())
                        else {
                            continue;
                        };
                        if let Some(game) = library::GAMES
                            .iter()
                            .copied()
                            .find(|g| g.rom_code_and_revision() == (&code, rev))
                        {
                            supported.push(game);
                        }
                    }
                }
            }
            versions.insert(
                sv,
                PatchVersion {
                    netplay_compatibility: ver.netplay_compatibility,
                    supported,
                },
            );
        }
        if versions.is_empty() {
            continue;
        }
        out.push(Patch {
            name,
            title: info.patch.title,
            versions,
        });
    }
    out.sort_by(|a, b| a.title.cmp(&b.title));
    out
}

/// A patch version's `rom_overrides` (charset + name/description
/// overrides for the save view's assets), read fresh from the synced
/// `info.toml`. The scanned [`Patch`] list deliberately doesn't carry
/// these — they're only needed when a selection actually loads, and the
/// parse is cheap at that point. `Default` when the patch or version is
/// missing or the file doesn't parse.
pub async fn version_overrides(
    storage: &Storage,
    name: &str,
    version: &semver::Version,
) -> crate::rom_overrides::Overrides {
    #[derive(serde::Deserialize)]
    struct VersionOverrides {
        #[serde(default)]
        rom_overrides: crate::rom_overrides::Overrides,
    }
    #[derive(serde::Deserialize)]
    struct MetadataOverrides {
        versions: HashMap<String, VersionOverrides>,
    }
    if name.contains('/') || name.contains('\\') {
        return Default::default();
    }
    let Ok(dir) = dir_at(storage.patches(), &[name]).await else {
        return Default::default();
    };
    let Ok(Some(raw)) = storage::read(&dir, "info.toml").await else {
        return Default::default();
    };
    let Ok(info) = toml::from_str::<MetadataOverrides>(&String::from_utf8_lossy(&raw)) else {
        return Default::default();
    };
    info.versions
        .into_iter()
        .find(|(v, _)| semver::Version::parse(v).is_ok_and(|v| v == *version))
        .map(|(_, v)| v.rom_overrides)
        .unwrap_or_default()
}

/// Read and apply `patches/<name>/v<version>/<CODE>_<REV>.bps` on top
/// of `rom` — byte-for-byte the desktop's `apply_patch_from_disk`.
pub async fn apply(
    storage: &Storage,
    rom: &[u8],
    game: GameRef,
    name: &str,
    version: &semver::Version,
) -> anyhow::Result<Vec<u8>> {
    if name.contains('/') || name.contains('\\') {
        anyhow::bail!("attempted path traversal in patch name");
    }
    let (code, revision) = game.rom_code_and_revision();
    let dir = dir_at(storage.patches(), &[name, &format!("v{version}")]).await?;
    let file = format!("{}_{revision:02}.bps", std::str::from_utf8(code).unwrap());
    let raw = storage::read(&dir, &file)
        .await
        .map_err(|e| anyhow::anyhow!("read {file}: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("{name} v{version} has no patch for this game"))?;
    Ok(bps::Patch::decode(&raw)?.apply(rom)?)
}

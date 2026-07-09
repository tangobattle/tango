//! BPS patch application + disk scanner + repo sync, copied from
//! `tango/src/patch.rs` and slimmed: no rom_overrides (save-view
//! rendering only) and no Scanner wrapper (tango-ng rescans via its
//! event channel instead).

use std::collections::{BTreeMap, HashMap, HashSet};

use futures::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::rom::GameRef;

#[derive(serde::Deserialize, Debug)]
struct Metadata {
    pub patch: PatchMetadata,
    pub versions: HashMap<String, VersionMetadata>,
}

#[derive(serde::Deserialize, Debug)]
struct PatchMetadata {
    pub title: String,
    #[serde(default)]
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub source: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default)]
struct VersionMetadata {
    pub netplay_compatibility: String,
}

pub struct Version {
    pub netplay_compatibility: String,
    pub supported_games: HashSet<GameRef>,
    /// Per-game save templates the patch ships. Keyed by template name
    /// (empty string = the default template).
    #[allow(dead_code)] // used when new-save creation lands
    pub save_templates: HashMap<GameRef, BTreeMap<String, Box<dyn tango_dataview::save::Save + Send + Sync>>>,
}

pub struct Patch {
    #[allow(dead_code)] // used when the open-folder action lands
    pub path: std::path::PathBuf,
    pub title: String,
    /// Author display strings — parsed via `mailparse` and reduced to a
    /// display name (or the bare address if no display name), like
    /// tango's scanner.
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub source: Option<String>,
    pub readme: Option<String>,
    pub versions: BTreeMap<semver::Version, std::sync::Arc<Version>>,
}

pub type PatchMap = BTreeMap<String, std::sync::Arc<Patch>>;

/// The background autoupdater's cadence — same as tango's
/// `patch::Autoupdater::INTERVAL`: fast enough to pick up new patches
/// within the hour, slow enough not to hammer the repo.
pub const AUTOUPDATE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Fetch the patch repo's index.json and download any missing /
/// updated files via tango_filesync (ported from `tango/src/patch.rs`).
pub async fn update(url: String, root: std::path::PathBuf) -> anyhow::Result<()> {
    std::fs::create_dir_all(&root)?;

    let client = reqwest::Client::new();
    let entries = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        Ok::<_, anyhow::Error>(
            client
                .get(format!("{}/index.json", url))
                .header("User-Agent", "tango")
                .send()
                .await?
                .json::<tango_filesync::Entries>()
                .await?,
        )
    })
    .await??;

    tango_filesync::sync(
        &root,
        &entries,
        {
            let url = url.clone();
            let root = root.clone();
            // One shared client across all downloads — reqwest::Client is an
            // Arc'd pool, so clones reuse connections + TLS sessions.
            let client = client.clone();
            move |path| {
                let url = url.clone();
                let root = root.clone();
                let client = client.clone();
                Box::pin(async move {
                    let mut output_file = tokio::fs::File::create(&root.join(path)).await?;
                    let mut stream = tokio::time::timeout(
                        std::time::Duration::from_secs(30),
                        client
                            .get(format!(
                                "{}/{}",
                                url,
                                path.components()
                                    .map(|v| v.as_os_str().to_string_lossy())
                                    .collect::<Vec<_>>()
                                    .join("/")
                            ))
                            .header("User-Agent", "tango")
                            .send(),
                    )
                    .await?
                    .map_err(std::io::Error::other)?
                    .bytes_stream();
                    while let Some(chunk) =
                        tokio::time::timeout(std::time::Duration::from_secs(30), stream.next()).await?
                    {
                        let chunk = chunk.map_err(std::io::Error::other)?;
                        output_file.write_all(&chunk).await?;
                    }
                    log::info!("filesynced: {}", path.display());
                    Ok(())
                })
            }
        },
        4,
    )
    .await?;
    Ok(())
}

/// Walk `<patches>/<name>/info.toml` + `v<version>/` dirs and build the
/// patch map. Version support is derived from which `CODE_rr.bps` files
/// exist; template saves (`CODE_rr[_name].sav`) are parsed eagerly.
pub fn scan(path: &std::path::Path) -> PatchMap {
    let mut patches = BTreeMap::new();

    let read_dir = match std::fs::read_dir(path) {
        Ok(r) => r,
        Err(_) => return patches,
    };

    let patch_filename_re = regex::Regex::new(r"^(\S{4})_(\d{2})\.bps$").unwrap();
    let save_template_filename_re = regex::Regex::new(r"^(\S{4})_(\d{2})(?:|_(.+?))\.sav$").unwrap();

    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                log::warn!("patch scan: {e:?}");
                continue;
            }
        };
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        if entry.file_type().ok().map(|ft| !ft.is_dir()).unwrap_or(false) {
            continue;
        }

        let info_path = entry.path().join("info.toml");
        let raw = match std::fs::read_to_string(&info_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let info = match toml::from_str::<Metadata>(&raw) {
            Ok(i) => i,
            Err(e) => {
                log::warn!("{}: {e}", info_path.display());
                continue;
            }
        };

        // The first directory entry whose name is "README",
        // case-insensitive (tango matches the bare name, no extension).
        let readme = std::fs::read_dir(entry.path())
            .ok()
            .and_then(|mut it| {
                it.find(|p| {
                    p.as_ref()
                        .map(|e| e.file_name().eq_ignore_ascii_case("readme"))
                        .unwrap_or(false)
                })
                .and_then(|r| r.ok())
            })
            .and_then(|e| std::fs::read(e.path()).ok())
            .map(|buf| String::from_utf8_lossy(&buf).to_string());

        let mut versions = BTreeMap::new();
        for (v, ver) in info.versions {
            let sv = match semver::Version::parse(&v) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("{}: bad version {v}: {e}", entry.path().display());
                    continue;
                }
            };
            if sv.to_string() != v {
                log::warn!("{}: semver did not round trip: {v}", entry.path().display());
                continue;
            }

            let vdir = entry.path().join(format!("v{sv}"));
            let vread = match std::fs::read_dir(&vdir) {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("{}: {e}", vdir.display());
                    continue;
                }
            };

            let mut supported_games: HashSet<GameRef> = HashSet::new();
            let mut save_templates: HashMap<
                GameRef,
                BTreeMap<String, Box<dyn tango_dataview::save::Save + Send + Sync>>,
            > = HashMap::new();
            for f in vread {
                let Ok(f) = f else { continue };
                let Some(filename) = f.file_name().into_string().ok() else {
                    continue;
                };

                if let Some(captures) = patch_filename_re.captures(&filename) {
                    let rom_code: [u8; 4] = match captures[1].as_bytes().try_into() {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    let Ok(revision) = captures[2].parse::<u8>() else {
                        continue;
                    };
                    let Some(game) = crate::game::find_by_rom_info(&rom_code, revision) else {
                        continue;
                    };
                    supported_games.insert(game);
                } else if let Some(captures) = save_template_filename_re.captures(&filename) {
                    let rom_code: [u8; 4] = match captures[1].as_bytes().try_into() {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    let Ok(revision) = captures[2].parse::<u8>() else {
                        continue;
                    };
                    let Some(game) = crate::game::find_by_rom_info(&rom_code, revision) else {
                        continue;
                    };
                    let template_name = captures.get(3).map(|m| m.as_str().to_string()).unwrap_or_default();
                    let raw = match std::fs::read(f.path()) {
                        Ok(r) => r,
                        Err(e) => {
                            log::warn!("{}: {e}", f.path().display());
                            continue;
                        }
                    };
                    match game.parse_save(&raw) {
                        Ok(save) => {
                            save_templates.entry(game).or_default().insert(template_name, save);
                        }
                        Err(e) => log::warn!("{}: not a valid template save: {e}", f.path().display()),
                    }
                }
            }

            versions.insert(
                sv,
                std::sync::Arc::new(Version {
                    netplay_compatibility: ver.netplay_compatibility,
                    supported_games,
                    save_templates,
                }),
            );
        }

        // Reduce "Display Name <addr>" mailbox strings to the display
        // name (or the address); unparseable strings pass through.
        let authors = info
            .patch
            .authors
            .into_iter()
            .map(|s| match mailparse::addrparse(&s) {
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
                Err(_) => s,
            })
            .collect();

        patches.insert(
            name,
            std::sync::Arc::new(Patch {
                path: entry.path(),
                title: info.patch.title,
                authors,
                license: info.patch.license,
                source: info.patch.source,
                readme,
                versions,
            }),
        );
    }

    patches
}

/// Read and decode the .bps for `game` from `patches_path/<patch_name>/v<version>/`,
/// then apply it on top of the supplied ROM. Returns the patched ROM bytes.
pub fn apply_patch_from_disk(
    rom: &[u8],
    game: GameRef,
    patches_path: &std::path::Path,
    patch_name: &str,
    patch_version: &semver::Version,
) -> anyhow::Result<Vec<u8>> {
    let patch_name_path = std::path::Path::new(patch_name);
    if patch_name_path.components().count() > 1 {
        anyhow::bail!("attempted path traversal in patch name");
    }

    let (rom_code, revision) = game.rom_code_and_revision();
    let bps_path = patches_path
        .join(patch_name_path)
        .join(format!("v{patch_version}"))
        .join(format!(
            "{}_{:02}.bps",
            std::str::from_utf8(rom_code).unwrap(),
            revision
        ));
    let raw = std::fs::read(&bps_path)?;
    Ok(bps::Patch::decode(&raw)?.apply(rom)?)
}

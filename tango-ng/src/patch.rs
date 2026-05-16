//! Patch scanner. Slim port of `tango/src/patch.rs` — no autoupdate / HTTP
//! sync / patch application yet. Just reads `info.toml`, README, and the
//! per-version directory to discover which games each version supports.

use crate::rom::GameRef;
use crate::rom_overrides::Overrides;
use crate::scanner;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Deserialize, Debug)]
struct Metadata {
    pub patch: PatchMetadata,
    pub versions: HashMap<String, VersionMetadata>,
}

#[derive(Deserialize, Debug)]
struct PatchMetadata {
    pub title: String,
    #[serde(default)]
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub source: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
struct VersionMetadata {
    #[serde(default)]
    pub rom_overrides: Overrides,
    pub netplay_compatibility: String,
}

#[derive(Clone)]
pub struct Version {
    pub rom_overrides: Overrides,
    pub netplay_compatibility: String,
    pub supported_games: HashSet<GameRef>,
    /// Per-game save templates the patch ships. Keyed by template name
    /// (empty string = the default template); values are owned Save
    /// trait objects ready to be serialized via `as_sram_dump`.
    pub save_templates:
        std::collections::HashMap<GameRef, BTreeMap<String, Box<dyn tango_dataview::save::Save + Send + Sync>>>,
}

pub struct Patch {
    pub path: PathBuf,
    pub title: String,
    /// Author display strings — parsed via `mailparse` and reduced to a
    /// display name (or the address if no display name).
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub source: Option<String>,
    pub readme: Option<String>,
    pub versions: BTreeMap<semver::Version, Arc<Version>>,
}

pub type PatchMap = BTreeMap<String, Arc<Patch>>;
pub type Scanner = scanner::Scanner<PatchMap>;

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

pub fn scan(path: &std::path::Path) -> std::io::Result<PatchMap> {
    let mut patches = BTreeMap::new();

    let read_dir = match std::fs::read_dir(path) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(patches),
        Err(e) => return Err(e),
    };

    let patch_filename_re = regex::Regex::new(r"^(\S{4})_(\d{2})\.bps$").unwrap();
    let save_template_filename_re =
        regex::Regex::new(r"^(\S{4})_(\d{2})(?:|_(.+?))\.sav$").unwrap();

    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                log::warn!("patch scan: {e:?}");
                continue;
            }
        };
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else { continue };
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
            let mut save_templates: std::collections::HashMap<
                GameRef,
                BTreeMap<String, Box<dyn tango_dataview::save::Save + Send + Sync>>,
            > = std::collections::HashMap::new();
            for f in vread {
                let Ok(f) = f else { continue };
                let Some(filename) = f.file_name().into_string().ok() else { continue };

                if let Some(captures) = patch_filename_re.captures(&filename) {
                    let rom_code: [u8; 4] = match captures[1].as_bytes().try_into() {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    let Ok(revision) = captures[2].parse::<u8>() else { continue };
                    let Some(game) = tango_gamedb::find_by_rom_info(&rom_code, revision) else { continue };
                    supported_games.insert(game);
                } else if let Some(captures) = save_template_filename_re.captures(&filename) {
                    let rom_code: [u8; 4] = match captures[1].as_bytes().try_into() {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    let Ok(revision) = captures[2].parse::<u8>() else { continue };
                    let Some(game) = tango_gamedb::find_by_rom_info(&rom_code, revision) else { continue };
                    let template_name =
                        captures.get(3).map(|m| m.as_str().to_string()).unwrap_or_default();
                    let raw = match std::fs::read(f.path()) {
                        Ok(r) => r,
                        Err(e) => {
                            log::warn!("{}: {e}", f.path().display());
                            continue;
                        }
                    };
                    match game.parse_save(&raw) {
                        Ok(save) => {
                            save_templates
                                .entry(game)
                                .or_default()
                                .insert(template_name, save);
                        }
                        Err(e) => log::warn!("{}: not a valid template save: {e}", f.path().display()),
                    }
                }
            }

            versions.insert(
                sv,
                Arc::new(Version {
                    rom_overrides: ver.rom_overrides,
                    netplay_compatibility: ver.netplay_compatibility,
                    supported_games,
                    save_templates,
                }),
            );
        }

        let authors = info
            .patch
            .authors
            .into_iter()
            .map(|s| match mailparse::addrparse(&s) {
                Ok(addrs) => addrs
                    .iter()
                    .filter_map(|addr| match addr {
                        mailparse::MailAddr::Single(info) => Some(
                            info.display_name
                                .clone()
                                .unwrap_or_else(|| info.addr.clone()),
                        ),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
                Err(_) => s,
            })
            .collect();

        patches.insert(
            name,
            Arc::new(Patch {
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

    Ok(patches)
}

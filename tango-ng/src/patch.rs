//! BPS patch application + disk scanner, copied from `tango/src/patch.rs`
//! and slimmed: no rom_overrides (save-view rendering only), no authors /
//! readme / license parsing, and no HTTP repo sync yet — those come over
//! with the Patches tab.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::rom::GameRef;

#[derive(serde::Deserialize, Debug)]
struct Metadata {
    pub patch: PatchMetadata,
    pub versions: HashMap<String, VersionMetadata>,
}

#[derive(serde::Deserialize, Debug)]
struct PatchMetadata {
    pub title: String,
}

#[derive(serde::Deserialize, Debug, Default)]
struct VersionMetadata {
    #[allow(dead_code)] // used by netplay matchup compatibility later
    pub netplay_compatibility: String,
}

pub struct Version {
    #[allow(dead_code)] // matchup compatibility, used when netplay lands
    pub netplay_compatibility: String,
    pub supported_games: HashSet<GameRef>,
    /// Per-game save templates the patch ships. Keyed by template name
    /// (empty string = the default template).
    #[allow(dead_code)] // used when new-save creation lands
    pub save_templates: HashMap<GameRef, BTreeMap<String, Box<dyn tango_dataview::save::Save + Send + Sync>>>,
}

pub struct Patch {
    #[allow(dead_code)] // used when the Patches tab lands
    pub path: std::path::PathBuf,
    #[allow(dead_code)] // used when the Patches tab lands
    pub title: String,
    pub versions: BTreeMap<semver::Version, std::sync::Arc<Version>>,
}

pub type PatchMap = BTreeMap<String, std::sync::Arc<Patch>>;

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

        patches.insert(
            name,
            std::sync::Arc::new(Patch {
                path: entry.path(),
                title: info.patch.title,
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

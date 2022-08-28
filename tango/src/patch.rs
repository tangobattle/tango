use crate::games;

#[derive(serde::Deserialize)]
struct Metadata {
    pub patch: PatchMetadata,
    pub versions: std::collections::HashMap<String, VersionMetadata>,
}

#[derive(serde::Deserialize)]
struct PatchMetadata {
    pub title: String,
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub source: Option<String>,
}

#[derive(serde::Deserialize)]
struct VersionMetadata {
    pub saveedit_overrides: Option<toml::value::Table>,
    pub netplay_compatibility: String,
}

#[derive(Debug)]
pub struct Version {
    pub saveedit_overrides: Option<toml::value::Table>,
    pub netplay_compatibility: String,
    pub supported_games: std::collections::HashSet<&'static (dyn games::Game + Send + Sync)>,
}

#[derive(Debug)]
pub struct Patch {
    pub title: String,
    pub authors: Vec<mailparse::SingleInfo>,
    pub license: Option<spdx::LicenseId>,
    pub source: Option<String>,
    pub readme: Option<String>,
    pub versions: std::collections::HashMap<semver::Version, Version>,
}

lazy_static! {
    static ref PATCH_FILENAME_REGEX: regex::Regex =
        regex::Regex::new(r"(\S{4})_(\d{2}).bps").unwrap();
}

pub fn scan(
    path: &std::path::Path,
) -> Result<std::collections::BTreeMap<std::ffi::OsString, Patch>, std::io::Error> {
    let mut patches = std::collections::BTreeMap::new();
    for entry in std::fs::read_dir(path)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                log::error!("failed to read dir: {:?}", e);
                continue;
            }
        };

        if entry
            .file_type()
            .ok()
            .map(|ft| !ft.is_dir())
            .unwrap_or(false)
        {
            continue;
        }

        let raw_info = match std::fs::read(entry.path().join("info.toml")) {
            Ok(buf) => buf,
            Err(_) => {
                continue;
            }
        };

        let info = match toml::from_slice::<Metadata>(&raw_info) {
            Ok(info) => info,
            Err(e) => {
                log::warn!("{}: {}", entry.path().display(), e);
                continue;
            }
        };

        let readme = std::fs::read_dir(entry.path())
            .ok()
            .and_then(|mut it| {
                it.find(|p| {
                    p.as_ref()
                        .map(|entry| entry.file_name().to_ascii_lowercase() == "readme")
                        .unwrap_or(false)
                })
                .and_then(|r| r.ok())
            })
            .and_then(|entry| std::fs::read(entry.path()).ok())
            .map(|buf| String::from_utf8_lossy(&buf).to_string());

        let mut versions = std::collections::HashMap::new();
        for (v, version) in info.versions.into_iter() {
            let sv = match semver::Version::parse(&v) {
                Ok(sv) => sv,
                Err(e) => {
                    log::warn!("{}: {}", entry.path().display(), e);
                    continue;
                }
            };

            let read_version_dir =
                match std::fs::read_dir(entry.path().join(format!("v{}", sv.to_string()))) {
                    Ok(read_version_dir) => read_version_dir,
                    Err(e) => {
                        log::warn!("{}: {}", entry.path().display(), e);
                        continue;
                    }
                };

            let mut supported_games = std::collections::HashSet::new();

            for entry in read_version_dir {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) => {
                        log::error!("failed to read dir: {:?}", e);
                        continue;
                    }
                };

                // Try parse file name.
                let filename = match entry.file_name().into_string() {
                    Ok(filename) => filename,
                    Err(e) => {
                        log::error!("failed to read dir: {:?}", e);
                        continue;
                    }
                };
                let captures = if let Some(captures) = PATCH_FILENAME_REGEX.captures(&filename) {
                    captures
                } else {
                    continue;
                };

                let rom_id = captures.get(1).unwrap().as_str().to_string();
                let revision = captures.get(2).unwrap().as_str().parse::<u8>().unwrap();

                let game = if let Some(game) =
                    games::find_by_rom_info(rom_id.as_bytes().try_into().unwrap(), revision)
                {
                    game
                } else {
                    continue;
                };

                supported_games.insert(game);
            }

            versions.insert(
                sv,
                Version {
                    saveedit_overrides: version.saveedit_overrides,
                    netplay_compatibility: version.netplay_compatibility,
                    supported_games,
                },
            );
        }

        patches.insert(
            entry.file_name(),
            Patch {
                title: info.patch.title,
                authors: info
                    .patch
                    .authors
                    .into_iter()
                    .flat_map(|author| match mailparse::addrparse(&author) {
                        Ok(addrs) => addrs
                            .into_inner()
                            .into_iter()
                            .flat_map(|addr| match addr {
                                mailparse::MailAddr::Group(group) => group.addrs,
                                mailparse::MailAddr::Single(single) => vec![single],
                            })
                            .collect(),
                        Err(_) => vec![mailparse::SingleInfo {
                            display_name: Some(author),
                            addr: "".to_string(),
                        }],
                    })
                    .collect(),
                license: info
                    .patch
                    .license
                    .and_then(|license| spdx::license_id(&license)),
                readme,
                source: info.patch.source,
                versions,
            },
        );
    }
    Ok(patches)
}

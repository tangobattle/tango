pub mod bps;

use serde::Deserialize;

use crate::{config, game, scanner};

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

fn deserialize_option_language_identifier<'de, D>(
    deserializer: D,
) -> Result<Option<unic_langid::LanguageIdentifier>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    if let Some(buf) = Option::<String>::deserialize(deserializer)? {
        buf.parse()
            .map(|v| Some(v))
            .map_err(serde::de::Error::custom)
    } else {
        Ok(None)
    }
}

#[derive(serde::Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct SaveeditOverrides {
    #[serde(deserialize_with = "deserialize_option_language_identifier")]
    pub language: Option<unic_langid::LanguageIdentifier>,
    pub charset: Option<Vec<String>>,
}

#[derive(serde::Deserialize)]
struct VersionMetadata {
    #[serde(default)]
    pub saveedit_overrides: SaveeditOverrides,
    pub netplay_compatibility: String,
}

#[derive(Debug, Clone)]
pub struct Version {
    pub saveedit_overrides: SaveeditOverrides,
    pub netplay_compatibility: String,
    pub supported_games: std::collections::HashSet<&'static (dyn game::Game + Send + Sync)>,
}

#[derive(Debug)]
pub struct Patch {
    pub path: std::path::PathBuf,
    pub title: String,
    pub authors: Vec<mailparse::SingleInfo>,
    pub license: Option<String>,
    pub source: Option<String>,
    pub readme: Option<String>,
    pub versions: std::collections::HashMap<semver::Version, Version>,
}

lazy_static! {
    static ref PATCH_FILENAME_REGEX: regex::Regex =
        regex::Regex::new(r"^(\S{4})_(\d{2}).bps$").unwrap();
}

pub fn update(
    url: &String,
    path: &std::path::Path,
) -> Result<std::collections::BTreeMap<String, Patch>, anyhow::Error> {
    let repo = git2::Repository::init(path)?;
    let _ = repo.remote_delete("origin");
    let mut remote = repo.remote("origin", url)?;
    remote.fetch(&["main"], None, None)?;
    let treeish = repo
        .find_reference("refs/remotes/origin/main")?
        .peel_to_tree()?;
    repo.checkout_tree(
        treeish.as_object(),
        Some(git2::build::CheckoutBuilder::new().force()),
    )?;
    Ok(scan(path)?)
}

pub fn scan(
    path: &std::path::Path,
) -> Result<std::collections::BTreeMap<String, Patch>, std::io::Error> {
    let mut patches = std::collections::BTreeMap::new();
    for entry in std::fs::read_dir(path)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                log::error!("failed to read dir: {:?}", e);
                continue;
            }
        };

        let name = if let Some(name) = entry.file_name().to_str().map(|s| s.to_owned()) {
            name
        } else {
            continue;
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

            if sv.to_string() != v {
                log::warn!("{}: semver did not round trip", entry.path().display());
                continue;
            }

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
                    game::find_by_rom_info(rom_id.as_bytes().try_into().unwrap(), revision)
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
            name.to_string(),
            Patch {
                path: entry.path(),
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
                license: info.patch.license,
                readme,
                source: info.patch.source,
                versions,
            },
        );
    }
    Ok(patches)
}

pub type Scanner = scanner::Scanner<std::collections::BTreeMap<String, Patch>>;

pub struct Autoupdater {
    config: std::sync::Arc<parking_lot::RwLock<config::Config>>,
    patches_scanner: Scanner,
    cancellation_token: Option<tokio_util::sync::CancellationToken>,
}

impl Autoupdater {
    pub fn new(
        config: std::sync::Arc<parking_lot::RwLock<config::Config>>,
        patches_scanner: Scanner,
    ) -> Self {
        Self {
            config,
            patches_scanner,
            cancellation_token: None,
        }
    }

    fn start(&mut self, handle: tokio::runtime::Handle) {
        if self.cancellation_token.is_some() {
            return;
        }

        log::info!("starting patch autoupdater");
        let cancellation_token = tokio_util::sync::CancellationToken::new();
        handle.spawn({
            let cancellation_token = cancellation_token.clone();
            let config = self.config.clone();
            let patches_scanner = self.patches_scanner.clone();
            async move {
                'l: loop {
                    let (repo_url, patches_path) = {
                        let config = config.read();
                        (
                            if !config.patch_repo.is_empty() {
                                config.patch_repo.clone()
                            } else {
                                config::DEFAULT_PATCH_REPO.to_owned()
                            },
                            config.patches_path().to_path_buf(),
                        )
                    };

                    let patches_scanner = patches_scanner.clone();
                    let _ = tokio::runtime::Handle::current()
                        .spawn_blocking(move || {
                            patches_scanner.rescan(move || {
                                match update(&repo_url, &patches_path) {
                                    Ok(patches) => Some(patches),
                                    Err(e) => {
                                        log::error!("failed to update patches: {:?}", e);
                                        return None;
                                    }
                                }
                            });
                        })
                        .await;
                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_secs(15)) => { }
                        _ = cancellation_token.cancelled() => { break 'l; }
                    }
                }
                log::info!("stopped patch autoupdater");
            }
        });
        self.cancellation_token = Some(cancellation_token);
    }

    fn stop(&mut self) {
        if let Some(cancellation_token) = self.cancellation_token.take() {
            cancellation_token.cancel();
        }
    }

    pub fn set_enabled(&mut self, handle: tokio::runtime::Handle, enabled: bool) {
        if enabled {
            self.start(handle);
        } else {
            self.stop();
        }
    }
}

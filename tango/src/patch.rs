use futures::StreamExt;
use itertools::Itertools;
use tokio::io::AsyncWriteExt;

use crate::{config, filesync, game, rom, scanner, sync};

#[derive(serde::Deserialize, Debug)]
struct Metadata {
    pub patch: PatchMetadata,
    pub versions: std::collections::HashMap<String, VersionMetadata>,
}

#[derive(serde::Deserialize, Debug)]
struct PatchMetadata {
    pub title: String,
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub source: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
struct VersionMetadata {
    #[serde(default)]
    pub rom_overrides: rom::Overrides,
    pub netplay_compatibility: String,
}

#[derive(Clone)]
pub struct Version {
    pub rom_overrides: rom::Overrides,
    pub netplay_compatibility: String,
    pub supported_games: std::collections::HashSet<&'static (dyn game::Game + Send + Sync)>,
    pub save_templates: std::collections::HashMap<
        &'static (dyn game::Game + Send + Sync),
        std::collections::BTreeMap<String, Box<dyn tango_dataview::save::Save + Send + Sync>>,
    >,
}

pub struct Patch {
    pub path: std::path::PathBuf,
    pub title: String,
    pub authors: Vec<mailparse::SingleInfo>,
    pub license: Option<String>,
    pub source: Option<String>,
    pub readme: Option<String>,
    pub versions: std::collections::HashMap<semver::Version, Version>,
}

pub async fn update(url: &String, root: &std::path::Path) -> Result<(), anyhow::Error> {
    std::fs::create_dir_all(root)?;

    let client = reqwest::Client::new();
    let entries = tokio::time::timeout(
        // 30 second timeout to fetch JSON.
        std::time::Duration::from_secs(30),
        (|| async {
            Ok::<_, anyhow::Error>(
                client
                    .get(format!("{}/index.json", url))
                    .header("User-Agent", "tango")
                    .send()
                    .await?
                    .json::<filesync::Entries>()
                    .await?,
            )
        })(),
    )
    .await??;

    let root = root.to_path_buf();
    filesync::sync(
        &root,
        &entries,
        {
            let url = url.clone();
            let root = root.clone();
            move |path| {
                let url = url.clone();
                let root = root.clone();
                Box::pin(async move {
                    let mut output_file = tokio::fs::File::create(&root.join(path)).await?;
                    let client = reqwest::Client::new();
                    let mut stream = tokio::time::timeout(
                        // 30 second timeout to initiate connection.
                        std::time::Duration::from_secs(30),
                        client
                            .get(format!(
                                "{}/{}",
                                url,
                                path.components().map(|v| v.as_os_str().to_string_lossy()).join("/")
                            ))
                            .header("User-Agent", "tango")
                            .send(),
                    )
                    .await?
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
                    .bytes_stream();
                    while let Some(chunk) = tokio::time::timeout(
                        // 30 second timeout per stream chunk.
                        std::time::Duration::from_secs(30),
                        stream.next(),
                    )
                    .await?
                    {
                        let chunk = chunk.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
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

pub fn scan(path: &std::path::Path) -> Result<std::collections::BTreeMap<String, Patch>, std::io::Error> {
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

        if entry.file_type().ok().map(|ft| !ft.is_dir()).unwrap_or(false) {
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

            let read_version_dir = match std::fs::read_dir(entry.path().join(format!("v{}", sv.to_string()))) {
                Ok(read_version_dir) => read_version_dir,
                Err(e) => {
                    log::warn!("{}: {}", entry.path().display(), e);
                    continue;
                }
            };

            let mut supported_games = std::collections::HashSet::new();
            let mut save_templates = std::collections::HashMap::new();

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

                lazy_static! {
                    static ref PATCH_FILENAME_REGEX: regex::Regex =
                        regex::Regex::new(r"^(\S{4})_(\d{2}).bps$").unwrap();
                    static ref SAVE_TEMPLATE_FILENAME_REGEX: regex::Regex =
                        regex::Regex::new(r"^(\S{4})_(\d{2})(?:|_(.+?)).sav$").unwrap();
                }

                enum FileType {
                    BpsPatch,
                    SaveTemplate(String),
                }

                let (rom_id, revision, file_type) = if let Some(captures) = PATCH_FILENAME_REGEX.captures(&filename) {
                    let rom_id = captures.get(1).unwrap().as_str().to_string();
                    let revision = captures.get(2).unwrap().as_str().parse::<u8>().unwrap();
                    (rom_id, revision, FileType::BpsPatch)
                } else if let Some(captures) = SAVE_TEMPLATE_FILENAME_REGEX.captures(&filename) {
                    let rom_id = captures.get(1).unwrap().as_str().to_string();
                    let revision = captures.get(2).unwrap().as_str().parse::<u8>().unwrap();
                    let name = captures.get(3).map(|v| v.as_str()).unwrap_or("").to_string();
                    (rom_id, revision, FileType::SaveTemplate(name))
                } else {
                    continue;
                };

                let game = if let Some(game) = game::find_by_rom_info(rom_id.as_bytes().try_into().unwrap(), revision) {
                    game
                } else {
                    continue;
                };

                match file_type {
                    FileType::BpsPatch => {
                        supported_games.insert(game);
                    }
                    FileType::SaveTemplate(name) => {
                        let save = match std::fs::read(&entry.path())
                            .map_err(|e| e.into())
                            .and_then(|raw| game.parse_save(&raw))
                        {
                            Ok(save) => save,
                            Err(e) => {
                                log::error!("failed to read save template: {:?}", e);
                                continue;
                            }
                        };
                        save_templates
                            .entry(game)
                            .or_insert_with(|| std::collections::BTreeMap::new())
                            .insert(name, save);
                    }
                }
            }

            versions.insert(
                sv,
                Version {
                    rom_overrides: version.rom_overrides,
                    netplay_compatibility: version.netplay_compatibility,
                    supported_games,
                    save_templates,
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
    pub fn new(config: std::sync::Arc<parking_lot::RwLock<config::Config>>, patches_scanner: Scanner) -> Self {
        Self {
            config,
            patches_scanner,
            cancellation_token: None,
        }
    }

    fn start(&mut self) {
        if self.cancellation_token.is_some() {
            return;
        }

        log::info!("starting patch autoupdater");
        let cancellation_token = tokio_util::sync::CancellationToken::new();
        tokio::task::spawn({
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
                    let _ = tokio::task::spawn_blocking(move || {
                        patches_scanner.rescan(move || {
                            if let Err(e) = sync::block_on(update(&repo_url, &patches_path)) {
                                log::error!("failed to update patches: {:?}", e);
                            }
                            scan(&patches_path).ok()
                        });
                        log::info!("patch autoupdate completed");
                    })
                    .await;
                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_secs(15 * 60)) => { }
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

    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled {
            self.start();
        } else {
            self.stop();
        }
    }
}

pub fn apply_patch_from_disk(
    rom: &[u8],
    game: &'static (dyn game::Game + Send + Sync),
    patches_path: &std::path::Path,
    patch_name: &str,
    patch_version: &semver::Version,
) -> Result<Vec<u8>, anyhow::Error> {
    let patch_name = std::path::Path::new(patch_name);
    if patch_name.components().count() > 1 {
        anyhow::bail!("attempted path traversal in patch name");
    }

    let (rom_code, revision) = game.gamedb_entry().rom_code_and_revision;
    let raw = std::fs::read(
        patches_path
            .join(&patch_name)
            .join(format!("v{}", patch_version))
            .join(format!(
                "{}_{:02}.bps",
                std::str::from_utf8(rom_code).unwrap(),
                revision
            )),
    )?;
    Ok(bps::Patch::decode(&raw)?.apply(rom)?)
}

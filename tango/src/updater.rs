use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

const GITHUB_RELEASES_URL: &str = "https://api.github.com/repos/tangobattle/tango/releases";

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    UpToDate,
    Downloading { current: u64, total: u64 },
    ReadyToUpdate,
}

#[derive(Debug, Clone)]
pub struct Status {
    pub latest_version: semver::Version,
    pub state: State,
}

#[derive(serde::Deserialize)]
struct GithubReleaseAssetInfo {
    name: String,
    browser_download_url: String,
}

#[derive(serde::Deserialize)]
struct GithubReleaseInfo {
    tag_name: String,
    prerelease: bool,
    assets: Vec<GithubReleaseAssetInfo>,
}

pub struct Updater {
    path: std::path::PathBuf,
    status: std::sync::Arc<parking_lot::Mutex<Status>>,
    cancellation_token: Option<tokio_util::sync::CancellationToken>,
}

#[cfg(target_os = "macos")]
fn is_target_installer(s: &str) -> bool {
    s.ends_with("-macos.zip")
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn is_target_installer(s: &str) -> bool {
    s.ends_with("-x86_64-windows.msi")
}

#[cfg(target_os = "macos")]
const PENDING_FILENAME: &str = "pending.zip";
#[cfg(target_os = "macos")]
const IN_PROGRESS_FILENAME: &str = "in_progress.zip";
#[cfg(target_os = "macos")]
const INCOMPLETE_FILENAME: &str = "incomplete.zip";

#[cfg(target_os = "windows")]
const PENDING_FILENAME: &str = "pending.msi";
#[cfg(target_os = "windows")]
const IN_PROGRESS_FILENAME: &str = "in_progress.msi";
#[cfg(target_os = "windows")]
const INCOMPLETE_FILENAME: &str = "incomplete.msi";

#[cfg(target_os = "macos")]
fn do_update(_path: &std::path::Path) {
    // Surprise, this does nothing!
}

#[cfg(target_os = "windows")]
fn do_update(path: &std::path::Path) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x00000008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
    let mut command = std::process::Command::new("msiexec");
    let new_path = path.with_file_name(IN_PROGRESS_FILENAME);
    std::fs::rename(path, &new_path).unwrap();
    command
        .arg("/passive")
        .arg(new_path)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .unwrap();
    // Is this racy? Can we spawn msiexec before we exit?
    std::process::exit(0);
}

impl Updater {
    pub fn new(path: &std::path::Path) -> Updater {
        Self {
            path: path.to_owned(),
            status: std::sync::Arc::new(parking_lot::Mutex::new(Status {
                latest_version: env!("CARGO_PKG_VERSION").parse().unwrap(),
                state: State::UpToDate,
            })),
            cancellation_token: None,
        }
    }

    pub fn do_update(&self) {
        let pending_path = self.path.join(PENDING_FILENAME);
        if std::fs::metadata(&pending_path).is_ok() {
            do_update(&pending_path);
        }
    }

    fn start(&mut self) {
        if self.cancellation_token.is_some() {
            return;
        }

        let _ = std::fs::remove_file(self.path.join(INCOMPLETE_FILENAME));
        self.do_update();

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        tokio::task::spawn({
            let cancellation_token = cancellation_token.clone();
            let status = self.status.clone();
            let path = self.path.clone();
            async move {
                'l: loop {
                    let status = status.clone();
                    let path = path.clone();
                    if let Err(e) = (move || async move {
                        let client = reqwest::Client::new();
                        let releases = client
                            .get(GITHUB_RELEASES_URL)
                            .header("User-Agent", "tango")
                            .send()
                            .await?
                            .json::<Vec<GithubReleaseInfo>>()
                            .await?;

                        let (version, info) = if let Some(release) = releases
                            .into_iter()
                            .flat_map(|r| {
                                if r.prerelease {
                                    return vec![];
                                }

                                if r.tag_name.chars().next() != Some('v') {
                                    return vec![];
                                }

                                let v = if let Ok(v) = r.tag_name[1..].parse::<semver::Version>() {
                                    v
                                } else {
                                    return vec![];
                                };

                                vec![(v, r)]
                            })
                            .max_by_key(|(v, _)| v.clone())
                        {
                            release
                        } else {
                            anyhow::bail!("no releases found at all");
                        };

                        // If this version is older or the one we already know about, skip.
                        if version <= status.lock().latest_version {
                            log::info!("already up to date! latest version: {}", version);
                            return Ok(());
                        }

                        // Find the appropriate release.
                        let asset = if let Some(asset) =
                            info.assets.into_iter().find(|asset| is_target_installer(&asset.name))
                        {
                            asset
                        } else {
                            anyhow::bail!("version {} is missing assets", version);
                        };

                        let resp = reqwest::get(&asset.browser_download_url).await?;
                        let mut current = 0u64;
                        let total = resp.content_length().unwrap_or(0);

                        let incomplete_output_path = path.join(INCOMPLETE_FILENAME);
                        {
                            let mut output_file = tokio::fs::File::create(&incomplete_output_path).await?;
                            let mut stream = resp.bytes_stream();
                            while let Some(chunk) = stream.next().await {
                                let mut chunk = chunk?;
                                output_file.write_buf(&mut chunk).await?;
                                current += chunk.len() as u64;
                                status.lock().state = State::Downloading { current, total };
                            }
                        }
                        std::fs::rename(incomplete_output_path, path.join(PENDING_FILENAME))?;

                        status.lock().state = State::ReadyToUpdate;

                        Ok::<(), anyhow::Error>(())
                    })()
                    .await
                    {
                        log::error!("updater failed: {:?}", e);
                    }

                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_secs(30 * 60)) => { }
                        _ = cancellation_token.cancelled() => { break 'l; }
                    }
                }

                let mut status = status.lock();
                if let State::Downloading { .. } = status.state {
                    // Do cleanup.
                    let _ = std::fs::remove_file(&path.join(IN_PROGRESS_FILENAME));
                    let _ = std::fs::remove_file(&path.join(INCOMPLETE_FILENAME));
                    let _ = std::fs::remove_file(&path.join(PENDING_FILENAME));
                    status.state = State::UpToDate;
                }
            }
        });
        self.cancellation_token = Some(cancellation_token);
    }

    fn stop(&mut self) {
        if let Some(cancellation_token) = self.cancellation_token.take() {
            cancellation_token.cancel();
        }
    }

    pub fn status(&self) -> Status {
        self.status.lock().clone()
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled {
            self.start();
        } else {
            self.stop();
        }
    }
}

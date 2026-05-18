//! Self-updater. Ported from `tango/src/updater.rs`.
//!
//! Lifecycle (matches legacy):
//!
//!   1. Query GitHub for the latest tango release.
//!   2. If newer than the current build, stream the platform's
//!      installer asset to `<data>/updater/incomplete`.
//!   3. On clean download, rename → `pending.<ext>`.
//!   4. Next launch (or "Update Now" button) renames pending →
//!      `in_progress.<ext>` and runs it. The rename is a poison
//!      flag so a busted installer doesn't get re-run on the
//!      following launch.
//!   5. The installer replaces the current binary; we exit.
//!
//! The `path` passed to [`Updater::new`] is the cache dir for
//! step (2)/(3)/(4) files. For tango-ng that's
//! `config.data_path/updater`.

use crate::config;
use futures::StreamExt;
use tokio::io::AsyncWriteExt;

const GITHUB_RELEASES_URL: &str = "https://api.github.com/repos/tangobattle/tango/releases";

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: semver::Version,
    pub info: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// Either we haven't queried yet (`release: None`) or we
    /// queried and the latest is what we're running
    /// (`release: Some(...)`).
    UpToDate { release: Option<Option<Release>> },
    UpdateAvailable { release: Release },
    Downloading { release: Release, current: u64, total: u64 },
    ReadyToUpdate { release: Release },
}

#[derive(serde::Deserialize)]
struct GithubReleaseAssetInfo {
    name: String,
    browser_download_url: String,
}

#[derive(serde::Deserialize)]
struct GithubReleaseInfo {
    tag_name: String,
    assets: Vec<GithubReleaseAssetInfo>,
    body: String,
    prerelease: bool,
}

fn is_target_installer(s: &str) -> bool {
    if cfg!(target_os = "macos") {
        s.ends_with("-macos.dmg")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        s.ends_with("-x86_64-windows.exe")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        s.ends_with("-x86_64-linux.AppImage")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        s.ends_with("-aarch64-linux.AppImage")
    } else {
        false
    }
}

const INCOMPLETE_FILENAME: &str = "incomplete";

#[cfg(target_os = "windows")]
const PENDING_FILENAME: &str = "pending.exe";
#[cfg(target_os = "windows")]
const IN_PROGRESS_FILENAME: &str = "in_progress.exe";

#[cfg(target_os = "macos")]
const PENDING_FILENAME: &str = "pending.dmg";
#[cfg(target_os = "macos")]
const IN_PROGRESS_FILENAME: &str = "in_progress.dmg";

#[cfg(target_os = "linux")]
const PENDING_FILENAME: &str = "pending.AppImage";
#[cfg(target_os = "linux")]
const IN_PROGRESS_FILENAME: &str = "in_progress.AppImage";

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
const PENDING_FILENAME: &str = "pending.bin";
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
const IN_PROGRESS_FILENAME: &str = "in_progress.bin";

#[cfg(target_os = "windows")]
fn do_update(path: &std::path::Path) {
    // Same flags legacy uses — detach so closing tango-ng's
    // process doesn't take the installer with it.
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x00000008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
    let _ = std::process::Command::new(path)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn();
    std::process::exit(0);
}

#[cfg(not(target_os = "windows"))]
fn do_update(path: &std::path::Path) {
    // Non-Windows hand-off isn't ported yet (legacy uses macOS
    // CFBundle + Linux execve gymnastics). Open the downloaded
    // asset and let the user finish manually rather than fail
    // silently.
    let _ = open::that(path);
    std::process::exit(0);
}

pub struct Updater {
    path: std::path::PathBuf,
    current_version: semver::Version,
    allow_prerelease_upgrades: bool,
    status: std::sync::Arc<tokio::sync::Mutex<Status>>,
    cancellation_token: Option<tokio_util::sync::CancellationToken>,
}

impl Updater {
    /// `path` is the cache directory (downloads land inside).
    /// `allow_prerelease_upgrades` is sampled once at start —
    /// changes via Settings take effect on next launch.
    pub fn new(path: &std::path::Path, allow_prerelease_upgrades: bool) -> Self {
        let current_version: semver::Version = env!("CARGO_PKG_VERSION").parse().unwrap();
        Self {
            path: path.to_owned(),
            current_version,
            allow_prerelease_upgrades,
            status: std::sync::Arc::new(tokio::sync::Mutex::new(Status::UpToDate { release: None })),
            cancellation_token: None,
        }
    }

    /// If a previous session left a `pending.<ext>` installer in
    /// the cache, rename it (poison flag) and execute it. Exits
    /// the process on success. Safe no-op when nothing pending.
    pub fn finish_update(&self) {
        let pending_path = self.path.join(PENDING_FILENAME);
        if std::fs::metadata(&pending_path).is_ok() {
            let new_path = self.path.join(IN_PROGRESS_FILENAME);
            if std::fs::rename(&pending_path, &new_path).is_ok() {
                do_update(&new_path);
            }
        }
    }

    pub fn status_blocking(&self) -> Status {
        self.status.blocking_lock().clone()
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled {
            self.start();
        } else {
            self.stop();
        }
    }

    fn start(&mut self) {
        if self.cancellation_token.is_some() {
            return;
        }

        // Clean up stale crash artifacts; attempt any pending
        // update again in case a previous session was killed
        // between rename and exec.
        let _ = std::fs::remove_file(self.path.join(INCOMPLETE_FILENAME));
        let _ = std::fs::remove_file(self.path.join(IN_PROGRESS_FILENAME));
        self.finish_update();

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        let captured = cancellation_token.clone();
        let status = self.status.clone();
        let path = self.path.clone();
        let current_version = self.current_version.clone();
        let allow_prerelease = self.allow_prerelease_upgrades;

        tokio::task::spawn(async move {
            'l: loop {
                let res = async {
                    let client = reqwest::Client::new();
                    let releases = tokio::time::timeout(
                        std::time::Duration::from_secs(30),
                        async {
                            Ok::<_, anyhow::Error>(
                                client
                                    .get(GITHUB_RELEASES_URL)
                                    .header("User-Agent", "tango-ng")
                                    .send()
                                    .await?
                                    .json::<Vec<GithubReleaseInfo>>()
                                    .await?,
                            )
                        },
                    )
                    .await??;

                    // Pick the highest semver among non-prerelease
                    // (or all, if the user opted in).
                    let Some((version, info)) = releases
                        .into_iter()
                        .flat_map(|r| {
                            if !r.tag_name.starts_with('v') {
                                return None;
                            }
                            let v: semver::Version = r.tag_name[1..].parse().ok()?;
                            if !allow_prerelease && (r.prerelease || !v.pre.is_empty()) {
                                return None;
                            }
                            Some((v, r))
                        })
                        .max_by_key(|(v, _)| v.clone())
                    else {
                        anyhow::bail!("no releases found");
                    };

                    let release = Release {
                        version: version.clone(),
                        info: info.body.clone(),
                    };

                    // Skip if we already know about this one.
                    {
                        let mut g = status.lock().await;
                        match &*g {
                            Status::UpToDate { .. } => {
                                let has_latest = version == current_version
                                    || (allow_prerelease && version < current_version);
                                if has_latest {
                                    *g = Status::UpToDate {
                                        release: Some(if version == current_version {
                                            Some(release.clone())
                                        } else {
                                            None
                                        }),
                                    };
                                    return Ok(());
                                }
                            }
                            Status::ReadyToUpdate { release: r } => {
                                let has_latest = version == r.version
                                    || (allow_prerelease && version < r.version);
                                if has_latest {
                                    return Ok(());
                                }
                            }
                            _ => {}
                        }
                    }

                    // Find platform installer asset.
                    let Some(asset) = info.assets.into_iter().find(|a| is_target_installer(&a.name)) else {
                        log::info!("release {version} has no asset for this target");
                        return Ok(());
                    };

                    *status.lock().await = Status::UpdateAvailable {
                        release: release.clone(),
                    };

                    // Stream the download into `incomplete`,
                    // updating the progress status as we go.
                    let resp = tokio::time::timeout(
                        std::time::Duration::from_secs(30),
                        reqwest::get(&asset.browser_download_url),
                    )
                    .await??;
                    let total = resp.content_length().unwrap_or(0);
                    let mut current = 0u64;
                    let incomplete_path = path.join(INCOMPLETE_FILENAME);
                    {
                        let mut f = tokio::fs::File::create(&incomplete_path).await?;
                        let mut stream = resp.bytes_stream();
                        while let Some(chunk) = tokio::time::timeout(
                            std::time::Duration::from_secs(30),
                            stream.next(),
                        )
                        .await?
                        {
                            let chunk = chunk?;
                            f.write_all(&chunk).await?;
                            current += chunk.len() as u64;
                            *status.lock().await = Status::Downloading {
                                release: release.clone(),
                                current,
                                total,
                            };
                        }
                    }
                    std::fs::rename(incomplete_path, path.join(PENDING_FILENAME))?;
                    *status.lock().await = Status::ReadyToUpdate { release };
                    Ok::<(), anyhow::Error>(())
                }
                .await;
                if let Err(e) = res {
                    log::warn!("updater failed: {e:?}");
                }

                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(30 * 60)) => {}
                    _ = captured.cancelled() => { break 'l; }
                }
            }

            // On cancel: clean up the half-download so we don't
            // leak it across runs. Pending stays — the user may
            // still want to apply it on next launch.
            let _ = std::fs::remove_file(path.join(INCOMPLETE_FILENAME));
        });
        self.cancellation_token = Some(cancellation_token);
    }

    fn stop(&mut self) {
        if let Some(t) = self.cancellation_token.take() {
            t.cancel();
        }
    }
}

impl Drop for Updater {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Convenience: where the updater cache lives for a given
/// config. Used by both `Updater::new` and any UI that needs
/// to show the user where the downloads are.
pub fn updater_cache_dir(config: &config::Config) -> std::path::PathBuf {
    config.data_path.join("updater")
}

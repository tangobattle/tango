/// Autoupdater.
///
/// The autoupdater is a little contrived, but it works like this:
///
/// 1. We query Github for the latest release that is greater than our current release.
/// 2. If found, we download it as INCOMPLETE_FILENAME. Once it's downloaded, we rename it to PENDING_FILENAME.
/// 3. On the next launch of Tango or if manually triggered, if PENDING_FILENAME is found, we run the update routine.
/// 4. To prevent the updater from getting wedged, we rename PENDING_FILENAME to IN_PROGRESS_FILENAME, such that on a second launch of Tango we don't try a bad upgrade.
/// 5. We delete IN_PROGRESS_FILENAME.
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::{config, version};

const GITHUB_RELEASES_URL: &str = "https://api.github.com/repos/tangobattle/tango/releases";

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: semver::Version,
    pub info: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
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
}

pub struct Updater {
    config: std::sync::Arc<parking_lot::RwLock<config::Config>>,
    ui_callback: std::sync::Arc<tokio::sync::Mutex<Option<Box<dyn Fn() + Sync + Send>>>>,
    current_version: semver::Version,
    path: std::path::PathBuf,
    status: std::sync::Arc<tokio::sync::Mutex<Status>>,
    cancellation_token: Option<tokio_util::sync::CancellationToken>,
}

#[cfg(target_os = "macos")]
fn is_target_installer(s: &str) -> bool {
    s.ends_with("-macos.dmg")
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn is_target_installer(s: &str) -> bool {
    s.ends_with("-x86_64-windows.exe")
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn is_target_installer(s: &str) -> bool {
    s.ends_with("-x86_64-linux.AppImage")
}

const INCOMPLETE_FILENAME: &str = "incomplete";

#[cfg(target_os = "macos")]
const PENDING_FILENAME: &str = "pending.dmg";
#[cfg(target_os = "macos")]
const IN_PROGRESS_FILENAME: &str = "in_progress.dmg";

#[cfg(target_os = "windows")]
const PENDING_FILENAME: &str = "pending.exe";
#[cfg(target_os = "windows")]
const IN_PROGRESS_FILENAME: &str = "in_progress.exe";

#[cfg(target_os = "linux")]
const PENDING_FILENAME: &str = "pending.AppImage";
#[cfg(target_os = "linux")]
const IN_PROGRESS_FILENAME: &str = "in_progress.AppImage";

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(&dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            std::fs::copy(&entry.path(), &dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn do_update(path: &std::path::Path) {
    let bundle = core_foundation::bundle::CFBundle::main_bundle();
    if bundle.info_dictionary().is_empty() {
        // Application is not bundled.
        return;
    }

    let output = std::process::Command::new("/usr/bin/hdiutil")
        .arg("attach")
        .arg("-noverify")
        .arg("-plist")
        .arg(path)
        .output()
        .unwrap();

    let mount_point = std::path::PathBuf::from(
        plist::Value::from_reader_xml(&mut output.stdout.as_slice())
            .unwrap()
            .as_dictionary()
            .and_then(|d| d.get("system-entities"))
            .and_then(|e| e.as_array())
            .and_then(|a| {
                a.iter()
                    .flat_map(|v| {
                        v.as_dictionary()
                            .and_then(|d| {
                                d.get("mount-point")
                                    .and_then(|mp| mp.as_string().map(|s| s.to_string()))
                            })
                            .into_iter()
                    })
                    .next()
            })
            .unwrap(),
    );

    log::info!("dmg is mounted at {}", mount_point.display());

    if let Err(e) = (|| -> Result<(), anyhow::Error> {
        let bundle_path = bundle.path().ok_or(anyhow::anyhow!("no bundle path"))?;
        std::fs::remove_dir_all(&bundle_path)?;

        copy_dir_all(&mount_point.join("Tango.app"), &bundle_path)?;

        let _ = std::process::Command::new("/usr/bin/hdiutil")
            .arg("detach")
            .arg(&mount_point)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        std::process::Command::new("/usr/bin/open")
            .arg(bundle_path)
            .spawn()
            .unwrap();

        Ok(())
    })() {
        log::error!("failed to update automatically: {:?}", e);
        // Unable to update automatically, open DMG.
        std::process::Command::new("/usr/bin/open")
            .arg(mount_point)
            .spawn()
            .unwrap();
    }
    std::process::exit(0);
}

#[cfg(target_os = "windows")]
fn do_update(path: &std::path::Path) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x00000008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
    let mut command = std::process::Command::new(path);
    command
        .current_dir(std::env::temp_dir())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .unwrap();
    // Is this racy? Can we exit before the installer finishes?
    std::process::exit(0);
}

#[cfg(target_os = "linux")]
fn do_update(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;
    let appimage_path = std::env::var("APPIMAGE").unwrap();
    // Unlink the current file first, otherwise we will get ETXTBSY while copying.
    std::fs::remove_file(&appimage_path).unwrap();
    std::fs::copy(path, &appimage_path).unwrap();
    std::fs::set_permissions(&appimage_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    if let nix::unistd::ForkResult::Child = unsafe { nix::unistd::fork() }.unwrap() {
        nix::unistd::setsid().unwrap();
        let mut command = std::process::Command::new(appimage_path);
        Err::<(), _>(command.exec()).unwrap();
    }
    std::process::exit(0);
}

impl Updater {
    pub fn new(path: &std::path::Path, config: std::sync::Arc<parking_lot::RwLock<config::Config>>) -> Updater {
        let current_version = version::current();
        Self {
            config,
            current_version: current_version.clone(),
            path: path.to_owned(),
            ui_callback: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            status: std::sync::Arc::new(tokio::sync::Mutex::new(Status::UpToDate { release: None })),
            cancellation_token: None,
        }
    }

    pub fn finish_update(&self) {
        let pending_path = self.path.join(PENDING_FILENAME);
        if std::fs::metadata(&pending_path).is_ok() {
            let new_path = self.path.join(IN_PROGRESS_FILENAME);
            std::fs::rename(&pending_path, &new_path).unwrap();
            do_update(&new_path);
        }
    }

    pub fn set_ui_callback(&self, cb: Option<Box<dyn Fn() + Sync + Send>>) {
        *self.ui_callback.blocking_lock() = cb;
    }

    pub fn current_version(&self) -> &semver::Version {
        &self.current_version
    }

    fn start(&mut self) {
        if self.cancellation_token.is_some() {
            return;
        }

        let _ = std::fs::remove_file(self.path.join(INCOMPLETE_FILENAME));
        let _ = std::fs::remove_file(self.path.join(IN_PROGRESS_FILENAME));
        self.finish_update();

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        tokio::task::spawn({
            let cancellation_token = cancellation_token.clone();
            let status = self.status.clone();
            let path = self.path.clone();
            let ui_callback = self.ui_callback.clone();
            let current_version = self.current_version.clone();
            let config = self.config.clone();
            async move {
                'l: loop {
                    let status = status.clone();
                    let path = path.clone();
                    let ui_callback = ui_callback.clone();
                    let current_version = current_version.clone();
                    let config = config.clone();
                    if let Err(e) = (move || async move {
                        let client = reqwest::Client::new();
                        let releases = tokio::time::timeout(
                            // 30 second timeout to get release info.
                            std::time::Duration::from_secs(30),
                            (|| async {
                                Ok::<_, anyhow::Error>(
                                    client
                                        .get(GITHUB_RELEASES_URL)
                                        .header("User-Agent", "tango")
                                        .send()
                                        .await?
                                        .json::<Vec<GithubReleaseInfo>>()
                                        .await?,
                                )
                            })(),
                        )
                        .await??;

                        let (version, info) = if let Some(release) = releases
                            .into_iter()
                            .flat_map(|r| {
                                if r.tag_name.chars().next() != Some('v') {
                                    return vec![];
                                }
                                let v = if let Ok(v) = r.tag_name[1..].parse::<semver::Version>() {
                                    v
                                } else {
                                    return vec![];
                                };

                                if !config.read().allow_prerelease_upgrades && !v.pre.is_empty() {
                                    return vec![];
                                }

                                vec![(v, r)]
                            })
                            .max_by_key(|(v, _)| v.clone())
                        {
                            release
                        } else {
                            anyhow::bail!("no releases found at all");
                        };

                        let release = Release {
                            version: version.clone(),
                            info: info.body.clone(),
                        };

                        // If this version is older or the one we already know about, skip.
                        {
                            let mut status_guard = status.lock().await;
                            match &*status_guard {
                                Status::UpToDate { .. } => {
                                    if version <= current_version {
                                        log::info!(
                                            "current version is already latest: {} vs {}",
                                            version,
                                            current_version
                                        );

                                        *status_guard = Status::UpToDate {
                                            release: Some(if version == current_version {
                                                Some(release.clone())
                                            } else {
                                                None
                                            }),
                                        };

                                        return Ok(());
                                    }
                                }
                                Status::ReadyToUpdate {
                                    release:
                                        Release {
                                            version: update_version,
                                            ..
                                        },
                                } => {
                                    if version <= *update_version {
                                        log::info!(
                                            "latest version already downloaded: {} vs {}",
                                            version,
                                            update_version
                                        );
                                        return Ok(());
                                    }
                                }
                                _ => {
                                    // If we are in update available or downloading, nothing interesting is happening, so let's just clobber it.
                                }
                            }
                        }

                        // Find the appropriate release.
                        let asset = if let Some(asset) =
                            info.assets.into_iter().find(|asset| is_target_installer(&asset.name))
                        {
                            asset
                        } else {
                            log::info!("version {} has no assets right now", version);
                            return Ok(());
                        };

                        *status.lock().await = Status::UpdateAvailable {
                            release: release.clone(),
                        };
                        if let Some(cb) = ui_callback.lock().await.as_ref() {
                            cb();
                        }

                        let resp = tokio::time::timeout(
                            // 30 second timeout to initiate connection.
                            std::time::Duration::from_secs(30),
                            reqwest::get(&asset.browser_download_url),
                        )
                        .await??;
                        let mut current = 0u64;
                        let total = resp.content_length().unwrap_or(0);

                        let incomplete_output_path = path.join(INCOMPLETE_FILENAME);
                        {
                            let mut output_file = tokio::fs::File::create(&incomplete_output_path).await?;
                            let mut stream = resp.bytes_stream();
                            while let Some(chunk) = tokio::time::timeout(
                                // 30 second timeout per stream chunk.
                                std::time::Duration::from_secs(30),
                                stream.next(),
                            )
                            .await?
                            {
                                let chunk = chunk?;
                                output_file.write_all(&chunk).await?;
                                current += chunk.len() as u64;
                                *status.lock().await = Status::Downloading {
                                    release: release.clone(),
                                    current,
                                    total,
                                };
                                if let Some(cb) = ui_callback.lock().await.as_ref() {
                                    cb();
                                }
                            }
                        }
                        std::fs::rename(incomplete_output_path, path.join(PENDING_FILENAME))?;

                        *status.lock().await = Status::ReadyToUpdate { release };
                        if let Some(cb) = ui_callback.lock().await.as_ref() {
                            cb();
                        }

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

                let mut status = status.lock().await;
                if let Status::Downloading { release, .. } = &*status {
                    // Do cleanup.
                    let _ = std::fs::remove_file(&path.join(IN_PROGRESS_FILENAME));
                    let _ = std::fs::remove_file(&path.join(INCOMPLETE_FILENAME));
                    let _ = std::fs::remove_file(&path.join(PENDING_FILENAME));
                    *status = Status::UpdateAvailable {
                        release: release.clone(),
                    };
                    if let Some(cb) = ui_callback.lock().await.as_ref() {
                        cb();
                    }
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

    pub async fn status(&self) -> Status {
        self.status.lock().await.clone()
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled {
            self.start();
        } else {
            self.stop();
        }
    }
}

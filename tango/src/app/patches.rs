//! Patch downloads and patch-tab actions.

use super::desktop::{open_path, reveal_path};
use super::{App, Message, RescanFollowup};
use crate::library::patch;
use crate::tabs;

impl App {
    pub(super) fn update_patches(&mut self, msg: tabs::patches::Message) -> iced::Task<Message> {
        // Bookkeeping before delegating: the map is App-level (see
        // `patch::Downloads`) because the tab isn't the only thing that
        // starts downloads or the only thing that renders them.
        match &msg {
            tabs::patches::Message::InstallProgress(key, downloaded, total) => {
                self.downloads.insert(
                    key.clone(),
                    patch::Download::Running(patch::Progress {
                        downloaded: *downloaded,
                        total: *total,
                    }),
                );
            }
            tabs::patches::Message::InstallCancelled(key) => {
                self.download_cancels.remove(key);
                self.downloads.remove(key);
            }
            tabs::patches::Message::InstallFinished(key, Ok(())) => {
                self.download_cancels.remove(key);
                self.downloads.remove(key);
            }
            tabs::patches::Message::InstallFinished(key, Err(_)) => {
                self.download_cancels.remove(key);
                self.downloads.insert(key.clone(), patch::Download::Failed);
            }
            _ => {}
        }
        let Some(effect) = self.patches.update(msg, &self.scanners.patches.read()) else {
            return iced::Task::none();
        };
        use tabs::patches::Effect as E;
        match effect {
            E::OpenPath(s) => open_path(s),
            E::RevealPath(p) => reveal_path(p),
            E::Rescan => {
                // ForceRebuildLoaded, not Refresh: the selection tuple
                // is unchanged by an install, so `refresh_loaded` would
                // take its early-return and leave the Play tab showing
                // the game unpatched.
                let followup = if self.pending_watch.is_some() {
                    RescanFollowup::RetryPendingWatch
                } else {
                    RescanFollowup::ForceRebuildLoaded
                };
                self.rescan_off_thread(followup)
            }
            E::RefreshIndex => {
                let url = self.config.patch_repo_url();
                let root = self.config.patches_path();
                iced::Task::perform(
                    async move {
                        patch::fetch_index(crate::library::http(), crate::library::storage(), &url, &root)
                            .await
                            .map(|_changed| ())
                            .map_err(|e| e.to_string())
                    },
                    tabs::patches::Message::RefreshFinished,
                )
                .map(Message::Patches)
            }
            E::Install(key) => self.install_patch(key),
            E::CancelInstall(key) => self.cancel_download(key),
            E::Uninstall((name, version)) => {
                if let Err(e) =
                    patch::uninstall(crate::library::storage(), &self.config.patches_path(), &name, &version)
                {
                    log::warn!("uninstalling {name} {version}: {e}");
                }
                self.rescan_off_thread(RescanFollowup::Refresh)
            }
            E::FetchReadme(key) => {
                let (name, version) = key.clone();
                let url = self.config.patch_repo_url();
                let Some(path) = self
                    .scanners
                    .patches
                    .read()
                    .entry(&name, &version)
                    .and_then(|e| e.readme.clone())
                else {
                    return iced::Task::none();
                };
                iced::Task::perform(
                    async move {
                        reqwest::Client::new()
                            .get(format!("{}/{path}", url.trim_end_matches('/')))
                            .header("User-Agent", "tango")
                            .timeout(std::time::Duration::from_secs(30))
                            .send()
                            .await
                            .ok()?
                            .error_for_status()
                            .ok()?
                            .text()
                            .await
                            .ok()
                    },
                    move |readme| tabs::patches::Message::ReadmeFetched(key.clone(), readme),
                )
                .map(Message::Patches)
            }
            E::InstallFailed => {
                // Don't leave a replay queued behind a download that
                // isn't coming.
                self.pending_watch = None;
                iced::Task::none()
            }
            E::ToggleFavorite(name) => {
                if !self.config.favorite_patches.remove(&name) {
                    self.config.favorite_patches.insert(name);
                }
                self.persist_config();
                iced::Task::none()
            }
        }
    }

    /// Download one patch version, reporting byte progress as it goes.
    ///
    /// Progress and the terminal result travel down one channel, so the
    /// stream ends exactly when the download does — no polling and no
    /// separate completion signal to keep in sync.
    pub(super) fn install_patch(&mut self, key: patch::VersionKey) -> iced::Task<Message> {
        // Already on its way — the user clicked twice, or two of the
        // four triggers want the same package.
        if self.downloads.get(&key).is_some_and(|d| d.is_running()) {
            return iced::Task::none();
        }
        let (name, version) = key.clone();
        let Some(entry) = self.scanners.patches.read().entry(&name, &version).cloned() else {
            return iced::Task::done(Message::Patches(tabs::patches::Message::InstallFinished(
                key,
                Err("not offered by this patch repo".to_string()),
            )));
        };
        let url = self.config.patch_repo_url();
        let root = self.config.patches_path();
        self.downloads.insert(
            key.clone(),
            patch::Download::Running(patch::Progress {
                downloaded: 0,
                total: 0,
            }),
        );

        let token = tokio_util::sync::CancellationToken::new();
        self.download_cancels.insert(key.clone(), token.clone());

        let (tx, rx) = futures::channel::mpsc::unbounded::<tabs::patches::Message>();
        let progress_tx = tx.clone();
        let progress_key = key.clone();
        tokio::task::spawn(async move {
            let result = patch::download(
                crate::library::http(),
                crate::library::storage(),
                &url,
                &root,
                &name,
                &version,
                &entry,
                move |p| {
                    let _ = progress_tx.unbounded_send(tabs::patches::Message::InstallProgress(
                        progress_key.clone(),
                        p.downloaded,
                        p.total,
                    ));
                    !token.is_cancelled()
                },
            )
            .await;
            let msg = match result {
                // The cancel already cleaned up; the UI just drops it.
                Ok(patch::Outcome::Cancelled) => tabs::patches::Message::InstallCancelled(key),
                Ok(patch::Outcome::Installed) => tabs::patches::Message::InstallFinished(key, Ok(())),
                Err(e) => tabs::patches::Message::InstallFinished(key, Err(format!("{e:#}"))),
            };
            let _ = tx.unbounded_send(msg);
        });
        iced::Task::stream(rx).map(Message::Patches)
    }

    /// Stop an in-flight download and forget it: the loop notices its
    /// token once per chunk, removes the partial file and reports back
    /// as cancelled. Dropping the row here rather than waiting for that
    /// keeps the click feeling immediate.
    pub(super) fn cancel_download(&mut self, key: patch::VersionKey) -> iced::Task<Message> {
        if let Some(token) = self.download_cancels.remove(&key) {
            token.cancel();
        }
        self.downloads.remove(&key);
        // Nothing is going to arrive for a replay queued behind it.
        if self.pending_watch.is_some() {
            self.pending_watch = None;
        }
        iced::Task::none()
    }

    /// Fetch the patch the loadout currently names, if we don't have it.
    ///
    /// The picker lists everything the repo offers, not just what's on
    /// disk, so choosing an entry is how you install it — and the same
    /// goes for the selection restored at startup, which comes back off
    /// the same index and can equally name something this machine has
    /// never downloaded. Cheap to call on every selection change:
    /// `install_patch` ignores a request for something already
    /// downloading, and this returns early for anything already
    /// installed. A download that failed is retried, so a selection
    /// change picks up again once the network comes back.
    pub(super) fn fetch_selected_patch(&mut self) -> iced::Task<Message> {
        let (Some(name), Some(version)) = (self.loadout.patch.clone(), self.loadout.patch_version.clone()) else {
            return iced::Task::none();
        };
        {
            let patches = self.scanners.patches.read();
            // Nothing to do if we have it, and nothing we *can* do if the
            // repo doesn't offer it (a sideloaded patch that was deleted).
            if patches.is_installed(&name, &version) || patches.entry(&name, &version).is_none() {
                return iced::Task::none();
            }
        }
        log::info!("selection needs {name} {version}, fetching");
        self.install_patch((name, version))
    }
}

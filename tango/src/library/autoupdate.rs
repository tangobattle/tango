//! Background patch-index refresher.
//!
//! Fetching and scanning are the library's (`tango_library::patch`);
//! what lives here is only the loop that drives them on a timer, which
//! owns a tokio task and a cancellation token — runtime glue, and so
//! the frontend's business rather than the library's. A browser build
//! would spell the same policy with its own timer.
//!
//! Under the old patch format this re-hashed every file in the patch
//! directory and downloaded whatever differed; now it re-fetches one
//! small conditional GET, so it costs a 304 in the steady state.

use crate::library::patch::{fetch_index, scan, scan_roots, Scanner};
use std::path::PathBuf;

pub struct Autoupdater {
    patches_path: PathBuf,
    patch_repo: String,
    patches_scanner: Scanner,
    cancellation_token: Option<tokio_util::sync::CancellationToken>,
}

impl Autoupdater {
    /// Fast enough to notice a new patch within the hour, slow enough
    /// not to hammer the repo.
    const INTERVAL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

    pub fn new(patches_path: PathBuf, patch_repo: String, patches_scanner: Scanner) -> Self {
        Self {
            patches_path,
            patch_repo,
            patches_scanner,
            cancellation_token: None,
        }
    }

    /// Start the background loop. Idempotent.
    pub fn start(&mut self) {
        if self.cancellation_token.is_some() {
            return;
        }
        log::info!("starting patch index autoupdater (every {:?})", Self::INTERVAL);
        let token = tokio_util::sync::CancellationToken::new();
        let scanner = self.patches_scanner.clone();
        let patches_path = self.patches_path.clone();
        let patch_repo = if self.patch_repo.is_empty() {
            tango_library::config::DEFAULT_PATCH_REPO.to_string()
        } else {
            self.patch_repo.clone()
        };
        tokio::task::spawn({
            let token = token.clone();
            async move {
                let storage = crate::library::storage();
                let http = crate::library::http();
                loop {
                    match fetch_index(http, storage, &patch_repo, &patches_path).await {
                        // Only a changed index is worth a rescan.
                        Ok(true) => {
                            let listing = storage.list(&scan_roots(&patches_path)).await;
                            scanner.rescan(|| scan(storage, &patches_path, &listing).ok());
                        }
                        Ok(false) => {}
                        Err(e) => log::error!("patch index autoupdate failed: {e:?}"),
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(Self::INTERVAL) => {}
                        _ = token.cancelled() => break,
                    }
                }
                log::info!("stopped patch index autoupdater");
            }
        });
        self.cancellation_token = Some(token);
    }

    pub fn stop(&mut self) {
        if let Some(token) = self.cancellation_token.take() {
            token.cancel();
        }
    }
}

impl Drop for Autoupdater {
    fn drop(&mut self) {
        self.stop();
    }
}

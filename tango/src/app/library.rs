//! Library scans and reconstruction of the selected save.

use super::{App, Message, RescanFollowup, Tab};
use crate::library::{rom, Scanners};
use crate::{loadout, selection};

impl App {
    /// The startup scan, in two stages: first everything the play tab
    /// is built from, then the replay index. They're separate because
    /// they're waited on separately — the play tab has no reason to sit
    /// behind a replay collection that can run to thousands of files,
    /// and the replays tab is the only screen that does.
    ///
    /// Each stage lands as its own `Rescanned` message, so the counter
    /// takes two.
    pub(super) fn boot_scan(&mut self) -> iced::Task<Message> {
        self.rescans_in_flight += 2;
        let scanners = self.scanners.clone();
        let config = self.config.clone();
        // One enumeration feeds both stages: it covers all four roots
        // and costs metadata only, so the first stage hands the replays
        // listing to the second rather than walking that tree twice.
        iced::Task::perform(
            async move {
                let listings = Scanners::list(&config).await;
                let replays = listings.replays.clone();
                let _ = tokio::task::spawn_blocking(move || scanners.rescan_library(&config, &listings)).await;
                replays
            },
            |listing| listing,
        )
        .then({
            let scanners = self.scanners.clone();
            move |listing| {
                let scanners = scanners.clone();
                // The play tab is live from here; the replay index
                // lands whenever it lands.
                iced::Task::done(Message::Rescanned(RescanFollowup::Boot)).chain(iced::Task::perform(
                    async move {
                        let _ = tokio::task::spawn_blocking(move || scanners.rescan_replays(&listing)).await;
                    },
                    |()| Message::Rescanned(RescanFollowup::BootReplays),
                ))
            }
        })
    }

    /// Run a full `Scanners::rescan` on a tokio blocking worker so
    /// the disk walk + TOML parse for patches (the slowest of the
    /// four) doesn't stall iced's update loop. Returns a task that
    /// emits `Message::Rescanned(followup)` once the worker is
    /// done; the followup tells the handler which post-scan work to
    /// chain (refresh `self.loaded`, warm stats, auto-pick a save).
    ///
    /// Bumps `rescans_in_flight` synchronously so the very next
    /// `view` call (and auto-rescan trigger) sees the rescan as
    /// live — without this, back-to-back triggers would stack
    /// workers until the first one actually gets scheduled.
    pub(super) fn rescan_off_thread(&mut self, followup: RescanFollowup) -> iced::Task<Message> {
        self.rescans_in_flight += 1;
        let scanners = self.scanners.clone();
        let config = self.config.clone();
        iced::Task::perform(
            async move {
                // Enumerate here (cheap, metadata only), then read and
                // parse on a blocking worker so the walk-and-parse of
                // every ROM and save doesn't stall iced's update loop.
                let listings = Scanners::list(&config).await;
                let _ = tokio::task::spawn_blocking(move || scanners.rescan(&config, &listings)).await;
            },
            move |()| Message::Rescanned(followup),
        )
    }

    /// Whether any rescan worker spawned by [`Self::rescan_off_thread`] is
    /// still in flight. Gates the automatic rescan triggers and the
    /// welcome screen's rescan button.
    pub(super) fn is_rescanning(&self) -> bool {
        self.rescans_in_flight > 0
    }

    /// Inputs that determine `loaded`, used to skip unchanged rebuilds.
    fn loaded_key(&self) -> Option<(rom::GameRef, std::path::PathBuf, Option<(String, semver::Version)>)> {
        let game = self.loadout.game?;
        let save_path = self.loadout.save.clone()?;
        let patch = match (&self.loadout.patch, &self.loadout.patch_version) {
            (Some(n), Some(v)) => Some((n.clone(), v.clone())),
            _ => None,
        };
        Some((game, save_path, patch))
    }

    /// Recompute `self.loaded` from the loadout's game + save +
    /// patch[+version]. Cheap when nothing's changed; expensive when
    /// ROM/assets need a fresh parse (BPS + asset parsing + icon
    /// decode), which is why we don't call it from view().
    pub(super) fn refresh_loaded(&mut self) {
        let Some((game, save_path, patch)) = self.loaded_key() else {
            self.loaded = None;
            return;
        };

        // Reuse existing if all inputs still match.
        if let Some(l) = &self.loaded {
            let cur_patch = l.patch.as_ref().map(|p| (p.name.clone(), p.version.clone()));
            if l.game == game && l.save_path == save_path && cur_patch == patch {
                return;
            }
        }

        let roms = self.scanners.roms.read();
        let saves = self.scanners.saves.read();
        let patches = self.scanners.patches.read();
        let Some(rom) = roms.get(&game).cloned() else {
            self.loaded = None;
            return;
        };
        let Some(scanned) = saves.get(&game).and_then(|v| v.iter().find(|s| s.path == save_path)) else {
            // Save was deleted out from under us (e.g. user deleted
            // it on disk and a rescan noticed). Drop the stale
            // selection so the picker stops showing a missing entry.
            self.loaded = None;
            drop(saves);
            drop(roms);
            drop(patches);
            self.loadout.save = None;
            return;
        };
        let save = scanned.save.clone_box();
        let patch_meta = patch.and_then(|(name, version)| {
            patches
                .version(&name, &version)
                .map(|v| (name.clone(), version.clone(), v.clone()))
        });
        drop(patches);
        drop(saves);
        drop(roms);

        log::info!(
            "loading selection: {:?} {} {}",
            game.family_and_variant(),
            save_path.display(),
            patch_meta
                .as_ref()
                .map(|(n, v, _)| format!("[{n} v{v}]"))
                .unwrap_or_default(),
        );
        let patches_path = self.config.patches_path();
        // The view state rides inside the LoadedSave, so swapping in a
        // freshly-built one drops any in-progress edit with the save it
        // was staged against — nothing to reset by hand.
        // A disk load carries no session payload — the editor opens on
        // the game's own default and the file picker takes it from
        // there.
        self.loaded = Some(selection::build(game, rom, save_path, save, &patches_path, patch_meta));
    }

    pub(super) fn finish_rescan(&mut self, followup: RescanFollowup) -> iced::Task<Message> {
        self.rescans_in_flight = self.rescans_in_flight.saturating_sub(1);
        let task = match followup {
            RescanFollowup::Boot => {
                self.library_scanned = true;
                log::info!(
                    "initial scan: {} rom(s), {} save game(s), {} patch(es)",
                    self.scanners.roms.read().len(),
                    self.scanners.saves.read().values().map(|v| v.len()).sum::<usize>(),
                    self.scanners.patches.read().installed.len(),
                );
                self.restore_selection();
                self.refresh_loaded();
                iced::Task::none()
            }
            RescanFollowup::BootReplays => {
                self.replays_scanned = true;
                log::info!("initial scan: {} replay(s)", self.scanners.replays.read().len());
                self.refresh_replay_stats().map(Message::Replays)
            }
            RescanFollowup::Refresh => {
                self.refresh_loaded();
                iced::Task::none()
            }
            RescanFollowup::RetryPendingWatch => {
                self.refresh_loaded();
                match self.pending_watch.take() {
                    Some(path) => self.watch_replay(path),
                    None => iced::Task::none(),
                }
            }
            RescanFollowup::RefreshAndReplayStats => {
                self.refresh_loaded();
                self.refresh_replay_stats().map(Message::Replays)
            }
            RescanFollowup::RefreshAndPickFirstSave => {
                // Land on the next available save anywhere in the
                // family (a sibling color variant is fine), not just
                // the deleted save's own game, and fix the loadout's
                // game to whatever that save resolves to.
                if self.loadout.save.is_none() {
                    if let Some(family) = self.loadout.family {
                        if let Some((game, path)) = loadout::first_available_family_save(&self.scanners, family) {
                            self.loadout.select_save(game, path, &self.config, &self.scanners);
                        }
                    }
                }
                self.refresh_loaded();
                iced::Task::none()
            }
            RescanFollowup::ForceRebuildLoaded => {
                self.loaded = None;
                self.refresh_loaded();
                iced::Task::none()
            }
        };
        // One rule for every landing: the selection's patch
        // should be on disk. At startup that's the restored
        // selection, which resolves against the repo index and
        // so can name a version this machine never downloaded;
        // afterwards it's the play tab re-asserting itself on
        // entry, since a trip to the patches tab can have
        // removed the package underneath it. Deliberately not
        // while the patches tab is up: re-downloading what the
        // user just removed, as they watch, is not help.
        if followup == RescanFollowup::Boot || self.tab == Tab::Play {
            let fetch = self.fetch_selected_patch();
            iced::Task::batch([task, fetch])
        } else {
            task
        }
    }
}

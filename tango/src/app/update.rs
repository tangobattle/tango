use super::*;

impl App {
    /// Apply a loadout-strip message (from either tab) to the shared
    /// App-level [`loadout::Loadout`] and run the selection-change
    /// follow-ups. The caller batches a lobby settings-resend after
    /// this, so a mid-lobby save/patch switch reaches the peer.
    pub(super) fn update_loadout(&mut self, msg: loadout::Message) -> iced::Task<Message> {
        // Download controls act on the fetch, not the selection, so they
        // carry their own key and skip the selection-changed follow-ups.
        let download_key = match &msg {
            loadout::Message::RetryPatchDownload(key) | loadout::Message::CancelPatchDownload(key) => Some(key.clone()),
            _ => None,
        };
        let Some(effect) = self.loadout.update(msg, &self.scanners, &self.config) else {
            return iced::Task::none();
        };
        match effect {
            loadout::Effect::CancelDownload => {
                let Some(key) = download_key else {
                    return iced::Task::none();
                };
                return self.cancel_download(key);
            }
            loadout::Effect::RetryDownload => {
                let Some(key) = download_key else {
                    return iced::Task::none();
                };
                self.downloads.remove(&key);
                return self.install_patch(key);
            }
            loadout::Effect::SelectionChanged => {
                self.refresh_loaded();
                self.persist_selection();
                // Game might have just changed — if so, the lobby
                // picker should show this game's default match
                // type (Triple where supported) instead of the
                // last game's pick.
                self.apply_default_match_type();
                // The picker offers patches that aren't downloaded, so
                // picking one is a request to fetch it.
                self.fetch_selected_patch()
            }
        }
    }

    pub(super) fn update_play(&mut self, msg: tabs::play::Message) -> iced::Task<Message> {
        let Some(effect) = self
            .play
            .update(msg, &self.scanners, &self.config, self.loaded.as_mut(), &self.loadout)
        else {
            return iced::Task::none();
        };
        use tabs::play::Effect as E;
        match effect {
            E::SetFrameDelay(d) => {
                // Lobby slider. Persisted to config; it's this side's local
                // frame delay (snapshotted into the match at start, not
                // negotiated with the peer), so there's no live match to push it
                // to here.
                self.config.frame_delay = d;
                self.persist_config();
                iced::Task::none()
            }
            E::Connect { ident, copy_code } => {
                let task = match ident {
                    netplay::LinkIdent::Matchmaking(link_code) => netplay::connect(
                        &mut self.netplay,
                        netplay::MatchmakingParams {
                            link_code,
                            endpoint: self.config.matchmaking_endpoint.clone(),
                            use_relay: self.config.relay_mode.use_relay(),
                        },
                    ),
                    netplay::LinkIdent::Direct(role) => netplay::connect_direct(&mut self.netplay, role),
                };
                // Connect wipes lobby state — re-apply the
                // default-MT policy now so the picker shows the
                // right value from the moment the waiting screen
                // appears, instead of flickering to Triple later
                // when the first Lobby-phase resend runs.
                self.apply_default_match_type();
                // Seed the blind-setup checkbox from the user's last
                // choice (cancel_and_renew reset it to false). Only
                // here, not in the per-resend default pass, so a
                // mid-lobby toggle still sticks.
                self.netplay.lobby.blind_setup = self.config.last_blind_setup;
                match copy_code {
                    // Fight auto-generated this code — put it straight on
                    // the clipboard so the host can paste it to their
                    // opponent right away.
                    Some(code) => iced::Task::batch([iced::clipboard::write(code), task]),
                    None => task,
                }
            }
            E::Disconnect => {
                self.netplay.disconnect();
                iced::Task::none()
            }
            E::SetMatchType(mt) => {
                self.netplay.set_match_type(mt);
                // An explicit user pick of match type pre-Lobby
                // would otherwise be clobbered the first time
                // `resend_settings_if_lobby` runs in Lobby —
                // that helper's "default to Triple" policy
                // fires whenever `default_mt_for_game` doesn't
                // match the current game, which is the case
                // when the user picked their match type before
                // any default was applied. Stamp the slot here
                // so the policy treats the pick as already
                // having defaulted for this game.
                if let Some(g) = self.loadout.game {
                    let (fam, var) = g.family_and_variant();
                    self.netplay.lobby.default_mt_for_game = Some((fam.to_string(), var));
                }
                self.resend_settings_if_lobby()
            }
            E::SetBlindSetup(v) => {
                self.netplay.set_blind_setup(v);
                // Remember the choice so the next lobby (this session or
                // a future launch) defaults to it.
                self.config.last_blind_setup = v;
                self.persist_config();
                self.resend_settings_if_lobby()
            }
            E::Unready => {
                self.netplay.uncommit();
                iced::Task::none()
            }
            E::ReadyWithSave => {
                // The editor's copy, so a staged edit is what gets
                // committed rather than the file it was staged against.
                // View-time gating disables Ready with no save selected,
                // so the None arm is defense in depth.
                let Some(loaded) = self.loaded.as_ref() else {
                    return iced::Task::none();
                };
                let save_sram = loaded.editor.sram(loaded);
                match self.netplay.commit(save_sram) {
                    Some(netplay::Event::MatchReady) => self.start_pvp_handoff(),
                    None => iced::Task::none(),
                }
            }
            E::OpenPath(p) => open_path(p),
            E::RevealPath(p) => reveal_path(p),
            E::CopyText(s) => iced::clipboard::write(s),
            E::CopyImage(img) => {
                copy_image_to_clipboard(img);
                iced::Task::none()
            }
            E::StartSinglePlayer => {
                let Some(loaded) = self.loaded.as_ref() else {
                    return iced::Task::none();
                };
                let save_path = loaded.save_path.clone();
                match session::spawn_singleplayer(&self.scanners, &self.config, &self.audio_binder, loaded) {
                    Ok((s, audio, save, drive)) => {
                        self.session.active = Some(Box::new(s));
                        self.session.audio_binding = audio;
                        self.session.attach_save_backup(save_path, save);
                        self.session.attach_drive_threads([drive]);
                        self.session.session_installed();
                    }
                    Err(e) => {
                        // Log-only: the Play button is gated on a fully
                        // parsed rom + save (`self.loaded`), so what's left
                        // here is core construction failing — exceptional
                        // enough that the log is the right home for it.
                        log::error!("singleplayer start failed: {e:#}");
                    }
                }
                iced::Task::none()
            }
            E::StartTraining => {
                // Training runs the *staged* save, so it needs the
                // editor's copy rather than the file on disk — which is
                // why it stays on the loaded save.
                let Some(loaded) = self.loaded.as_ref() else {
                    return iced::Task::none();
                };
                match session::spawn_training(&self.scanners, &self.config, &self.audio_binder, loaded) {
                    Ok((s, audio, drive)) => {
                        self.session.active = Some(Box::new(s));
                        self.session.audio_binding = audio;
                        self.session.attach_drive_threads([drive]);
                        self.session.session_installed();
                    }
                    Err(e) => {
                        // Log-only, same rationale as StartSinglePlayer: the
                        // button is gated on a fully parsed rom + save, so
                        // what's left is core construction failing.
                        log::error!("training start failed: {e:#}");
                    }
                }
                iced::Task::none()
            }
            E::SaveDuplicate { new_stem } => {
                if let Some(src) = self.loadout.save.clone() {
                    match duplicate_save(&src, &new_stem) {
                        Ok(dst) => {
                            log::info!("duplicated save: {} → {}", src.display(), dst.display());
                            self.loadout.save = Some(dst);
                            self.persist_selection();
                            return self.rescan_off_thread(RescanFollowup::Refresh);
                        }
                        Err(e) => log::error!("duplicate save: {e}"),
                    }
                }
                iced::Task::none()
            }
            E::SaveRename { new_stem } => {
                if let Some(src) = self.loadout.save.clone() {
                    match rename_save(&src, &new_stem) {
                        Ok(dst) => {
                            log::info!("renamed save: {} → {}", src.display(), dst.display());
                            self.loadout.save = Some(dst);
                            self.persist_selection();
                            return self.rescan_off_thread(RescanFollowup::Refresh);
                        }
                        Err(e) => log::error!("rename save: {e}"),
                    }
                }
                iced::Task::none()
            }
            E::SaveDelete => {
                if let Some(src) = self.loadout.save.clone() {
                    if let Err(e) = std::fs::remove_file(&src) {
                        log::error!("delete save: {e}");
                    } else {
                        log::info!("deleted save: {}", src.display());
                    }
                    // Clear the selection now so the picker shows
                    // "no save" while the rescan is in flight;
                    // PickFirstSave restores the first remaining
                    // entry once the scan finishes.
                    self.loadout.save = None;
                    self.persist_selection();
                    return self.rescan_off_thread(RescanFollowup::RefreshAndPickFirstSave);
                }
                iced::Task::none()
            }
            E::SaveNew { name, template, game } => {
                // The new save is created for `game` (the variant the
                // user picked), which may differ from the currently
                // selected one — so adopt it as the loadout's game too,
                // keeping game/save consistent for `refresh_loaded`.
                if let Some(template) = tabs::play::creation_template(game, &template, &self.loadout, &self.scanners) {
                    match create_new_save(&self.config.saves_path(), &name, template.as_ref()) {
                        Ok(dst) => {
                            log::info!(
                                "created new save for {:?}: {}",
                                game.family_and_variant(),
                                dst.display()
                            );
                            // Templates are only offered for patch-supported
                            // variants, so the patch normally still applies;
                            // drop it only if it somehow doesn't support the
                            // created variant.
                            if !loadout::patch_supports(&self.loadout, &self.scanners, game) {
                                self.loadout.patch = None;
                                self.loadout.patch_version = None;
                            }
                            self.loadout.game = Some(game);
                            self.loadout.family = Some(game.family_and_variant().0);
                            self.loadout.save = Some(dst);
                            // Records the save→patch association too — a
                            // template-created save is born remembering the
                            // patch it was created under.
                            self.persist_selection();
                            return self.rescan_off_thread(RescanFollowup::Refresh);
                        }
                        Err(e) => log::error!("create save: {e}"),
                    }
                }
                iced::Task::none()
            }
            E::SaveEditCommit { sram } => {
                // The edit session already staged everything into the
                // in-memory save, recomputed the checksum, and serialized
                // it — all that's left app-side is the disk write.
                // `Some(sram)` once written; the SRAM is reused below to
                // refresh a live netplay commitment.
                let saved_sram = match self.loaded.as_ref().map(|l| l.save_path.as_path()) {
                    Some(path) if !path.as_os_str().is_empty() => match std::fs::write(path, &sram) {
                        Ok(()) => {
                            log::info!("saved edited save: {}", path.display());
                            Some(sram)
                        }
                        Err(e) => {
                            log::error!("save edited save: {e}");
                            None
                        }
                    },
                    _ => None,
                };
                let Some(sram) = saved_sram else {
                    return iced::Task::none();
                };
                // If we're in a lobby and already committed (Ready), the saved
                // edits changed the save our commitment was made over — re-commit
                // so the opponent gets the new commitment (and chunks) instead of
                // a hash of our pre-edit save.
                let recommit =
                    if matches!(self.netplay.phase, netplay::Phase::Lobby { .. }) && self.netplay.local_ready() {
                        match self.netplay.commit(sram) {
                            Some(netplay::Event::MatchReady) => self.start_pvp_handoff(),
                            None => iced::Task::none(),
                        }
                    } else {
                        iced::Task::none()
                    };
                // Reconcile the scanner cache with the new on-disk bytes (the
                // in-memory loaded is already current, so refresh_loaded will
                // early-return and keep it).
                let rescan = self.rescan_off_thread(RescanFollowup::Refresh);
                iced::Task::batch([rescan, recommit])
            }
            E::SaveEditCancel => {
                // Staged edits live only in the in-memory loaded save;
                // the on-disk file and the scanner cache still hold the
                // original. Drop and rebuild loaded to revert every tab.
                self.loaded = None;
                self.refresh_loaded();
                iced::Task::none()
            }
            E::SaveEditorTask(t) => t.map(Message::Play),
        }
    }

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

    /// Start playback of a replay, downloading the patch it was
    /// recorded with if we don't have it.
    ///
    /// Under the old format every patch was already mirrored, so this
    /// could never come up; now a replay is the most likely reason to
    /// need a patch you never installed — including a version that was
    /// superseded years ago, which is exactly why the repo keeps them.
    pub(super) fn watch_replay(&mut self, p: std::path::PathBuf) -> iced::Task<Message> {
        if let Some(key) = self.replay_missing_patch(&p) {
            log::info!("replay {} needs {} {}, fetching", p.display(), key.0, key.1);
            self.pending_watch = Some(p);
            return self.install_patch(key);
        }

        let (stats_job, stats_task) = self.replay_stats_takeover(&p);
        match session::build_playback(&self.scanners, &self.config, &self.audio_binder, &p, stats_job) {
            Ok((s, audio, threads)) => {
                self.session.replay_path = Some(p.clone());
                self.session.active = Some(Box::new(s));
                // A queue handoff carries the speed of the replay it
                // replaced; a plain Watch has nothing pending and starts at
                // realtime.
                if let Some(factor) = self.queue_carry_speed.take() {
                    if let Some(s) = self.session.active.as_ref() {
                        s.set_speed(factor);
                    }
                }
                self.session.audio_binding = audio;
                self.session.attach_drive_threads(threads);
                self.session.session_installed();
            }
            // The dropped job closes its stream, whose completion
            // message clears the tab's pending marker — a later
            // focus retries the analysis.
            Err(e) => log::warn!("failed to play replay {}: {e}", p.display()),
        }
        stats_task
    }

    /// The first patch a replay needs that isn't installed but is
    /// offered by the repo. `None` when playback can go ahead — or when
    /// the patch is one we could never get, which fails as before.
    fn replay_missing_patch(&self, path: &std::path::Path) -> Option<patch::VersionKey> {
        // The replay scanner already parsed every metadata header, so
        // this costs a lookup rather than a decode.
        let wanted: Vec<(String, String)> = {
            let replays = self.scanners.replays.read();
            let scanned = replays.iter().find(|r| r.path == path)?;
            [scanned.metadata.side(0), scanned.metadata.side(1)]
                .into_iter()
                .flatten()
                .filter_map(|s| s.game_info.as_ref()?.patch.as_ref())
                .map(|p| (p.name.clone(), p.version.clone()))
                .collect()
        };
        let patches = self.scanners.patches.read();
        wanted.into_iter().find_map(|(name, version)| {
            let version = semver::Version::parse(&version).ok()?;
            // Only worth waiting on something the repo actually offers;
            // a patch nobody publishes fails at playback as it always did.
            (!patches.is_installed(&name, &version) && patches.entry(&name, &version).is_some())
                .then_some((name, version))
        })
    }

    pub(super) fn update_replays(&mut self, msg: tabs::replays::Message) -> iced::Task<Message> {
        // An analysis that ran to completion on its own — whether from
        // the tab's worker or a playback session's prefetcher — reports
        // in as this message; drop its cancel handles.
        let finished = match &msg {
            tabs::replays::Message::HpStatsLoaded(p, _) => Some(p.clone()),
            _ => None,
        };
        let effect = self.replays.update(msg, &self.scanners, &self.config);
        if let Some(p) = finished {
            self.replay_analysis_jobs.remove(&p);
        }
        // Pure state mutations live in the tab module; only side
        // effects (clipboard, OS open, session host handoff,
        // file dialog, export task spawn) come back here as an
        // Effect for the App to interpret.
        let Some(effect) = effect else {
            return iced::Task::none();
        };
        use tabs::replays::Effect as E;
        match effect {
            E::OpenPath(p) => open_path(p),
            E::RevealPath(p) => reveal_path(p),
            E::Watch(p) => self.watch_replay(p),
            E::CancelPatchDownload(key) => self.cancel_download(key),
            // The dropped job closes its stream, whose completion
            // message clears the tab's pending marker — a later
            // focus retries the analysis.
            E::CopyText(s) => iced::clipboard::write(s),
            E::CopyImage(img) => {
                copy_image_to_clipboard(img);
                iced::Task::none()
            }
            E::OpenExportSaveDialog {
                replay: replay_path,
                lossless,
            } => {
                let replay_for_msg = replay_path.clone();
                self.export_save_dialog(replay_path, lossless, "", move |output| {
                    tabs::replays::Message::Export(tabs::replays::ExportMessage::Start {
                        replay: replay_for_msg.clone(),
                        output,
                    })
                })
            }
            E::StartExport {
                replay,
                output,
                settings,
                rounds,
                clip,
            } => self
                .spawn_replay_render(replay, output, settings, rounds, clip)
                .map(Message::Replays),
            E::AnalyzeReplay(path) => {
                // Full re-simulation of the replay — seconds of CPU on a
                // blocking worker, with per-tick progress streamed back for
                // the detail pane's bar. The final message clears the tab's
                // pending marker either way; failure (missing ROM/patch,
                // undecodable) just means no chart, retried on re-focus.
                // `replay_stats_takeover` can cancel the whole job mid-pass
                // when a playback session's prefetcher takes the work over.
                let scanners = self.scanners.clone();
                let patches_path = self.config.patches_path();
                let cache_path = self.config.cache_path();
                let replays_path = self.config.replays_path();
                let (progress_tx, progress_rx) =
                    futures::channel::mpsc::unbounded::<tango_match::analysis::MatchStats>();
                let done: std::sync::Arc<std::sync::Mutex<Option<tango_match::analysis::MatchStats>>> =
                    std::sync::Arc::new(std::sync::Mutex::new(None));
                let done_worker = done.clone();
                let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let cancel_worker = cancel.clone();
                let p = path.clone();
                tokio::task::spawn_blocking(move || {
                    // Live preview cadence: each report clones the folded
                    // rounds and folds the round in progress, and each one
                    // becomes a chart rebuild on the UI thread — so pace it
                    // to the display, not to the simulation. ~30/s keeps
                    // the growth reading as continuous motion (at 100ms
                    // the sim advances a visible chunk between frames).
                    const PREVIEW_EVERY: std::time::Duration = std::time::Duration::from_millis(33);
                    let mut last_preview = std::time::Instant::now();
                    let result = replays::compute_and_cache_match_stats(
                        scanners,
                        patches_path,
                        cache_path,
                        replays_path,
                        p.clone(),
                        &mut |_d, _t, builder| {
                            let now = std::time::Instant::now();
                            if now.duration_since(last_preview) < PREVIEW_EVERY {
                                return;
                            }
                            last_preview = now;
                            let _ = progress_tx.unbounded_send(builder.snapshot());
                        },
                        &cancel_worker,
                    )
                    .map_err(|e| {
                        if cancel_worker.load(std::sync::atomic::Ordering::Relaxed) {
                            log::debug!("replay analysis cancelled for {}", p.display());
                        } else {
                            log::warn!("replay analysis failed for {}: {e}", p.display());
                        }
                    })
                    .ok();
                    // Park the result before the sender (captured by the
                    // closure above) drops and closes the channel — the
                    // chained completion message below reads it on close.
                    *done_worker.lock().unwrap() = result;
                });
                use futures::StreamExt;
                let progress_path = path.clone();
                let loaded_path = path.clone();
                let stream = progress_rx
                    .map(move |partial| tabs::replays::Message::HpStatsPartial(progress_path.clone(), partial))
                    .chain(futures::stream::once(async move {
                        tabs::replays::Message::HpStatsLoaded(loaded_path, done.lock().unwrap().take())
                    }));
                let (task, handle) = iced::Task::stream(stream).map(Message::Replays).abortable();
                self.replay_analysis_jobs.insert(path, (cancel, handle));
                task
            }
            E::SaveEditorTask(t) => t.map(Message::Replays),
        }
    }

    /// Open the native Save-File dialog for a replay's rendered
    /// video and dispatch `make_msg(picked_path)` into the replays-tab
    /// message stream — or NoOp on dismissal, keeping any open form
    /// untouched since no job ever started. `lossless` selects the
    /// default extension and filter, by asking the exporter which
    /// container that setting writes rather than restating the mapping.
    /// `stem_suffix` is appended to the replay's file stem (the clip
    /// flow names its file apart so it doesn't collide with a
    /// whole-replay export's default).
    pub(super) fn export_save_dialog(
        &self,
        replay_path: std::path::PathBuf,
        lossless: bool,
        stem_suffix: &str,
        make_msg: impl Fn(std::path::PathBuf) -> tabs::replays::Message + Send + Sync + 'static,
    ) -> iced::Task<Message> {
        let container = crate::replay_render::container(lossless);
        let ext = container.extension();
        let filter_name = match container {
            encoder_facade::Container::Mp4 => "MP4",
            encoder_facade::Container::Matroska => "Matroska",
        };
        let stem = replay_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "replay".to_string());
        let default_name = format!("{stem}{stem_suffix}.{ext}");
        let initial_dir = replay_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.config.replays_path());
        iced::Task::perform(
            async move {
                rfd::AsyncFileDialog::new()
                    .set_directory(&initial_dir)
                    .set_file_name(&default_name)
                    .add_filter(filter_name, &[ext])
                    .save_file()
                    .await
                    .map(|h| h.path().to_path_buf())
            },
            move |maybe_path| match maybe_path {
                Some(output) => make_msg(output),
                None => tabs::replays::Message::NoOp,
            },
        )
        .map(Message::Replays)
    }

    /// Spawn the crate::replay_render task with a progress
    /// callback that forwards into the replays-tab message
    /// stream. The user-picked output path + form snapshot come
    /// from the tab module's `ExportStart` effect.
    fn spawn_replay_render(
        &mut self,
        replay_path: std::path::PathBuf,
        output_path: std::path::PathBuf,
        user_settings: tabs::replays::ExportSettings,
        rounds_mask: Vec<bool>,
        clip: Option<crate::replay_render::Clip>,
    ) -> iced::Task<tabs::replays::Message> {
        // Decode just enough of the replay to get both sides' game
        // registrations + raw ROM bytes. Failures show up as a
        // Done(Err) status — same as runtime errors below.
        let prep = (|| -> anyhow::Result<ExportPrep> {
            let f = std::fs::File::open(&replay_path)?;
            let replay = tango_replay::Replay::decode(f)?;
            // The export re-simulates both sides from the recorded
            // inputs, so each side's ROM must be the exact patched ROM
            // that was used when the match was recorded — otherwise the
            // re-sim desyncs. Mirror `session::build_playback`'s
            // `resolve_rom`: apply the side's patch from disk before
            // handing the bytes to export.
            let patches_path = self.config.patches_path();
            let resolve = |side: Option<&tango_replay::metadata::Side>| -> anyhow::Result<(
                crate::library::rom::GameRef,
                Vec<u8>,
            )> {
                let gi = side
                    .and_then(|s| s.game_info.as_ref())
                    .ok_or_else(|| anyhow::anyhow!("replay side missing game info"))?;
                // The export re-sim is as version-sensitive as playback,
                // so this resolve also enforces the family's replay
                // version.
                let entry = crate::library::game::find_for_replay_side(gi)?;
                let rom = self
                    .scanners
                    .roms
                    .read()
                    .get(&entry)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("rom for {:?} not scanned", entry.family_and_variant()))?;
                let rom = if let Some(patch_info) = gi.patch.as_ref() {
                    let v = semver::Version::parse(&patch_info.version)?;
                    patch::apply_patch(crate::library::storage(), &rom, entry, &patches_path, &patch_info.name, &v)?
                } else {
                    rom
                };
                Ok((entry, rom))
            };
            let (p1_game, p1_rom) = resolve(replay.metadata.side(0))?;
            let (p2_game, p2_rom) = resolve(replay.metadata.side(1))?;
            Ok(ExportPrep {
                games: [p1_game, p2_game],
                roms: [p1_rom, p2_rom],
                replay,
            })
        })();
        let prep = match prep {
            Ok(p) => p,
            Err(e) => {
                let mut job = tabs::replays::ExportJob::new(output_path.clone());
                job.result = Some(Err(format!("{e}")));
                self.replays.per.entry(replay_path).or_default().job = Some(job);
                return iced::Task::none();
            }
        };

        if clip.is_none() && !rounds_mask.iter().any(|b| *b) {
            let mut job = tabs::replays::ExportJob::new(output_path.clone());
            job.result = Some(Err("no rounds selected for export".to_string()));
            self.replays.per.entry(replay_path).or_default().job = Some(job);
            return iced::Task::none();
        }

        // Chapter titles for the output container, one per round in
        // mask order — resolved here because the export thread has no
        // access to the locale bundle.
        let title_count = clip
            .as_ref()
            .map(|c| c.round_marks.len() + 1)
            .unwrap_or(rounds_mask.len());
        let round_titles: Vec<String> = (0..title_count)
            .map(|i| crate::t!(&self.config.language, "session-results-round", number = (i + 1) as i64))
            .collect();

        let (progress_tx, progress_rx) = futures::channel::mpsc::unbounded::<(usize, usize)>();
        let done_arc: std::sync::Arc<std::sync::Mutex<Option<Result<std::path::PathBuf, String>>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let done_arc_thread = done_arc.clone();
        let output_for_thread = output_path.clone();
        // The ExportJob the tab module created in `ExportStart` already
        // owns the canceller. Clone it for the thread; the tab's
        // Cancel button calls `kill()` on its copy.
        let canceller_thread = self
            .replays
            .per
            .get(&replay_path)
            .and_then(|e| e.job.as_ref())
            .map(|j| j.canceller.clone())
            .unwrap_or_default();
        // Run the export on a dedicated OS thread. The export is fully
        // synchronous (std::process ffmpeg subprocesses, no async), so
        // it lives entirely outside the iced/tokio worker pool — no
        // shared-runtime starvation regardless of how tight the
        // export inner loop runs.
        std::thread::Builder::new()
            .name("replay-export".to_string())
            .spawn(move || {
                let ExportPrep { games, roms, replay } = prep;
                // scale == 0 is the slider's lossless stop (RGB-domain
                // H.264, no upscale); 1..=10 is a lossy render at that
                // nearest-neighbor upscale. The exporter picks the
                // codecs and container to match.
                let scale_arg = if user_settings.scale == 0 {
                    None
                } else {
                    Some(user_settings.scale as usize)
                };
                // Clone the sender into the callback. The original
                // `progress_tx` stays alive on the thread scope until
                // *after* `done_arc_thread` is set; otherwise the
                // futures channel closes the moment `cb` (and thus the
                // moved sender) is dropped, the iced stream wakes up,
                // sees `None`, races to read `done_arc` while it's
                // still unset, and reports "export task ended without
                // result".
                let cb_tx = progress_tx.clone();
                let cb = move |current: usize, total: usize| {
                    let _ = cb_tx.unbounded_send((current, total));
                };
                let local_player = replay.local_player_index as usize;
                // The replay's input stream is already absolute pair
                // order — just widen into the seam's vocabulary.
                let inputs: Vec<[tango_match::HostInput; 2]> = replay
                    .inputs
                    .iter()
                    .map(|&row| {
                        row.map(|input| tango_match::HostInput {
                            keys: input.keys as u32,
                            touch: input.touch.map(|(x, y)| (x as u16, y as u16)),
                        })
                    })
                    .collect();
                let total_ticks = inputs.len() as u32;
                // The same boot the player uses, through the local
                // seat's own engine door — which engine that is stays
                // the game's business.
                let backend = games[local_player].pvp;
                let config = tango_match::ReplayConfig {
                    roms,
                    saves: replay.srams.clone(),
                    inputs: std::sync::Arc::new(inputs),
                    rng_seed: replay.rng_seed,
                    rtc: replay.rtc_time(),
                    match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
                    local_player,
                    peer_rom: tango_match::PeerRom {
                        code: *games[1 - local_player].rom_code,
                        revision: games[1 - local_player].revision,
                    },
                    want_stats: false,
                    want_round_marks: false,
                    disable_bgm: user_settings.disable_bgm,
                };
                // A whole-replay export is the degenerate clip covering
                // the full stream, with the file's own round marks; the
                // player's clip brings its marks (the session's
                // boundaries, which cover marker-less recordings too)
                // and a mask selecting every round — its gate is the
                // span. A snapshot restore would erase the priming-time
                // BGM-disable poke, so muted renders re-sim from boot.
                let (mut clip, rounds_mask) = match clip {
                    Some(c) => {
                        let all_rounds = vec![true; c.round_marks.len() + 1];
                        (c, all_rounds)
                    }
                    None => (
                        crate::replay_render::Clip {
                            start: 0,
                            end: total_ticks,
                            snapshot: None,
                            round_marks: replay.round_starts.iter().skip(1).map(|&i| i as u32).collect(),
                        },
                        rounds_mask,
                    ),
                };
                if user_settings.disable_bgm {
                    clip.snapshot = None;
                }
                let request = crate::replay_render::Request {
                    backend,
                    config,
                    rounds_mask: &rounds_mask,
                    round_titles: &round_titles,
                    clip: &clip,
                    scale: scale_arg,
                    twosided: user_settings.twosided,
                };
                let result = crate::replay_render::render(request, &output_for_thread, &canceller_thread, cb)
                    .map(|()| output_for_thread)
                    .map_err(|e| format!("{e}"));
                *done_arc_thread.lock().unwrap() = Some(result);
                // `progress_tx` drops here, closing the channel, which
                // signals the iced stream to read `done_arc` — which is
                // now safely set above.
                drop(progress_tx);
            })
            .expect("spawn replay-export thread");

        // Drain progress + a synthetic final ExportFinished from
        // the same stream. We poll done_arc whenever the channel
        // drains so the finished message arrives even if the
        // export errored before sending any progress.
        let replay_for_stream = replay_path;
        let stream = futures::stream::unfold(
            (progress_rx, done_arc, replay_for_stream, false),
            |(mut rx, done, replay, finished_sent)| async move {
                use futures::StreamExt;
                if finished_sent {
                    return None;
                }
                tokio::select! {
                    biased;
                    next = rx.next() => match next {
                        Some((c, t)) => Some((
                            tabs::replays::Message::Export(tabs::replays::ExportMessage::Progress {
                                replay: replay.clone(),
                                completed: c,
                                total: t,
                            }),
                            (rx, done, replay, false),
                        )),
                        None => {
                            // Channel closed — the task is done.
                            // Pull the result out of done_arc.
                            let r = done.lock().unwrap().take().unwrap_or_else(|| {
                                Err("export task ended without result".to_string())
                            });
                            Some((
                                tabs::replays::Message::Export(tabs::replays::ExportMessage::Finished {
                                    replay: replay.clone(),
                                    result: r,
                                }),
                                (rx, done, replay, true),
                            ))
                        }
                    }
                }
            },
        );
        iced::Task::stream(stream)
    }

    pub(super) fn update_settings(&mut self, msg: tabs::settings::Message) -> iced::Task<tabs::settings::Message> {
        // UpdateNow is a side effect (kicks the installer +
        // exits the process) not a config change; intercept
        // before delegating to settings::State::update.
        if matches!(msg, tabs::settings::Message::UpdateNow) {
            self.updater.finish_update();
            return iced::Task::none();
        }
        // The data-folder "Change…" button opens a native folder picker. It's
        // async, so intercept here and surface the result as DataFolderPicked.
        if matches!(msg, tabs::settings::Message::OpenDataFolderPicker) {
            let initial = self.config.data_path.clone();
            return iced::Task::perform(
                async move {
                    rfd::AsyncFileDialog::new()
                        .set_directory(&initial)
                        .pick_folder()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                tabs::settings::Message::DataFolderPicked,
            );
        }
        use tabs::settings::ConfigChange as C;
        let Some(change) = self.settings.update(msg) else {
            return iced::Task::none();
        };
        match change {
            C::Language(l) => self.config.language = l,
            C::Nickname(s) => self.config.nickname = if s.is_empty() { None } else { Some(s) },
            C::StreamerMode(b) => self.config.streamer_mode = b,
            C::MatchmakingEndpoint(s) => self.config.matchmaking_endpoint = s,
            C::RelayMode(m) => self.config.relay_mode = m,
            C::PatchRepo(s) => self.config.patch_repo = s,
            C::DataPath(path) => {
                self.config.data_path = path;
                // Make sure the standard subfolders exist in the new location
                // so scanners and writers have somewhere to go.
                for dir in [
                    self.config.roms_path(),
                    self.config.saves_path(),
                    self.config.patches_path(),
                    self.config.replays_path(),
                    self.config.logs_path(),
                ] {
                    let _ = std::fs::create_dir_all(&dir);
                }
                // Re-scan so the new folder's contents show up immediately, and
                // re-point the patch autoupdater at the new patches folder
                // (it captured the old path at construction). The self-updater
                // cache and log file follow the new path on next launch.
                let listings = futures::executor::block_on(Scanners::list(&self.config));
                self.scanners.rescan(&self.config, &listings);
                self.patch_autoupdater = crate::library::autoupdate::Autoupdater::new(
                    self.config.patches_path(),
                    self.config.patch_repo.clone(),
                    self.scanners.patches.clone(),
                );
                if self.config.enable_patch_autoupdate {
                    self.patch_autoupdater.start();
                }
            }
            C::PatchAutoupdate(b) => {
                self.config.enable_patch_autoupdate = b;
                if b {
                    self.patch_autoupdater.start();
                } else {
                    self.patch_autoupdater.stop();
                }
            }
            C::VideoFilter(s) => self.config.video_filter = s,
            C::FractionalScaling(b) => self.config.fractional_scaling = b,
            C::DsScreenStacking(s) => self.config.ds_screen_stacking = s,
            C::DsPrimaryScreen(s) => self.config.ds_primary_screen = s,
            C::HideEmulatorBorder(b) => self.config.hide_emulator_border = b,
            C::Fullscreen(b) => {
                self.config.fullscreen = b;
                self.persist_config();
                let mode = if b {
                    iced::window::Mode::Fullscreen
                } else {
                    iced::window::Mode::Windowed
                };
                return iced::window::latest().and_then(move |id| iced::window::set_mode(id, mode));
            }
            C::UiScale(s) => self.config.ui_scale = s,
            C::Resolution(w, h) => {
                // Picking a windowed resolution implies leaving
                // fullscreen — iced's Mode::Fullscreen is
                // borderless and always covers the monitor, so a
                // sub-monitor resize has no visible effect until
                // we drop back to Windowed. Do both atomically.
                let was_fullscreen = self.config.fullscreen;
                self.config.fullscreen = false;
                self.config.last_window_size = Some((w, h));
                self.persist_config();
                let size = iced::Size::new(w, h);
                return iced::window::latest().and_then(move |id| {
                    let resize = iced::window::resize(id, size);
                    if was_fullscreen {
                        iced::window::set_mode(id, iced::window::Mode::Windowed).chain(resize)
                    } else {
                        resize
                    }
                });
            }
            C::EnableUpdater(b) => {
                self.config.enable_updater = b;
                self.updater.set_enabled(b);
            }
            C::AllowPrereleaseUpgrades(b) => {
                // Sampled by Updater at start; takes effect on
                // next launch. Config change still gets
                // persisted so it survives the restart.
                self.config.allow_prerelease_upgrades = b;
            }
            C::Volume(v) => {
                let v = v.clamp(0.0, 1.0);
                self.config.volume = v;
                self.audio_binder.set_volume(v);
            }
            // Sampled by spawn_pvp at match start; nothing live to poke.
            C::DisableBgmInPvp(b) => self.config.disable_bgm_in_pvp = b,
            // Sampled when the next PvP session is installed
            // (Message::PvpSessionBuilt); nothing live to poke.
            C::ShowOpponentSetup(b) => self.config.show_opponent_setup = b,
            C::Theme(t) => self.config.theme = t,
            C::Accent(a) => self.config.accent = a,
            C::AddInputBinding(slot, binding) => {
                let bindings = self.config.input_mapping.slot_mut(slot);
                // Avoid dupes — a single binding could be added
                // twice if the user hits the same key fast.
                if !bindings.contains(&binding) {
                    bindings.push(binding);
                }
            }
            C::RemoveInputBinding(slot, idx) => {
                let bindings = self.config.input_mapping.slot_mut(slot);
                if idx < bindings.len() {
                    bindings.remove(idx);
                }
            }
            C::ResetInputBindings => {
                self.config.input_mapping = input::Mapping::default();
            }
        }
        self.persist_config();
        iced::Task::none()
    }

    pub(super) fn update_welcome(&mut self, msg: tabs::welcome::Message) -> iced::Task<Message> {
        use tabs::welcome::Message as M;
        match msg {
            M::NicknameChanged(s) => {
                self.welcome.nickname_draft = s;
                iced::Task::none()
            }
            M::Continue => {
                if let Some(nickname) = self.welcome.finalize_nickname() {
                    self.config.nickname = Some(nickname);
                    self.persist_config();
                }
                iced::Task::none()
            }
            M::LanguageSelected(l) => {
                self.config.language = l;
                self.persist_config();
                iced::Task::none()
            }
            M::OpenRomsFolder => {
                let p = self.config.roms_path();
                let _ = std::fs::create_dir_all(&p);
                if let Err(e) = open::that(&p) {
                    log::error!("open roms folder: {e}");
                }
                iced::Task::none()
            }
            M::RescanRoms => self.rescan_off_thread(RescanFollowup::Refresh),
        }
    }
}

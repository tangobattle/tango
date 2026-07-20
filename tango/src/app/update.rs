use super::*;

impl App {
    /// Apply a loadout-strip message (from either tab) to the shared
    /// App-level [`loadout::Loadout`] and run the selection-change
    /// follow-ups. The caller batches a lobby settings-resend after
    /// this, so a mid-lobby save/patch switch reaches the peer.
    pub(super) fn update_loadout(&mut self, msg: loadout::Message) -> iced::Task<Message> {
        let Some(effect) = self.loadout.update(msg, &self.scanners, &self.config) else {
            return iced::Task::none();
        };
        match effect {
            loadout::Effect::SelectionChanged => {
                self.refresh_loaded();
                self.persist_selection();
                // Game might have just changed — if so, the lobby
                // picker should show this game's default match
                // type (Triple where supported) instead of the
                // last game's pick.
                self.apply_default_match_type();
                iced::Task::none()
            }
        }
    }

    pub(super) fn update_play(&mut self, msg: tabs::play::Message) -> iced::Task<Message> {
        let Some(effect) = self
            .play
            .update(msg, &self.scanners, &self.config, self.loaded.as_ref(), &self.loadout)
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
                let msg = match ident {
                    netplay::LinkIdent::Matchmaking(link_code) => netplay::Message::Connect {
                        link_code,
                        endpoint: self.config.matchmaking_endpoint.clone(),
                        use_relay: self.config.relay_mode.use_relay(),
                        identity: self.identity.clone(),
                    },
                    netplay::LinkIdent::Direct(role) => netplay::Message::ConnectDirect { role },
                };
                let task = self.netplay.update(msg).map(Message::Netplay);
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
            E::Netplay(m) => {
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
                if let netplay::Message::SetMatchType(_) = &m {
                    if let Some(g) = self.loadout.game {
                        let (fam, var) = g.family_and_variant();
                        self.netplay.lobby.default_mt_for_game = Some((fam.to_string(), var));
                    }
                }
                // Remember the blind-setup choice so the next lobby
                // (this session or a future launch) defaults to it.
                if let netplay::Message::SetBlindSetup(v) = &m {
                    self.config.last_blind_setup = *v;
                    self.persist_config();
                }
                self.netplay.update(m).map(Message::Netplay)
            }
            E::ReadyWithSave => {
                // View-time gating disables the Ready button when
                // no save is loaded, so this is just defense in
                // depth — fall through silently if reached.
                let Some(loaded) = self.loaded.as_ref() else {
                    return iced::Task::none();
                };
                let save_sram = loaded.save.to_sram_dump();
                self.netplay
                    .update(netplay::Message::Commit { save_sram })
                    .map(Message::Netplay)
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
                match session::spawn_singleplayer(&self.scanners, &self.config, &self.audio_binder, loaded) {
                    Ok(s) => {
                        self.session.active = Some(Box::new(s));
                        self.session.wake_controls();
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
            E::Edit(edit) => {
                // Stage one edit into the in-memory loaded save. The UI
                // reads `loaded.save` directly, so the change shows
                // immediately; nothing is written to disk until Save.
                if let Some(loaded) = self.loaded.as_mut() {
                    crate::save_edit::apply_edit(loaded, edit);
                }
                iced::Task::none()
            }
            E::SaveEditCommit => {
                // `Some(sram)` once the edited save is written; the SRAM is
                // reused below to refresh a live netplay commitment.
                let saved_sram = if let Some(loaded) = self.loaded.as_mut() {
                    if loaded.save_path.as_os_str().is_empty() {
                        None
                    } else {
                        // Every staged edit already keeps its view's derived
                        // caches in sync as it's applied — the anti-cheat
                        // folder/patch-card mirror (chips, patch cards) and
                        // the materialized WRAM caches (navicust, auto-battle
                        // data). So commit only has to recompute the whole-SRAM
                        // checksum and write once.
                        loaded.save.rebuild_checksum();
                        // Refresh the baked Navi-view image from the updated
                        // save (commit keeps the in-memory Loaded, so without
                        // this the read-only grid lags until reselection).
                        loaded.rebuild_navicust_render();
                        let sram = loaded.save.to_sram_dump();
                        let path = loaded.save_path.clone();
                        match std::fs::write(&path, &sram) {
                            Ok(()) => {
                                log::info!("saved edited save: {}", path.display());
                                Some(sram)
                            }
                            Err(e) => {
                                log::error!("save edited save: {e}");
                                None
                            }
                        }
                    }
                } else {
                    None
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
                        self.netplay
                            .update(netplay::Message::Commit { save_sram: sram })
                            .map(Message::Netplay)
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
            E::SaveViewTask(t) => t.map(Message::Play),
        }
    }

    pub(super) fn update_patches(&mut self, msg: tabs::patches::Message) -> iced::Task<Message> {
        let Some(effect) = self.patches.update(msg, &self.scanners, &self.config) else {
            return iced::Task::none();
        };
        use tabs::patches::Effect as E;
        match effect {
            E::OpenPath(s) => open_path(s),
            E::UpdateRescan => self.rescan_off_thread(RescanFollowup::Refresh),
            E::StartUpdate { url, root } => iced::Task::perform(
                async move { patch::update(url, root).await.map_err(|e| e.to_string()) },
                tabs::patches::Message::UpdateFinished,
            )
            .map(Message::Patches),
            E::ToggleFavorite(name) => {
                if !self.config.favorite_patches.remove(&name) {
                    self.config.favorite_patches.insert(name);
                }
                self.persist_config();
                iced::Task::none()
            }
        }
    }

    pub(super) fn update_replays(&mut self, msg: tabs::replays::Message) -> iced::Task<Message> {
        // A replay being watched follows its analysis live: whether the
        // stats come from the tab's worker or the session's own
        // prefetcher, they arrive as these progress messages, and the
        // strip re-cooks straight from the carried stats — no detour
        // through the tab's chart cache, which only holds the selected
        // replay (a results-screen watch may not be selected at all).
        let stats_progress = match &msg {
            tabs::replays::Message::HpStatsPartial(p, partial) => Some((p.clone(), Some(partial.clone()), false)),
            tabs::replays::Message::HpStatsLoaded(p, stats) => Some((p.clone(), stats.clone(), true)),
            _ => None,
        };
        let effect = self.replays.update(msg, &self.scanners, &self.config);
        if let Some((p, stats, is_final)) = stats_progress {
            if is_final {
                // Worker ran to completion on its own — drop its cancel
                // handles.
                self.replay_analysis_jobs.remove(&p);
            }
            if self.session.replay_chart.as_ref().is_some_and(|c| c.path == p) {
                if let (Some(stats), Some(s)) = (&stats, self.session.active_as::<session::replay::ReplaySession>()) {
                    let rounds = widgets::cook_hp_rounds(stats, [None, None], Some(&planned_spans(s))).0;
                    self.session.replay_chart = Some(session::ReplayChart { path: p, rounds });
                }
            }
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
            E::Watch(p) => {
                let (stats_job, stats_task) = self.replay_stats_takeover(&p);
                match session::build_playback(&self.scanners, &self.config, &self.audio_binder, &p, stats_job) {
                    Ok(s) => {
                        self.session.replay_chart = Some(self.replay_chart_for(&p, &s));
                        self.session.active = Some(Box::new(s));
                        self.session.wake_controls();
                    }
                    // The dropped job closes its stream, whose completion
                    // message clears the tab's pending marker — a later
                    // focus retries the analysis.
                    Err(e) => log::warn!("failed to play replay {}: {e}", p.display()),
                }
                stats_task
            }
            E::CopyText(s) => iced::clipboard::write(s),
            E::CopyImage(img) => {
                copy_image_to_clipboard(img);
                iced::Task::none()
            }
            E::OpenExportSaveDialog {
                replay: replay_path,
                lossless,
            } => {
                // Lossless export muxes libx264rgb + flac, which .mkv holds
                // natively; scaled export targets the more portable .mp4.
                let ext = if lossless { "mkv" } else { "mp4" };
                let filter_name = if lossless { "Matroska" } else { "MP4" };
                let stem = replay_path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "replay".to_string());
                let default_name = format!("{stem}.{ext}");
                let initial_dir = replay_path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| self.config.replays_path());
                let replay_for_msg = replay_path;
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
                        Some(output) => tabs::replays::Message::Export(tabs::replays::ExportMessage::Start {
                            replay: replay_for_msg.clone(),
                            output,
                        }),
                        // User dismissed the dialog without picking — keep
                        // the form open and untouched. ExportDismiss would
                        // also close the panel, which is wrong here since
                        // no job ever started.
                        None => tabs::replays::Message::NoOp,
                    },
                )
                .map(Message::Replays)
            }
            E::StartExport {
                replay,
                output,
                settings,
                rounds,
            } => self
                .spawn_replay_export(replay, output, settings, rounds)
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
                let (progress_tx, progress_rx) = futures::channel::mpsc::unbounded::<tango_pvp::analysis::MatchStats>();
                let done: std::sync::Arc<std::sync::Mutex<Option<tango_pvp::analysis::MatchStats>>> =
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
                            let _ = progress_tx.unbounded_send(builder.preview());
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
            E::SaveViewTask(t) => t.map(Message::Replays),
        }
    }

    /// Spawn the crate::replay_export task with a progress
    /// callback that forwards into the replays-tab message
    /// stream. The user-picked output path + form snapshot come
    /// from the tab module's `ExportStart` effect.
    fn spawn_replay_export(
        &mut self,
        replay_path: std::path::PathBuf,
        output_path: std::path::PathBuf,
        user_settings: tabs::replays::ExportSettings,
        rounds_mask: Vec<bool>,
    ) -> iced::Task<tabs::replays::Message> {
        // Decode just enough of the replay to get both sides' game
        // registrations + raw ROM bytes. Failures show up as a
        // Done(Err) status — same as runtime errors below.
        let prep = (|| -> anyhow::Result<ExportPrep> {
            let f = std::fs::File::open(&replay_path)?;
            let replay = tango_pvp::replay::Replay::decode(f)?;
            // The export re-simulates both sides from the recorded
            // inputs, so each side's ROM must be the exact patched ROM
            // that was used when the match was recorded — otherwise the
            // re-sim desyncs. Mirror `session::build_playback`'s
            // `resolve_rom`: apply the side's patch from disk before
            // handing the bytes to export.
            let patches_path = self.config.patches_path();
            let resolve = |side: Option<&tango_pvp::replay::metadata::Side>| -> anyhow::Result<(
                crate::library::rom::GameRef,
                Vec<u8>,
            )> {
                let gi = side
                    .and_then(|s| s.game_info.as_ref())
                    .ok_or_else(|| anyhow::anyhow!("replay side missing game info"))?;
                let variant = u8::try_from(gi.rom_variant)?;
                let entry = crate::library::game::find_by_family_and_variant(&gi.rom_family, variant)
                    .ok_or_else(|| {
                        anyhow::anyhow!("unknown rom {}/{}", gi.rom_family, variant)
                    })?;
                let rom = self
                    .scanners
                    .roms
                    .read()
                    .get(&entry)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("rom for {:?} not scanned", entry.family_and_variant()))?;
                let rom = if let Some(patch_info) = gi.patch.as_ref() {
                    let v = semver::Version::parse(&patch_info.version)?;
                    patch::apply_patch_from_disk(&rom, entry, &patches_path, &patch_info.name, &v)?
                } else {
                    rom
                };
                Ok((entry, rom))
            };
            let (local_game, local_rom) = resolve(replay.metadata.local_side.as_ref())?;
            let (remote_game, remote_rom) = resolve(replay.metadata.remote_side.as_ref())?;
            Ok(ExportPrep {
                local_game,
                local_rom,
                remote_game,
                remote_rom,
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

        if !rounds_mask.iter().any(|b| *b) {
            let mut job = tabs::replays::ExportJob::new(output_path.clone());
            job.result = Some(Err("no rounds selected for export".to_string()));
            self.replays.per.entry(replay_path).or_default().job = Some(job);
            return iced::Task::none();
        }

        // Chapter titles for the output container, one per round in
        // mask order — resolved here because the export thread has no
        // access to the locale bundle.
        let round_titles: Vec<String> = (0..rounds_mask.len())
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
                let ExportPrep {
                    local_game,
                    local_rom,
                    remote_game,
                    remote_rom,
                    replay,
                } = prep;
                // scale == 0 is the slider's lossless stop → libx264rgb
                // -qp 0 (RGB-domain lossless); 1..=10 → libx264 + nearest
                // upscale at that factor. `default_with_scale` builds the
                // ffmpeg flags accordingly.
                let scale_arg = if user_settings.scale == 0 {
                    None
                } else {
                    Some(user_settings.scale as usize)
                };
                let mut settings = crate::replay_export::Settings::default_with_scale(scale_arg);
                settings.disable_bgm = user_settings.disable_bgm;
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
                // Orient the replay's local/remote pairs back to absolute
                // pair order — same contract as playback and analysis.
                let local_player = replay.local_player_index as usize;
                let inputs: Vec<[u32; 2]> = replay
                    .inputs
                    .iter()
                    .map(|(local, remote)| {
                        let mut keys = [0u32; 2];
                        keys[local_player] = local.joyflags as u32;
                        keys[1 - local_player] = remote.joyflags as u32;
                        keys
                    })
                    .collect();
                let (roms, saves, support): ([Vec<u8>; 2], [Vec<u8>; 2], _) = if local_player == 0 {
                    (
                        [local_rom, remote_rom],
                        [replay.local_sram.clone(), replay.remote_sram.clone()],
                        [local_game.pvp, remote_game.pvp],
                    )
                } else {
                    (
                        [remote_rom, local_rom],
                        [replay.remote_sram.clone(), replay.local_sram.clone()],
                        [remote_game.pvp, local_game.pvp],
                    )
                };
                let config = tango_pvp::playback::BootConfig {
                    roms,
                    saves,
                    support,
                    match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
                    rng_seed: replay.rng_seed,
                    rtc: replay.rtc_time(),
                    disable_bgm: user_settings.disable_bgm,
                };
                let result = crate::replay_export::export(
                    &config,
                    &inputs,
                    local_player,
                    &rounds_mask,
                    &round_titles,
                    &output_for_thread,
                    &settings,
                    &canceller_thread,
                    cb,
                    user_settings.twosided,
                )
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
                self.scanners.rescan(&self.config);
                self.patch_autoupdater = patch::Autoupdater::new(
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

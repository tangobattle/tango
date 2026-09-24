//! Replay playback, queueing, statistics, and video export.

use super::desktop::{copy_html_to_clipboard, copy_image_to_clipboard, open_path, reveal_path};
use super::{App, Message};
use crate::library::{patch, replays};
use crate::{session, tabs};

/// What [`App::replay_stats_takeover`] settled for a playback session:
/// whether its prefetcher owes anyone an analysis, what it can be told
/// up front about where the rounds are, and the task wiring its progress
/// back into the replays tab.
struct ReplayStatsDuty {
    job: Option<session::PrefetchStatsFeed>,
    round_boundaries: Vec<u32>,
    task: iced::Task<Message>,
}

impl App {
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
            self.replay_controller.defer(p);
            return self.install_patch(key);
        }

        let duty = self.replay_stats_takeover(&p);
        match session::build_playback(
            &self.scanners,
            &self.config,
            &self.audio_binder,
            &p,
            duty.job,
            duty.round_boundaries,
        ) {
            Ok(launch) => {
                self.session.install(launch, &self.audio_binder, &self.config);
                if let Some(factor) = self.replay_controller.take_speed() {
                    if let Some(s) = self.session.active() {
                        s.set_speed(factor);
                    }
                }
            }
            // The dropped job closes its stream, whose completion
            // message clears the tab's pending marker — a later
            // focus retries the analysis.
            Err(e) => log::warn!("failed to play replay {}: {e}", p.display()),
        }
        duty.task
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
        let effect = self.replays.update(msg, &self.config);
        if let Some(p) = finished {
            self.replay_controller.finished(&p);
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
            E::CopyHtml { text, html } => {
                copy_html_to_clipboard(text, html);
                iced::Task::none()
            }
            E::CopyImage(img) => {
                copy_image_to_clipboard(img);
                iced::Task::none()
            }
            E::OpenExportSaveDialog {
                replay: replay_path,
                raw_output,
            } => {
                let replay_for_msg = replay_path.clone();
                self.export_save_dialog(replay_path, raw_output, "", move |output| {
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
                round_marks,
                has_setup,
                clip,
                swap_sides,
                canceller,
            } => self
                .spawn_replay_render(
                    replay,
                    output,
                    settings,
                    rounds,
                    round_marks,
                    has_setup,
                    clip,
                    swap_sides,
                    canceller,
                )
                .map(Message::Replays),
            E::LoadPreview { replay, builds } => {
                let scanners = self.scanners.clone();
                let config = self.config.clone();
                let path = replay.clone();
                iced::Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || load_replay_preview(&scanners, &config, &path, builds))
                            .await
                            .ok()
                    },
                    move |preview| {
                        Message::Replays(tabs::replays::Message::PreviewLoaded(
                            replay,
                            std::sync::Arc::new(std::sync::Mutex::new(preview)),
                        ))
                    },
                )
            }
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
                self.replay_controller.track(path, cancel, handle);
                task
            }
            E::SaveEditorTask(t) => t.map(Message::Replays),
        }
    }

    /// Open the native Save-File dialog for a replay's rendered
    /// video and dispatch `make_msg(picked_path)` into the replays-tab
    /// message stream — or NoOp on dismissal, keeping any open form
    /// untouched since no job ever started. `raw_output` selects the
    /// default extension and filter, by asking the exporter which
    /// container that setting writes rather than restating the mapping.
    /// `stem_suffix` is appended to the replay's file stem (the clip
    /// flow names its file apart so it doesn't collide with a
    /// whole-replay export's default).
    fn export_save_dialog(
        &self,
        replay_path: std::path::PathBuf,
        raw_output: bool,
        stem_suffix: &str,
        make_msg: impl Fn(std::path::PathBuf) -> tabs::replays::Message + Send + Sync + 'static,
    ) -> iced::Task<Message> {
        let container = crate::replay_render::container(raw_output);
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

    /// Prepare a render of `replay_path` and stream its progress back into
    /// the replays tab. The user-picked output path, form snapshot, and
    /// the job's canceller come from the tab module's `StartExport`
    /// effect; a request that can't start reports as a finished job.
    #[allow(clippy::too_many_arguments)]
    fn spawn_replay_render(
        &mut self,
        replay_path: std::path::PathBuf,
        output_path: std::path::PathBuf,
        user_settings: tabs::replays::ExportSettings,
        rounds_mask: Vec<bool>,
        // Where the whole-replay render cuts its chapters, from this
        // replay's finished telemetry analysis. Empty when there isn't
        // one yet, and the render is a single chapter.
        round_marks: Vec<u32>,
        // The recording opens on a setup section (bn6 random battle),
        // so the first chapter is it — titled as such — and the round
        // numbering starts at the second.
        has_setup: bool,
        clip: Option<crate::replay_render::Clip>,
        // Render the opposite seat's perspective — the viewer's swap
        // toggle when the clip export started.
        swap_sides: bool,
        canceller: crate::replay_render::Canceller,
    ) -> iced::Task<tabs::replays::Message> {
        use tabs::replays::{ExportError, ExportMessage, Message as M};
        let failed = |replay: std::path::PathBuf, error| {
            iced::Task::done(M::Export(ExportMessage::Finished {
                replay,
                result: Err(error),
            }))
        };

        // Decode just enough of the replay to get both sides' game
        // registrations + raw ROM bytes. Failures show up as a
        // finished job with an error — same as runtime errors.
        let prep = (|| -> anyhow::Result<session::replay::EngineReplay> {
            let (replay, resolved) =
                self.scanners
                    .open_replay(crate::library::storage(), &self.config, &replay_path)?;
            Ok(session::replay::EngineReplay::new(
                resolved.games,
                resolved.roms,
                &replay,
            )?)
        })();
        let mut engine = match prep {
            Ok(p) => p,
            Err(e) => return failed(replay_path, ExportError::Prepare(std::sync::Arc::new(e))),
        };

        if clip.is_none() && !rounds_mask.iter().any(|b| *b) {
            return failed(replay_path, ExportError::NoRoundsSelected);
        }

        // Chapter titles for the output container, one per section in
        // mask order — resolved here because the export thread has no
        // access to the locale bundle. A recording that opens on a
        // setup section titles its first chapter as that, and the
        // round numbering starts after it.
        let title_count = clip
            .as_ref()
            .map(|c| c.round_marks.len() + 1)
            .unwrap_or(rounds_mask.len());
        let round_titles: Vec<String> = (0..title_count)
            .map(|i| {
                if has_setup && i == 0 {
                    crate::t!(&self.config.language, "replays-export-setup")
                } else {
                    let number = (i + 1 - has_setup as usize) as i64;
                    crate::t!(&self.config.language, "session-results-round", number = number)
                }
            })
            .collect();

        // The same boot the player uses, through the local seat's own
        // engine door.
        let total_ticks = engine.total_ticks();
        engine.config.disable_bgm = user_settings.disable_bgm;
        // A whole-replay export is the degenerate clip covering the full
        // stream, cut at the analysis's round marks; the player's clip
        // brings the live session's boundaries and a mask selecting
        // every round — its gate is the span. A snapshot restore would
        // erase the priming-time BGM-disable poke, so muted renders
        // re-sim from boot.
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
                    round_marks,
                },
                rounds_mask,
            ),
        };
        if user_settings.disable_bgm {
            clip.snapshot = None;
        }
        let job = crate::replay_render::Job {
            engine,
            rounds_mask,
            round_titles,
            clip,
            // scale == 0 is the slider's raw-output stop (RGB24 + PCM,
            // no upscale); 1..=10 is a lossy render at that
            // nearest-neighbor upscale. The exporter picks the codecs and
            // container to match.
            scale: (user_settings.scale != 0).then_some(user_settings.scale as usize),
            twosided: user_settings.twosided,
            swap_sides,
        };
        iced::Task::stream(crate::replay_render::spawn(job, output_path, canceller)).map(move |event| {
            let replay = replay_path.clone();
            M::Export(match event {
                crate::replay_render::Event::Progress { completed, total } => ExportMessage::Progress {
                    replay,
                    completed,
                    total,
                },
                crate::replay_render::Event::Finished(result) => ExportMessage::Finished {
                    replay,
                    result: result.map_err(|e| ExportError::Render(std::sync::Arc::new(e))),
                },
            })
        })
    }

    /// Hand a played-out replay session over to the next queued replay.
    ///
    /// Called on every frame message, so it is mostly a pair of atomic loads.
    /// It fires only on the transition into end-of-stream while playback was
    /// actually running: `replay_was_playing` is what separates "ran out" from
    /// "the user dragged the playhead to the last tick", since scrubbing
    /// pauses before it seeks. With nothing queued the finished replay just
    /// stays parked on its final frame, as it always has.
    pub(super) fn advance_replay_queue(&mut self) -> iced::Task<Message> {
        let Some(s) = self.session.active_as::<session::replay::ReplaySession>() else {
            self.replay_controller.reset_playback();
            return iced::Task::none();
        };
        let ran_out = self.replay_controller.observe(
            !s.is_paused(),
            s.pending_seek_target().is_some(),
            s.current_tick(),
            s.total_ticks(),
        );
        if !ran_out || self.replays.queue.is_empty() {
            return iced::Task::none();
        }
        self.watch_next_replay()
    }

    /// Manual skip and automatic queue advancement share the same handoff.
    pub(super) fn watch_next_replay(&mut self) -> iced::Task<Message> {
        if self.replays.queue.is_empty() {
            return iced::Task::none();
        }
        let next = self.replays.queue.remove(0);
        let speed = self
            .session
            .active_as::<session::replay::ReplaySession>()
            .map(|s| s.speed());
        self.replay_controller.handoff(speed);
        self.session.close_session();
        self.watch_replay(next)
    }

    /// Stats duty for a playback session about to start on `path`. With
    /// no readable stats sidecar, the session's prefetcher — which runs
    /// the very simulation the analysis needs anyway — takes the
    /// analysis over: any in-flight tab worker for this replay is
    /// cancelled (its simulation stops, its progress stream aborts
    /// mid-air so the tab's pending marker survives the handover), and
    /// the returned job + progress-stream task plug the prefetcher into
    /// the tab's usual `HpStatsPartial`/`HpStatsLoaded` pipeline. With a
    /// sidecar on disk there is nothing to compute — just the round
    /// boundaries out of it, so the scrub bar has its marks from the
    /// first frame rather than waiting out a pass to rediscover them.
    fn replay_stats_takeover(&mut self, path: &std::path::Path) -> ReplayStatsDuty {
        if let Some(stats) = replays::load_match_stats(&self.config.cache_path(), &self.config.replays_path(), path) {
            return ReplayStatsDuty {
                job: None,
                round_boundaries: stats.round_marks(),
                task: iced::Task::none(),
            };
        }
        self.replay_controller.takeover(path);
        // Marked pending so a tab focus during playback doesn't spawn a
        // duplicate worker; the prefetch stream's completion clears it.
        self.replays.hp_pending.insert(path.to_path_buf());

        let (partial_tx, partial_rx) = futures::channel::mpsc::unbounded::<tango_match::analysis::MatchStats>();
        let done: std::sync::Arc<std::sync::Mutex<Option<tango_match::analysis::MatchStats>>> = Default::default();
        let job = session::PrefetchStatsFeed {
            partial_tx,
            done: done.clone(),
        };
        use futures::StreamExt;
        let progress_path = path.to_path_buf();
        let path = path.to_path_buf();
        let stats_file = replays::stats_path(&self.config.cache_path(), &self.config.replays_path(), path.as_path());
        let stream = partial_rx
            .map(move |partial| tabs::replays::Message::HpStatsPartial(progress_path.clone(), partial))
            .chain(futures::stream::once(async move {
                let stats = done.lock().unwrap().take();
                if let Some(stats) = &stats {
                    if let Err(e) = replays::write_match_stats(&stats_file, stats) {
                        log::warn!("stats cache write failed: {e}");
                    }
                }
                tabs::replays::Message::HpStatsLoaded(path, stats)
            }));
        ReplayStatsDuty {
            job: Some(job),
            // Nothing analyzed yet, so nothing to hand the scrub bar;
            // the prefetcher publishes each boundary as it reaches it.
            round_boundaries: vec![],
            task: iced::Task::stream(stream).map(Message::Replays),
        }
    }

    /// Drops cached replay stats for paths that no longer exist in
    /// the latest scan, then kicks the worker for any newly-scanned
    /// paths that don't have stats yet. Returns tab-scoped Task —
    /// caller wraps with `.map(Message::Replays)` if at App level.
    pub(super) fn refresh_replay_stats(&mut self) -> iced::Task<tabs::replays::Message> {
        let live: std::collections::HashSet<std::path::PathBuf> =
            self.scanners.replays.read().iter().map(|r| r.path.clone()).collect();
        self.replays.stats.retain(|p, _| live.contains(p));
        self.replays.hp_charts.retain(|p, _| live.contains(p));
        self.kick_replay_stats_loader()
    }

    /// Spawn a streaming task that decodes each not-yet-cached
    /// replay on a blocking worker, one at a time, posting each
    /// result back as a `StatsLoaded` message. Returns Task::none
    /// when there's no work to do.
    fn kick_replay_stats_loader(&self) -> iced::Task<tabs::replays::Message> {
        let paths: Vec<std::path::PathBuf> = self
            .scanners
            .replays
            .read()
            .iter()
            .filter(|r| !self.replays.stats.contains_key(&r.path))
            .map(|r| r.path.clone())
            .collect();
        if paths.is_empty() {
            return iced::Task::none();
        }
        use futures::StreamExt;
        // The round count isn't in the recording — only a telemetry
        // analysis knows it — so it comes from this replay's cached one
        // where there is one, and the caption goes without until then.
        let cache_path = self.config.cache_path();
        let replays_path = self.config.replays_path();
        let stream = futures::stream::iter(paths)
            .then(move |path| {
                let (cache_path, replays_path) = (cache_path.clone(), replays_path.clone());
                async move {
                    let p = path.clone();
                    let stats = tokio::task::spawn_blocking(move || {
                        let mut stats = replays::compute_stats(crate::library::storage(), &p).ok()?;
                        stats.round_count =
                            replays::load_match_stats(&cache_path, &replays_path, &p).map(|s| s.rounds.len() as u32);
                        Some(stats)
                    })
                    .await
                    .ok()
                    .flatten();
                    (path, stats)
                }
            })
            .filter_map(|(path, stats)| async move { stats.map(|s| tabs::replays::Message::StatsLoaded(path, s)) });
        iced::Task::stream(stream)
    }

    /// Ask where to write a clip the playing session captured, then start
    /// its export. Export jobs are owned by the Replays tab.
    pub(super) fn export_replay_clip(
        &mut self,
        path: std::path::PathBuf,
        clip: crate::replay_render::Clip,
        swap_sides: bool,
    ) -> iced::Task<Message> {
        let raw_output = self.replays.export_settings.scale == 0;
        let replay_for_msg = path.clone();
        self.export_save_dialog(path, raw_output, "-clip", move |output| {
            tabs::replays::Message::Export(tabs::replays::ExportMessage::StartClip {
                replay: replay_for_msg.clone(),
                output,
                clip: clip.clone(),
                swap_sides,
            })
        })
    }
}

/// Read what selecting the replay at `path` needs: its stats sidecar and,
/// when `builds`, each participant's save-view build — the library's
/// best-effort preview of that seat, which renders even when the seat's
/// patch can't be applied.
fn load_replay_preview(
    scanners: &crate::library::Catalog,
    config: &crate::config::Config,
    path: &std::path::Path,
    builds: bool,
) -> tabs::replays::ReplayPreview {
    let builds = builds.then(|| {
        let f = std::fs::File::open(path)?;
        let replay = tango_replay::Replay::decode(f)?;
        let resolver = scanners.resolver(crate::library::storage(), config);
        let seat = |player: u8, side: &str| {
            resolver
                .preview_replay_seat(&replay, player)
                .inspect_err(|e| log::warn!("{side} replay build preview failed: {e}"))
                .ok()
                .map(crate::selection::load)
        };
        Ok(tabs::replays::ReplayBuilds {
            local: seat(replay.local_player_index, "local"),
            opponent: seat(1 - replay.local_player_index, "opponent"),
            total_ticks: replay.inputs.len() as u32,
        })
    });
    tabs::replays::ReplayPreview {
        builds,
        stats: replays::load_match_stats(&config.cache_path(), &config.replays_path(), path),
    }
}

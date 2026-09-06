//! Central message dispatch and screen transition bookkeeping.

use super::*;

impl App {
    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        let screen_before = self.screen_key();
        let family_before = self.loadout.family;
        // Candidate snapshot for the lobby's exit animation — taken
        // before dispatch (the handler about to run may reset the
        // phase/lobby), kept only if the lobby actually left.
        let lobby_live = self.lobby_on_screen().then(|| {
            (
                self.netplay.phase.clone(),
                self.netplay.lobby.clone(),
                self.netplay.ready_view(),
            )
        });
        let task = self.update_inner(message);
        let now = iced::time::Instant::now();
        let screen_after = self.screen_key();
        if screen_before != screen_after {
            self.screen_enter.start(now);
            self.screen_enter_scope = match (screen_before, screen_after) {
                (ScreenKey::Tabs(t1), ScreenKey::Tabs(t2)) => {
                    // The nav strip lays the tabs out in declaration
                    // order, so the discriminants double as nav
                    // positions: moving right brings the new pane in
                    // from the right, moving left from the left.
                    let dx = if (t2 as u8) > (t1 as u8) {
                        PANE_SLIDE
                    } else {
                        -PANE_SLIDE
                    };
                    EnterScope::Body { dx }
                }
                // Closing a session descends — the menu comes back
                // in from above, mirroring the rise that brought
                // the session up.
                (ScreenKey::Session, _) => EnterScope::Root { dy: -ROOT_SLIDE },
                _ => EnterScope::Root { dy: ROOT_SLIDE },
            };
        }
        // The Play tab's band swap follows the netplay phase: the
        // view morphs the link-code strip into the lobby (and back)
        // off this transition. When the lobby leaves, freeze its last
        // live state for the exit half to render from.
        let lobby_after = self.lobby_on_screen();
        if let (Some(snap), false) = (lobby_live, lobby_after) {
            self.lobby_exit_snapshot = Some(snap);
            // A Fight-generated code never touched the input on the way in
            // (it debuted in the lobby band) — drop it in now that the
            // strip is coming back, so a retry re-hosts the same code.
            self.play.restore_generated_link_code();
        }
        self.lobby_swap.set(lobby_after, now);
        // A different family swaps the entire bottom of the tab —
        // rise the whole save-view pane in. A different game or save
        // within the family only re-renders the save's content, and
        // that entrance rides in with the reloaded save itself: its
        // view state is minted by the load and starts its own rise.
        if family_before != self.loadout.family {
            self.play.animate_family_switch(now);
        }
        task
    }

    fn update_inner(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::NoOp => iced::Task::none(),
            Message::AnimTick => iced::Task::none(),
            // Keep the runtime alive long enough for PvP's supervisor to
            // announce a deliberate quit. An immediate Alt-F4 teardown races
            // that send, leaving the peer with an ambiguous clean EOF and an
            // unnecessary reconnect attempt.
            Message::Quit => {
                if self.exit_pending {
                    return iced::Task::none();
                }
                // Complete any queued config write before the runtime
                // tears down (Drop also flushes, as the backstop for the
                // window-close exit path).
                self.config_writer.flush();
                if let Some(done) = self.session.request_app_close() {
                    self.exit_pending = true;
                    iced::Task::perform(
                        async move {
                            let _ = tokio::time::timeout(PVP_EXIT_TIMEOUT, done.cancelled()).await;
                        },
                        |_| Message::Exit,
                    )
                } else {
                    iced::exit()
                }
            }
            Message::Exit => iced::exit(),
            Message::TabSelected(t) => {
                self.tab = t;
                // A tab switch unmounts the input settings pane's capture
                // wrapper, so key/button releases stop arriving — drop the
                // held set rather than show stale-lit binding chips on the
                // way back.
                self.settings.held = Default::default();
                // Clicking a scanner-backed tab re-runs the scan in the
                // background — there are no Rescan buttons; this is how
                // new files on disk get noticed. Cheap when nothing
                // changed (stat-fingerprint gated, see Scanners::rescan).
                // Deliberately not limited to *entering* the tab: pressing
                // the tab you are already on is what a user reaches for
                // when they have just dropped a file in, and it is the
                // only gesture left that means "look again".
                // Play additionally throws away the loaded bundle after
                // the scan, even when the selected paths did not change:
                // the click is an explicit reload from disk, including
                // discarding staged edits and rebuilding ROM/patch assets.
                // Settings doesn't read the scanners, so skip it there.
                if !self.is_rescanning() {
                    if let Some(followup) = t.rescan_followup() {
                        return self.rescan_off_thread(followup);
                    }
                }
                iced::Task::none()
            }
            // Loadout strip interactions route to the shared
            // App-level Loadout — the tab never sees them.
            // Every dispatch below is followed by a Settings resend —
            // the netplay handler dedupes against the last-sent value
            // via `Settings: Eq`, so unchanged dispatches are free.
            Message::Play(tabs::play::Message::Loadout(m)) => {
                let task = self.update_loadout(m);
                iced::Task::batch([task, self.resend_settings_if_lobby()])
            }
            Message::Play(m) => {
                let task = self.update_play(m);
                iced::Task::batch([task, self.resend_settings_if_lobby()])
            }
            Message::Patches(m) => self.update_patches(m),
            Message::DiscordTick => {
                self.handle_discord_tick();
                iced::Task::none()
            }
            Message::Window(id, ev) => {
                match ev {
                    iced::window::Event::Opened { .. } => {
                        // The window came up, so the geometry we
                        // launched with is presentable — disarm the
                        // safe mode `main::restore_window_size` armed
                        // on disk before the window was built.
                        if self.config.window_geometry_unverified {
                            self.config.window_geometry_unverified = false;
                            self.persist_config();
                        }
                        // What this window's DPI scale is decides how
                        // large it may get before its surface outgrows
                        // the GPU — see `WindowScaleQueried`. Nothing
                        // else about the opened size is second-guessed:
                        // a window bigger than the monitor it landed on
                        // is a legitimate thing to want (spanning two
                        // displays, say), and the only size we're in a
                        // position to refuse is one that can't be drawn
                        // at all.
                        return iced::window::scale_factor(id)
                            .map(move |scale| Message::WindowScaleQueried { id, scale });
                    }
                    iced::window::Event::Resized(size) => {
                        // The Resized size could be either a user-driven
                        // resize or the result of maximize/unmaximize.
                        // We need is_maximized to decide whether to keep
                        // it as the restore size, so query it and finish
                        // the bookkeeping in WindowMaximizedQueried.
                        return iced::window::is_maximized(id)
                            .map(move |maximized| Message::WindowMaximizedQueried { size, maximized });
                    }
                    iced::window::Event::Moved(point) => {
                        // Only remember position while fullscreen.
                        // Entering fullscreen parks the window at its
                        // monitor's origin and fires Moved (with
                        // fullscreen already set, see C::Fullscreen) —
                        // so the persisted value identifies the
                        // fullscreen monitor for the next launch.
                        // Windowed positions are deliberately not
                        // persisted: restoring an exact x/y is janky on
                        // multi-monitor setups (saved coords can land
                        // off-screen or on the wrong display).
                        if self.config.fullscreen {
                            self.config.last_window_position = Some((point.x, point.y));
                            self.persist_config();
                        }
                    }
                    iced::window::Event::CloseRequested => {
                        return self.update_inner(Message::Quit);
                    }
                    _ => {}
                }
                iced::Task::none()
            }
            Message::WindowScaleQueried { id, scale } => {
                // Cap how big the window can get dragged. A surface is
                // the window size in *physical* pixels, so on a wide
                // enough desktop a drag-resize can walk it past what
                // the GPU will configure — and that isn't an error the
                // app gets to handle, it's a panic from inside wgpu.
                // iced converts this cap back to physical with the
                // same `scale × ui_scale` it reports sizes with, so
                // what winit ends up enforcing is exactly
                // `MAX_SURFACE_SIZE` physical pixels — which stays the
                // right cap even if either scale changes later.
                let (min_w, min_h) = crate::tabs::settings::MINIMUM_RESOLUTION;
                let scale = (scale * self.config.ui_scale).max(0.01);
                let cap = crate::MAX_SURFACE_SIZE / scale;
                let max = iced::Size::new(cap.max(min_w as f32), cap.max(min_h as f32));
                iced::window::set_max_size(id, Some(max))
            }
            Message::WindowMaximizedQueried { size, maximized } => {
                if !maximized {
                    self.config.last_window_size = Some((size.width, size.height));
                }
                self.config.last_window_maximized = maximized;
                self.persist_config();
                iced::Task::none()
            }
            Message::Replays(m) => self.update_replays(m),
            Message::Settings(m) => self.update_settings(m).map(Message::Settings),
            Message::Welcome(m) => self.update_welcome(m),
            Message::Session(m) => {
                // In-match frame-delay slider: persist the new value to config so
                // the choice sticks for the next match (session.update applies it
                // to the live session). Mirrors the lobby slider's persistence.
                if let session::Message::Pvp(session::view::pvp::Message::SetFrameDelay(d)) = &m {
                    self.config.frame_delay = *d;
                    self.persist_config();
                }
                // A setup drawer just finished being dragged: the live
                // widths ride on the session's panes (the drag moves
                // them there), so mirror the pair into config on
                // release — one write per drag, not per mouse move.
                if let session::Message::Pvp(session::view::pvp::Message::EndPaneResize) = &m {
                    if let Some(panes) = self.session.pvp_panes.as_ref() {
                        self.config.pvp_setup_pane_widths = panes.pane_widths;
                        self.persist_config();
                    }
                }
                // Replay input display toggle: the flag lives in config
                // (so the choice sticks across replays); the session
                // handler itself is a no-op.
                if let session::Message::Replay(session::view::replay::Message::ToggleInputDisplay) = &m {
                    self.config.show_replay_inputs = !self.config.show_replay_inputs;
                    self.persist_config();
                }
                // Custom-screen fast-forwarding is a replay-viewer
                // preference, so the toggle applies live through the session
                // handler and remains selected for the next replay too.
                if let session::Message::Replay(session::view::replay::Message::ToggleCustomScreenSpeedup) = &m {
                    self.config.replay_custom_screen_speedup = !self.config.replay_custom_screen_speedup;
                    self.persist_config();
                }
                // Clip exports share the replay tab's quality setting, so a
                // choice made in the player's clip strip also appears in the
                // full export form and feeds the same renderer snapshot.
                if let session::Message::Replay(session::view::replay::Message::SetClipExportScale(scale)) = &m {
                    return self.update_replays(tabs::replays::Message::Export(
                        tabs::replays::ExportMessage::SetScale(*scale),
                    ));
                }
                // Replay and training share one opponent-view preference.
                // Their session handlers switch the auxiliary renderer; this
                // keeps the selected layout across either kind of session.
                let selected_opponent_view = match &m {
                    session::Message::Replay(session::view::replay::Message::SetOpponentView(view))
                    | session::Message::Training(session::view::training::Message::SetOpponentView(view)) => {
                        Some(*view)
                    }
                    _ => None,
                };
                if let Some(view) = selected_opponent_view {
                    self.config.opponent_view = view;
                    self.persist_config();
                }
                // The transport bar's Export-clip chip: everything
                // positional is captured NOW, while the session is alive —
                // the jump-start snapshot nearest the span start, the
                // session's round boundaries, and the perspective swap —
                // then the save dialog runs async and the pick flows
                // through the replays tab's export-job machinery
                // (progress, cancel, and the panel all live there). The
                // session handler itself is a no-op.
                if let session::Message::Replay(session::view::replay::Message::ExportClip { start, end }) = &m {
                    let (start, end) = (*start, *end);
                    let Some(path) = self.session.replay_path.clone() else {
                        return iced::Task::none();
                    };
                    let (snapshot, round_marks, swap_sides) = self
                        .session
                        .active_as::<session::replay::ReplaySession>()
                        .map(|s| (s.clip_start_capture(start), s.round_boundaries(), s.swap_perspective()))
                        .unwrap_or_default();
                    let clip = crate::replay_render::Clip {
                        start,
                        end,
                        snapshot,
                        round_marks,
                    };
                    let raw_output = self.replays.export_settings.scale == 0;
                    let replay_for_msg = path.clone();
                    return self.export_save_dialog(path, raw_output, "-clip", move |output| {
                        tabs::replays::Message::Export(tabs::replays::ExportMessage::StartClip {
                            replay: replay_for_msg.clone(),
                            output,
                            clip: clip.clone(),
                            swap_sides,
                        })
                    });
                }
                // The clip strip's cancel: forward to the replays tab's
                // own cancel handler — the job and its canceller live
                // there, whichever surface started the export.
                if let session::Message::Replay(session::view::replay::Message::CancelClipExport) = &m {
                    if let Some(path) = self.session.replay_path.clone() {
                        return self.update_replays(tabs::replays::Message::Export(
                            tabs::replays::ExportMessage::Cancel(path),
                        ));
                    }
                    return iced::Task::none();
                }
                // The bar's up-next chip: drop this replay and start the next
                // queued one. Same handoff `advance_replay_queue` does when a
                // replay plays out, just user-driven and without waiting.
                if let session::Message::Replay(session::view::replay::Message::SkipToQueued) = &m {
                    if self.replays.queue.is_empty() {
                        return iced::Task::none();
                    }
                    let next = self.replays.queue.remove(0);
                    self.queue_carry_speed = self
                        .session
                        .active_as::<session::replay::ReplaySession>()
                        .map(|s| s.speed());
                    self.session.close_session();
                    self.replay_was_playing = false;
                    return self.watch_replay(next);
                }
                // Results screen's Watch button: building a playback session
                // needs the scanners + config, so it's handled here (the
                // session module's handler is a no-op). The results stay set
                // underneath — closing the replay lands back on them. On
                // failure (e.g. the replay is still flushing or unreadable),
                // log and leave the results screen up.
                if let session::Message::Results(session::view::results::Message::WatchReplay) = &m {
                    if let Some(path) = self.session.results.as_ref().and_then(|r| r.replay_path.clone()) {
                        let duty = self.replay_stats_takeover(&path);
                        match session::build_playback(
                            &self.scanners,
                            &self.config,
                            &self.audio_binder,
                            &path,
                            duty.job,
                            duty.round_boundaries,
                        ) {
                            Ok((s, audio, threads)) => {
                                self.session.replay_path = Some(path.clone());
                                self.session.active = Some(Box::new(s));
                                self.session.audio_binding = audio;
                                self.session.attach_drive_threads(threads);
                                self.session.session_installed();
                            }
                            // The dropped job closes its stream, whose
                            // completion message clears the tab's pending
                            // marker — a later focus retries the analysis.
                            Err(e) => log::warn!("failed to play replay {}: {e}", path.display()),
                        }
                        return duty.task;
                    }
                    return iced::Task::none();
                }
                // The active session may have changed the user's
                // save: a single-player session runs off an in-memory
                // image that `SaveBackup` writes back. When the session
                // ends, drop it first so that write lands, then re-scan
                // saves + force a LoadedSave rebuild so the play tab's
                // save view reflects the fresh on-disk SRAM. Detected
                // by the active-slot transition (not the Close message)
                // because Esc closes SP sessions inside the session
                // update.
                let was_sp = self
                    .session
                    .active_as::<session::singleplayer::SinglePlayerSession>()
                    .is_some();
                // Snapshot "was PvP" before dispatch — PvP
                // sessions can auto-tear-down inside
                // `UpdateFramebuffer` (peer-end / disconnect /
                // grace timeout), not just from a Close message.
                // We trigger the replay rescan whenever a PvP
                // session was active before and isn't after.
                let was_pvp = self.session.active_as::<session::pvp::PvpSession>().is_some();
                let task = self
                    .session
                    .update(m, &self.config.input_mapping, &self.config.language)
                    .map(Message::Session);
                // A replay that just played out hands off to the queue. Done
                // here rather than through `Session::is_ended` on purpose:
                // that would tear a finished replay down even with an empty
                // queue, and parking on the last frame (scrub back, export a
                // clip) is the behavior playback has always had.
                let queue_advance = self.advance_replay_queue();
                // Rescan + reload run off-thread; the Rescanned
                // followup forces a `loaded` rebuild past the
                // same-key dedupe so the play tab's save view
                // reflects the fresh on-disk SRAM.
                let sp_rescan = if was_sp && self.session.active.is_none() {
                    self.rescan_off_thread(RescanFollowup::ForceRebuildLoaded)
                } else {
                    iced::Task::none()
                };
                // PvP sessions write a `.tangoreplay` next to
                // the saves dir on match end; once the session
                // clears we want the new file to show up in the
                // Replays tab without a manual rescan. The
                // `RefreshAndReplayStats` followup also warms the
                // stats sidebar with the just-landed match.
                let pvp_closed = was_pvp && self.session.active.is_none();
                let pvp_rescan = if pvp_closed {
                    self.rescan_off_thread(RescanFollowup::RefreshAndReplayStats)
                } else {
                    iced::Task::none()
                };
                iced::Task::batch([task, queue_advance, sp_rescan, pvp_rescan])
            }
            Message::Netplay(delivery) => {
                // A re-delivery of an already-applied report: nothing left
                // in the cell (see `netplay::Delivery`).
                let Some(incoming) = delivery.take() else {
                    return iced::Task::none();
                };
                // Always resend after a report: this covers the
                // Negotiating → Lobby transition (first announce) and
                // lobby-state mutations. The dedupe inside
                // `send_local_settings` makes unchanged dispatches a no-op.
                let was_lobby = matches!(self.netplay.phase, netplay::Phase::Lobby { .. });
                let event = self.netplay.apply(incoming);
                let task = match event {
                    Some(netplay::Event::MatchReady) => self.start_pvp_handoff(),
                    None => iced::Task::none(),
                };
                let became_lobby = !was_lobby && matches!(self.netplay.phase, netplay::Phase::Lobby { .. });
                // Opponent just completed the handshake — flash the
                // taskbar / bounce the dock so the lobby host
                // notices even if Tango isn't focused. No-op if the
                // window is already focused (per iced docs).
                let attention = if became_lobby {
                    iced::window::latest().and_then(|id| {
                        iced::window::request_user_attention(id, Some(iced::window::UserAttention::Critical))
                    })
                } else {
                    iced::Task::none()
                };
                let resend = self.resend_settings_if_lobby();
                self.uncommit_if_incompat();
                let fetch = self.fetch_missing_patch();
                iced::Task::batch([task, resend, fetch, attention])
            }
            Message::PvpSessionBuilt(attempt, slot) => {
                let Some(result) = slot.lock().unwrap().take() else {
                    return iced::Task::none();
                };
                // Leave stays enabled all the way through the handoff,
                // so the lobby this build belonged to may already be
                // gone — the user's disconnect bumped the attempt id on
                // its way out. A build with no lobby behind it gets
                // wound down close_session-style: request_close first
                // (the supervisor's best-effort Goodbye reaches the
                // peer), audio unbound, then wait for the drive thread.
                // Netplay state is deliberately untouched — it's Idle,
                // or already carrying a brand-new attempt this stale
                // build has no business installing over or failing.
                if attempt != self.netplay.session_id() {
                    match result {
                        Ok((session, _panes, audio, drive)) => {
                            session::Session::request_close(&session);
                            drop(audio);
                            drop(session);
                            let _ = drive.join();
                        }
                        Err(e) => log::info!("pvp session build failed after the lobby was left: {e:#}"),
                    }
                    return iced::Task::none();
                }
                match result {
                    Ok((session, panes, audio, drive)) => {
                        // The frame-delay slider stays live during the
                        // build (purely local), but spawn_pvp sampled
                        // the config at handoff — re-apply so a drag
                        // made while the match spun up isn't lost.
                        session.set_frame_delay(
                            self.config
                                .frame_delay
                                .clamp(session::pvp::MIN_FRAME_DELAY, session::pvp::MAX_FRAME_DELAY),
                        );
                        // Both setup drawers start closed — the edge
                        // handles are the invitation; a pane that
                        // barges in over the match start isn't.
                        // Except when the user opted in: slide the
                        // opponent's drawer open at match start if
                        // their setup is actually visible.
                        let auto_open = self.config.show_opponent_setup && panes.opponent_loaded.is_some();
                        self.session.active = Some(Box::new(session));
                        self.session.pvp_panes = Some(panes);
                        self.session.audio_binding = audio;
                        self.session.attach_drive_threads([drive]);
                        if auto_open {
                            self.session.opponent_panel.open();
                        } else {
                            self.session.opponent_panel.close();
                        }
                        self.session.self_panel.close();
                        self.session.session_installed();
                        // Drop the post-handoff lobby snapshot now
                        // that the PvP view is taking over the
                        // screen. take_pre_match deliberately left
                        // it in place so the bottom strip didn't
                        // flash blank while spawn_pvp ran.
                        self.netplay.finish_handoff();
                    }
                    Err(e) => {
                        // Surface the failure where every other netplay
                        // failure lands: the lobby band's sticky Failed
                        // status, which is still on screen (the handoff
                        // kept it up while the session was built).
                        log::error!("pvp session build failed: {e:#}");
                        self.netplay.fail_session_build(netplay::Error::Other(format!("{e:#}")));
                    }
                }
                iced::Task::none()
            }
            Message::Rescanned(followup) => {
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
                                if let Some((game, path)) = loadout::first_available_family_save(&self.scanners, family)
                                {
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
    }
}

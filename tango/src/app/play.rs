//! Play tab actions and selection persistence.

use super::desktop::{copy_html_to_clipboard, copy_image_to_clipboard, open_path, reveal_path};
use super::{App, Message, RescanFollowup};
use crate::library::save;
use crate::tabs::play::loadout_strip;
use crate::{netplay, session, tabs};

impl App {
    /// Apply a loadout-strip message to the shared
    /// App-level selection and run the selection-change
    /// follow-ups. The caller batches a lobby settings-resend after
    /// this, so a mid-lobby save/patch switch reaches the peer.
    pub(super) fn update_loadout(&mut self, msg: loadout_strip::Message) -> iced::Task<Message> {
        // Download controls act on the fetch, not the selection, so they
        // carry their own key and skip the selection-changed follow-ups.
        let download_key = match &msg {
            loadout_strip::Message::RetryPatchDownload(key) | loadout_strip::Message::CancelPatchDownload(key) => {
                Some(key.clone())
            }
            _ => None,
        };
        let Some(effect) = loadout_strip::update(&mut self.loadout, msg, &self.scanners, &self.config) else {
            return iced::Task::none();
        };
        match effect {
            loadout_strip::Effect::CancelDownload => {
                let Some(key) = download_key else {
                    return iced::Task::none();
                };
                return self.cancel_download(key);
            }
            loadout_strip::Effect::RetryDownload => {
                let Some(key) = download_key else {
                    return iced::Task::none();
                };
                return self.install_patch(key);
            }
            loadout_strip::Effect::SelectionChanged => {
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
                // Stamping the family keeps the default policy from
                // clobbering a pick made before it ever ran (see
                // `netplay::State::pick_match_type`).
                let family = self.loadout.game().map(|g| g.family_and_variant().0);
                self.netplay.pick_match_type(family, mt);
                if let Some(family) = family {
                    // And remember it for the next time this family
                    // comes up, here or in a future launch.
                    self.config.last_match_type_per_family.insert(family.to_string(), mt);
                    self.persist_config();
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
                let save_sram = loaded.snapshot_sram();
                match self.netplay.commit(save_sram) {
                    Some(netplay::Event::MatchReady) => self.start_pvp_handoff(),
                    None => iced::Task::none(),
                }
            }
            E::OpenPath(p) => open_path(p),
            E::RevealPath(p) => reveal_path(p),
            E::CopyText(s) => iced::clipboard::write(s),
            E::CopyHtml { text, html } => {
                copy_html_to_clipboard(text, html);
                iced::Task::none()
            }
            E::CopyImage(img) => {
                copy_image_to_clipboard(img);
                iced::Task::none()
            }
            E::StartSinglePlayer => {
                if self.loaded.is_none() {
                    return iced::Task::none();
                }
                match session::spawn_singleplayer(&self.scanners, &self.config, &self.audio_binder, &self.loadout) {
                    Ok(launch) => self.session.install(launch, &self.audio_binder, &self.config),
                    Err(e) => {
                        log::error!("singleplayer start failed: {e:#}");
                    }
                }
                iced::Task::none()
            }
            E::StartTraining => {
                // Snapshot staged edits at the UI boundary. Launch preparation
                // uses the selected identity, independent of preview assets.
                let Some(loaded) = self.loaded.as_ref() else {
                    return iced::Task::none();
                };
                match session::spawn_training(
                    &self.scanners,
                    &self.config,
                    &self.audio_binder,
                    &self.loadout,
                    &loaded.snapshot_sram(),
                ) {
                    Ok(launch) => self.session.install(launch, &self.audio_binder, &self.config),
                    Err(e) => {
                        log::error!("training start failed: {e:#}");
                    }
                }
                iced::Task::none()
            }
            E::SaveDuplicate { new_stem } => {
                if let Some(src) = self.loadout.save().map(std::path::Path::to_path_buf) {
                    match save::duplicate(crate::library::storage(), &src, &new_stem) {
                        Ok(dst) => {
                            log::info!("duplicated save: {} → {}", src.display(), dst.display());
                            self.loadout.follow_save_file(dst);
                            self.persist_selection();
                            return self.rescan_off_thread(RescanFollowup::Refresh);
                        }
                        Err(e) => log::error!("duplicate save: {e}"),
                    }
                }
                iced::Task::none()
            }
            E::SaveRename { new_stem } => {
                if let Some(src) = self.loadout.save().map(std::path::Path::to_path_buf) {
                    match save::rename(crate::library::storage(), &src, &new_stem) {
                        Ok(dst) => {
                            log::info!("renamed save: {} → {}", src.display(), dst.display());
                            self.loadout.follow_save_file(dst);
                            self.persist_selection();
                            return self.rescan_off_thread(RescanFollowup::Refresh);
                        }
                        Err(e) => log::error!("rename save: {e}"),
                    }
                }
                iced::Task::none()
            }
            E::SaveDelete => {
                if let Some(src) = self.loadout.save().map(std::path::Path::to_path_buf) {
                    if let Err(e) = save::delete(crate::library::storage(), &src) {
                        log::error!("delete save: {e}");
                    } else {
                        log::info!("deleted save: {}", src.display());
                    }
                    // Clear the selection now so the picker shows
                    // "no save" while the rescan is in flight;
                    // PickFirstSave restores the first remaining
                    // entry once the scan finishes.
                    self.loadout.clear_save();
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
                let template = save::template(game, &self.scanners.patches.read(), self.loadout.patch(), &template);
                if let Some(template) = template {
                    match save::create(
                        crate::library::storage(),
                        &self.config.saves_path(),
                        &name,
                        template.as_ref(),
                    ) {
                        Ok(dst) => {
                            log::info!(
                                "created new save for {:?}: {}",
                                game.family_and_variant(),
                                dst.display()
                            );
                            self.loadout.adopt_created_save(game, dst, &self.scanners);
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
                    Some(path) if !path.as_os_str().is_empty() => match crate::library::storage().write(path, &sram) {
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
                // original. Drop and rebuild loaded to revert every tab
                // — then put the view back where it was, since a commit
                // leaves it there and a cancel should read the same.
                let previous = self.loaded.take();
                self.refresh_loaded();
                if let (Some(previous), Some(loaded)) = (previous, self.loaded.as_mut()) {
                    loaded
                        .editor
                        .carry_view_position(previous.state.as_ref(), loaded.state.as_mut());
                }
                iced::Task::none()
            }
            E::SaveEditorTask(t) => t.map(Message::Play),
        }
    }

    /// Resolve the saved selection against the scanners, now that the
    /// startup scan has filled them. Only the family half is restorable
    /// without a scan (see [`App::new`]); the game, its save and that
    /// save's patch overlay each have to still exist to come back.
    pub(super) fn restore_selection(&mut self) {
        self.loadout.restore(&self.config, &self.scanners);
    }

    /// Record the current selection back to config; called after any
    /// selection change so the next launch restores it.
    pub(super) fn persist_selection(&mut self) {
        self.loadout.persist(&mut self.config);
        self.persist_config();
    }
}

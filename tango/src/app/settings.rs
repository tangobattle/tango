//! Settings and first-run actions.

use super::{App, Message, RescanFollowup};
use crate::library::Scanners;
use crate::platform::input;
use crate::tabs;

impl App {
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

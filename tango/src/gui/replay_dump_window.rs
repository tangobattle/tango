use crate::{config, i18n};
use fluent_templates::Loader;

use super::ui_windows::UiWindowKey;
const DEFAULT_SCALE: usize = 5;

pub struct ReplayDumpWindow {
    cancellation_token: Option<tokio_util::sync::CancellationToken>,
    output_path: std::path::PathBuf,
    local_rom: Vec<u8>,
    remote_rom: Option<Vec<u8>>,
    replays: Vec<tango_pvp::replay::Replay>,
    scale: Option<usize>,
    disable_bgm: bool,
    twosided: bool,
    progress: std::sync::Arc<parking_lot::Mutex<(usize, usize)>>,
    result: std::sync::Arc<parking_lot::Mutex<Option<anyhow::Result<()>>>>,
}

impl Drop for ReplayDumpWindow {
    fn drop(&mut self) {
        if let Some(cancellation_token) = self.cancellation_token.take() {
            cancellation_token.cancel();
        }
    }
}

impl ReplayDumpWindow {
    pub fn new(
        local_rom: Vec<u8>,
        remote_rom: Option<Vec<u8>>,
        replays: Vec<tango_pvp::replay::Replay>,
        output_path: std::path::PathBuf,
    ) -> Self {
        Self {
            cancellation_token: None,
            output_path,
            local_rom,
            remote_rom,
            replays,
            scale: Some(DEFAULT_SCALE),
            disable_bgm: false,
            twosided: false,
            progress: std::sync::Arc::new(parking_lot::Mutex::new((0, 0))),
            result: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    pub fn show(&mut self, id: UiWindowKey, ctx: &egui::Context, config: &mut config::Config) -> bool {
        let language = &config.language;

        let mut open = true;
        let mut open2 = open;

        let path = self
            .output_path
            .file_name()
            .map(|path| path.to_string_lossy())
            .unwrap_or_else(|| {
                let export_text_id = if self.replays.len() == 1 {
                    "replays-export"
                } else {
                    "replays-export-multi"
                };

                i18n::LOCALES.lookup(language, export_text_id).unwrap().into()
            });

        egui::Window::new(path)
            .id(egui::Id::new(("replay-dump-window", id)))
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_enabled_ui(self.cancellation_token.is_none(), |ui| {
                    egui::Grid::new(("replay-dump-window-grid", id))
                        .num_columns(2)
                        .show(ui, |ui| {
                            ui.strong(i18n::LOCALES.lookup(language, "replays-export-path").unwrap());
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut format!("{}", self.output_path.display()))
                                        .desired_width(300.0)
                                        .interactive(false),
                                );

                                if ui
                                    .button(i18n::LOCALES.lookup(language, "replays-export-path.change").unwrap())
                                    .clicked()
                                {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .set_directory(self.output_path.parent().unwrap_or(&std::path::PathBuf::new()))
                                        .set_file_name(
                                            self
                                                .output_path
                                                .file_name()
                                                .and_then(|filename| filename.to_str())
                                                .unwrap_or("replay.mp4"),
                                        )
                                        .add_filter("MP4", &["mp4"])
                                        .save_file()
                                    {
                                        self.output_path = path;

                                        if let Some(folder_path ) = self.output_path.parent() {
                                            config.last_export_folder = Some(folder_path.to_owned());
                                        }
                                    }
                                }
                            });
                            ui.end_row();

                            ui.strong(i18n::LOCALES.lookup(language, "replays-export-scale-factor").unwrap());
                            ui.horizontal(|ui| {
                                let mut scale = self.scale.unwrap_or(1);
                                ui.add_enabled(self.scale.is_some(), egui::DragValue::new(&mut scale).speed(1).range(1..=10));
                                if self.scale.is_some() {
                                    self.scale = Some(scale);
                                }

                                let mut lossless = self.scale.is_none();
                                let was_lossless = lossless;
                                ui.checkbox(&mut lossless, i18n::LOCALES.lookup(language, "replays-export-lossless").unwrap());
                                if lossless {
                                    self.scale = None;
                                } else if was_lossless {
                                    self.scale = Some(DEFAULT_SCALE);
                                }
                            });
                            ui.end_row();

                            ui.strong(i18n::LOCALES.lookup(language, "replays-export-disable-bgm").unwrap());
                            ui.add(egui::Checkbox::new(&mut self.disable_bgm, ""));
                            ui.end_row();

                            ui.strong(i18n::LOCALES.lookup(language, "replays-export-twosided").unwrap());
                            ui.add_enabled(self.remote_rom.is_some(), egui::Checkbox::new(&mut self.twosided, ""));
                            ui.end_row();
                        });
                });

                if let Some(result) = self.result.lock().as_ref() {
                    match result {
                        Ok(()) => {
                            ui.add(
                                egui::widgets::ProgressBar::new(1.0)
                                    .text(i18n::LOCALES.lookup(language, "replays-export-success").unwrap()),
                            );
                            if ui
                                .button(format!(
                                    "📄 {}",
                                    i18n::LOCALES
                                        .lookup(language, "replays-export-open")
                                        .unwrap()
                                ))
                                .clicked()
                            {
                                let _ = open::that(&self.output_path);
                            }
                        }
                        Err(e) => {
                            ui.label(
                                i18n::LOCALES
                                    .lookup_with_args(
                                        language,
                                        "replays-export-error",
                                        &std::collections::HashMap::from([("error", format!("{:?}", e).into())]),
                                    )
                                    .unwrap(),
                            );
                            if ui
                                .button(format!(
                                    "❎ {}",
                                    i18n::LOCALES.lookup(language, "replays-export-confirm-error").unwrap()
                                ))
                                .clicked()
                            {
                                open2 = false;
                            }
                        }
                    }
                } else if self.cancellation_token.is_some() {
                    ui.add(
                        egui::widgets::ProgressBar::new({
                            let (current, total) = *self.progress.lock();
                            if total > 0 {
                                current as f32 / total as f32
                            } else {
                                -1.0
                            }
                        })
                        .show_percentage(),
                    );
                    if ui
                        .button(format!(
                            "❎ {}",
                            i18n::LOCALES.lookup(language, "replays-export-cancel").unwrap(),
                        ))
                        .clicked()
                    {
                        open2 = false;
                    }
                } else if ui
                    .button(format!(
                        "💾 {}",
                        i18n::LOCALES.lookup(language, "replays-export").unwrap()
                    ))
                    .clicked()
                {
                    let egui_ctx = ui.ctx().clone();
                    let local_rom = self.local_rom.clone();
                    let remote_rom = self.remote_rom.clone();
                    let replays = self.replays.clone();
                    let path = self.output_path.clone();
                    let progress = self.progress.clone();
                    let result = self.result.clone();
                    let mut settings = tango_pvp::replay::export::Settings::default_with_scale(self.scale);
                    let twosided = self.twosided;
                    settings.disable_bgm = self.disable_bgm;
                    let cancellation_token = tokio_util::sync::CancellationToken::new();
                    self.cancellation_token = Some(cancellation_token.clone());
                    tokio::task::spawn(async move {
                        let cb = |current, total| {
                            *progress.lock() = (current, total);
                            egui_ctx.request_repaint();
                        };

                        let first_replay = &replays[0];

                        if twosided {
                            let local_game_info = first_replay
                                .metadata
                                .local_side
                                .as_ref()
                                .and_then(|side| side.game_info.as_ref())
                                .ok_or(anyhow::anyhow!("missing local game info")).unwrap();
                            let local_game = crate::game::find_by_family_and_variant(&local_game_info.rom_family, local_game_info.rom_variant as u8).unwrap();
                            let local_hooks = tango_pvp::hooks::hooks_for_gamedb_entry(local_game.gamedb_entry()).unwrap();

                            let remote_game_info = first_replay
                                .metadata
                                .remote_side
                                .as_ref()
                                .and_then(|side| side.game_info.as_ref())
                                .ok_or(anyhow::anyhow!("missing remote game info")).unwrap();
                            let remote_game = crate::game::find_by_family_and_variant(&remote_game_info.rom_family, remote_game_info.rom_variant as u8).unwrap();
                            let remote_hooks = tango_pvp::hooks::hooks_for_gamedb_entry(remote_game.gamedb_entry()).unwrap();

                            tokio::select! {
                                r = tango_pvp::replay::export::export_twosided(&local_rom, local_hooks, remote_rom.as_ref().unwrap(), remote_hooks, &replays, &path, &settings, cb) => {
                                    *result.lock() = Some(r);
                                    egui_ctx.request_repaint();
                                }
                                _ = cancellation_token.cancelled() => { }
                            }
                        } else {
                            let local_game_info = first_replay
                                .metadata
                                .local_side
                                .as_ref()
                                .and_then(|side| side.game_info.as_ref())
                                .ok_or(anyhow::anyhow!("missing local game info")).unwrap();
                            let local_game = crate::game::find_by_family_and_variant(&local_game_info.rom_family, local_game_info.rom_variant as u8).unwrap();
                            let local_hooks = tango_pvp::hooks::hooks_for_gamedb_entry(local_game.gamedb_entry()).unwrap();

                            tokio::select! {
                                r = tango_pvp::replay::export::export(&local_rom, local_hooks, &replays, &path, &settings, cb) => {
                                    *result.lock() = Some(r);
                                    egui_ctx.request_repaint();
                                }
                                _ = cancellation_token.cancelled() => { }
                            }
                        }
                    });
                }
            });

        open && open2
    }
}

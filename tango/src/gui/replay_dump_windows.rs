use fluent_templates::Loader;

use crate::i18n;

pub struct State {
    children: std::collections::HashMap<u64, ChildState>,
    next_id: u64,
}

const DEFAULT_SCALE: usize = 5;

impl State {
    pub fn new() -> Self {
        Self {
            children: std::collections::HashMap::new(),
            next_id: 0,
        }
    }

    pub fn add_child(
        &mut self,
        local_rom: Vec<u8>,
        remote_rom: Option<Vec<u8>>,
        replays: Vec<tango_pvp::replay::Replay>,
        output_path: std::path::PathBuf,
    ) {
        let id = self.next_id;
        self.next_id += 1;
        self.children.insert(
            id,
            ChildState {
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
            },
        );
    }
}

pub struct ChildState {
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

impl Drop for ChildState {
    fn drop(&mut self) {
        if let Some(cancellation_token) = self.cancellation_token.take() {
            cancellation_token.cancel();
        }
    }
}

pub fn show(ctx: &egui::Context, state: &mut State, language: &unic_langid::LanguageIdentifier) {
    state.children.retain(|id, state| {
        let mut open = true;
        let mut open2 = open;

        let path = state.output_path.file_name().map(|path| path.to_string_lossy())
        .unwrap_or_else(|| {
            let export_text_id = if state.replays.len() == 1 {
                "replays-export"
            } else {
                "replays-export-multi"
            };

            i18n::LOCALES.lookup(language, export_text_id).unwrap().into()
        });

        egui::Window::new(path)
            .id(egui::Id::new(format!("replay-dump-window-{}", id)))
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_enabled_ui(state.cancellation_token.is_none(), |ui| {
                    egui::Grid::new(format!("replay-dump-window-{}-grid", id))
                        .num_columns(2)
                        .show(ui, |ui| {
                            ui.strong(i18n::LOCALES.lookup(language, "replays-export-path").unwrap());
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut format!("{}", state.output_path.display()))
                                        .desired_width(300.0)
                                        .interactive(false),
                                );

                                if ui
                                    .button(i18n::LOCALES.lookup(language, "replays-export-path.change").unwrap())
                                    .clicked()
                                {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .set_directory(state.output_path.parent().unwrap_or(&std::path::PathBuf::new()))
                                        .set_file_name(
                                            state
                                                .output_path
                                                .file_name()
                                                .and_then(|filename| filename.to_str())
                                                .unwrap_or("replay.mp4"),
                                        )
                                        .add_filter("MP4", &["mp4"])
                                        .save_file()
                                    {
                                        state.output_path = path;
                                    }
                                }
                            });
                            ui.end_row();

                            ui.strong(i18n::LOCALES.lookup(language, "replays-export-scale-factor").unwrap());
                            ui.horizontal(|ui| {
                                let mut scale = state.scale.unwrap_or(1);
                                ui.add_enabled(state.scale.is_some(), egui::DragValue::new(&mut scale).speed(1).clamp_range(1..=10));
                                if state.scale.is_some() {
                                    state.scale = Some(scale);
                                }

                                let mut lossless = state.scale.is_none();
                                let was_lossless = lossless;
                                ui.checkbox(&mut lossless, i18n::LOCALES.lookup(language, "replays-export-lossless").unwrap());
                                if lossless {
                                    state.scale = None;
                                } else if was_lossless {
                                    state.scale = Some(DEFAULT_SCALE);
                                }
                            });
                            ui.end_row();

                            ui.strong(i18n::LOCALES.lookup(language, "replays-export-disable-bgm").unwrap());
                            ui.add(egui::Checkbox::new(&mut state.disable_bgm, ""));
                            ui.end_row();

                            ui.strong(i18n::LOCALES.lookup(language, "replays-export-twosided").unwrap());
                            ui.add_enabled(state.remote_rom.is_some(), egui::Checkbox::new(&mut state.twosided, ""));
                            ui.end_row();
                        });
                });

                if let Some(result) = state.result.lock().as_ref() {
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
                                let _ = open::that(&state.output_path);
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
                } else if state.cancellation_token.is_some() {
                    ui.add(
                        egui::widgets::ProgressBar::new({
                            let (current, total) = *state.progress.lock();
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
                    let local_rom = state.local_rom.clone();
                    let remote_rom = state.remote_rom.clone();
                    let replays = state.replays.clone();
                    let path = state.output_path.clone();
                    let progress = state.progress.clone();
                    let result = state.result.clone();
                    let mut settings = tango_pvp::replay::export::Settings::default_with_scale(state.scale);
                    let twosided = state.twosided;
                    settings.disable_bgm = state.disable_bgm;
                    let cancellation_token = tokio_util::sync::CancellationToken::new();
                    state.cancellation_token = Some(cancellation_token.clone());
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
    });
}

use crate::{i18n, replay};

pub struct State {
    children: std::collections::HashMap<u64, ChildState>,
    next_id: u64,
}

impl State {
    pub fn new() -> Self {
        Self {
            children: std::collections::HashMap::new(),
            next_id: 0,
        }
    }

    pub fn add_child(&mut self, rom: Vec<u8>, replay: replay::Replay, path: std::path::PathBuf) {
        let id = self.next_id;
        self.next_id += 1;
        let mut output_path = path.clone();
        output_path.set_extension("mp4");
        self.children.insert(
            id,
            ChildState {
                cancellation_token: None,
                output_path,
                rom,
                replay,
                path,
                progress: std::sync::Arc::new(parking_lot::Mutex::new((0, 0))),
                result: std::sync::Arc::new(parking_lot::Mutex::new(None)),
            },
        );
    }
}

pub struct ChildState {
    cancellation_token: Option<tokio_util::sync::CancellationToken>,
    output_path: std::path::PathBuf,
    rom: Vec<u8>,
    replay: replay::Replay,
    path: std::path::PathBuf,
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

pub fn show(
    ctx: &egui::Context,
    handle: tokio::runtime::Handle,
    state: &mut State,
    replays_path: &std::path::Path,
) {
    state.children.retain(|id, state| {
        let mut open = true;
        let mut open2 = open;
        let path = state
            .path
            .strip_prefix(replays_path)
            .unwrap_or(state.path.as_path());
        egui::Window::new(format!("{}", path.display()))
            .id(egui::Id::new(format!("replay-dump-window-{}", id)))
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_enabled_ui(state.cancellation_token.is_none(), |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut format!("{}", state.output_path.display()))
                                .interactive(false),
                        );

                        if ui
                            .button("TODO: CHANGE")
                            .clicked()
                        {
                            if let Some(path) = native_dialog::FileDialog::new()
                                .set_location(state.output_path.parent().unwrap_or(&std::path::PathBuf::new()))
                                .set_filename(state.output_path.file_name().and_then(|filename| filename.to_str()).unwrap_or("replay.mp4"))
                                .add_filter("MP4", &["mp4"])
                                .show_save_single_file()
                                .unwrap()
                            {
                                state.output_path = path;
                            }
                        }
                    });
                });

                if let Some(result) = state.result.lock().as_ref() {
                    match result {
                        Ok(()) => {
                            ui.label("TODO: DONE");
                        }
                        Err(e) =>{
                            ui.label(format!("ERROR: {:?}", e));
                        }
                    }
                    if ui.button("TODO: OK").clicked() {
                        open2 = false;
                    }
                } else if state.cancellation_token.is_some() {
                    ui.add(egui::widgets::ProgressBar::new({
                        let (current, total) = *state.progress.lock();
                        if total > 0 { current as f32 / total as f32 } else { -1.0 }
                    }).show_percentage());
                    if ui.button("TODO: CANCEL").clicked() {
                        open2 = false;
                    }
                } else {
                    if ui.button("TODO: EXPORT").clicked() {
                        let egui_ctx = ui.ctx().clone();
                        let rom = state.rom.clone();
                        let replay = state.replay.clone();
                        let path = state.output_path.clone();
                        let progress = state.progress.clone();
                        let result = state.result.clone();
                        let cancellation_token = tokio_util::sync::CancellationToken::new();
                        state.cancellation_token = Some(cancellation_token.clone());
                        handle.spawn(async move {
                            let settings = replay::export::Settings::default();
                            tokio::select! {
                                r = replay::export::export(&rom, &replay, &path, &settings, |current, total| {
                                    *progress.lock() = (current, total);
                                    egui_ctx.request_repaint();
                                }) => {
                                    *result.lock() = Some(r);
                                    egui_ctx.request_repaint();
                                }
                                _ = cancellation_token.cancelled() => { }
                            }
                        });
                    }
                }
            });

        open && open2
    });
}

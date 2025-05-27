use super::{memoize::ResultCacheSingle, replay_dump_window::ReplayDumpWindow};
use crate::{config, game, gui, i18n, patch, scanner, session};
use fluent_templates::Loader;
use itertools::Itertools;
use std::{borrow::Cow, rc::Rc, sync::Arc};
use tango_dataview::save::Save;
use tango_pvp::replay::Replay;

struct CachedData {
    replay: Replay,
    patch: Option<(String, semver::Version, Arc<crate::patch::Version>)>,
    rom_assets: Option<Box<dyn tango_dataview::rom::Assets + Send + Sync>>,
    local_rom: Vec<u8>,
    remote_rom: Option<Vec<u8>>,
    save: Box<dyn Save + Sync + Send>,
}

pub struct State {
    replays_scanner: scanner::Scanner<Vec<(std::path::PathBuf, bool, tango_pvp::replay::Metadata)>>,
    selection: Option<std::ops::Range<usize>>,
    folder_filter: Option<std::path::PathBuf>,
    save_view: gui::save_view::State,
    replay_cache: ResultCacheSingle<std::path::PathBuf, Option<Rc<CachedData>>>,
}

impl State {
    pub fn new() -> Self {
        Self {
            selection: None,
            replays_scanner: scanner::Scanner::new(),
            folder_filter: None,
            save_view: gui::save_view::State::new(),
            replay_cache: Default::default(),
        }
    }

    fn update_selection(&mut self, i: usize, multi_select: bool) {
        if multi_select {
            if let Some(range) = &mut self.selection {
                if i < range.start {
                    range.start = i;
                } else if i + 1 > range.end {
                    range.end = i + 1;
                }

                return;
            }
        }

        let new_selection = Some(i..i + 1);

        if self.selection == new_selection {
            // deselect
            self.selection = None;
            return;
        }

        self.save_view = gui::save_view::State::new();
        self.selection = new_selection;
    }

    pub fn rescan(&self, ctx: &egui::Context, replays_path: &std::path::Path) {
        tokio::task::spawn_blocking({
            let replays_scanner = self.replays_scanner.clone();
            let replays_path = replays_path.to_path_buf();
            let egui_ctx = ctx.clone();
            move || {
                replays_scanner.rescan(move || {
                    let mut replays = vec![];
                    for entry in walkdir::WalkDir::new(&replays_path) {
                        let entry = match entry {
                            Ok(entry) => entry,
                            Err(_) => {
                                continue;
                            }
                        };

                        if !entry.file_type().is_file() {
                            continue;
                        }

                        let path = entry.path();
                        let mut f = match std::fs::File::open(path) {
                            Ok(f) => f,
                            Err(_) => {
                                continue;
                            }
                        };

                        let (num_inputs, metadata) = match tango_pvp::replay::read_metadata(&mut f) {
                            Ok((n, metadata)) => (n, metadata),
                            Err(_) => {
                                continue;
                            }
                        };

                        replays.push((path.to_path_buf(), num_inputs > 0, metadata));
                    }
                    replays.sort_by_key(|(_, _, metadata)| {
                        (
                            std::cmp::Reverse(metadata.ts),
                            metadata.link_code.clone(),
                            metadata.round,
                        )
                    });
                    Some(replays)
                });
                egui_ctx.request_repaint();
            }
        });
    }
}

fn format_path(replays_path: &std::path::Path, path: &std::path::Path) -> String {
    let string = path.strip_prefix(replays_path).unwrap_or(path).to_string_lossy();

    if string.is_empty() {
        String::from("/")
    } else {
        format!("/{string}/")
    }
}

pub fn show(
    ui: &mut egui::Ui,
    config: &config::Config,
    shared_root_state: &mut gui::SharedRootState,
    state: &mut State,
) {
    let language = &config.language;
    let patches_path = &config.patches_path();
    let replays_path = &config.replays_path();

    let roms_scanner = shared_root_state.scanners.roms.clone();
    let patches_scanner = shared_root_state.scanners.patches.clone();
    let roms = roms_scanner.read();
    let patches = patches_scanner.read();

    let max_dropdown_height = ui.available_height();

    egui::Frame::default()
        .inner_margin(egui::Margin {
            bottom: 2,
            ..egui::Margin::from(8)
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let default_label = i18n::LOCALES.lookup(language, "replays-all-replays").unwrap();

                egui::ComboBox::from_id_salt("game-select-combobox")
                    .selected_text(
                        state
                            .folder_filter
                            .as_ref()
                            .map(|path| Cow::Owned(format_path(replays_path, path)))
                            .unwrap_or(Cow::Borrowed(default_label.as_str())),
                    )
                    .wrap()
                    .width(ui.spacing().combo_width.max(ui.available_width() / 3.0))
                    .height(max_dropdown_height)
                    .show_ui(ui, |ui| {
                        const CHAR_WIDTH: f32 = 6.5;

                        let replays = state.replays_scanner.read();
                        let mut parent_folders = replays
                            .iter()
                            .flat_map(|(path, _, _)| path.parent())
                            .unique()
                            .map(|path| (path, format_path(replays_path, path)))
                            .collect::<Vec<_>>();

                        parent_folders.sort_by(|(_, a), (_, b)| a.cmp(b));

                        // attempt to provide room to fix weird staircasing from using an imgui
                        let mut max_width: f32 = default_label.len() as f32 * CHAR_WIDTH;

                        for (_, path_str) in &parent_folders {
                            max_width = max_width.max(path_str.len() as f32 * CHAR_WIDTH);
                        }

                        ui.allocate_space(egui::Vec2::new(max_width, 0.0));

                        // render options
                        if ui.selectable_label(false, default_label.as_str()).clicked() {
                            state.folder_filter = None;
                        }

                        for (path, path_str) in parent_folders {
                            if ui.selectable_label(false, path_str).clicked() {
                                state.folder_filter = Some(path.to_path_buf());
                            }
                        }
                    });
            });
        });

    ui.separator();

    egui::SidePanel::left("replays-window-left-panel")
        .frame(egui::Frame::default().inner_margin(egui::Margin {
            right: 8,
            ..Default::default()
        }))
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .id_salt("replays-window-left")
                .show(ui, |ui| {
                    if state.replays_scanner.is_scanning() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(i18n::LOCALES.lookup(language, "replays-scanning").unwrap());
                        });
                        return;
                    }

                    let replays = state.replays_scanner.read();
                    let mut clicked_index = None;

                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        let mut last_fingerprint = None;
                        let mut alternate = true;

                        // remove gaps between items, we'll handle this through padding
                        let default_spacing = ui.style().spacing.item_spacing;
                        ui.style_mut().spacing.item_spacing = Default::default();

                        let folder_filter = state.folder_filter.as_ref();

                        for (i, (path, _, metadata)) in replays.iter().enumerate() {
                            if folder_filter.is_some_and(|filter| path.parent().is_some_and(|parent| parent != filter))
                            {
                                continue;
                            }

                            let Some(ts) =
                                std::time::UNIX_EPOCH.checked_add(std::time::Duration::from_millis(metadata.ts))
                            else {
                                continue;
                            };

                            let Some(local_side) = metadata.local_side.as_ref() else {
                                continue;
                            };

                            let Some(remote_side) = metadata.remote_side.as_ref() else {
                                continue;
                            };

                            let Some(local_game_info) = local_side.game_info.as_ref() else {
                                continue;
                            };

                            let Some(remote_game_info) = remote_side.game_info.as_ref() else {
                                continue;
                            };

                            let Some(local_game) = game::find_by_family_and_variant(
                                local_game_info.rom_family.as_str(),
                                local_game_info.rom_variant as u8,
                            ) else {
                                continue;
                            };

                            // resolve background color for visual grouping
                            let fingerprint = Some((
                                &metadata.link_code,
                                local_game_info,
                                remote_game_info,
                                &remote_side.nickname,
                            ));

                            if fingerprint != last_fingerprint {
                                alternate = !alternate;
                                last_fingerprint = fingerprint;
                            }

                            let mut frame = egui::Frame::default().inner_margin(default_spacing);

                            if alternate {
                                frame = frame.fill(ui.ctx().style().visuals.faint_bg_color);
                            }

                            frame.show(ui, |ui| {
                                let text_body_style = ui.style().text_styles.get(&egui::TextStyle::Body).unwrap();
                                let text_small_style = ui.style().text_styles.get(&egui::TextStyle::Small).unwrap();

                                let selected = state.selection.as_ref().is_some_and(|r| r.contains(&i));

                                let text_color = if selected {
                                    ui.ctx().style().visuals.selection.stroke.color
                                } else {
                                    ui.visuals().text_color()
                                };

                                let mut layout_job = egui::text::LayoutJob::default();
                                layout_job.append(
                                    &chrono::DateTime::<chrono::Local>::from(ts).to_string(),
                                    0.0,
                                    egui::TextFormat::simple(text_body_style.clone(), text_color),
                                );
                                layout_job.append(
                                    "\n",
                                    0.0,
                                    egui::TextFormat::simple(text_body_style.clone(), text_color),
                                );
                                layout_job.append(
                                    &i18n::LOCALES
                                        .lookup_with_args(
                                            language,
                                            "replay-subtitle",
                                            &std::collections::HashMap::from([
                                                (
                                                    "game_family",
                                                    i18n::LOCALES
                                                        .lookup(
                                                            language,
                                                            &format!(
                                                                "game-{}.short",
                                                                local_game.gamedb_entry().family_and_variant.0
                                                            ),
                                                        )
                                                        .unwrap()
                                                        .into(),
                                                ),
                                                ("link_code", metadata.link_code.clone().into()),
                                                ("nickname", remote_side.nickname.clone().into()),
                                            ]),
                                        )
                                        .unwrap(),
                                    0.0,
                                    egui::TextFormat::simple(text_small_style.clone(), text_color),
                                );

                                if ui.selectable_label(selected, layout_job).clicked() {
                                    clicked_index = Some(i);
                                }
                            });
                        }
                    });

                    std::mem::drop(replays);

                    if let Some(i) = clicked_index {
                        let shift_held = ui.input(|i| i.modifiers.shift);
                        state.update_selection(i, shift_held);
                    }
                });
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("replays-window-info")
            .vscroll(false)
            .show(ui, |ui| {
                let Some(selection) = state.selection.as_mut() else {
                    return;
                };

                let replays = state.replays_scanner.read();
                let (path, _, metadata) = &replays[selection.start];

                let Some(local_side) = metadata.local_side.as_ref() else {
                    return;
                };

                let Some(local_game_info) = local_side.game_info.as_ref() else {
                    return;
                };

                let Some(local_game) = game::find_by_family_and_variant(
                    local_game_info.rom_family.as_str(),
                    local_game_info.rom_variant as u8,
                ) else {
                    return;
                };

                let cached_result = state.replay_cache.calculate(path.clone(), |path| {
                    let remote_side = metadata.remote_side.as_ref()?;
                    let remote_game_info = remote_side.game_info.as_ref()?;

                    let remote_game = game::find_by_family_and_variant(
                        remote_game_info.rom_family.as_str(),
                        remote_game_info.rom_variant as u8,
                    )?;

                    let mut f = match std::fs::File::open(&path) {
                        Ok(f) => f,
                        Err(e) => {
                            log::error!("failed to load replay {}: {:?}", path.display(), e);
                            return None;
                        }
                    };

                    let replay = match tango_pvp::replay::Replay::decode(&mut f) {
                        Ok(replay) => replay,
                        Err(e) => {
                            log::error!("failed to load replay {}: {:?}", path.display(), e);
                            return None;
                        }
                    };

                    let save = match local_game.save_from_wram(replay.local_state.wram()) {
                        Ok(save) => save,
                        Err(e) => {
                            log::error!("failed to load replay {}: {:?}", path.display(), e);
                            return None;
                        }
                    };

                    let mut local_rom = roms.get(&local_game).cloned()?;

                    let patch = if let Some(patch_info) = local_game_info.patch.as_ref() {
                        let patch = patches.get(&patch_info.name)?;
                        let version = semver::Version::parse(&patch_info.version).ok()?;
                        let version_meta = patch.versions.get(&version)?;

                        let (rom_code, revision) = local_game.gamedb_entry().rom_code_and_revision;

                        local_rom = match patch::apply_patch_from_disk(
                            &local_rom,
                            local_game,
                            patches_path,
                            &patch_info.name,
                            &version,
                        ) {
                            Ok(r) => r,
                            Err(e) => {
                                log::error!(
                                    "failed to apply patch {}: {:?}: {:?}",
                                    patch_info.name,
                                    (rom_code, revision),
                                    e
                                );
                                return None;
                            }
                        };

                        Some((patch_info.name.clone(), version, version_meta.clone()))
                    } else {
                        None
                    };

                    let assets = match local_game.load_rom_assets(
                        &local_rom,
                        replay.local_state.wram(),
                        &patch
                            .as_ref()
                            .map(|(_, _, metadata)| metadata.rom_overrides.clone())
                            .unwrap_or_default(),
                    ) {
                        Ok(assets) => Some(assets),
                        Err(e) => {
                            log::error!("failed to load assets: {:?}", e);
                            None
                        }
                    };

                    let remote_rom = roms.get(&remote_game).and_then(|rom| {
                        let mut rom = rom.clone();

                        if let Some(patch_info) = remote_game_info.patch.as_ref() {
                            let Ok(version) = semver::Version::parse(&patch_info.version) else {
                                return None;
                            };

                            let (rom_code, revision) = remote_game.gamedb_entry().rom_code_and_revision;

                            rom = match patch::apply_patch_from_disk(
                                &rom,
                                remote_game,
                                patches_path,
                                &patch_info.name,
                                &version,
                            ) {
                                Ok(r) => r,
                                Err(e) => {
                                    log::error!(
                                        "failed to apply patch {}: {:?}: {:?}",
                                        patch_info.name,
                                        (rom_code, revision),
                                        e
                                    );
                                    return None;
                                }
                            };
                        }

                        Some(rom)
                    });

                    Some(Rc::new(CachedData {
                        replay,
                        rom_assets: assets,
                        local_rom,
                        remote_rom,
                        patch,
                        save,
                    }))
                });

                let Some(cached_result) = cached_result else {
                    return;
                };

                let local_rom = &cached_result.local_rom;
                let remote_rom = &cached_result.remote_rom;
                let assets = &cached_result.rom_assets;
                let patch = &cached_result.patch;
                let save = &cached_result.save;
                let replay = cached_result.replay.clone();

                ui.vertical(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        if ui
                            .button(format!(
                                "▶️ {}",
                                i18n::LOCALES.lookup(language, "replays-play").unwrap()
                            ))
                            .clicked()
                        {
                            tokio::task::spawn_blocking({
                                let egui_ctx = ui.ctx().clone();
                                let audio_binder = shared_root_state.audio_binder.clone();
                                let game = local_game;
                                let patch = patch.as_ref().map(|(name, version, _)| (name.clone(), version.clone()));
                                let rom = local_rom.clone();
                                let emu_tps_counter = shared_root_state.emu_tps_counter.clone();
                                let replay = replay.clone();
                                let session = shared_root_state.session.clone();

                                move || {
                                    *session.lock() = Some(
                                        session::Session::new_replayer(
                                            audio_binder,
                                            game,
                                            patch,
                                            &rom,
                                            emu_tps_counter,
                                            &replay,
                                        )
                                        .unwrap(),
                                    ); // TODO: Don't unwrap maybe
                                    egui_ctx.request_repaint();
                                }
                            });
                        }

                        let export_text_id = if selection.len() == 1 {
                            "replays-export"
                        } else {
                            "replays-export-multi"
                        };

                        let export_label = format!("💾 {}", i18n::LOCALES.lookup(language, export_text_id).unwrap());

                        if ui.button(export_label).clicked() {
                            let replays_to_render = replays[selection.clone()]
                                .iter()
                                .rev()
                                .flat_map(|(path, _, _)| {
                                    let mut f = match std::fs::File::open(path) {
                                        Ok(f) => f,
                                        Err(e) => {
                                            log::error!("failed to load replay {}: {:?}", path.display(), e);
                                            return None;
                                        }
                                    };

                                    match tango_pvp::replay::Replay::decode(&mut f) {
                                        Ok(replay) => Some(replay),
                                        Err(e) => {
                                            log::error!("failed to load replay {}: {:?}", path.display(), e);
                                            None
                                        }
                                    }
                                })
                                .collect();

                            let mut save_path = if let Some(folder) = &config.last_export_folder {
                                let mut save_path = folder.clone();
                                save_path.push(path.file_name().unwrap());
                                save_path
                            } else {
                                path.clone()
                            };

                            if selection.len() > 1 {
                                save_path.set_extension("multi.mp4");
                            } else {
                                save_path.set_extension("mp4");
                            }

                            let mut window = ReplayDumpWindow::new(
                                local_rom.clone(),
                                remote_rom.clone(),
                                replays_to_render,
                                save_path,
                            );
                            shared_root_state
                                .ui_windows
                                .push(move |id, ctx, config, _| window.show(id, ctx, config));
                        }

                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                            ui.horizontal(|ui| {
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Max).with_main_wrap(true),
                                    |ui| {
                                        ui.heading(format!(
                                            "{}",
                                            path.strip_prefix(replays_path).unwrap_or(path.as_path()).display()
                                        ));
                                    },
                                );
                            });
                        });
                    });

                    if let Some(assets) = assets.as_ref() {
                        let game_language = crate::game::region_to_language(local_game.gamedb_entry().region);
                        gui::save_view::show(
                            ui,
                            false,
                            config,
                            shared_root_state,
                            patch
                                .as_ref()
                                .and_then(|(_, _, metadata)| metadata.rom_overrides.language.as_ref())
                                .unwrap_or(&game_language),
                            save.as_ref(),
                            assets.as_ref(),
                            &mut state.save_view,
                            false,
                        );
                    }
                });
            });
    });
}

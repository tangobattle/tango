use crate::patch::{self, PatchMap};
use crate::{game, gui, i18n, net, rom, save};
use fluent_templates::Loader;
use std::io::Write;

#[derive(Clone)]
pub struct Selection {
    pub game: &'static (dyn game::Game + Send + Sync),
    pub save_path: Option<std::path::PathBuf>,
    pub patch: Option<(String, semver::Version, std::sync::Arc<patch::Version>)>,
}

pub struct State {
    selection: Option<Selection>,
}

impl State {
    pub fn new(selection: Option<Selection>) -> Self {
        Self { selection }
    }
}

fn save_name<'a>(saves_path: &std::path::Path, save_path: &'a std::path::Path) -> std::borrow::Cow<'a, str> {
    save_path
        .strip_prefix(saves_path)
        .map(|path| path.to_string_lossy())
        .unwrap_or_else(|_| save_path.to_string_lossy())
}

fn create_next_file(path: &std::path::Path) -> Result<(std::path::PathBuf, std::fs::File), std::io::Error> {
    let mut counter = 0;

    let file_stem = path.file_stem().unwrap_or_else(|| std::ffi::OsStr::new(""));
    let ext = path.extension().unwrap_or_else(|| std::ffi::OsStr::new(""));

    loop {
        let mut new_path = path.to_path_buf();
        if counter > 0 {
            let mut new_file_name = file_stem.to_os_string();
            new_file_name.push(format!(" {}", counter));
            if !ext.is_empty() {
                new_file_name.push(".");
                new_file_name.push(ext);
            }
            new_path.set_file_name(new_file_name);
        }
        match std::fs::File::options().write(true).create_new(true).open(&new_path) {
            Ok(f) => {
                return Ok((new_path, f));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                counter += 1;
                continue;
            }
            Err(e) => {
                return Err(e);
            }
        }
    }
}

fn create_new_save(
    language: &unic_langid::LanguageIdentifier,
    saves_path: &std::path::Path,
    game: &(dyn game::Game + Send + Sync),
    patch: Option<&(String, semver::Version, std::sync::Arc<patch::Version>)>,
    name: &str,
) -> Result<(std::path::PathBuf, std::fs::File), std::io::Error> {
    let (family, variant) = game.gamedb_entry().family_and_variant;
    let mut prefix = i18n::LOCALES
        .lookup(language, &format!("game-{}.variant-{}", family, variant))
        .unwrap();

    if let Some((name, version, _)) = patch {
        prefix.push_str(" + ");
        prefix.push_str(&format!("{} v{}", name, version));
    }

    if !name.is_empty() {
        prefix.push_str(" - ");
        prefix.push_str(
            &i18n::LOCALES
                .lookup(language, &format!("game-{}.save-{}", family, name))
                .unwrap(),
        );
    }
    prefix = prefix.replace(':', "").replace('/', " ");
    prefix.push_str(".sav");

    create_next_file(&saves_path.join(prefix))
}

fn commit_patch(
    roms: &std::collections::HashMap<&'static (dyn crate::game::Game + Send + Sync), Vec<u8>>,
    patches_path: &std::path::Path,
    committed_selection: &mut Option<gui::Selection>,
    selection_state: &Selection,
) {
    let Some(committed_selection) = committed_selection else {
        return;
    };

    let mut rom = roms.get(&selection_state.game).unwrap().clone();

    if let Some((name, version, _)) = selection_state.patch.as_ref() {
        let (rom_code, revision) = selection_state.game.gamedb_entry().rom_code_and_revision;
        rom = match patch::apply_patch_from_disk(&rom, selection_state.game, patches_path, name, version) {
            Ok(r) => r,
            Err(e) => {
                log::error!("failed to apply patch {}: {:?}: {:?}", name, (rom_code, revision), e);
                return;
            }
        };
    }

    committed_selection.rom = rom;
    committed_selection.patch = selection_state.patch.clone();
}

fn commit_save(
    roms: &std::collections::HashMap<&'static (dyn crate::game::Game + Send + Sync), Vec<u8>>,
    patches_path: &std::path::Path,
    committed_selection: &mut Option<gui::Selection>,
    selection_state: &Selection,
    save: crate::save::ScannedSave,
) {
    let (game, rom, patch) = if let Some(committed_selection) = committed_selection
        .take()
        .filter(|committed_selection| committed_selection.game == selection_state.game)
    {
        (
            committed_selection.game,
            committed_selection.rom,
            committed_selection.patch,
        )
    } else {
        let mut rom = roms.get(&selection_state.game).unwrap().clone();
        if let Some((name, version, _)) = selection_state.patch.as_ref() {
            let (rom_code, revision) = selection_state.game.gamedb_entry().rom_code_and_revision;
            rom = match patch::apply_patch_from_disk(&rom, selection_state.game, patches_path, name, version) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("failed to apply patch {}: {:?}: {:?}", name, (rom_code, revision), e);
                    return;
                }
            };
        }
        (selection_state.game, rom, selection_state.patch.clone())
    };

    *committed_selection = Some(gui::Selection::new(game, save, patch, rom));
}

fn rescan_saves(config: &crate::config::Config, saves_scanner: &save::Scanner, egui_ctx: &egui::Context) {
    let saves_scanner = saves_scanner.clone();
    let saves_path = config.saves_path();
    let egui_ctx = egui_ctx.clone();

    tokio::task::spawn_blocking(move || {
        saves_scanner.rescan(move || Some(save::scan_saves(&saves_path)));
        egui_ctx.request_repaint();
    });
}

fn game_compatibility_warning(
    patches: &PatchMap,
    remote_settings: Option<&net::protocol::Settings>,
    game: &'static (dyn crate::game::Game + Send + Sync),
) -> Option<gui::play_pane::Warning> {
    let remote_settings = remote_settings.as_ref()?;

    // check if the remote has our rom
    let remote_has_rom = remote_settings
        .available_games
        .iter()
        .any(|(family, variant)| game.gamedb_entry().family_and_variant == (family, *variant));

    if !remote_has_rom {
        return Some(gui::play_pane::Warning::NoRemoteROM(game));
    }

    // check if the remote is using a compatible rom + patch
    let remote_gi = remote_settings.game_info.as_ref()?;

    if let Some(netplay_compatibility) = gui::play_pane::get_netplay_compatibility_from_game_info(remote_gi, patches) {
        let family = game.gamedb_entry().family_and_variant.0;

        if netplay_compatibility != family
            && !patches.values().any(|metadata| {
                metadata.versions.values().any(|version| {
                    version.supported_games.contains(&game) && version.netplay_compatibility == netplay_compatibility
                })
            })
        {
            return Some(gui::play_pane::Warning::Incompatible);
        }
    }
    None
}

fn patch_compatibility_warning(
    patches: &PatchMap,
    remote_settings: Option<&net::protocol::Settings>,
    game: &'static (dyn crate::game::Game + Send + Sync),
    patch_name: Option<&str>,
) -> Option<gui::play_pane::Warning> {
    let remote_settings = remote_settings.as_ref()?;
    let remote_gi = remote_settings.game_info.as_ref()?;

    let remote_game =
        game::find_by_family_and_variant(&remote_gi.family_and_variant.0, remote_gi.family_and_variant.1)?;

    if let Some(patch_name) = patch_name {
        if !remote_settings.available_patches.iter().any(|(n, _)| patch_name == n) {
            return Some(gui::play_pane::Warning::NoRemotePatches(patch_name.to_string()));
        }

        let local_netplay_compatibilities: Vec<_> = patches
            .get(patch_name)
            .map(|patch| {
                patch
                    .versions
                    .values()
                    .map(|vi| vi.netplay_compatibility.as_str())
                    .collect()
            })
            .unwrap_or_else(|| vec![game.gamedb_entry().family_and_variant.0]);

        if let Some(nc) = gui::play_pane::get_netplay_compatibility(
            remote_game,
            remote_gi.patch.as_ref().map(|pi| (pi.name.as_str(), &pi.version)),
            patches,
        ) {
            if !local_netplay_compatibilities.contains(&nc.as_str()) {
                return Some(gui::play_pane::Warning::Incompatible);
            }
        }
    } else if let Some(nc) = gui::play_pane::get_netplay_compatibility(
        remote_game,
        remote_gi.patch.as_ref().map(|pi| (pi.name.as_str(), &pi.version)),
        patches,
    ) {
        if nc != game.gamedb_entry().family_and_variant.0 {
            return Some(gui::play_pane::Warning::Incompatible);
        }
    }

    None
}

fn patch_version_compatibility_warning(
    patches: &PatchMap,
    remote_settings: Option<&net::protocol::Settings>,
    game: &'static (dyn crate::game::Game + Send + Sync),
    patch: Option<(&str, &semver::Version)>,
) -> Option<gui::play_pane::Warning> {
    let remote_settings = remote_settings.as_ref()?;
    let remote_gi = remote_settings.game_info.as_ref()?;

    let remote_game =
        game::find_by_family_and_variant(&remote_gi.family_and_variant.0, remote_gi.family_and_variant.1)?;

    if let Some((patch_name, patch_version)) = patch {
        let remote_has_patch = remote_settings
            .available_patches
            .iter()
            .any(|(name, versions)| patch_name == name && versions.iter().any(|v| v == patch_version));

        if !remote_has_patch {
            return Some(gui::play_pane::Warning::NoRemotePatch(
                patch_name.to_string(),
                patch_version.clone(),
            ));
        }
    }

    let local_netplay_compatibility = gui::play_pane::get_netplay_compatibility(game, patch, patches);

    let remote_netplay_compatibility = gui::play_pane::get_netplay_compatibility(
        remote_game,
        remote_gi.patch.as_ref().map(|pi| (pi.name.as_str(), &pi.version)),
        patches,
    );

    if local_netplay_compatibility != remote_netplay_compatibility {
        return Some(gui::play_pane::Warning::Incompatible);
    }

    None
}

pub fn show(
    ui: &mut egui::Ui,
    config: &crate::config::Config,
    state: &mut State,
    committed_selection: &mut Option<gui::Selection>,
    patch_selection: &mut Option<String>,
    language: &unic_langid::LanguageIdentifier,
    saves_path: &std::path::Path,
    patches_path: &std::path::Path,
    starred_patches: &std::collections::HashSet<String>,
    roms_scanner: rom::Scanner,
    saves_scanner: save::Scanner,
    patches_scanner: patch::Scanner,
    remote_settings: Option<&net::protocol::Settings>,
) {
    let games = game::sorted_all_games(language);
    let roms = roms_scanner.read();
    let saves = saves_scanner.read();
    let patches = patches_scanner.read();

    const BODY_CHAR_WIDTH: f32 = 6.5;
    const SMALL_CHAR_WIDTH: f32 = 4.5;
    const WARNING_WIDTH: f32 = 9.0;

    const PATCH_VERSION_WIDTH: f32 = 100.0;
    let item_spacing_x = ui.spacing().item_spacing.x;

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            let wide_width = (ui.available_width() - PATCH_VERSION_WIDTH) * 0.5 - item_spacing_x;

            let strip_builder = egui_extras::StripBuilder::new(ui)
                .sizes(egui_extras::Size::exact(wide_width), 2)
                .size(egui_extras::Size::exact(PATCH_VERSION_WIDTH));

            strip_builder.horizontal(|mut strip| {
                // games
                strip.cell(|ui| {
                    let mut layout_job = egui::text::LayoutJob::default();
                    layout_job.wrap.break_anywhere = true;
                    layout_job.wrap.max_rows = 1;

                    let game_warning = state
                        .selection
                        .as_ref()
                        .and_then(|selection| game_compatibility_warning(&patches, remote_settings, selection.game));

                    if game_warning.is_some() {
                        gui::warning::append_to_layout_job(ui, &mut layout_job);
                    }

                    let selected_game_text = if let Some(selection) = &state.selection {
                        let (family, variant) = selection.game.gamedb_entry().family_and_variant;
                        i18n::LOCALES
                            .lookup(language, &format!("game-{}.variant-{}", family, variant))
                            .unwrap()
                    } else {
                        i18n::LOCALES.lookup(language, "play-no-game").unwrap()
                    };

                    layout_job.append(
                        &selected_game_text,
                        0.0,
                        egui::TextFormat::simple(
                            ui.style().text_styles.get(&egui::TextStyle::Body).unwrap().clone(),
                            ui.visuals().text_color(),
                        ),
                    );

                    let resp = egui::ComboBox::from_id_source("game-select-combobox")
                        .selected_text(layout_job)
                        .width(wide_width)
                        .wrap(true)
                        .show_ui(ui, |ui| {
                            // attempt to provide room to fix weird staircasing from using an imgui
                            let mut max_width: f32 = 0.0;

                            for game in &games {
                                let mut width = item_spacing_x * 2.0;

                                let warning = game_compatibility_warning(&patches, remote_settings, *game);

                                if warning.is_some() {
                                    width += WARNING_WIDTH;
                                }

                                let (family, variant) = game.gamedb_entry().family_and_variant;

                                let localized_name = i18n::LOCALES
                                    .lookup(language, &format!("game-{}.variant-{}", family, variant))
                                    .unwrap();

                                width += localized_name.len() as f32 * BODY_CHAR_WIDTH;

                                max_width = max_width.max(width);
                            }

                            ui.allocate_space(egui::Vec2::new(max_width, 0.0));

                            // game list
                            for (available, game) in games
                                .iter()
                                .filter(|g| roms.contains_key(*g))
                                .map(|g| (true, g))
                                .chain(games.iter().filter(|g| !roms.contains_key(*g)).map(|g| (false, g)))
                            {
                                let (family, variant) = game.gamedb_entry().family_and_variant;

                                let selected = state
                                    .selection
                                    .as_ref()
                                    .is_some_and(|selection| selection.game == *game);

                                let warning = game_compatibility_warning(&patches, remote_settings, *game);

                                let mut layout_job = egui::text::LayoutJob::default();
                                if warning.is_some() {
                                    gui::warning::append_to_layout_job(ui, &mut layout_job);
                                }
                                layout_job.append(
                                    &i18n::LOCALES
                                        .lookup(language, &format!("game-{}.variant-{}", family, variant))
                                        .unwrap(),
                                    0.0,
                                    egui::TextFormat::simple(
                                        ui.style().text_styles.get(&egui::TextStyle::Body).unwrap().clone(),
                                        if selected {
                                            ui.visuals().selection.stroke.color
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                    ),
                                );

                                let mut resp =
                                    ui.add_enabled(available, egui::SelectableLabel::new(selected, layout_job));
                                if let Some(warning) = warning {
                                    resp = resp.on_hover_text(warning.description(language));
                                }

                                if resp.clicked() {
                                    *committed_selection = None;
                                    state.selection = Some(Selection {
                                        game: *game,
                                        save_path: None,
                                        patch: None,
                                    });
                                }
                            }
                        })
                        .response;

                    if let Some(warning) = game_warning {
                        resp.on_hover_text(warning.description(language));
                    }
                });

                // patches + patch versions
                let mut supported_patches = std::collections::HashMap::new();
                if let Some(selection) = state.selection.as_ref() {
                    for (name, info) in patches.iter() {
                        let mut supported_versions = info
                            .versions
                            .iter()
                            .filter(|(_, v)| v.supported_games.contains(&selection.game))
                            .map(|(v, _)| v)
                            .collect::<Vec<_>>();
                        supported_versions.sort();
                        supported_versions.reverse();

                        if supported_versions.is_empty() {
                            continue;
                        }

                        supported_patches.insert(name, (info, supported_versions));
                    }
                }

                // patches
                strip.cell(|ui| {
                    ui.add_enabled_ui(state.selection.is_some(), |ui| {
                        let patch_warning = state.selection.as_ref().and_then(|selection| {
                            patch_compatibility_warning(
                                &patches,
                                remote_settings,
                                selection.game,
                                selection.patch.as_ref().map(|(patch_name, _, _)| patch_name.as_str()),
                            )
                        });

                        let mut layout_job = egui::text::LayoutJob::default();
                        layout_job.wrap.break_anywhere = true;
                        layout_job.wrap.max_rows = 1;

                        if patch_warning.is_some() {
                            gui::warning::append_to_layout_job(ui, &mut layout_job);
                        }

                        layout_job.append(
                            state
                                .selection
                                .as_ref()
                                .and_then(|s| s.patch.as_ref().map(|(name, _, _)| name.as_str()))
                                .unwrap_or(&i18n::LOCALES.lookup(language, "play-no-patch").unwrap()),
                            0.0,
                            egui::TextFormat::simple(
                                ui.style().text_styles.get(&egui::TextStyle::Body).unwrap().clone(),
                                ui.visuals().text_color(),
                            ),
                        );

                        if let Some(name) = state
                            .selection
                            .as_ref()
                            .and_then(|s| s.patch.as_ref().map(|(name, _, _)| name.as_str()))
                        {
                            layout_job.append(
                                patches.get(name).as_ref().map(|p| p.title.as_str()).unwrap_or(""),
                                0.0,
                                egui::TextFormat::simple(
                                    ui.style().text_styles.get(&egui::TextStyle::Small).unwrap().clone(),
                                    ui.visuals().text_color(),
                                ),
                            );
                        }

                        let resp = egui::ComboBox::from_id_source("patch-select-combobox")
                            .selected_text(layout_job)
                            .width(wide_width)
                            .wrap(true)
                            .show_ui(ui, |ui| {
                                let Some(selection) = state.selection.as_mut() else {
                                    return;
                                };

                                let mut supported_patches_list = supported_patches.iter().collect::<Vec<_>>();
                                supported_patches_list.sort_by_key(|(name, _)| {
                                    (if starred_patches.contains(**name) { 0 } else { 1 }, *name)
                                });

                                // attempt to provide room to fix weird staircasing from using an imgui
                                let mut max_width: f32 = 0.0;

                                for &(patch_name, (meta, _)) in &supported_patches_list {
                                    let mut width = item_spacing_x * 2.0;

                                    let warning = patch_compatibility_warning(
                                        &patches,
                                        remote_settings,
                                        selection.game,
                                        Some(patch_name),
                                    );

                                    if warning.is_some() {
                                        width += WARNING_WIDTH;
                                    }

                                    if starred_patches.contains(*patch_name) {
                                        width += 2.0 * BODY_CHAR_WIDTH;
                                    }

                                    width += (patch_name.len() + 1) as f32 * BODY_CHAR_WIDTH;
                                    width += meta.title.len() as f32 * SMALL_CHAR_WIDTH;

                                    max_width = max_width.max(width);
                                }

                                ui.allocate_space(egui::Vec2::new(max_width, 0.0));

                                {
                                    let no_patch_warning =
                                        patch_compatibility_warning(&patches, remote_settings, selection.game, None);

                                    let checked = selection.patch.is_none();
                                    let mut layout_job = egui::text::LayoutJob::default();
                                    if no_patch_warning.is_some() {
                                        gui::warning::append_to_layout_job(ui, &mut layout_job);
                                    }
                                    layout_job.append(
                                        &i18n::LOCALES.lookup(language, "play-no-patch").unwrap(),
                                        0.0,
                                        egui::TextFormat::simple(
                                            ui.style().text_styles.get(&egui::TextStyle::Body).unwrap().clone(),
                                            if checked {
                                                ui.visuals().selection.stroke.color
                                            } else {
                                                ui.visuals().text_color()
                                            },
                                        ),
                                    );
                                    let mut resp = ui.selectable_label(checked, layout_job);
                                    if let Some(warning) = no_patch_warning {
                                        resp = resp.on_hover_text(warning.description(language));
                                    }
                                    if resp.clicked() {
                                        if let Some(s) = committed_selection.take() {
                                            let rom = roms.get(&s.game).unwrap().clone();
                                            *committed_selection = Some(gui::Selection::new(s.game, s.save, None, rom));
                                        }
                                        selection.patch = None;
                                    }
                                }

                                for (name, (meta, supported_versions)) in supported_patches_list {
                                    let warning = patch_compatibility_warning(
                                        &patches,
                                        remote_settings,
                                        selection.game,
                                        Some(name),
                                    );

                                    let checked = selection.patch.as_ref().map(|(name, _, _)| name) == Some(*name);
                                    let mut layout_job = egui::text::LayoutJob::default();
                                    if warning.is_some() {
                                        gui::warning::append_to_layout_job(ui, &mut layout_job);
                                    }
                                    if starred_patches.contains(*name) {
                                        layout_job.append(
                                            "★ ",
                                            0.0,
                                            egui::TextFormat::simple(
                                                ui.style().text_styles.get(&egui::TextStyle::Body).unwrap().clone(),
                                                egui::Color32::GOLD,
                                            ),
                                        );
                                    }
                                    layout_job.append(
                                        &format!("{} ", *name),
                                        0.0,
                                        egui::TextFormat::simple(
                                            ui.style().text_styles.get(&egui::TextStyle::Body).unwrap().clone(),
                                            if checked {
                                                ui.visuals().selection.stroke.color
                                            } else {
                                                ui.visuals().text_color()
                                            },
                                        ),
                                    );
                                    layout_job.append(
                                        meta.title.as_str(),
                                        0.0,
                                        egui::TextFormat::simple(
                                            ui.style().text_styles.get(&egui::TextStyle::Small).unwrap().clone(),
                                            if checked {
                                                ui.visuals().selection.stroke.color
                                            } else {
                                                ui.visuals().text_color()
                                            },
                                        ),
                                    );
                                    let mut resp = ui.selectable_label(checked, layout_job);

                                    if let Some(warning) = warning {
                                        resp = resp.on_hover_text(warning.description(language));
                                    }

                                    if resp.clicked() {
                                        *patch_selection = Some(name.to_string());

                                        let version = *supported_versions.first().unwrap();

                                        let Some(version_metadata) =
                                            patches.get(*name).and_then(|p| p.versions.get(version)).cloned()
                                        else {
                                            continue;
                                        };

                                        let patch = Some(((*name).clone(), version.clone(), version_metadata));

                                        selection.patch = patch;
                                        commit_patch(&roms, patches_path, committed_selection, selection);
                                    }
                                }
                            });

                        if let Some(warning) = patch_warning {
                            resp.response.on_hover_text(warning.description(language));
                        }
                    });
                });

                // patch version
                strip.cell(|ui| {
                    ui.add_enabled_ui(
                        state
                            .selection
                            .as_ref()
                            .and_then(|selection| selection.patch.as_ref())
                            .and_then(|patch| supported_patches.get(&patch.0))
                            .map(|(_, vs)| !vs.is_empty())
                            .unwrap_or(false),
                        |ui| {
                            let version_warning = state.selection.as_ref().and_then(|selection| {
                                patch_version_compatibility_warning(
                                    &patches,
                                    remote_settings,
                                    selection.game,
                                    selection
                                        .patch
                                        .as_ref()
                                        .map(|(name, version, _)| (name.as_str(), version)),
                                )
                            });

                            let mut layout_job = egui::text::LayoutJob::default();

                            if version_warning.is_some() {
                                gui::warning::append_to_layout_job(ui, &mut layout_job);
                            }

                            layout_job.append(
                                &state
                                    .selection
                                    .as_ref()
                                    .and_then(|s| s.patch.as_ref().map(|(_, version, _)| version.to_string()))
                                    .unwrap_or("".to_string()),
                                0.0,
                                egui::TextFormat::simple(
                                    ui.style().text_styles.get(&egui::TextStyle::Body).unwrap().clone(),
                                    ui.visuals().text_color(),
                                ),
                            );

                            let resp = egui::ComboBox::from_id_source("patch-version-select-combobox")
                                .selected_text(layout_job)
                                .width(PATCH_VERSION_WIDTH)
                                .show_ui(ui, |ui| {
                                    let Some(selection) = state.selection.as_mut() else {
                                        return;
                                    };

                                    let Some((patch_name, patch_version, _)) = selection.patch.clone() else {
                                        return;
                                    };

                                    let supported_versions = if let Some(supported_versions) =
                                        supported_patches.get(&patch_name).map(|(_, vs)| vs)
                                    {
                                        supported_versions
                                    } else {
                                        return;
                                    };

                                    for version in supported_versions.iter() {
                                        let warning = patch_version_compatibility_warning(
                                            &patches,
                                            remote_settings,
                                            selection.game,
                                            Some((patch_name.as_str(), version)),
                                        );

                                        let checked = &patch_version == *version;
                                        let mut layout_job = egui::text::LayoutJob::default();
                                        if warning.is_some() {
                                            gui::warning::append_to_layout_job(ui, &mut layout_job);
                                        }
                                        layout_job.append(
                                            &version.to_string(),
                                            0.0,
                                            egui::TextFormat::simple(
                                                ui.style().text_styles.get(&egui::TextStyle::Body).unwrap().clone(),
                                                if checked {
                                                    ui.visuals().selection.stroke.color
                                                } else {
                                                    ui.visuals().text_color()
                                                },
                                            ),
                                        );

                                        let mut resp = ui.selectable_label(checked, layout_job);
                                        if let Some(warning) = warning {
                                            resp = resp.on_hover_text(warning.description(language));
                                        }
                                        if resp.clicked() {
                                            let version_metadata = if let Some(version_metadata) =
                                                patches.get(&patch_name).and_then(|p| p.versions.get(version)).cloned()
                                            {
                                                version_metadata
                                            } else {
                                                continue;
                                            };

                                            let patch =
                                                Some((patch_name.clone(), (*version).clone(), version_metadata));

                                            selection.patch = patch.clone();
                                            commit_patch(&roms, patches_path, committed_selection, selection);
                                        }
                                    }
                                });

                            if let Some(warning) = version_warning {
                                resp.response.on_hover_text(warning.description(language));
                            }
                        },
                    );
                });
            });
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            // open save folder button
            if ui
                .button(format!(
                    "📂 {}",
                    i18n::LOCALES.lookup(language, "select-save.open-folder").unwrap(),
                ))
                .clicked()
            {
                let _ = open::that(saves_path);
            }

            // save list
            let mut layout_job = egui::text::LayoutJob::default();
            layout_job.wrap.break_anywhere = true;
            layout_job.wrap.max_rows = 1;

            layout_job.append(
                &state
                    .selection
                    .as_ref()
                    .and_then(|selection| selection.save_path.as_ref())
                    .map(|save_path| save_name(saves_path, save_path).to_string())
                    .unwrap_or_else(|| i18n::LOCALES.lookup(language, "select-save.no-save-selected").unwrap()),
                0.0,
                egui::TextFormat::simple(
                    ui.style().text_styles.get(&egui::TextStyle::Body).unwrap().clone(),
                    ui.visuals().text_color(),
                ),
            );

            ui.add_enabled_ui(state.selection.is_some(), |ui| {
                egui::ComboBox::from_id_source("save-select-combobox")
                    .selected_text(layout_job)
                    .width(ui.available_width())
                    .wrap(true)
                    .show_ui(ui, |ui| {
                        let Some(selection_state) = state.selection.as_mut() else {
                            return;
                        };

                        let Some(saves) = saves.get(&selection_state.game) else {
                            return;
                        };

                        // attempt to provide room to fix weird staircasing from using an imgui
                        let mut max_width: f32 = 0.0;

                        for save in saves {
                            let mut width = item_spacing_x * 2.0;

                            width += save_name(saves_path, &save.path).len() as f32 * BODY_CHAR_WIDTH;

                            max_width = max_width.max(width);
                        }

                        ui.allocate_space(egui::Vec2::new(max_width, 0.0));

                        // save list
                        for save in saves {
                            let selected = selection_state
                                .save_path
                                .as_ref()
                                .is_some_and(|path| path == save.path.as_path());

                            let mut layout_job = egui::text::LayoutJob::default();
                            layout_job.append(
                                &save_name(saves_path, &save.path),
                                0.0,
                                egui::TextFormat::simple(
                                    ui.style().text_styles.get(&egui::TextStyle::Body).unwrap().clone(),
                                    if selected {
                                        ui.visuals().selection.stroke.color
                                    } else {
                                        ui.visuals().text_color()
                                    },
                                ),
                            );

                            let save_ui_label = ui.selectable_label(selected, layout_job);

                            if save_ui_label.clicked() {
                                selection_state.save_path = Some(save.path.clone());
                                commit_save(&roms, patches_path, committed_selection, selection_state, save.clone());
                            }
                        }
                    })
            });
        });

        ui.separator();

        ui.horizontal(|ui| {
            // new save button + list
            let cloned_selection = state.selection.clone();
            let mut from_patch = false;
            let mut save_templates: Vec<(&str, &(dyn tango_dataview::save::Save + Send + Sync))> = vec![];

            if let Some(selection) = cloned_selection.as_ref() {
                if let Some(patch_save_templates) = selection
                    .patch
                    .as_ref()
                    .and_then(|(_, _, patch_version)| patch_version.save_templates.get(&selection.game))
                {
                    from_patch = true;
                    save_templates = patch_save_templates
                        .iter()
                        .map(|(name, save)| (name.as_str(), save.as_ref()))
                        .collect::<Vec<_>>();
                } else {
                    save_templates = selection.game.save_templates().to_vec();
                }
            };

            ui.add_enabled_ui(!save_templates.is_empty(), |ui| {
                ui.menu_button(
                    format!("➕ {}", i18n::LOCALES.lookup(language, "select-save.new-save").unwrap()),
                    |ui| {
                        let Some(selection_state) = &mut state.selection else {
                            return;
                        };

                        let mut menu_selection = None;

                        if save_templates.len() == 1 {
                            menu_selection = save_templates.first().map(|(name, save)| (*name, *save));
                        } else {
                            for (name, save) in save_templates {
                                let localized_name = if !name.is_empty() {
                                    let text_id = format!(
                                        "game-{}.save-{}",
                                        selection_state.game.gamedb_entry().family_and_variant.0,
                                        name
                                    );

                                    i18n::LOCALES.lookup(language, &text_id).unwrap_or(name.to_string())
                                } else {
                                    i18n::LOCALES.lookup(language, "select-save.default-save").unwrap()
                                };

                                let save_label = if from_patch {
                                    let localization_args =
                                        std::collections::HashMap::from([("name", localized_name.into())]);

                                    i18n::LOCALES
                                        .lookup_with_args(language, "select-save.from-patch-save", &localization_args)
                                        .unwrap()
                                } else {
                                    localized_name
                                };

                                if ui.button(save_label).clicked() {
                                    menu_selection = Some((name, save));
                                }
                            }
                        }

                        if let Some((name, save)) = menu_selection {
                            let (path, mut f) = match create_new_save(
                                language,
                                saves_path,
                                selection_state.game,
                                selection_state.patch.as_ref(),
                                name,
                            ) {
                                Ok((path, f)) => (path, f),
                                Err(e) => {
                                    log::error!("failed to create save: {}", e);
                                    ui.close_menu();
                                    return;
                                }
                            };

                            let mut save = save.clone_box();
                            save.rebuild_checksum();

                            if let Err(e) = f.write_all(&save.as_sram_dump()) {
                                log::error!("failed to write save: {}", e);
                                ui.close_menu();
                                return;
                            }

                            selection_state.save_path = Some(path.clone());

                            let save = save::ScannedSave { path, save };
                            commit_save(&roms, patches_path, committed_selection, selection_state, save);
                            rescan_saves(config, &saves_scanner, ui.ctx());

                            ui.close_menu();
                        }
                    },
                );
            });

            ui.add_enabled_ui(committed_selection.is_some(), |ui| {
                let duplicate_label = egui::RichText::new(format!(
                    "📄 {}",
                    i18n::LOCALES.lookup(language, "select-save.duplicate-save").unwrap()
                ));

                if ui.button(duplicate_label).clicked() {
                    (|| {
                        let Some(previous_selection) = committed_selection.take() else {
                            return;
                        };

                        let save = &previous_selection.save;

                        let (path, mut f) = match create_next_file(&save.path) {
                            Ok((path, f)) => (path, f),
                            Err(e) => {
                                log::error!("failed to create save: {}", e);
                                return;
                            }
                        };

                        if let Err(e) = f.write_all(&save.save.as_sram_dump()) {
                            log::error!("failed to write save: {}", e);
                            return;
                        }

                        if let Some(selection) = &mut state.selection {
                            selection.save_path = Some(path.clone());
                        }

                        *committed_selection = Some(gui::Selection::new(
                            previous_selection.game,
                            save::ScannedSave {
                                path,
                                save: save.save.clone_box(),
                            },
                            previous_selection.patch,
                            previous_selection.rom,
                        ));

                        rescan_saves(config, &saves_scanner, ui.ctx());
                    })()
                }

                // let rename_label = egui::RichText::new(format!(
                //     "✏️ {}",
                //     i18n::LOCALES.lookup(language, "select-save.rename-save").unwrap()
                // ));

                // if ui.button(rename_label).clicked() {
                //     // TODO: Show rename dialog.
                //     rescan_saves(config, &saves_scanner, ui.ctx());
                // }

                // let delete_label = egui::RichText::new(format!(
                //     "🗑️ {}",
                //     i18n::LOCALES.lookup(language, "select-save.delete-save").unwrap()
                // ));

                // if ui.button(delete_label).clicked() {
                //     // TODO: Show confirm dialog.
                //     rescan_saves(config, &saves_scanner, ui.ctx());
                // }
            })
        });
    });
}

use std::io::Write;

use fluent_templates::Loader;

use crate::{game, gui, i18n, net, patch, rom, save};

#[derive(Clone)]
pub struct Selection {
    pub game: &'static (dyn game::Game + Send + Sync),
    pub save_path: Option<std::path::PathBuf>,
    pub patch: Option<(String, semver::Version, patch::Version)>,
}

pub struct State {
    selection: Option<Selection>,
}

impl State {
    pub fn new(selection: Option<Selection>) -> Self {
        Self { selection }
    }
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
    patch: Option<&(String, semver::Version, patch::Version)>,
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
    prefix = prefix.replace(":", "").replace("/", " ");
    prefix.push_str(".sav");

    create_next_file(&saves_path.join(prefix))
}

pub fn show(
    ui: &mut egui::Ui,
    show: &mut Option<State>,
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
    let roms = roms_scanner.read();
    let saves = saves_scanner.read();
    let patches = patches_scanner.read();

    ui.vertical(|ui| {
        let games = game::sorted_all_games(language);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            if ui
                .button(format!(
                    "📂 {}",
                    i18n::LOCALES.lookup(language, "select-save.open-folder").unwrap(),
                ))
                .clicked()
            {
                let _ = open::that(saves_path);
            }

            if let Some(selection_state) = show.as_ref().unwrap().selection.as_ref() {
                let (family, variant) = selection_state.game.gamedb_entry().family_and_variant;
                let game_name = i18n::LOCALES
                    .lookup(language, &format!("game-{}.variant-{}", family, variant))
                    .unwrap();
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(
                            egui::Layout::left_to_right(egui::Align::Max).with_main_wrap(true),
                            |ui| {
                                ui.label(if let Some((name, version, _)) = selection_state.patch.as_ref() {
                                    format!("{} + {} v{}", game_name, name, version)
                                } else {
                                    game_name
                                });
                            },
                        );
                    });
                });
            }
        });

        ui.horizontal_top(|ui| {
            let patches = patches_scanner.read();

            let mut supported_patches = std::collections::HashMap::new();
            if let Some(selection) = show.as_ref().unwrap().selection.as_ref() {
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

            const PATCH_VERSION_COMBOBOX_WIDTH: f32 = 100.0;
            ui.add_enabled_ui(show.as_ref().unwrap().selection.is_some(), |ui| {
                let warning = (|| {
                    let Some(selection) = show.as_ref().unwrap().selection.as_ref() else {
                        return None;
                    };

                    let Some(remote_settings) = remote_settings.as_ref() else {
                        return None;
                    };

                    let Some(remote_gi) = remote_settings.game_info.as_ref() else {
                        return None;
                    };

                    let Some(remote_game) = game::find_by_family_and_variant(
                        &remote_gi.family_and_variant.0,
                        remote_gi.family_and_variant.1,
                    ) else {
                        return None;
                    };

                    if let Some((name, _, _)) = selection.patch.as_ref() {
                        if !remote_settings.available_patches.iter().any(|(n, _)| name == n) {
                            return Some(gui::play_pane::Warning::NoRemotePatches(name.clone()));
                        }
                    }

                    let local_netplay_compatibilities = if let Some((patch_name, _, _)) = selection.patch.as_ref() {
                        patches
                            .get(patch_name)
                            .map(|patch| {
                                patch
                                    .versions
                                    .values()
                                    .map(|vi| vi.netplay_compatibility.as_str())
                                    .collect()
                            })
                            .unwrap_or_default()
                    } else {
                        vec![selection.game.gamedb_entry().family_and_variant.0]
                    };

                    if let Some(nc) = gui::play_pane::get_netplay_compatibility(
                        remote_game,
                        remote_gi.patch.as_ref().map(|pi| (pi.name.as_str(), &pi.version)),
                        &patches,
                    ) {
                        if !local_netplay_compatibilities.contains(&nc.as_str()) {
                            return Some(gui::play_pane::Warning::Incompatible);
                        }
                    }

                    None
                })();

                let mut layout_job = egui::text::LayoutJob::default();
                if warning.is_some() {
                    gui::warning::append_to_layout_job(ui, &mut layout_job);
                }
                layout_job.append(
                    &format!(
                        "{} ",
                        show.as_ref()
                            .unwrap()
                            .selection
                            .as_ref()
                            .and_then(|s| s.patch.as_ref().map(|(name, _, _)| name.as_str()))
                            .unwrap_or(&i18n::LOCALES.lookup(language, "play-no-patch").unwrap())
                    ),
                    0.0,
                    egui::TextFormat::simple(
                        ui.style().text_styles.get(&egui::TextStyle::Body).unwrap().clone(),
                        ui.visuals().text_color(),
                    ),
                );

                if let Some(name) = show
                    .as_ref()
                    .unwrap()
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
                    .width(ui.available_width() - ui.spacing().item_spacing.x - PATCH_VERSION_COMBOBOX_WIDTH)
                    .show_ui(ui, |ui| {
                        let selection = if let Some(selection) = show.as_mut().unwrap().selection.as_mut() {
                            selection
                        } else {
                            return;
                        };
                        {
                            let warning = (|| {
                                let Some(remote_settings) = remote_settings.as_ref() else {
                                    return None;
                                };

                                let Some(remote_gi) = remote_settings.game_info.as_ref() else {
                                    return None;
                                };

                                let Some(remote_game) = game::find_by_family_and_variant(
                                    &remote_gi.family_and_variant.0,
                                    remote_gi.family_and_variant.1,
                                ) else {
                                    return None;
                                };

                                if let Some(nc) = gui::play_pane::get_netplay_compatibility(
                                    remote_game,
                                    remote_gi.patch.as_ref().map(|pi| (pi.name.as_str(), &pi.version)),
                                    &patches,
                                ) {
                                    if nc != selection.game.gamedb_entry().family_and_variant.0 {
                                        return Some(gui::play_pane::Warning::Incompatible);
                                    }
                                }

                                None
                            })();

                            let checked = selection.patch.is_none();
                            let mut layout_job = egui::text::LayoutJob::default();
                            if warning.is_some() {
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
                            if let Some(warning) = warning {
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

                        let mut supported_patches_list = supported_patches.iter().collect::<Vec<_>>();
                        supported_patches_list
                            .sort_by_key(|(name, _)| (if starred_patches.contains(**name) { 0 } else { 1 }, *name));
                        for (name, (meta, supported_versions)) in supported_patches_list {
                            let warning = (|| {
                                let Some(remote_settings) = remote_settings.as_ref() else {
                                    return None;
                                };

                                let Some(remote_gi) = remote_settings.game_info.as_ref() else {
                                    return None;
                                };

                                let Some(remote_game) = game::find_by_family_and_variant(
                                    &remote_gi.family_and_variant.0,
                                    remote_gi.family_and_variant.1,
                                ) else {
                                    return None;
                                };

                                if !remote_settings.available_patches.iter().any(|(n, _)| *name == n) {
                                    return Some(gui::play_pane::Warning::NoRemotePatches((*name).clone()));
                                }

                                let local_netplay_compatibilities: Vec<_> = patches
                                    .get(*name)
                                    .map(|patch| {
                                        patch
                                            .versions
                                            .values()
                                            .map(|vi| vi.netplay_compatibility.as_str())
                                            .collect()
                                    })
                                    .unwrap_or_default();

                                if let Some(nc) = gui::play_pane::get_netplay_compatibility(
                                    remote_game,
                                    remote_gi.patch.as_ref().map(|pi| (pi.name.as_str(), &pi.version)),
                                    &patches,
                                ) {
                                    if !local_netplay_compatibilities.contains(&nc.as_str()) {
                                        return Some(gui::play_pane::Warning::Incompatible);
                                    }
                                }

                                None
                            })();

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

                                let rom = roms.get(&selection.game).unwrap().clone();
                                let (rom_code, revision) = selection.game.gamedb_entry().rom_code_and_revision;
                                let version = *supported_versions.first().unwrap();

                                let version_metadata = if let Some(version_metadata) =
                                    patches.get(*name).and_then(|p| p.versions.get(version)).cloned()
                                {
                                    version_metadata
                                } else {
                                    return;
                                };

                                let patch = Some(((*name).clone(), version.clone(), version_metadata));
                                if committed_selection
                                    .as_ref()
                                    .map(|s| s.game == selection.game)
                                    .unwrap_or(false)
                                {
                                    if let Some(gui::Selection { game, save, .. }) = committed_selection.take() {
                                        let rom = match patch::apply_patch_from_disk(
                                            &rom,
                                            selection.game,
                                            patches_path,
                                            name,
                                            version,
                                        ) {
                                            Ok(r) => r,
                                            Err(e) => {
                                                log::error!(
                                                    "failed to apply patch {}: {:?}: {:?}",
                                                    name,
                                                    (rom_code, revision),
                                                    e
                                                );
                                                return;
                                            }
                                        };

                                        *committed_selection =
                                            Some(gui::Selection::new(game, save, patch.clone(), rom));
                                    }
                                } else {
                                    *committed_selection = None;
                                }
                                selection.patch = patch;
                            }
                        }
                    });
                if let Some(warning) = warning {
                    resp.response.on_hover_text(warning.description(language));
                }

                ui.add_enabled_ui(
                    show.as_ref()
                        .unwrap()
                        .selection
                        .as_ref()
                        .and_then(|selection| selection.patch.as_ref())
                        .and_then(|patch| supported_patches.get(&patch.0))
                        .map(|(_, vs)| !vs.is_empty())
                        .unwrap_or(false),
                    |ui| {
                        let warning = (|| {
                            let Some(selection) = show.as_ref().unwrap().selection.as_ref() else {
                                return None;
                            };

                            let Some(remote_settings) = remote_settings.as_ref() else {
                                return None;
                            };

                            let Some(remote_gi) = remote_settings.game_info.as_ref() else {
                                return None;
                            };

                            let Some(remote_game) = game::find_by_family_and_variant(
                                &remote_gi.family_and_variant.0,
                                remote_gi.family_and_variant.1,
                            ) else {
                                return None;
                            };

                            if let Some((patch_name, patch_version, _)) = selection.patch.as_ref() {
                                if !remote_settings.available_patches.iter().any(|(name, versions)| {
                                    patch_name == name && versions.iter().any(|v| v == patch_version)
                                }) {
                                    return Some(gui::play_pane::Warning::NoRemotePatch(
                                        patch_name.clone(),
                                        patch_version.clone(),
                                    ));
                                }
                            }

                            let local_netplay_compatibility = gui::play_pane::get_netplay_compatibility(
                                selection.game,
                                selection
                                    .patch
                                    .as_ref()
                                    .map(|(name, version, _)| (name.as_str(), version)),
                                &patches,
                            );

                            let remote_netplay_compatibility = gui::play_pane::get_netplay_compatibility(
                                remote_game,
                                remote_gi.patch.as_ref().map(|pi| (pi.name.as_str(), &pi.version)),
                                &patches,
                            );

                            if local_netplay_compatibility != remote_netplay_compatibility {
                                return Some(gui::play_pane::Warning::Incompatible);
                            }

                            None
                        })();

                        let mut layout_job = egui::text::LayoutJob::default();
                        if warning.is_some() {
                            gui::warning::append_to_layout_job(ui, &mut layout_job);
                        }
                        layout_job.append(
                            &show
                                .as_ref()
                                .unwrap()
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
                            .width(PATCH_VERSION_COMBOBOX_WIDTH)
                            .selected_text(layout_job)
                            .show_ui(ui, |ui| {
                                let Some(selection) = show.as_mut().unwrap().selection.as_mut() else {
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
                                    let warning = (|| {
                                        let Some(remote_settings) = remote_settings.as_ref() else {
                                            return None;
                                        };

                                        let Some(remote_gi) = remote_settings.game_info.as_ref() else {
                                            return None;
                                        };

                                        let Some(remote_game) = game::find_by_family_and_variant(
                                            &remote_gi.family_and_variant.0,
                                            remote_gi.family_and_variant.1,
                                        ) else {
                                            return None;
                                        };

                                        if !remote_settings.available_patches.iter().any(|(name, versions)| {
                                            &patch_name == name && versions.iter().any(|v| v == *version)
                                        }) {
                                            return Some(gui::play_pane::Warning::NoRemotePatch(
                                                patch_name.clone(),
                                                (*version).clone(),
                                            ));
                                        }

                                        let local_netplay_compatibility = gui::play_pane::get_netplay_compatibility(
                                            selection.game,
                                            Some((patch_name.as_str(), *version)),
                                            &patches,
                                        );

                                        let remote_netplay_compatibility = gui::play_pane::get_netplay_compatibility(
                                            remote_game,
                                            remote_gi.patch.as_ref().map(|pi| (pi.name.as_str(), &pi.version)),
                                            &patches,
                                        );

                                        if local_netplay_compatibility != remote_netplay_compatibility {
                                            return Some(gui::play_pane::Warning::Incompatible);
                                        }

                                        None
                                    })();

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
                                        let rom = roms.get(&selection.game).unwrap().clone();
                                        let (rom_code, revision) = selection.game.gamedb_entry().rom_code_and_revision;

                                        let version_metadata = if let Some(version_metadata) =
                                            patches.get(&patch_name).and_then(|p| p.versions.get(version)).cloned()
                                        {
                                            version_metadata
                                        } else {
                                            return;
                                        };

                                        let patch = Some((patch_name.clone(), (*version).clone(), version_metadata));

                                        if committed_selection
                                            .as_ref()
                                            .map(|s| s.game == selection.game)
                                            .unwrap_or(false)
                                        {
                                            if let Some(gui::Selection { game, save, .. }) = committed_selection.take()
                                            {
                                                let rom = match patch::apply_patch_from_disk(
                                                    &rom,
                                                    selection.game,
                                                    patches_path,
                                                    &patch_name,
                                                    version,
                                                ) {
                                                    Ok(r) => r,
                                                    Err(e) => {
                                                        log::error!(
                                                            "failed to apply patch {}: {:?}: {:?}",
                                                            patch_name,
                                                            (rom_code, revision),
                                                            e
                                                        );
                                                        return;
                                                    }
                                                };

                                                *committed_selection =
                                                    Some(gui::Selection::new(game, save, patch.clone(), rom));
                                            }
                                        } else {
                                            *committed_selection = None;
                                        }
                                        selection.patch = patch.clone();
                                    }
                                }
                            });
                        if let Some(warning) = warning {
                            resp.response.on_hover_text(warning.description(language));
                        }
                    },
                );
            });
        });

        ui.group(|ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                if show.as_ref().unwrap().selection.is_some() {
                    if ui
                        .button(format!(
                            "⬅️ {}",
                            i18n::LOCALES
                                .lookup(language, "select-save.return-to-games-list")
                                .unwrap()
                        ))
                        .clicked()
                    {
                        show.as_mut().unwrap().selection = None;
                    }

                    if let Some(selection_state) = show.as_ref().unwrap().selection.clone() {
                        let (from_patch, save_templates) = if let Some(save_templates) = selection_state
                            .patch
                            .as_ref()
                            .and_then(|(_, _, patch_version)| patch_version.save_templates.get(&selection_state.game))
                        {
                            (
                                true,
                                save_templates
                                    .iter()
                                    .map(|(name, save)| (name.as_str(), save.as_ref()))
                                    .collect::<Vec<_>>(),
                            )
                        } else {
                            (
                                false,
                                selection_state
                                    .game
                                    .save_templates()
                                    .iter()
                                    .map(|(name, save)| (*name, *save))
                                    .collect::<Vec<_>>(),
                            )
                        };

                        ui.add_enabled_ui(!save_templates.is_empty(), |ui| {
                            ui.menu_button(
                                format!("➕ {}", i18n::LOCALES.lookup(language, "select-save.new-save").unwrap()),
                                |ui| {
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
                                                    .lookup_with_args(
                                                        language,
                                                        "select-save.from-patch-save",
                                                        &localization_args,
                                                    )
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

                                        let (game, rom, patch) = if let Some(committed_selection) =
                                            committed_selection.take().filter(|committed_selection| {
                                                committed_selection.game == selection_state.game
                                            }) {
                                            (
                                                committed_selection.game,
                                                committed_selection.rom,
                                                committed_selection.patch,
                                            )
                                        } else {
                                            let mut rom = roms.get(&selection_state.game).unwrap().clone();
                                            if let Some((name, version, _)) = selection_state.patch.as_ref() {
                                                let (rom_code, revision) =
                                                    selection_state.game.gamedb_entry().rom_code_and_revision;
                                                rom = match patch::apply_patch_from_disk(
                                                    &rom,
                                                    selection_state.game,
                                                    patches_path,
                                                    name,
                                                    version,
                                                ) {
                                                    Ok(r) => r,
                                                    Err(e) => {
                                                        log::error!(
                                                            "failed to apply patch {}: {:?}: {:?}",
                                                            name,
                                                            (rom_code, revision),
                                                            e
                                                        );
                                                        return;
                                                    }
                                                };
                                            }
                                            (selection_state.game, rom, selection_state.patch.clone())
                                        };

                                        let mut save = save.clone_box();
                                        save.rebuild_checksum();

                                        if let Err(e) = f.write_all(&save.as_sram_dump()) {
                                            log::error!("failed to write save: {}", e);
                                            ui.close_menu();
                                            return;
                                        }

                                        *show = None;
                                        *committed_selection = Some(gui::Selection::new(
                                            game,
                                            save::ScannedSave { path, save },
                                            patch,
                                            rom,
                                        ));

                                        ui.close_menu();
                                    }
                                },
                            );
                        });
                    }
                }

                if show.is_none() {
                    return;
                }

                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        if let Some(selection_state) = show.as_ref().unwrap().selection.clone() {
                            if let Some(saves) = saves.get(&selection_state.game) {
                                for save in saves {
                                    if show.is_none() {
                                        return;
                                    }

                                    let selected = show
                                        .as_ref()
                                        .unwrap()
                                        .selection
                                        .as_ref()
                                        .map(|selection| {
                                            selection
                                                .save_path
                                                .as_ref()
                                                .map(|path| path == save.path.as_path())
                                                .unwrap_or(false)
                                        })
                                        .unwrap_or(false);
                                    let mut layout_job = egui::text::LayoutJob::default();
                                    layout_job.append(
                                        &format!(
                                            "{}",
                                            save.path
                                                .as_path()
                                                .strip_prefix(saves_path)
                                                .unwrap_or(save.path.as_path())
                                                .display()
                                        ),
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
                                    if ui
                                        .selectable_label(selected, layout_job)
                                        .context_menu(|ui| {
                                            if ui
                                                .button(egui::RichText::new(format!(
                                                    "📄 {}",
                                                    i18n::LOCALES
                                                        .lookup(language, "select-save.duplicate-save")
                                                        .unwrap()
                                                )))
                                                .clicked()
                                            {
                                                let (path, mut f) = match create_next_file(&save.path) {
                                                    Ok((path, f)) => (path, f),
                                                    Err(e) => {
                                                        log::error!("failed to create save: {}", e);
                                                        ui.close_menu();
                                                        return;
                                                    }
                                                };

                                                if let Err(e) = f.write_all(&save.save.as_sram_dump()) {
                                                    log::error!("failed to write save: {}", e);
                                                    ui.close_menu();
                                                    return;
                                                }

                                                let (game, rom, patch) = if let Some(committed_selection) =
                                                    committed_selection.take().filter(|committed_selection| {
                                                        committed_selection.game == selection_state.game
                                                    }) {
                                                    (
                                                        committed_selection.game,
                                                        committed_selection.rom,
                                                        committed_selection.patch,
                                                    )
                                                } else {
                                                    let mut rom = roms.get(&selection_state.game).unwrap().clone();
                                                    if let Some((name, version, _)) = selection_state.patch.as_ref() {
                                                        let (rom_code, revision) =
                                                            selection_state.game.gamedb_entry().rom_code_and_revision;
                                                        rom = match patch::apply_patch_from_disk(
                                                            &rom,
                                                            selection_state.game,
                                                            patches_path,
                                                            name,
                                                            version,
                                                        ) {
                                                            Ok(r) => r,
                                                            Err(e) => {
                                                                log::error!(
                                                                    "failed to apply patch {}: {:?}: {:?}",
                                                                    name,
                                                                    (rom_code, revision),
                                                                    e
                                                                );
                                                                return;
                                                            }
                                                        };
                                                    }
                                                    (selection_state.game, rom, selection_state.patch.clone())
                                                };

                                                *show = None;
                                                *committed_selection = Some(gui::Selection::new(
                                                    game,
                                                    save::ScannedSave {
                                                        path,
                                                        save: save.save.clone_box(),
                                                    },
                                                    patch,
                                                    rom,
                                                ));

                                                ui.close_menu();
                                            }

                                            // if ui
                                            //     .button(egui::RichText::new(format!(
                                            //         "✏️ {}",
                                            //         i18n::LOCALES.lookup(language, "select-save.rename-save").unwrap()
                                            //     )))
                                            //     .clicked()
                                            // {
                                            //     // TODO: Show rename dialog.
                                            //     ui.close_menu();
                                            // }

                                            // if ui
                                            //     .button(
                                            //         egui::RichText::new(format!(
                                            //             "🗑️ {}",
                                            //             i18n::LOCALES
                                            //                 .lookup(language, "select-save.delete-save")
                                            //                 .unwrap()
                                            //         ))
                                            //         .color(egui::Color32::RED),
                                            //     )
                                            //     .clicked()
                                            // {
                                            //     // TODO: Show confirm dialog.
                                            //     ui.close_menu();
                                            // }
                                        })
                                        .clicked()
                                    {
                                        let (game, rom, patch) = if let Some(committed_selection) =
                                            committed_selection.take().filter(|committed_selection| {
                                                committed_selection.game == selection_state.game
                                            }) {
                                            (
                                                committed_selection.game,
                                                committed_selection.rom,
                                                committed_selection.patch,
                                            )
                                        } else {
                                            let mut rom = roms.get(&selection_state.game).unwrap().clone();
                                            if let Some((name, version, _)) = selection_state.patch.as_ref() {
                                                let (rom_code, revision) =
                                                    selection_state.game.gamedb_entry().rom_code_and_revision;
                                                rom = match patch::apply_patch_from_disk(
                                                    &rom,
                                                    selection_state.game,
                                                    patches_path,
                                                    name,
                                                    version,
                                                ) {
                                                    Ok(r) => r,
                                                    Err(e) => {
                                                        log::error!(
                                                            "failed to apply patch {}: {:?}: {:?}",
                                                            name,
                                                            (rom_code, revision),
                                                            e
                                                        );
                                                        return;
                                                    }
                                                };
                                            }
                                            (selection_state.game, rom, selection_state.patch.clone())
                                        };

                                        *show = None;
                                        *committed_selection =
                                            Some(gui::Selection::new(game, save.clone(), patch, rom));
                                    }
                                }
                            }
                        } else {
                            for (available, game) in games
                                .iter()
                                .filter(|g| roms.contains_key(*g))
                                .map(|g| (true, g))
                                .chain(games.iter().filter(|g| !roms.contains_key(*g)).map(|g| (false, g)))
                            {
                                let (family, variant) = game.gamedb_entry().family_and_variant;

                                let selected = show
                                    .as_ref()
                                    .unwrap()
                                    .selection
                                    .as_ref()
                                    .map(|selection| selection.game == *game)
                                    .unwrap_or(false);

                                let warning = (|| {
                                    let Some(remote_settings) = remote_settings.as_ref() else {
                                        return None;
                                    };

                                    if !remote_settings.available_games.iter().any(|(family, variant)| {
                                        game.gamedb_entry().family_and_variant == (family, *variant)
                                    }) {
                                        return Some(gui::play_pane::Warning::NoRemoteROM(*game));
                                    }

                                    let Some(remote_gi) = remote_settings.game_info.as_ref() else {
                                        return None;
                                    };

                                    if let Some(netplay_compatibility) =
                                        gui::play_pane::get_netplay_compatibility_from_game_info(remote_gi, &patches)
                                    {
                                        if netplay_compatibility != family
                                            && !patches.values().any(|metadata| {
                                                metadata.versions.values().any(|version| {
                                                    version.supported_games.contains(game)
                                                        && version.netplay_compatibility == netplay_compatibility
                                                })
                                            })
                                        {
                                            return Some(gui::play_pane::Warning::Incompatible);
                                        }
                                    }
                                    None
                                })();

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
                                    show.as_mut().unwrap().selection = Some(Selection {
                                        game: *game,
                                        save_path: None,
                                        patch: None,
                                    });
                                }
                            }
                        }
                    });
                });
            });
        });
    });
}

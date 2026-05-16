use crate::i18n::t;
use crate::{
    config, game, rom, save_view, selection, Scanners, PRIMARY_PADDING, PRIMARY_TEXT_SIZE, STANDARD_PADDING,
    STANDARD_TEXT_SIZE,
};
use iced::widget::{
    button, column, container, horizontal_rule, horizontal_space, pick_list, row, text, text_input,
};
use iced::{Alignment, Element, Fill, Length};
use unic_langid::LanguageIdentifier;

// ---------- Messages ----------

#[derive(Debug, Clone)]
pub enum Message {
    LocalGameSelected(GameOption),
    LocalSaveSelected(SaveOption),
    LocalPatchSelected(String),
    LocalPatchVersionSelected(semver::Version),
    SaveTabSelected(save_view::Tab),
    ToggleFolderGrouped(bool),
    LinkCodeChanged(String),
    PlayPressed,
    Rescan,

    CopyTabAsText(save_view::Tab),

    SaveOpenFolder,
    SaveDuplicate,
    SaveRenameStart,
    SaveRenameDraftChanged(String),
    SaveRenameConfirm,
    SaveDeleteStart,
    SaveDeleteConfirm,
    SaveActionCancel,
    SaveNewStart,
    SaveNewDraftChanged(String),
    SaveNewTemplateSelected(String),
    SaveNewConfirm,

    /// Mouse moved over the NaviCust image; payload is the resolved
    /// part index from the materialized grid, or None when no cell.
    NavicustHover(Option<usize>),
}

// ---------- Game / Save pick_list options ----------

#[derive(Clone)]
pub struct GameOption {
    pub game: rom::GameRef,
    pub display: String,
}

impl PartialEq for GameOption {
    fn eq(&self, o: &Self) -> bool {
        self.game == o.game
    }
}
impl Eq for GameOption {}
impl std::hash::Hash for GameOption {
    fn hash<H: std::hash::Hasher>(&self, s: &mut H) {
        self.game.hash(s);
    }
}
impl std::fmt::Display for GameOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}
impl std::fmt::Debug for GameOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct SaveOption {
    pub path: std::path::PathBuf,
}

impl std::fmt::Display for SaveOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = self
            .path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string());
        f.write_str(&name)
    }
}

// ---------- Play tab state ----------

pub struct PlayState {
    pub local_game: Option<rom::GameRef>,
    pub local_save: Option<std::path::PathBuf>,
    pub local_patch: Option<String>,
    pub local_patch_version: Option<semver::Version>,
    /// Explicit save-tab pick; `None` means "auto-pick from available".
    pub save_tab: Option<save_view::Tab>,
    pub folder_grouped: bool,
    /// Inline state for the save-management actions (rename / delete).
    pub save_action: SaveAction,
    pub link_code: String,
    pub playing: bool,
    /// Transient one-shot status message shown beneath the link-code
    /// input; reset by the next user action. Used today to flag that
    /// netplay isn't implemented; will likely host real lobby status
    /// messages once it is.
    pub flash_status: Option<String>,
    /// Part index currently under the cursor on the NaviCust image, or
    /// None when not hovered.
    pub hovered_ncp_idx: Option<usize>,
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub enum SaveAction {
    #[default]
    None,
    Renaming { draft: String },
    ConfirmDelete,
    /// Creating a new save. `template` is the template name (empty
    /// string is the default unnamed template); `draft` is the user's
    /// chosen filename.
    NewSave { draft: String, template: String },
}

impl Default for PlayState {
    fn default() -> Self {
        Self {
            local_game: None,
            local_save: None,
            local_patch: None,
            local_patch_version: None,
            save_tab: None,
            folder_grouped: true,
            save_action: SaveAction::None,
            link_code: String::new(),
            playing: false,
            flash_status: None,
            hovered_ncp_idx: None,
        }
    }
}

impl PlayState {
    pub fn view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        loaded: Option<&'a selection::Loaded>,
        streamer_mode: bool,
        config: &'a config::Config,
    ) -> Element<'a, Message> {
        column![
            self.selector_strip(lang, scanners),
            self.body(lang, scanners, loaded, streamer_mode, config),
            horizontal_rule(1),
            self.bottom_strip(lang),
        ]
        .width(Fill)
        .height(Fill)
        .into()
    }

    /// Picks between the save view, an empty-state hint, or a "pick a
    /// save" hint based on what the user has installed and selected.
    fn body<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        loaded: Option<&'a selection::Loaded>,
        streamer_mode: bool,
        config: &'a config::Config,
    ) -> Element<'a, Message> {
        // No ROMs at all: explain where to put them.
        if scanners.roms.read().is_empty() {
            return empty_state_card(
                t(lang, "empty-no-roms-title"),
                vec![
                    t(lang, "empty-no-roms-body"),
                    config.roms_path().display().to_string(),
                ],
            );
        }
        // Game selected but no save files for it.
        if let Some(game) = self.local_game {
            let has_saves = scanners
                .saves
                .read()
                .get(&game)
                .map(|v| !v.is_empty())
                .unwrap_or(false);
            if !has_saves && self.local_save.is_none() {
                return empty_state_card(
                    t(lang, "empty-no-saves-title"),
                    vec![
                        t(lang, "empty-no-saves-body"),
                        config.saves_path().display().to_string(),
                    ],
                );
            }
        }
        self.save_view(lang, loaded, streamer_mode)
    }

    fn selector_strip<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
    ) -> Element<'a, Message> {
        let roms = scanners.roms.read();
        let saves = scanners.saves.read();

        let mut installed_games: Vec<rom::GameRef> = roms.keys().copied().collect();
        game::sort_games(lang, &mut installed_games);

        let game_options: Vec<GameOption> = installed_games
            .iter()
            .map(|g| GameOption {
                game: *g,
                display: game::display_name(lang, *g),
            })
            .collect();

        let selected_game = self
            .local_game
            .and_then(|g| game_options.iter().find(|opt| opt.game == g).cloned());

        let game = pick_list(game_options, selected_game, Message::LocalGameSelected)
            .placeholder(t(lang, "play-no-game"))
            .width(Length::FillPortion(3));

        let save_options: Vec<SaveOption> = self
            .local_game
            .and_then(|g| saves.get(&g))
            .map(|saves| saves.iter().map(|s| SaveOption { path: s.path.clone() }).collect())
            .unwrap_or_default();

        let selected_save = self
            .local_save
            .as_ref()
            .and_then(|p| save_options.iter().find(|s| &s.path == p).cloned());

        let save = pick_list(save_options, selected_save, Message::LocalSaveSelected)
            .placeholder(t(lang, "play-no-save"))
            .width(Length::Fill);

        let no_patch_label = t(lang, "play-no-patch");
        let patches = scanners.patches.read();
        let mut compatible_names: Vec<String> = patches
            .iter()
            .filter(|(_, p)| {
                if let Some(game) = self.local_game {
                    p.versions.values().any(|v| v.supported_games.contains(&game))
                } else {
                    false
                }
            })
            .map(|(n, _)| n.clone())
            .collect();
        compatible_names.sort();
        let patch_options: Vec<String> = std::iter::once(no_patch_label.clone())
            .chain(compatible_names.into_iter())
            .collect();
        let patch = pick_list(
            patch_options,
            Some(self.local_patch.clone().unwrap_or(no_patch_label)),
            Message::LocalPatchSelected,
        )
        .width(Length::FillPortion(2));

        let version_options: Vec<semver::Version> = self
            .local_patch
            .as_ref()
            .and_then(|n| patches.get(n))
            .map(|p| {
                let game = self.local_game;
                let mut vs: Vec<semver::Version> = p
                    .versions
                    .iter()
                    .filter(|(_, v)| {
                        game.map(|g| v.supported_games.contains(&g)).unwrap_or(true)
                    })
                    .map(|(k, _)| k.clone())
                    .collect();
                vs.sort_by(|a, b| b.cmp(a));
                vs
            })
            .unwrap_or_default();
        let version = pick_list(
            version_options,
            self.local_patch_version.clone(),
            Message::LocalPatchVersionSelected,
        )
        .placeholder(t(lang, "play-version-placeholder"))
        .width(Length::Fixed(100.0));

        let refresh = button(text(t(lang, "rescan")).size(STANDARD_TEXT_SIZE))
            .padding(STANDARD_PADDING)
            .on_press(Message::Rescan);

        let game_row = row![game, patch, version, refresh]
            .spacing(8)
            .align_y(Alignment::Center);

        let save_row: Element<'_, Message> = match &self.save_action {
            SaveAction::None => {
                let actions = self.save_action_buttons(lang, scanners);
                row![save, actions]
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .into()
            }
            SaveAction::Renaming { draft } => row![
                text_input(&t(lang, "save-name-placeholder"), draft)
                    .on_input(Message::SaveRenameDraftChanged)
                    .on_submit(Message::SaveRenameConfirm)
                    .padding(8)
                    .width(Length::Fill),
                button(text(t(lang, "save-rename-confirm")).size(STANDARD_TEXT_SIZE))
                    .padding(STANDARD_PADDING)
                    .style(button::primary)
                    .on_press(Message::SaveRenameConfirm),
                button(text(t(lang, "save-action-cancel")).size(STANDARD_TEXT_SIZE))
                    .padding(STANDARD_PADDING)
                    .on_press(Message::SaveActionCancel),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            SaveAction::ConfirmDelete => row![
                text(t(lang, "save-delete-prompt")).style(save_view::muted_text_style).width(Length::Fill),
                button(text(t(lang, "save-delete-confirm")).size(STANDARD_TEXT_SIZE))
                    .padding(STANDARD_PADDING)
                    .style(button::danger)
                    .on_press(Message::SaveDeleteConfirm),
                button(text(t(lang, "save-action-cancel")).size(STANDARD_TEXT_SIZE))
                    .padding(STANDARD_PADDING)
                    .on_press(Message::SaveActionCancel),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            SaveAction::NewSave { draft, template } => {
                // Templates available for the current game+patch (incl.
                // built-ins). Names get sorted with the default ("") first.
                let mut names: Vec<String> = templates_for_selection(self, scanners)
                    .map(|t| t.keys().cloned().collect())
                    .unwrap_or_default();
                names.sort_by(|a, b| match (a.is_empty(), b.is_empty()) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.cmp(b),
                });
                // Display the empty string as a localized "(default)".
                let default_label = t(lang, "save-template-default");
                let display_names: Vec<String> = names
                    .iter()
                    .map(|n| if n.is_empty() { default_label.clone() } else { n.clone() })
                    .collect();
                let selected_display = if template.is_empty() {
                    default_label.clone()
                } else {
                    template.clone()
                };
                let default_label_for_select = default_label.clone();
                row![
                    pick_list(display_names, Some(selected_display), move |picked| {
                        let real = if picked == default_label_for_select {
                            String::new()
                        } else {
                            picked
                        };
                        Message::SaveNewTemplateSelected(real)
                    })
                    .width(Length::Fixed(180.0)),
                    text_input(&t(lang, "save-name-placeholder"), draft)
                        .on_input(Message::SaveNewDraftChanged)
                        .on_submit(Message::SaveNewConfirm)
                        .padding(8)
                        .width(Length::Fill),
                    button(text(t(lang, "save-new-confirm")).size(STANDARD_TEXT_SIZE))
                        .padding(STANDARD_PADDING)
                        .style(button::primary)
                        .on_press(Message::SaveNewConfirm),
                    button(text(t(lang, "save-action-cancel")).size(STANDARD_TEXT_SIZE))
                        .padding(STANDARD_PADDING)
                        .on_press(Message::SaveActionCancel),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
        };

        container(
            column![game_row, save_row]
                .spacing(6)
                .padding(8),
        )
        .width(Fill)
        .into()
    }

    fn save_action_buttons<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
    ) -> Element<'a, Message> {
        let enabled = self.local_save.is_some();
        let mk = |label: String, msg: Message, on: bool| {
            let mut b = button(text(label).size(STANDARD_TEXT_SIZE)).padding(STANDARD_PADDING);
            if on {
                b = b.on_press(msg);
            }
            b
        };
        // "New save" is enabled only when the active patch+version ships
        // a save template for the selected game.
        let can_new = templates_for_selection(self, scanners).is_some();
        row![
            mk(t(lang, "save-new"), Message::SaveNewStart, can_new).style(if can_new {
                button::primary
            } else {
                button::text
            }),
            mk(t(lang, "save-open-folder"), Message::SaveOpenFolder, enabled),
            mk(t(lang, "save-duplicate"), Message::SaveDuplicate, enabled),
            mk(t(lang, "save-rename"), Message::SaveRenameStart, enabled),
            mk(t(lang, "save-delete"), Message::SaveDeleteStart, enabled).style(if enabled {
                button::danger
            } else {
                button::text
            }),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    }

    fn save_view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        loaded: Option<&'a selection::Loaded>,
        streamer_mode: bool,
    ) -> Element<'a, Message> {
        let Some(loaded) = loaded else {
            return container(text(t(lang, "play-no-selection")).size(13))
                .center(Fill)
                .into();
        };

        let available = save_view::available_tabs(loaded.save.as_ref(), streamer_mode);
        if available.is_empty() {
            return container(text(t(lang, "save-empty")).size(13))
                .center(Fill)
                .into();
        }

        let active = self
            .save_tab
            .filter(|t| available.contains(t))
            .unwrap_or(available[0]);

        let opts = save_view::RenderOpts {
            folder_grouped: self.folder_grouped,
            hovered_ncp_idx: self.hovered_ncp_idx,
        };

        let tab_button = |label: String, tab: save_view::Tab| {
            let style = if tab == active { button::primary } else { button::text };
            button(text(label).size(STANDARD_TEXT_SIZE))
                .padding(STANDARD_PADDING)
                .style(style)
                .on_press(Message::SaveTabSelected(tab))
        };

        let mut tab_row = row![].spacing(2).align_y(Alignment::Center);
        for tab in &available {
            tab_row = tab_row.push(tab_button(t(lang, save_view::tab_key(*tab)), *tab));
        }
        tab_row = tab_row.push(horizontal_space());
        // save_view::tab_strip_extras returns Element<save_view::Message>;
        // it's currently the same Message type for those controls (folder
        // group toggle, copy). We map them through the play Message.
        if let Some(extras) = tab_strip_extras(lang, active, opts) {
            tab_row = tab_row.push(extras);
        }
        let tabs = container(tab_row.padding([4, 8]))
            .width(Fill)
            .style(|_| iced::widget::container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgba8(255, 255, 255, 0.02))),
                ..iced::widget::container::Style::default()
            });

        let body = save_view::render(lang, active, loaded, opts);

        column![tabs, body].width(Fill).height(Fill).into()
    }

    fn bottom_strip<'a>(&'a self, lang: &'a LanguageIdentifier) -> Element<'a, Message> {
        let play_button = if self.playing {
            button(text(t(lang, "play-cancel")).size(PRIMARY_TEXT_SIZE))
                .padding(PRIMARY_PADDING)
                .style(button::danger)
                .on_press(Message::PlayPressed)
        } else {
            button(text(t(lang, "play-play")).size(PRIMARY_TEXT_SIZE))
                .padding(PRIMARY_PADDING)
                .style(button::success)
                .on_press(Message::PlayPressed)
        };

        let status: Element<'_, _> = if let Some(flash) = self.flash_status.as_ref() {
            text(flash.clone()).size(12).style(text::danger).into()
        } else if self.playing {
            text(t(lang, "play-status-connecting")).size(13).style(text::primary).into()
        } else {
            text(t(lang, "play-status-idle")).size(12).into()
        };

        container(
            row![
                text_input(&t(lang, "play-link-code"), &self.link_code)
                    .on_input(Message::LinkCodeChanged)
                    .padding(8)
                    .width(Length::Fixed(260.0)),
                play_button,
                horizontal_space(),
                status,
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .padding(8),
        )
        .width(Fill)
        .into()
    }
}

/// Play-local wrapper for save_view's tab strip extras. The extras
/// (folder group toggle + copy buttons) were defined in save_view
/// against `crate::Message` before the per-tab split; here we
/// inline them so they emit `play::Message` directly.
fn tab_strip_extras<'a>(
    lang: &'a LanguageIdentifier,
    tab: save_view::Tab,
    opts: save_view::RenderOpts,
) -> Option<Element<'static, Message>> {
    let copy_btn = |tab: save_view::Tab| -> Element<'static, Message> {
        button(text(t(lang, "save-copy")).size(13))
            .padding([4, 10])
            .style(button::secondary)
            .on_press(Message::CopyTabAsText(tab))
            .into()
    };
    match tab {
        save_view::Tab::Folder => Some(
            row![
                iced::widget::checkbox(t(lang, "folder-group"), opts.folder_grouped)
                    .on_toggle(Message::ToggleFolderGrouped)
                    .size(14)
                    .text_size(12),
                copy_btn(save_view::Tab::Folder),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into(),
        ),
        save_view::Tab::PatchCards => Some(copy_btn(save_view::Tab::PatchCards)),
        save_view::Tab::AutoBattleData => Some(copy_btn(save_view::Tab::AutoBattleData)),
        _ => None,
    }
}

/// Lookup the patch save templates for the current game+patch+version
/// selection. Returns `None` if any of (game / patch / version /
/// template-for-game) are missing. The returned map is the templates
/// keyed by template name (empty string = default).
pub fn templates_for_selection_public(
    state: &PlayState,
    scanners: &Scanners,
) -> Option<std::collections::BTreeMap<String, Box<dyn tango_dataview::save::Save + Send + Sync>>> {
    templates_for_selection(state, scanners)
}

fn templates_for_selection<'a>(
    state: &PlayState,
    scanners: &'a Scanners,
) -> Option<std::collections::BTreeMap<String, Box<dyn tango_dataview::save::Save + Send + Sync>>> {
    let game = state.local_game?;
    let mut out = std::collections::BTreeMap::new();

    // Patch-provided templates first (so a patch can override the
    // bundled default), then fall back to the built-in for this game.
    if let (Some(patch_name), Some(version)) = (state.local_patch.as_ref(), state.local_patch_version.as_ref()) {
        let patches = scanners.patches.read();
        if let Some(patch) = patches.get(patch_name) {
            if let Some(v) = patch.versions.get(version) {
                if let Some(m) = v.save_templates.get(&game) {
                    for (name, save) in m.iter() {
                        out.insert(name.clone(), save.clone_box());
                    }
                }
            }
        }
    }
    // Fall back to bundled per-game templates registered via the Game
    // trait. Patch templates take precedence: if a patch ships a
    // "heat-guts" template, it overrides the built-in of the same name.
    if let Some(game_impl) = game::from_gamedb_entry(game) {
        for (name, save) in game_impl.save_templates() {
            out.entry((*name).to_string())
                .or_insert_with(|| save.clone_box());
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Write a template's SRAM to `saves_dir/<name>.sav`. The filename is
/// taken verbatim from `name` (trimmed); on collisions returns Err.
///
/// `rebuild_checksum()` is required before `as_sram_dump()` — without
/// it the SRAM checksum is stale (computed at template-construction
/// time, before this game-specific clone) and both the GBA game and
/// Tango's `parse_save` reject the resulting file. The legacy app
/// does the same in `gui/save_select_view.rs::create_new_save`.
pub fn create_new_save(
    saves_dir: &std::path::Path,
    name: &str,
    template: &dyn tango_dataview::save::Save,
) -> anyhow::Result<std::path::PathBuf> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("empty save name");
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        anyhow::bail!("invalid save name");
    }
    let filename = if name.ends_with(".sav") {
        name.to_string()
    } else {
        format!("{name}.sav")
    };
    let dst = saves_dir.join(filename);
    if dst.exists() {
        anyhow::bail!("destination already exists");
    }
    std::fs::create_dir_all(saves_dir)?;
    let mut save = template.clone_box();
    save.rebuild_checksum();
    let sram = save.as_sram_dump();
    std::fs::write(&dst, sram)?;
    Ok(dst)
}

/// Centered card used for the no-roms / no-saves hints. Title is
/// rendered larger, body lines stack underneath in muted text.
fn empty_state_card(title: String, body_lines: Vec<String>) -> Element<'static, Message> {
    let mut col = column![text(title).size(18)].spacing(8).align_x(Alignment::Center);
    for line in body_lines {
        col = col.push(text(line).size(12).style(save_view::muted_text_style));
    }
    container(col.padding(20).max_width(520))
        .center(Fill)
        .into()
}

// ---------- File-level save helpers ----------

/// Copy `src` to a sibling file with " (copy)" inserted before the
/// extension (with " (copy 2)", " (copy 3)", ... on collisions).
pub fn duplicate_save(src: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let parent = src
        .parent()
        .ok_or_else(|| anyhow::anyhow!("save has no parent dir"))?;
    let stem = src
        .file_stem()
        .ok_or_else(|| anyhow::anyhow!("save has no file stem"))?
        .to_string_lossy()
        .into_owned();
    let ext = src.extension().map(|e| e.to_string_lossy().into_owned());

    for n in 1..1000 {
        let suffix = if n == 1 {
            " (copy)".to_string()
        } else {
            format!(" (copy {n})")
        };
        let new_name = if let Some(ext) = &ext {
            format!("{stem}{suffix}.{ext}")
        } else {
            format!("{stem}{suffix}")
        };
        let candidate = parent.join(new_name);
        if !candidate.exists() {
            std::fs::copy(src, &candidate)?;
            return Ok(candidate);
        }
    }
    anyhow::bail!("could not find unused name after 999 tries");
}

/// Rename `src` to use `new_stem` (extension preserved). Refuses
/// path-traversal or empty names.
pub fn rename_save(src: &std::path::Path, new_stem: &str) -> anyhow::Result<std::path::PathBuf> {
    if new_stem.is_empty() {
        anyhow::bail!("empty save name");
    }
    if new_stem.contains('/') || new_stem.contains('\\') || new_stem.contains("..") {
        anyhow::bail!("invalid save name");
    }
    let parent = src
        .parent()
        .ok_or_else(|| anyhow::anyhow!("save has no parent dir"))?;
    let ext = src.extension().map(|e| e.to_string_lossy().into_owned());
    let new_name = if let Some(ext) = ext {
        format!("{new_stem}.{ext}")
    } else {
        new_stem.to_string()
    };
    let dst = parent.join(new_name);
    if dst == src {
        return Ok(dst);
    }
    if dst.exists() {
        anyhow::bail!("destination already exists");
    }
    std::fs::rename(src, &dst)?;
    Ok(dst)
}

use crate::i18n::t;
use crate::icons;
use crate::{
    config, game, rom, save_view, selection, Scanners, PRIMARY_PADDING, PRIMARY_TEXT_SIZE, STANDARD_PADDING,
    STANDARD_TEXT_SIZE, TEXT_BODY, TEXT_CAPTION, TEXT_HEADING, TEXT_TITLE,
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
    SaveViewAction(save_view::Action),
    LinkCodeChanged(String),
    /// Fill the link-code input with a fresh random
    /// adjective-word-noun handle from `randomcode::generate`.
    LinkCodeRandom,
    PlayPressed,
    NetplayDisconnect,
    /// Lobby UI: user picked a different match type. App routes
    /// this through netplay::Message::SetMatchType so the resend
    /// machinery picks it up.
    NetplaySetMatchType((u8, u8)),
    /// Lobby UI: user dragged the input-delay slider.
    NetplaySetInputDelay(u8),
    /// Lobby UI: user toggled the reveal-setup checkbox.
    NetplaySetRevealSetup(bool),
    /// Lobby UI: user pressed Ready. App loads the local
    /// save's raw SRAM, builds a NegotiatedState, and
    /// dispatches netplay::Message::Commit.
    NetplayReady,
    /// Lobby UI: user pressed Unready (Ready button while
    /// already committed). Sends an Uncommit packet.
    NetplayUnready,
    Rescan,

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
    /// Persistent state for the embedded save view (active tab,
    /// folder grouping). Apply incoming `SaveViewAction`s via
    /// [`save_view::State::apply`].
    pub save_view: save_view::State,
    /// Inline state for the save-management actions (rename / delete).
    pub save_action: SaveAction,
    pub link_code: String,
    /// Transient one-shot status message shown beneath the link-code
    /// input; reset by the next user action. Used today to flag that
    /// netplay isn't implemented; will likely host real lobby status
    /// messages once it is.
    pub flash_status: Option<String>,
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
            save_view: save_view::State::new(),
            save_action: SaveAction::None,
            link_code: String::new(),
            flash_status: None,
        }
    }
}

/// Side-effects bubble-up. Mirrors the [`crate::tabs::replays::Effect`]
/// convention: pure UI-state mutations happen inside
/// [`PlayState::update`]; anything that requires App-level
/// collaborators (scanners refresh + config persist, session host,
/// netplay subsystem, clipboard, file system) comes back as an
/// `Effect` for the caller to interpret.
#[derive(Debug)]
pub enum Effect {
    /// Selection (game / save / patch / version) changed. App
    /// should rebuild its `Loaded` cache + persist config.
    SelectionChanged,
    /// User clicked Rescan; App should scanner-rescan + refresh.
    Rescan,
    /// `open::that(_)` on a file or folder.
    OpenPath(std::path::PathBuf),
    /// Copy plain text to the clipboard.
    CopyText(String),
    /// Copy a raster image to the clipboard.
    CopyImage(image::RgbaImage),
    /// User pressed Play with no link code → start a single-player
    /// session from the current selection.
    StartSinglePlayer,
    /// User pressed Play with a link code → kick off netplay
    /// signaling against the matchmaking endpoint.
    NetplayConnect(String),
    /// Forward verbatim to the netplay subsystem.
    Netplay(crate::netplay::Message),
    /// Lobby Ready — App reads the local save SRAM and
    /// dispatches `netplay::Message::Commit`.
    NetplayReadyWithSave,
    /// Duplicate the currently-selected save file.
    SaveDuplicate,
    /// Rename the currently-selected save to `new_stem` (no
    /// extension; rename_save adds `.sav`).
    SaveRename { new_stem: String },
    /// Delete the currently-selected save file.
    SaveDelete,
    /// Create a fresh save in the saves dir from a bundled
    /// template.
    SaveNew { name: String, template: String },
}

impl PlayState {
    /// Apply a tab message. See [`crate::tabs::replays::Effect`]
    /// for the side-effect surface convention.
    pub fn update(
        &mut self,
        msg: Message,
        scanners: &Scanners,
        config: &config::Config,
        loaded: Option<&selection::Loaded>,
    ) -> Option<Effect> {
        match msg {
            Message::LocalGameSelected(g) => {
                self.local_game = Some(g.game);
                self.local_save = scanners
                    .saves
                    .read()
                    .get(&g.game)
                    .and_then(|v| v.first().map(|s| s.path.clone()));
                self.local_patch = None;
                self.local_patch_version = None;
                Some(Effect::SelectionChanged)
            }
            Message::LocalSaveSelected(s) => {
                self.local_save = Some(s.path);
                Some(Effect::SelectionChanged)
            }
            Message::LocalPatchSelected(p) => {
                if p == t(&config.language, "play-no-patch") {
                    self.local_patch = None;
                    self.local_patch_version = None;
                } else {
                    let v = scanners
                        .patches
                        .read()
                        .get(&p)
                        .and_then(|patch| patch.versions.keys().max().cloned());
                    self.local_patch = Some(p);
                    self.local_patch_version = v;
                }
                Some(Effect::SelectionChanged)
            }
            Message::LocalPatchVersionSelected(v) => {
                self.local_patch_version = Some(v);
                Some(Effect::SelectionChanged)
            }
            Message::SaveViewAction(action) => {
                self.save_view.apply(&action);
                let loaded = loaded?;
                match action {
                    save_view::Action::CopyTab(tab) => {
                        save_view::tab_as_text(&config.language, tab, loaded).map(Effect::CopyText)
                    }
                    save_view::Action::CopyTabImage(tab) => {
                        save_view::tab_as_image(tab, loaded).map(Effect::CopyImage)
                    }
                    _ => None,
                }
            }
            Message::LinkCodeChanged(s) => {
                self.link_code = s;
                self.flash_status = None;
                None
            }
            Message::LinkCodeRandom => {
                self.link_code = crate::randomcode::generate(&config.language);
                self.flash_status = None;
                None
            }
            Message::PlayPressed => {
                self.flash_status = None;
                let trimmed = self.link_code.trim();
                if trimmed.is_empty() {
                    if loaded.is_none() {
                        self.flash_status = Some(t(&config.language, "play-no-selection"));
                        return None;
                    }
                    Some(Effect::StartSinglePlayer)
                } else {
                    Some(Effect::NetplayConnect(trimmed.to_string()))
                }
            }
            Message::NetplayDisconnect => Some(Effect::Netplay(crate::netplay::Message::Disconnect)),
            Message::NetplaySetMatchType(mt) => {
                Some(Effect::Netplay(crate::netplay::Message::SetMatchType(mt)))
            }
            Message::NetplaySetInputDelay(d) => {
                Some(Effect::Netplay(crate::netplay::Message::SetInputDelay(d)))
            }
            Message::NetplaySetRevealSetup(v) => {
                Some(Effect::Netplay(crate::netplay::Message::SetRevealSetup(v)))
            }
            Message::NetplayReady => Some(Effect::NetplayReadyWithSave),
            Message::NetplayUnready => Some(Effect::Netplay(crate::netplay::Message::Uncommit)),
            Message::Rescan => Some(Effect::Rescan),
            Message::SaveOpenFolder => self
                .local_save
                .as_ref()
                .and_then(|p| p.parent())
                .map(|p| Effect::OpenPath(p.to_path_buf())),
            Message::SaveDuplicate => Some(Effect::SaveDuplicate),
            Message::SaveRenameStart => {
                let draft = self
                    .local_save
                    .as_ref()
                    .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                self.save_action = SaveAction::Renaming { draft };
                None
            }
            Message::SaveRenameDraftChanged(s) => {
                if let SaveAction::Renaming { draft } = &mut self.save_action {
                    *draft = s;
                }
                None
            }
            Message::SaveRenameConfirm => {
                let new_stem = if let SaveAction::Renaming { draft } = &self.save_action {
                    draft.trim().to_string()
                } else {
                    String::new()
                };
                self.save_action = SaveAction::None;
                if new_stem.is_empty() {
                    None
                } else {
                    Some(Effect::SaveRename { new_stem })
                }
            }
            Message::SaveDeleteStart => {
                self.save_action = SaveAction::ConfirmDelete;
                None
            }
            Message::SaveDeleteConfirm => {
                self.save_action = SaveAction::None;
                Some(Effect::SaveDelete)
            }
            Message::SaveActionCancel => {
                self.save_action = SaveAction::None;
                None
            }
            Message::SaveNewStart => {
                let saves_dir = config.saves_path();
                let mut draft = "new save".to_string();
                for n in 2..100 {
                    if !saves_dir.join(format!("{draft}.sav")).exists() {
                        break;
                    }
                    draft = format!("new save {n}");
                }
                self.save_action = SaveAction::NewSave {
                    draft,
                    template: String::new(),
                };
                None
            }
            Message::SaveNewDraftChanged(s) => {
                if let SaveAction::NewSave { draft, .. } = &mut self.save_action {
                    *draft = s;
                }
                None
            }
            Message::SaveNewTemplateSelected(name) => {
                if let SaveAction::NewSave { template, .. } = &mut self.save_action {
                    *template = name;
                }
                None
            }
            Message::SaveNewConfirm => {
                let (name, template) = if let SaveAction::NewSave { draft, template } = &self.save_action {
                    (draft.trim().to_string(), template.clone())
                } else {
                    (String::new(), String::new())
                };
                self.save_action = SaveAction::None;
                if name.is_empty() {
                    None
                } else {
                    Some(Effect::SaveNew { name, template })
                }
            }
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
        netplay_phase: &'a crate::netplay::Phase,
        netplay_lobby: &'a crate::netplay::LobbyState,
    ) -> Element<'a, Message> {
        // In Lobby phase the body splits top/bottom — save view
        // on top so the user can keep eyeing what they brought to
        // the match, lobby controls + opponent info underneath.
        // Outside Lobby, the body is just the save view (or the
        // empty-state hints).
        let body: Element<'a, Message> = match netplay_phase {
            crate::netplay::Phase::Lobby { .. } => column![
                // Save view soaks up the remaining vertical space.
                container(self.body(lang, scanners, loaded, streamer_mode, config))
                    .width(Fill)
                    .height(Fill),
                horizontal_rule(1),
                // Fixed-height lobby pane so the whole strip
                // (settings + controls + verdict + ready row)
                // is always fully visible and never squeezes
                // the save view to zero. Tuned by eyeball to
                // fit current contents — if you add more rows
                // here, bump this.
                container(lobby_view(lang, netplay_lobby, self.local_game, scanners))
                    .width(Fill)
                    .height(Length::Fixed(360.0)),
            ]
            .height(Fill)
            .into(),
            _ => self.body(lang, scanners, loaded, streamer_mode, config),
        };

        column![
            self.selector_strip(lang, scanners),
            body,
            horizontal_rule(1),
            self.bottom_strip(lang, netplay_phase),
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
            .text_size(STANDARD_TEXT_SIZE)
            .padding(STANDARD_PADDING)
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
            .text_size(STANDARD_TEXT_SIZE)
            .padding(STANDARD_PADDING)
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
        .text_size(STANDARD_TEXT_SIZE)
        .padding(STANDARD_PADDING)
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
        .text_size(STANDARD_TEXT_SIZE)
        .padding(STANDARD_PADDING)
        .width(Length::Fixed(100.0));

        let refresh = icons::icon_button(
            icons::RESCAN,
            t(lang, "rescan"),
            Message::Rescan,
            STANDARD_TEXT_SIZE,
            STANDARD_PADDING,
        );

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
                    .size(STANDARD_TEXT_SIZE)
                    .padding(STANDARD_PADDING)
                    .width(Length::Fill),
                icons::icon_button_styled(
                    icons::CONFIRM,
                    t(lang, "save-rename-confirm"),
                    Some(Message::SaveRenameConfirm),
                    STANDARD_TEXT_SIZE,
                    STANDARD_PADDING,
                    button::primary,
                ),
                icons::icon_button(
                    icons::CANCEL,
                    t(lang, "save-action-cancel"),
                    Message::SaveActionCancel,
                    STANDARD_TEXT_SIZE,
                    STANDARD_PADDING,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            SaveAction::ConfirmDelete => row![
                text(t(lang, "save-delete-prompt")).style(save_view::muted_text_style).width(Length::Fill),
                icons::labeled_icon_button(
                    icons::DELETE,
                    t(lang, "save-delete-confirm"),
                    Message::SaveDeleteConfirm,
                    STANDARD_TEXT_SIZE,
                    STANDARD_PADDING,
                    button::danger,
                ),
                icons::icon_button(
                    icons::CANCEL,
                    t(lang, "save-action-cancel"),
                    Message::SaveActionCancel,
                    STANDARD_TEXT_SIZE,
                    STANDARD_PADDING,
                ),
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
                    .text_size(STANDARD_TEXT_SIZE)
                    .padding(STANDARD_PADDING)
                    .width(Length::Fixed(180.0)),
                    text_input(&t(lang, "save-name-placeholder"), draft)
                        .on_input(Message::SaveNewDraftChanged)
                        .on_submit(Message::SaveNewConfirm)
                        .size(STANDARD_TEXT_SIZE)
                        .padding(STANDARD_PADDING)
                        .width(Length::Fill),
                    icons::labeled_icon_button(
                        icons::CONFIRM,
                        t(lang, "save-new-confirm"),
                        Message::SaveNewConfirm,
                        STANDARD_TEXT_SIZE,
                        STANDARD_PADDING,
                        button::primary,
                    ),
                    icons::icon_button(
                        icons::CANCEL,
                        t(lang, "save-action-cancel"),
                        Message::SaveActionCancel,
                        STANDARD_TEXT_SIZE,
                        STANDARD_PADDING,
                    ),
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
        let mk = |icon: &'static str, label: String, msg: Message, on: bool| {
            icons::icon_button_maybe(
                icon,
                label,
                if on { Some(msg) } else { None },
                STANDARD_TEXT_SIZE,
                STANDARD_PADDING,
            )
        };
        // Destructive variant for Delete — flags it red so it
        // doesn't look like just another toolbar action.
        let mk_danger = |icon: &'static str, label: String, msg: Message, on: bool| {
            icons::icon_button_styled(
                icon,
                label,
                if on { Some(msg) } else { None },
                STANDARD_TEXT_SIZE,
                STANDARD_PADDING,
                iced::widget::button::danger,
            )
        };
        // "New save" is enabled only when the active patch+version ships
        // a save template for the selected game.
        let can_new = templates_for_selection(self, scanners).is_some();
        row![
            mk(icons::NEW, t(lang, "save-new"), Message::SaveNewStart, can_new),
            mk(icons::FOLDER, t(lang, "save-open-folder"), Message::SaveOpenFolder, enabled),
            mk(icons::DUPLICATE, t(lang, "save-duplicate"), Message::SaveDuplicate, enabled),
            mk(icons::RENAME, t(lang, "save-rename"), Message::SaveRenameStart, enabled),
            mk_danger(icons::DELETE, t(lang, "save-delete"), Message::SaveDeleteStart, enabled),
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
            return container(text(t(lang, "play-no-selection")).size(TEXT_BODY))
                .center(Fill)
                .into();
        };
        save_view::view(lang, loaded, &self.save_view, streamer_mode)
            .map(Message::SaveViewAction)
    }

    fn bottom_strip<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        netplay: &'a crate::netplay::Phase,
    ) -> Element<'a, Message> {
        use crate::netplay::Phase;
        // The Play tab is only visible when no session is running
        // (App::view dispatches to session::view otherwise), so the
        // only "in-progress" state we need to surface here is netplay.
        // Cancel = disconnect, all phases other than Idle / Failed.
        let netplay_in_flight = !matches!(netplay, Phase::Idle | Phase::Failed { .. });
        let play_button: Element<'a, Message> = if netplay_in_flight {
            icons::labeled_icon_button(
                icons::CLOSE,
                t(lang, "play-cancel"),
                Message::NetplayDisconnect,
                PRIMARY_TEXT_SIZE,
                PRIMARY_PADDING,
                button::danger,
            )
        } else if self.link_code.trim().is_empty() {
            icons::labeled_icon_button(
                icons::PLAY,
                t(lang, "play-play"),
                Message::PlayPressed,
                PRIMARY_TEXT_SIZE,
                PRIMARY_PADDING,
                button::primary,
            )
        } else {
            // Non-empty link code → netplay-bound. Surface this
            // explicitly via "Fight" + a swords glyph so the user
            // can tell at a glance they're about to start a
            // match, not a singleplayer session.
            icons::labeled_icon_button(
                icons::FIGHT,
                t(lang, "play-fight"),
                Message::PlayPressed,
                PRIMARY_TEXT_SIZE,
                PRIMARY_PADDING,
                button::primary,
            )
        };

        // flash_status (single-player launch error etc.) takes priority
        // over the netplay phase label.
        let status: Element<'_, _> = if let Some(flash) = self.flash_status.as_ref() {
            text(flash.clone()).size(TEXT_CAPTION).style(text::danger).into()
        } else {
            let primary_style = text::primary;
            let success_style = |theme: &iced::Theme| iced::widget::text::Style {
                color: Some(theme.palette().success),
            };
            match netplay {
                Phase::Connecting { link_code } => text(format!(
                    "{} {link_code}",
                    t(lang, "play-status-connecting")
                ))
                .size(TEXT_BODY)
                .style(primary_style)
                .into(),
                Phase::Negotiating { link_code } => text(format!(
                    "{} {link_code}",
                    t(lang, "play-status-negotiating")
                ))
                .size(TEXT_BODY)
                .style(primary_style)
                .into(),
                Phase::Lobby { link_code } => text(format!(
                    "{} {link_code}",
                    t(lang, "play-status-lobby")
                ))
                .size(TEXT_BODY)
                .style(success_style)
                .into(),
                Phase::Failed { error } => {
                    text(format!("{}: {error}", t(lang, "play-status-failed")))
                        .size(TEXT_CAPTION)
                        .style(text::danger)
                        .into()
                }
                Phase::Idle => text(t(lang, "play-status-idle")).size(TEXT_CAPTION).into(),
            }
        };

        // Link-code field:
        //   * Lobby — hide entirely (the code is in the status
        //     line, no point editing it).
        //   * Connecting / Negotiating — show but read-only
        //     (omitting on_input disables the field in iced).
        //   * Idle / Failed — fully editable. The dice button
        //     fills it with a fresh random handle.
        let (link_input, shuffle_button): (Option<Element<'a, Message>>, Option<Element<'a, Message>>) =
            match netplay {
                Phase::Lobby { .. } => (None, None),
                Phase::Connecting { .. } | Phase::Negotiating { .. } => (
                    Some(
                        text_input(&t(lang, "play-link-code"), &self.link_code)
                            .size(STANDARD_TEXT_SIZE)
                            .padding(STANDARD_PADDING)
                            .width(Length::Fixed(260.0))
                            .into(),
                    ),
                    None,
                ),
                _ => (
                    Some(
                        text_input(&t(lang, "play-link-code"), &self.link_code)
                            .on_input(Message::LinkCodeChanged)
                            .on_submit(Message::PlayPressed)
                            .size(STANDARD_TEXT_SIZE)
                            .padding(STANDARD_PADDING)
                            .width(Length::Fixed(260.0))
                            .into(),
                    ),
                    Some(icons::icon_button(
                        icons::DICE,
                        t(lang, "play-link-code-random"),
                        Message::LinkCodeRandom,
                        STANDARD_TEXT_SIZE,
                        STANDARD_PADDING,
                    )),
                ),
            };

        let mut row = row![].spacing(8).align_y(Alignment::Center).padding(8);
        if let Some(input) = link_input {
            row = row.push(input);
        }
        if let Some(shuffle) = shuffle_button {
            row = row.push(shuffle);
        }
        row = row.push(play_button).push(horizontal_space()).push(status);

        container(row).width(Fill).into()
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

/// Lobby pane shown in the Play tab body while netplay is in
/// `Phase::Lobby`. Two columns — you on the left, opponent on the
/// right — plus a latency line at the top + match-type + input-
/// delay controls underneath. Settings round-trips asynchronously,
/// so either side may be `None` for a tick.
fn lobby_view<'a>(
    lang: &'a LanguageIdentifier,
    lobby: &'a crate::netplay::LobbyState,
    local_game: Option<rom::GameRef>,
    scanners: &'a Scanners,
) -> Element<'a, Message> {
    let side = |label: String, settings: Option<&crate::net::protocol::Settings>| -> Element<'static, Message> {
        let Some(settings) = settings else {
            return container(
                column![
                    text(label).size(TEXT_CAPTION).style(save_view::muted_text_style),
                    text(t(lang, "lobby-waiting"))
                        .size(TEXT_BODY)
                        .style(save_view::muted_text_style),
                ]
                .spacing(4),
            )
            .padding(12)
            .width(Length::Fill)
            .into();
        };
        let nickname = settings.nickname.clone();
        let game_label = settings
            .game_info
            .as_ref()
            .and_then(|gi| {
                let (family, variant) = (gi.family_and_variant.0.as_str(), gi.family_and_variant.1);
                tango_gamedb::find_by_family_and_variant(family, variant)
                    .map(|g| crate::game::display_name(lang, g))
            })
            .or_else(|| settings.game_info.as_ref().map(|gi| {
                format!("{} v{}", gi.family_and_variant.0, gi.family_and_variant.1)
            }))
            .unwrap_or_else(|| t(lang, "lobby-no-game"));
        let patch = settings
            .game_info
            .as_ref()
            .and_then(|gi| gi.patch.as_ref())
            .map(|p| format!("{} v{}", p.name, p.version));
        let mt = crate::game::match_type_name(
            lang,
            settings
                .game_info
                .as_ref()
                .map(|gi| gi.family_and_variant.0.as_str())
                .unwrap_or(""),
            settings.match_type.0,
            settings.match_type.1,
        );
        let mut col = column![
            text(label).size(TEXT_CAPTION).style(save_view::muted_text_style),
            text(nickname).size(TEXT_HEADING),
            text(game_label).size(TEXT_CAPTION),
        ]
        .spacing(4);
        if let Some(p) = patch {
            col = col.push(
                text(p)
                    .size(TEXT_CAPTION)
                    .style(|theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(theme.palette().primary),
                    }),
            );
        }
        col = col.push(
            text(format!("{}: {mt}", t(lang, "replays-match-type")))
                .size(TEXT_CAPTION)
                .style(save_view::muted_text_style),
        );
        container(col).padding(12).width(Length::Fill).into()
    };

    let header_line = if let Some(d) = lobby.latency {
        text(format!(
            "{}: {} ms",
            t(lang, "lobby-latency"),
            d.as_millis()
        ))
        .size(TEXT_CAPTION)
        .style(save_view::muted_text_style)
    } else {
        text(t(lang, "lobby-handshake"))
            .size(TEXT_CAPTION)
            .style(save_view::muted_text_style)
    };

    // Match-type pick_list — options pulled from the current
    // local game's Game::match_types() table (mode + subtype
    // counts), labeled with the per-game Fluent strings via
    // game::match_type_name. Disabled when no local game is
    // selected (no way to know what modes exist).
    let mt_picker: Element<'a, Message> = if let Some(g) = local_game {
        let game_impl = crate::game::from_gamedb_entry(g);
        let mt_table = game_impl.map(|gi| gi.match_types()).unwrap_or(&[]);
        let mut options = Vec::new();
        for (mode, subtype_count) in mt_table.iter().enumerate() {
            for sub in 0..*subtype_count {
                options.push(MatchTypeOption {
                    mode: mode as u8,
                    subtype: sub as u8,
                    label: crate::game::match_type_name(
                        lang,
                        g.family_and_variant().0,
                        mode as u8,
                        sub as u8,
                    ),
                });
            }
        }
        let selected = options
            .iter()
            .find(|o| o.mode == lobby.match_type.0 && o.subtype == lobby.match_type.1)
            .cloned();
        if options.is_empty() {
            text(t(lang, "lobby-no-match-types"))
                .size(STANDARD_TEXT_SIZE)
                .style(save_view::muted_text_style)
                .into()
        } else {
            pick_list(options, selected, |o| {
                Message::NetplaySetMatchType((o.mode, o.subtype))
            })
            .text_size(STANDARD_TEXT_SIZE)
            .padding(STANDARD_PADDING)
            .into()
        }
    } else {
        text(t(lang, "lobby-pick-game-first"))
            .size(STANDARD_TEXT_SIZE)
            .style(save_view::muted_text_style)
            .into()
    };

    // Input delay slider — legacy app caps at 10 frames. Each
    // increment is one full GBA frame (~16.7 ms one-way) of
    // smoothing for jittery connections.
    let id_slider = iced::widget::slider(2..=10u8, lobby.input_delay, Message::NetplaySetInputDelay);

    // Reveal-setup checkbox. Mirrors the legacy app's
    // `play-details-reveal-setup` checkbox — each side picks
    // independently; the peer can see (read-only) what we picked
    // via the remote status next to the checkbox.
    let reveal_label = if let Some(r) = lobby.remote.as_ref() {
        if r.reveal_setup {
            t(lang, "lobby-reveal-peer-on")
        } else {
            t(lang, "lobby-reveal-peer-off")
        }
    } else {
        t(lang, "lobby-reveal-peer-unknown")
    };

    let controls = row![
        column![
            text(t(lang, "replays-match-type"))
                .size(TEXT_CAPTION)
                .style(save_view::muted_text_style),
            mt_picker,
        ]
        .spacing(4),
        column![
            text(format!(
                "{}: {}",
                t(lang, "lobby-input-delay"),
                lobby.input_delay
            ))
            .size(TEXT_CAPTION)
            .style(save_view::muted_text_style),
            id_slider,
        ]
        .spacing(4)
        .width(Length::Fixed(220.0)),
        column![
            text(t(lang, "lobby-reveal-setup"))
                .size(TEXT_CAPTION)
                .style(save_view::muted_text_style),
            iced::widget::checkbox(t(lang, "lobby-reveal-mine"), lobby.reveal_setup)
                .on_toggle(Message::NetplaySetRevealSetup)
                .size(TEXT_HEADING)
                .text_size(STANDARD_TEXT_SIZE),
            text(reveal_label).size(TEXT_CAPTION).style(save_view::muted_text_style),
        ]
        .spacing(4),
    ]
    .spacing(20)
    .align_y(Alignment::Center);

    // Compatibility verdict line. Computed every render (cheap —
    // no IO, just lookups against the patches scanner). Drives the
    // colour + the user-facing reason text. Only Compatible
    // unlocks the Ready button below.
    let (verdict_line, compat_ok): (Element<'a, Message>, bool) =
        match (lobby.local.as_ref(), lobby.remote.as_ref()) {
            (Some(l), Some(r)) => {
                let patches = scanners.patches.read();
                let verdict = crate::netplay::compat::check(l, r, &*patches);
                let (key, style): (&'static str, fn(&iced::Theme) -> iced::widget::text::Style) =
                    match verdict {
                        crate::netplay::compat::Verdict::Compatible => {
                            ("lobby-compat-ok", |theme: &iced::Theme| {
                                iced::widget::text::Style {
                                    color: Some(theme.palette().success),
                                }
                            })
                        }
                        crate::netplay::compat::Verdict::MissingGame => {
                            ("lobby-compat-missing-game", save_view::muted_text_style)
                        }
                        crate::netplay::compat::Verdict::MissingRomOrPatch => {
                            ("lobby-compat-missing-rom", iced::widget::text::danger)
                        }
                        crate::netplay::compat::Verdict::DifferentVersions => {
                            ("lobby-compat-version-mismatch", iced::widget::text::danger)
                        }
                        crate::netplay::compat::Verdict::DifferentMatchTypes => {
                            ("lobby-compat-match-mismatch", iced::widget::text::danger)
                        }
                    };
                (
                    text(t(lang, key)).size(TEXT_CAPTION).style(style).into(),
                    matches!(verdict, crate::netplay::compat::Verdict::Compatible),
                )
            }
            _ => (
                text(t(lang, "lobby-handshake"))
                    .size(TEXT_CAPTION)
                    .style(save_view::muted_text_style)
                    .into(),
                false,
            ),
        };

    // Ready button. Disabled until compat says OK + the user
    // actually has a save selected (App routes NetplayReady to a
    // Commit packet built from that save's SRAM). Toggles to
    // Unready once we've sent our Commit. Match-ready overrides
    // both with a "starting…" affordance.
    let ready_button: Element<'a, Message> = if lobby.match_ready {
        icons::labeled_icon_button(
            icons::PLAY,
            t(lang, "lobby-match-starting"),
            Message::NetplayUnready,
            STANDARD_TEXT_SIZE,
            STANDARD_PADDING,
            button::success,
        )
    } else if lobby.local_ready {
        icons::labeled_icon_button(
            icons::CLOSE,
            t(lang, "lobby-unready"),
            Message::NetplayUnready,
            STANDARD_TEXT_SIZE,
            STANDARD_PADDING,
            button::secondary,
        )
    } else {
        icons::icon_button_styled(
            icons::CONFIRM,
            t(lang, "lobby-ready"),
            if compat_ok {
                Some(Message::NetplayReady)
            } else {
                None
            },
            STANDARD_TEXT_SIZE,
            STANDARD_PADDING,
            button::primary,
        )
    };

    // "You: ready / Opponent: ready" pair. Muted while neither
    // has clicked Ready; promotes to primary when each side
    // commits so the user can tell at a glance who's holding
    // things up.
    let ready_status = {
        let style_for = |ready: bool| -> fn(&iced::Theme) -> iced::widget::text::Style {
            if ready {
                |theme: &iced::Theme| iced::widget::text::Style {
                    color: Some(theme.palette().success),
                }
            } else {
                save_view::muted_text_style
            }
        };
        row![
            text(format!(
                "{}: {}",
                t(lang, "play-you"),
                if lobby.local_ready {
                    t(lang, "lobby-ready-yes")
                } else {
                    t(lang, "lobby-ready-no")
                }
            ))
            .size(TEXT_CAPTION)
            .style(style_for(lobby.local_ready)),
            text("·").size(TEXT_CAPTION).style(save_view::muted_text_style),
            text(format!(
                "{}: {}",
                t(lang, "replays-opponent"),
                if lobby.remote_ready {
                    t(lang, "lobby-ready-yes")
                } else {
                    t(lang, "lobby-ready-no")
                }
            ))
            .size(TEXT_CAPTION)
            .style(style_for(lobby.remote_ready)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    };

    container(
        column![
            header_line,
            iced::widget::row![
                side(t(lang, "play-you"), lobby.local.as_ref()),
                iced::widget::vertical_rule(1),
                side(t(lang, "replays-opponent"), lobby.remote.as_ref()),
            ]
            .spacing(12),
            horizontal_rule(1),
            controls,
            verdict_line,
            row![ready_button, horizontal_space(), ready_status]
                .spacing(12)
                .align_y(Alignment::Center),
        ]
        .spacing(12)
        .padding(16),
    )
    .width(Fill)
    // Outer wrapper in play::view sets the actual height (Fixed
    // 360); we Fill into that slot so padding/spacing distributes
    // evenly rather than clumping at the top.
    .height(Fill)
    .into()
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct MatchTypeOption {
    mode: u8,
    subtype: u8,
    label: String,
}
impl std::fmt::Display for MatchTypeOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// Centered card used for the no-roms / no-saves hints. Title is
/// rendered larger, body lines stack underneath in muted text.
fn empty_state_card(title: String, body_lines: Vec<String>) -> Element<'static, Message> {
    let mut col = column![text(title).size(TEXT_TITLE)].spacing(8).align_x(Alignment::Center);
    for line in body_lines {
        col = col.push(text(line).size(TEXT_CAPTION).style(save_view::muted_text_style));
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

use crate::i18n::t;
use crate::icons;
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
    SaveViewAction(save_view::Action),
    LinkCodeChanged(String),
    PlayPressed,
    NetplayDisconnect,
    /// Lobby UI: user picked a different match type. App routes
    /// this through netplay::Message::SetMatchType so the resend
    /// machinery picks it up.
    NetplaySetMatchType((u8, u8)),
    /// Lobby UI: user dragged the input-delay slider.
    NetplaySetInputDelay(u8),
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
                container(self.body(lang, scanners, loaded, streamer_mode, config))
                    .width(Fill)
                    .height(Length::FillPortion(3)),
                horizontal_rule(1),
                container(lobby_view(lang, netplay_lobby, self.local_game, scanners))
                    .width(Fill)
                    .height(Length::FillPortion(2)),
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
                    .padding(8)
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
                    .width(Length::Fixed(180.0)),
                    text_input(&t(lang, "save-name-placeholder"), draft)
                        .on_input(Message::SaveNewDraftChanged)
                        .on_submit(Message::SaveNewConfirm)
                        .padding(8)
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
            return container(text(t(lang, "play-no-selection")).size(13))
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
        } else {
            icons::labeled_icon_button(
                icons::PLAY,
                t(lang, "play-play"),
                Message::PlayPressed,
                PRIMARY_TEXT_SIZE,
                PRIMARY_PADDING,
                button::success,
            )
        };

        // flash_status (single-player launch error etc.) takes priority
        // over the netplay phase label.
        let status: Element<'_, _> = if let Some(flash) = self.flash_status.as_ref() {
            text(flash.clone()).size(12).style(text::danger).into()
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
                .size(13)
                .style(primary_style)
                .into(),
                Phase::Negotiating { link_code } => text(format!(
                    "{} {link_code}",
                    t(lang, "play-status-negotiating")
                ))
                .size(13)
                .style(primary_style)
                .into(),
                Phase::Lobby { link_code } => text(format!(
                    "{} {link_code}",
                    t(lang, "play-status-lobby")
                ))
                .size(13)
                .style(success_style)
                .into(),
                Phase::Failed { error } => {
                    text(format!("{}: {error}", t(lang, "play-status-failed")))
                        .size(12)
                        .style(text::danger)
                        .into()
                }
                Phase::Idle => text(t(lang, "play-status-idle")).size(12).into(),
            }
        };

        container(
            row![
                text_input(&t(lang, "play-link-code"), &self.link_code)
                    .on_input(Message::LinkCodeChanged)
                    .on_submit(Message::PlayPressed)
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
                    text(label).size(11).style(save_view::muted_text_style),
                    text(t(lang, "lobby-waiting"))
                        .size(13)
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
            text(label).size(11).style(save_view::muted_text_style),
            text(nickname).size(16),
            text(game_label).size(12),
        ]
        .spacing(4);
        if let Some(p) = patch {
            col = col.push(
                text(p)
                    .size(11)
                    .style(|theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(theme.palette().primary),
                    }),
            );
        }
        col = col.push(
            text(format!("{}: {mt}", t(lang, "replays-match-type")))
                .size(11)
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
        .size(11)
        .style(save_view::muted_text_style)
    } else {
        text(t(lang, "lobby-handshake"))
            .size(11)
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
    let id_slider = iced::widget::slider(0..=10u8, lobby.input_delay, Message::NetplaySetInputDelay);

    let controls = row![
        column![
            text(t(lang, "replays-match-type"))
                .size(11)
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
            .size(11)
            .style(save_view::muted_text_style),
            id_slider,
        ]
        .spacing(4)
        .width(Length::Fixed(220.0)),
    ]
    .spacing(20)
    .align_y(Alignment::Center);

    // Compatibility verdict line. Computed every render (cheap —
    // no IO, just lookups against the patches scanner). Drives the
    // colour + the user-facing reason text.
    let verdict_line: Element<'a, Message> = match (lobby.local.as_ref(), lobby.remote.as_ref()) {
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
            text(t(lang, key)).size(12).style(style).into()
        }
        _ => text(t(lang, "lobby-handshake"))
            .size(12)
            .style(save_view::muted_text_style)
            .into(),
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
        ]
        .spacing(12)
        .padding(16),
    )
    .width(Fill)
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

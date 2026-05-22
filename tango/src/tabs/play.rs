use crate::app::{Scanners, STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION, TEXT_HEADING, TEXT_TITLE};
use crate::i18n::t;
use crate::widgets;
use crate::{config, game, rom, save_view, selection};
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{button, column, container, pick_list, row, text, text_input, Space};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use unic_langid::LanguageIdentifier;

// ---------- Messages ----------

#[derive(Debug, Clone)]
pub enum Message {
    LocalGameSelected(GameOption),
    LocalSaveSelected(SaveOption),
    LocalPatchSelected(PatchOption),
    LocalPatchVersionSelected(semver::Version),
    SaveViewAction(save_view::Action),
    LinkCodeChanged(String),
    /// Fill the link-code input with a fresh random
    /// adjective-word-noun handle from `randomcode::generate`.
    LinkCodeRandom,
    FightPressed,
    NetplayDisconnect,
    /// Lobby UI: user picked a different match type. App routes
    /// this through netplay::Message::SetMatchType so the resend
    /// machinery picks it up.
    NetplaySetMatchType((u8, u8)),
    /// Lobby UI: user dragged the input-delay slider, OR pressed
    /// the "suggest" button (which dispatches a value computed
    /// from `lobby.latency`).
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
    /// Open an arbitrary folder in the OS file manager. Used by
    /// the no-saves / no-roms empty-state cards to give the user
    /// a one-click jump into the right directory.
    OpenSavesFolder(std::path::PathBuf),
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
    /// User clicked × on the inline error banner; clears
    /// `PlayState::last_error`.
    DismissError,
    /// Soft-disable sentinel for widgets that don't accept a
    /// `None` handler in iced 0.14 (pick_list, slider). The
    /// lobby reroutes match-type / input-delay changes here in
    /// Phase::Failed so the controls render inert without
    /// touching layout. The update handler drops it.
    Noop,
}

// ---------- Game / Save pick_list options ----------

#[derive(Clone)]
pub struct GameOption {
    pub game: rom::GameRef,
    pub display: String,
    /// `false` when no ROM for this game is in the scan results.
    /// Still shown in the dropdown (so users know what's supported)
    /// but `LocalGameSelected` ignores picks where this is false.
    pub available: bool,
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
        if self.available {
            f.write_str(&self.display)
        } else {
            // Lucide "file-x" glyph as a prefix marker. cosmic-text
            // falls back across loaded fonts for codepoints the
            // primary face doesn't have, so the PUA codepoint
            // resolves to the lucide font inside pick_list's
            // single-text-color renderer.
            write!(f, "{} {}", char::from(Icon::FileX), self.display)
        }
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
    /// Pre-computed display label: the save's path relative to the
    /// saves dir, forward-slash separated (so nested folders show up
    /// in the picker). Built when the option list is constructed
    /// because `Display::fmt` doesn't get the saves root as input.
    pub display: String,
}

impl SaveOption {
    pub fn new(saves_path: &std::path::Path, path: std::path::PathBuf) -> Self {
        let display = path
            .strip_prefix(saves_path)
            .ok()
            .map(|rel| {
                rel.components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| path.display().to_string());
        Self { path, display }
    }
}

impl std::fmt::Display for SaveOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct PatchOption {
    /// Real patch name. Empty string is the "no patch" sentinel.
    pub name: String,
    /// Display string. Favorites are prefixed with "★ " so they're
    /// visually distinct in the dropdown.
    pub display: String,
}

impl std::fmt::Display for PatchOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
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
    /// Last after-the-fact action failure (singleplayer launch
    /// errored, PvP session build failed, …) — rendered as a
    /// dismissable banner at the top of the play tab. Pre-condition
    /// errors ("you need a save first") are NOT funneled here;
    /// they're handled by view-time button gating + inline hints,
    /// because graying out the action surface explains itself.
    pub last_error: Option<String>,
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub enum SaveAction {
    #[default]
    None,
    Renaming {
        draft: String,
    },
    ConfirmDelete,
    /// Creating a new save. `template` is the template name (empty
    /// string is the default unnamed template); `draft` is the user's
    /// chosen filename.
    /// `template == None` until the user explicitly picks one
    /// (auto-selected when only one template is available). The
    /// Confirm button is disabled in the None state — there's no
    /// "default" template to fall back on.
    NewSave {
        draft: String,
        template: Option<String>,
        /// The auto-generated default we last wrote into `draft`. While
        /// the user hasn't typed over it, switching templates regenerates
        /// the suggestion; once they edit it, this is `None` and we leave
        /// their value alone.
        auto_default: Option<String>,
    },
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
            last_error: None,
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
    /// User pressed Play with a link code → kick off netplay.
    /// The `LinkIdent` variant tells the app handler whether to
    /// route via matchmaking signaling or direct TCP transport.
    NetplayConnect(crate::netplay::LinkIdent),
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
    /// Task returned from save_view::State::apply. Generic pipe
    /// so save_view-internal side effects (e.g. the scroll-to-top
    /// snap on tab change) flow through without per-feature
    /// Effect variants.
    SaveViewTask(iced::Task<Message>),
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
                if !g.available {
                    // Greyed-out entry in the dropdown — ignore the
                    // pick. The "(no ROM)" suffix in the label tells
                    // the user why it didn't take.
                    return None;
                }
                self.local_game = Some(g.game);
                self.local_patch = None;
                self.local_patch_version = None;
                self.local_save = resolve_remembered_save(config, scanners, g.game, None, None);
                Some(Effect::SelectionChanged)
            }
            Message::LocalSaveSelected(s) => {
                self.local_save = Some(s.path);
                Some(Effect::SelectionChanged)
            }
            Message::LocalPatchSelected(p) => {
                if p.name.is_empty() {
                    self.local_patch = None;
                    self.local_patch_version = None;
                } else {
                    let v = scanners
                        .patches
                        .read()
                        .get(&p.name)
                        .and_then(|patch| patch.versions.keys().max().cloned());
                    self.local_patch = Some(p.name);
                    self.local_patch_version = v;
                }
                if let Some(g) = self.local_game {
                    self.local_save = resolve_remembered_save(
                        config,
                        scanners,
                        g,
                        self.local_patch.as_deref(),
                        self.local_patch_version.as_ref(),
                    );
                }
                Some(Effect::SelectionChanged)
            }
            Message::LocalPatchVersionSelected(v) => {
                self.local_patch_version = Some(v);
                if let Some(g) = self.local_game {
                    self.local_save = resolve_remembered_save(
                        config,
                        scanners,
                        g,
                        self.local_patch.as_deref(),
                        self.local_patch_version.as_ref(),
                    );
                }
                Some(Effect::SelectionChanged)
            }
            Message::SaveViewAction(action) => {
                let sv_task = self.save_view.apply(&action);
                match action {
                    save_view::Action::CopyTab(tab) => loaded
                        .and_then(|l| save_view::tab_as_text(&config.language, tab, l))
                        .map(Effect::CopyText),
                    save_view::Action::CopyTabImage(tab) => loaded
                        .and_then(|l| save_view::tab_as_image(tab, l))
                        .map(Effect::CopyImage),
                    save_view::Action::PlayClicked => {
                        // Clear stale error from a prior attempt; the
                        // new launch's outcome takes its place.
                        self.last_error = None;
                        Some(Effect::StartSinglePlayer)
                    }
                    _ => Some(Effect::SaveViewTask(sv_task.map(Message::SaveViewAction))),
                }
            }
            Message::LinkCodeChanged(s) => {
                self.link_code = s;
                None
            }
            Message::LinkCodeRandom => {
                self.link_code = crate::randomcode::generate(&config.language);
                // Drop the freshly-generated code straight onto the
                // clipboard so the user can paste it into chat
                // without an extra select+copy round-trip.
                Some(Effect::CopyText(self.link_code.clone()))
            }
            Message::FightPressed => {
                // Bottom bar is netplay-only — Fight CTA is gated
                // at the view layer to require a non-empty link
                // code, so reaching this handler with an empty
                // input is a stale message + safe to ignore.
                let trimmed = self.link_code.trim();
                if trimmed.is_empty() {
                    return None;
                }
                // Clear any leftover after-the-fact error from a prior
                // attempt — the new attempt's outcome will replace it.
                self.last_error = None;
                Some(Effect::NetplayConnect(match parse_direct_command(trimmed) {
                    Some(role) => crate::netplay::LinkIdent::Direct(role),
                    None => crate::netplay::LinkIdent::Matchmaking(trimmed.to_string()),
                }))
            }
            Message::DismissError => {
                self.last_error = None;
                None
            }
            Message::Noop => None,
            Message::NetplayDisconnect => Some(Effect::Netplay(crate::netplay::Message::Disconnect)),
            Message::NetplaySetMatchType(mt) => Some(Effect::Netplay(crate::netplay::Message::SetMatchType(mt))),
            Message::NetplaySetInputDelay(d) => Some(Effect::Netplay(crate::netplay::Message::SetInputDelay(d))),
            Message::NetplaySetRevealSetup(v) => Some(Effect::Netplay(crate::netplay::Message::SetRevealSetup(v))),
            Message::NetplayReady => Some(Effect::NetplayReadyWithSave),
            Message::NetplayUnready => Some(Effect::Netplay(crate::netplay::Message::Uncommit)),
            Message::Rescan => Some(Effect::Rescan),
            Message::SaveOpenFolder => self
                .local_save
                .as_ref()
                .and_then(|p| p.parent())
                .map(|p| Effect::OpenPath(p.to_path_buf())),
            Message::OpenSavesFolder(path) => Some(Effect::OpenPath(path)),
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
                // Auto-select if only one template is offered;
                // otherwise leave None so the user has to pick
                // explicitly (Confirm stays disabled until they do).
                let template = templates_for_selection(self, scanners).and_then(|tmpls| {
                    if tmpls.len() == 1 {
                        tmpls.keys().next().cloned()
                    } else {
                        None
                    }
                });
                let draft = if let Some(game) = self.local_game {
                    disambiguate_save_name(
                        &saves_dir,
                        &suggest_save_name(&config.language, game, template.as_deref()),
                    )
                } else {
                    // No game selected ⇒ can_new was false ⇒ unreachable
                    // in practice, but keep a sane fallback.
                    "new save".to_string()
                };
                self.save_action = SaveAction::NewSave {
                    auto_default: Some(draft.clone()),
                    draft,
                    template,
                };
                None
            }
            Message::SaveNewDraftChanged(s) => {
                if let SaveAction::NewSave {
                    draft, auto_default, ..
                } = &mut self.save_action
                {
                    if auto_default.as_deref() != Some(s.as_str()) {
                        *auto_default = None;
                    }
                    *draft = s;
                }
                None
            }
            Message::SaveNewTemplateSelected(name) => {
                if let SaveAction::NewSave {
                    draft,
                    template,
                    auto_default,
                } = &mut self.save_action
                {
                    *template = Some(name);
                    if auto_default.as_deref() == Some(draft.as_str()) {
                        if let Some(game) = self.local_game {
                            let new_draft = disambiguate_save_name(
                                &config.saves_path(),
                                &suggest_save_name(&config.language, game, template.as_deref()),
                            );
                            *draft = new_draft.clone();
                            *auto_default = Some(new_draft);
                        }
                    }
                }
                None
            }
            Message::SaveNewConfirm => {
                let SaveAction::NewSave {
                    draft,
                    template: Some(template),
                    ..
                } = &self.save_action
                else {
                    return None;
                };
                let name = draft.trim().to_string();
                let template = template.clone();
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
    /// Single source of truth for the local side's
    /// `protocol::Settings`. App calls this when actually sending
    /// settings on the wire; lobby_view calls it as the "You"
    /// slot fallback during Connecting/Negotiating (before
    /// `lobby.local` has been populated by the netplay loop).
    pub fn make_local_settings(
        &self,
        config: &config::Config,
        lobby: &crate::netplay::LobbyState,
        scanners: &Scanners,
    ) -> crate::net::protocol::Settings {
        use crate::net::protocol::{GameInfo, PatchInfo, Settings};
        let roms = scanners.roms.read();
        let patches = scanners.patches.read();
        Settings {
            nickname: config.nickname.clone().unwrap_or_default(),
            match_type: lobby.match_type,
            game_info: self.local_game.map(|game| {
                let (family, variant) = game.family_and_variant();
                GameInfo {
                    family_and_variant: (family.to_string(), variant),
                    patch: match (&self.local_patch, &self.local_patch_version) {
                        (Some(name), Some(version)) => Some(PatchInfo {
                            name: name.clone(),
                            version: version.clone(),
                        }),
                        _ => None,
                    },
                }
            }),
            available_games: roms
                .keys()
                .map(|g| {
                    let (family, variant) = g.family_and_variant();
                    (family.to_string(), variant)
                })
                .collect(),
            available_patches: patches
                .iter()
                .map(|(name, info)| (name.clone(), info.versions.keys().cloned().collect()))
                .collect(),
            reveal_setup: lobby.reveal_setup,
        }
    }

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
        // Lobby_view stands in for the bottom bar from the moment
        // a netplay attempt is in flight — Connecting, Negotiating,
        // Lobby — so the user sees the versus screen + match
        // settings + Cancel button immediately on submitting a
        // link code, instead of staring at the singleplayer
        // bottom bar through the handshake. The verdict line and
        // opponent slot degrade gracefully when peer info isn't
        // there yet.
        let show_lobby = matches!(
            netplay_phase,
            crate::netplay::Phase::Connecting { .. }
                | crate::netplay::Phase::Negotiating { .. }
                | crate::netplay::Phase::Lobby { .. }
                | crate::netplay::Phase::Failed { .. }
        );
        // Synthesize the local side's Settings from the play
        // tab's current selection so the "You" slot fills in
        // immediately — pre-Lobby phases haven't populated
        // `lobby.local` yet, but everything it needs is already
        // on hand locally. Same builder the netplay loop uses to
        // ship settings on the wire, so the visible info during
        // the handshake exactly matches what gets sent.
        let local_fallback = self.make_local_settings(config, netplay_lobby, scanners);
        let save_body = self.body(lang, scanners, loaded, streamer_mode, config, netplay_phase);

        // Selector strip + save-view body live inside a single
        // PANE_GAP-padded column so every pane in that area shares
        // the same inset from the window edges and gap from one
        // another. The hud_scanline + bottom strip / lobby view
        // sit OUTSIDE that padding so they remain edge-to-edge
        // bottom bars.
        let inner = column![self.selector_strip(lang, scanners, config), save_body,]
            .spacing(widgets::PANE_GAP)
            .padding(widgets::PANE_GAP)
            .height(Fill);

        let mut col = column![].width(Fill).height(Fill);
        if let Some(err) = &self.last_error {
            col = col.push(error_banner(lang, err));
        }
        col = col.push(inner).push(widgets::hud_scanline());
        // While a netplay attempt is in flight (Connecting /
        // Negotiating / Lobby) the lobby_view IS the bottom band
        // — it carries the verdict/cancel/ready chrome. Otherwise
        // the normal bottom_strip handles the link code + Fight
        // CTA.
        if show_lobby {
            col = col.push(
                container(lobby_view(
                    lang,
                    netplay_lobby,
                    netplay_phase,
                    self.local_game,
                    scanners,
                    loaded.is_some(),
                    local_fallback,
                    streamer_mode,
                ))
                .width(Fill),
            );
        } else {
            col = col.push(self.bottom_strip(lang));
        }
        col.into()
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
        netplay_phase: &'a crate::netplay::Phase,
    ) -> Element<'a, Message> {
        // No ROMs at all: explain where to put them.
        if scanners.roms.read().is_empty() {
            let roms_path = config.roms_path();
            return empty_state_card(
                t!(lang, "empty-no-roms-title"),
                vec![t!(lang, "empty-no-roms-body"), roms_path.display().to_string()],
                Some((t!(lang, "save-open-folder"), roms_path)),
            );
        }
        // Game selected but no save files for it.
        if let Some(game) = self.local_game {
            let has_saves = scanners.saves.read().get(&game).map(|v| !v.is_empty()).unwrap_or(false);
            if !has_saves && self.local_save.is_none() {
                let saves_path = config.saves_path();
                return empty_state_card(
                    t!(lang, "empty-no-saves-title"),
                    vec![t!(lang, "empty-no-saves-body"), saves_path.display().to_string()],
                    Some((t!(lang, "save-open-folder"), saves_path)),
                );
            }
        }
        self.save_view(lang, loaded, streamer_mode, netplay_phase)
    }

    fn selector_strip<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        config: &'a config::Config,
    ) -> Element<'a, Message> {
        let roms = scanners.roms.read();
        let saves = scanners.saves.read();

        // Show every Battle Network game tango knows about, not
        // Show every supported BN, not just the ROMs we have. iced
        // 0.14's pick_list can't paint individual options in a
        // different color — its menu uses one text color for the
        // whole list — so we communicate "unavailable" via two
        // signals instead: (a) sort available items to the top so
        // there's a clear visual break, and (b) suffix unavailable
        // entries with "(no ROM)" in their Display impl. The
        // `LocalGameSelected` handler also refuses picks where
        // `available` is false, so the suffix doubles as a click-
        // through guard.
        let mut all_games: Vec<rom::GameRef> = tango_gamedb::GAMES.iter().copied().collect();
        game::sort_games(lang, &mut all_games);

        let mut game_options: Vec<GameOption> = all_games
            .iter()
            .map(|g| GameOption {
                game: *g,
                display: game::display_name(lang, *g),
                available: roms.contains_key(g),
            })
            .collect();
        // Stable sort: available first, otherwise preserve the
        // locale-sorted order from `sort_games` above.
        game_options.sort_by_key(|o| !o.available);

        let selected_game = self
            .local_game
            .and_then(|g| game_options.iter().find(|opt| opt.game == g).cloned());

        let game = pick_list(game_options, selected_game, Message::LocalGameSelected)
            .placeholder(t!(lang, "play-no-game"))
            .padding(STANDARD_PADDING)
            .width(Length::FillPortion(3))
            .style(widgets::chunky_pick_list);

        let saves_path = config.saves_path();
        let mut save_options: Vec<SaveOption> = self
            .local_game
            .and_then(|g| saves.get(&g))
            .map(|saves| {
                saves
                    .iter()
                    .map(|s| SaveOption::new(&saves_path, s.path.clone()))
                    .collect()
            })
            .unwrap_or_default();
        // Folder-first recursive sort: at the first differing path
        // component, whichever side still has components after it
        // (i.e. is "inside a folder at this level") wins. Files at
        // a given level sort below any subfolders at that level.
        save_options.sort_by(|a, b| {
            let av: Vec<&std::ffi::OsStr> = a.path.strip_prefix(&saves_path).unwrap_or(&a.path).iter().collect();
            let bv: Vec<&std::ffi::OsStr> = b.path.strip_prefix(&saves_path).unwrap_or(&b.path).iter().collect();
            for i in 0..av.len().min(bv.len()) {
                if av[i] != bv[i] {
                    let a_is_dir = i + 1 < av.len();
                    let b_is_dir = i + 1 < bv.len();
                    return match (a_is_dir, b_is_dir) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => av[i].cmp(bv[i]),
                    };
                }
            }
            av.len().cmp(&bv.len())
        });

        let selected_save = self
            .local_save
            .as_ref()
            .and_then(|p| save_options.iter().find(|s| &s.path == p).cloned());

        let save = pick_list(save_options, selected_save, Message::LocalSaveSelected)
            .placeholder(t!(lang, "play-no-save"))
            .padding(STANDARD_PADDING)
            .width(Length::Fill)
            .style(widgets::chunky_pick_list);

        let no_patch_label = t!(lang, "play-no-patch");
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
        // Favorites first, alphabetical within each group.
        compatible_names.sort_by(|a, b| {
            let fa = config.favorite_patches.contains(a);
            let fb = config.favorite_patches.contains(b);
            fb.cmp(&fa).then_with(|| a.cmp(b))
        });
        let no_patch_option = PatchOption {
            name: String::new(),
            display: no_patch_label.clone(),
        };
        let patch_options: Vec<PatchOption> = std::iter::once(no_patch_option.clone())
            .chain(compatible_names.into_iter().map(|n| {
                let display = if config.favorite_patches.contains(&n) {
                    format!("\u{2605} {n}")
                } else {
                    n.clone()
                };
                PatchOption { name: n, display }
            }))
            .collect();
        let selected_patch = match self.local_patch.as_ref() {
            Some(n) => patch_options.iter().find(|o| &o.name == n).cloned(),
            None => Some(no_patch_option),
        };
        let patch = pick_list(patch_options, selected_patch, Message::LocalPatchSelected)
            .padding(STANDARD_PADDING)
            .width(Length::FillPortion(2))
            .style(widgets::chunky_pick_list);

        let version_options: Vec<semver::Version> = self
            .local_patch
            .as_ref()
            .and_then(|n| patches.get(n))
            .map(|p| {
                let game = self.local_game;
                let mut vs: Vec<semver::Version> = p
                    .versions
                    .iter()
                    .filter(|(_, v)| game.map(|g| v.supported_games.contains(&g)).unwrap_or(true))
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
        .placeholder(t!(lang, "play-version-placeholder"))
        .padding(STANDARD_PADDING)
        .width(Length::Fixed(100.0))
        .style(widgets::chunky_pick_list);

        let refresh = widgets::icon_button(Icon::RefreshCw, t!(lang, "rescan"), Message::Rescan, STANDARD_PADDING);

        let game_row = row![game, patch, version, refresh]
            .spacing(8)
            .align_y(Alignment::Center);

        let save_row: Element<'_, Message> = match &self.save_action {
            SaveAction::None => {
                let actions = self.save_action_buttons(lang, scanners);
                row![save, actions].spacing(8).align_y(Alignment::Center).into()
            }
            SaveAction::Renaming { draft } => row![
                text_input(&t!(lang, "save-name-placeholder"), draft)
                    .on_input(Message::SaveRenameDraftChanged)
                    .on_submit(Message::SaveRenameConfirm)
                    .style(widgets::chunky_text_input)
                    .padding(STANDARD_PADDING)
                    .width(Length::Fill),
                widgets::icon_button_styled(
                    Icon::Check,
                    t!(lang, "save-rename-confirm"),
                    Some(Message::SaveRenameConfirm),
                    STANDARD_PADDING,
                    widgets::primary_button,
                ),
                widgets::icon_button(
                    Icon::X,
                    t!(lang, "save-action-cancel"),
                    Message::SaveActionCancel,
                    STANDARD_PADDING,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            SaveAction::ConfirmDelete => row![
                text(t!(lang, "save-delete-prompt"))
                    .style(widgets::muted_text_style)
                    .width(Length::Fill),
                widgets::labeled_icon_button(
                    Icon::Trash,
                    t!(lang, "save-delete-confirm"),
                    Message::SaveDeleteConfirm,
                    STANDARD_PADDING,
                    widgets::danger_button,
                ),
                widgets::icon_button(
                    Icon::X,
                    t!(lang, "save-action-cancel"),
                    Message::SaveActionCancel,
                    STANDARD_PADDING,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            SaveAction::NewSave { draft, template, .. } => {
                // Real template names from disk — no synthesized
                // "(default)" entry. Each option carries the raw
                // name plus a locale-aware display label so the
                // user sees "MegaMan.EXE" / "Saito" etc instead of
                // the bare filename suffix.
                let family = self.local_game.map(|g| g.family_and_variant().0).unwrap_or_default();
                let options: Vec<SaveTemplateOption> = templates_for_selection(self, scanners)
                    .map(|t| {
                        t.keys()
                            .map(|name| SaveTemplateOption::new(lang, family, name))
                            .collect()
                    })
                    .unwrap_or_default();
                let selected = template
                    .as_ref()
                    .and_then(|t| options.iter().find(|o| &o.raw == t).cloned());
                let can_confirm = template.is_some() && !draft.trim().is_empty();
                let confirm_btn = if can_confirm {
                    widgets::labeled_icon_button(
                        Icon::Check,
                        t!(lang, "save-new-confirm"),
                        Message::SaveNewConfirm,
                        STANDARD_PADDING,
                        widgets::primary_button,
                    )
                } else {
                    iced::widget::button(
                        row![Icon::Check.widget(), text(t!(lang, "save-new-confirm"))]
                            .spacing(8)
                            .align_y(Alignment::Center),
                    )
                    .padding(STANDARD_PADDING)
                    .style(widgets::neutral)
                    .into()
                };
                row![
                    pick_list(options, selected, |o| { Message::SaveNewTemplateSelected(o.raw) })
                        .placeholder(t!(lang, "save-template-pick"))
                        .padding(STANDARD_PADDING)
                        .width(Length::Fixed(180.0))
                        .style(widgets::chunky_pick_list),
                    text_input(&t!(lang, "save-name-placeholder"), draft)
                        .on_input(Message::SaveNewDraftChanged)
                        .on_submit(Message::SaveNewConfirm)
                        .padding(STANDARD_PADDING)
                        .width(Length::Fill)
                        .style(widgets::chunky_text_input),
                    confirm_btn,
                    widgets::icon_button(
                        Icon::X,
                        t!(lang, "save-action-cancel"),
                        Message::SaveActionCancel,
                        STANDARD_PADDING,
                    ),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
        };

        container(column![game_row, save_row].spacing(6))
            .padding(widgets::PANE_PADDING)
            .width(Fill)
            .style(widgets::pane)
            .into()
    }

    fn save_action_buttons<'a>(&'a self, lang: &'a LanguageIdentifier, scanners: &'a Scanners) -> Element<'a, Message> {
        let enabled = self.local_save.is_some();
        let mk = |icon: Icon, label: String, msg: Message, on: bool| {
            widgets::icon_button_maybe(icon, label, if on { Some(msg) } else { None }, STANDARD_PADDING)
        };
        // Destructive variant for Delete — flags it red so it
        // doesn't look like just another toolbar action.
        let mk_danger = |icon: Icon, label: String, msg: Message, on: bool| {
            widgets::icon_button_styled(
                icon,
                label,
                if on { Some(msg) } else { None },
                STANDARD_PADDING,
                widgets::danger_button,
            )
        };
        // "New save" is enabled only when the active patch+version ships
        // a save template for the selected game.
        let can_new = templates_for_selection(self, scanners).is_some();
        row![
            mk(Icon::FilePlus, t!(lang, "save-new"), Message::SaveNewStart, can_new),
            mk(
                Icon::FolderOpen,
                t!(lang, "save-open-folder"),
                Message::SaveOpenFolder,
                enabled
            ),
            mk(Icon::Files, t!(lang, "save-duplicate"), Message::SaveDuplicate, enabled),
            mk(
                Icon::PencilLine,
                t!(lang, "save-rename"),
                Message::SaveRenameStart,
                enabled
            ),
            mk_danger(Icon::Trash, t!(lang, "save-delete"), Message::SaveDeleteStart, enabled),
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
        netplay_phase: &'a crate::netplay::Phase,
    ) -> Element<'a, Message> {
        let Some(loaded) = loaded else {
            return container(text(t!(lang, "play-no-selection")).size(TEXT_BODY))
                .center(Fill)
                .into();
        };
        // Play button is the singleplayer entry point now —
        // disabled whenever the lobby is on-screen (in-flight or
        // sitting on a Failed banner the user hasn't dismissed)
        // so it can't fight with that lobby for the same
        // save/emulator slot.
        let play_button = Some(matches!(netplay_phase, crate::netplay::Phase::Idle));
        save_view::view(lang, loaded, &self.save_view, streamer_mode, play_button).map(Message::SaveViewAction)
    }

    fn bottom_strip<'a>(&'a self, lang: &'a LanguageIdentifier) -> Element<'a, Message> {
        // PlayState::view only reaches here in Idle / Failed
        // phases — the lobby_view replaces the bottom band for
        // every in-flight netplay phase, so this strip is pure
        // "enter a link code and fight". Singleplayer lives at
        // the top of the save_view now.
        const BOTTOM_SIZE: f32 = 15.0;
        const BOTTOM_PAD: [f32; 2] = [10.0, 16.0];
        const BOTTOM_CTA_PAD: [f32; 2] = [10.0, 22.0];
        let link_code_empty = self.link_code.trim().is_empty();
        let fight_button: Element<'a, Message> = {
            // Same chrome as the lobby's Ready button — both are
            // "commit to a match" CTAs. ready_button_style for
            // ReadyPalette::Idle falls back to neutral when the
            // button is disabled, so the empty-link-code case
            // renders as a plain greyed-out pill without a
            // separate branch here.
            let label = row![
                Icon::Swords.widget().size(BOTTOM_SIZE),
                text(t!(lang, "play-fight")).size(BOTTOM_SIZE),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            let mut btn = iced::widget::button(label)
                .padding(BOTTOM_CTA_PAD)
                .height(Length::Fixed(crate::app::BAR_CONTROL_HEIGHT))
                .style(|theme: &iced::Theme, status| ready_button_style(theme, status, ReadyPalette::Idle));
            if !link_code_empty {
                btn = btn.on_press(Message::FightPressed);
            }
            btn.into()
        };
        // Link-code input fills all the slack between the dice
        // button on its right and the row's left edge.
        // text_input doesn't expose a `.height()` method, so we
        // wrap it in a fixed-height container to match the
        // surrounding controls.
        let link_input: Element<'a, Message> = container(
            text_input(&t!(lang, "play-link-code"), &self.link_code)
                .on_input(Message::LinkCodeChanged)
                .on_submit(Message::FightPressed)
                .size(BOTTOM_SIZE)
                .padding(BOTTOM_PAD)
                .width(Length::Fill)
                .style(widgets::chunky_text_input),
        )
        .height(Length::Fixed(crate::app::BAR_CONTROL_HEIGHT))
        .width(Length::Fill)
        .into();
        let dice_button: Element<'a, Message> = iced::widget::tooltip(
            iced::widget::button(Icon::Dice5.widget().size(BOTTOM_SIZE))
                .padding(BOTTOM_PAD)
                .height(Length::Fixed(crate::app::BAR_CONTROL_HEIGHT))
                .style(widgets::neutral)
                .on_press(Message::LinkCodeRandom),
            container(text(t!(lang, "play-link-code-random")).size(TEXT_CAPTION))
                .padding(6)
                .style(|theme: &iced::Theme| {
                    let p = theme.extended_palette();
                    iced::widget::container::Style {
                        background: Some(iced::Background::Color(p.background.strong.color)),
                        text_color: Some(p.background.strong.text),
                        border: iced::Border {
                            radius: 4.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }),
            iced::widget::tooltip::Position::Top,
        )
        .gap(4)
        .into();

        container(
            row![link_input, dice_button, fight_button]
                .spacing(10)
                .align_y(Alignment::Center)
                .padding([10, 16]),
        )
        .width(Fill)
        .style(widgets::hud_bar)
        .into()
    }
}

/// Lookup the patch save templates for the current game+patch+version
/// selection. Returns `None` if any of (game / patch / version /
/// Pick the save to land on after a game/patch/version change.
/// Prefers the per-(game, patch, version) remembered save from config
/// if it's still in the scan; otherwise falls back to the first save
/// listed for the game.
fn resolve_remembered_save(
    config: &config::Config,
    scanners: &Scanners,
    game: rom::GameRef,
    patch_name: Option<&str>,
    patch_version: Option<&semver::Version>,
) -> Option<std::path::PathBuf> {
    let saves_map = scanners.saves.read();
    let saves_for_game = saves_map.get(&game);
    let key = config::save_memory_key(game, patch_name, patch_version);
    let remembered = config
        .last_save_per_game_per_patch
        .get(&key)
        .map(|rel| config.data_relative_to_absolute(rel))
        .filter(|p| saves_for_game.map(|v| v.iter().any(|s| s.path == *p)).unwrap_or(false));
    remembered.or_else(|| saves_for_game.and_then(|v| v.first().map(|s| s.path.clone())))
}

/// Localized "<game-variant> <template-display>" (or just "<game-variant>"
/// when no template is chosen yet), with filesystem-unsafe characters
/// stripped so it can be dropped straight into the new-save text field.
/// Uses the full variant-aware display name so multi-version games like
/// BN6 Gregar/Falzar get disambiguated.
fn suggest_save_name(lang: &unic_langid::LanguageIdentifier, game: rom::GameRef, template: Option<&str>) -> String {
    let game_name = crate::game::display_name(lang, game);
    let family = game.family_and_variant().0;
    let name = match template {
        Some(raw) => {
            let display = SaveTemplateOption::new(lang, family, raw).display;
            format!("{game_name} - {display}")
        }
        None => game_name,
    };
    sanitize_filename(&name)
}

fn sanitize_filename(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            c if (c as u32) < 0x20 => ' ',
            c => c,
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Appends ` 2`, ` 3`, ... to `base` until the resulting `<name>.sav`
/// doesn't already exist in `saves_dir`. Gives up at 99 to avoid an
/// unbounded scan if the directory is somehow saturated.
fn disambiguate_save_name(saves_dir: &std::path::Path, base: &str) -> String {
    let mut draft = base.to_string();
    for n in 2..100 {
        if !saves_dir.join(format!("{draft}.sav")).exists() {
            break;
        }
        draft = format!("{base} {n}");
    }
    draft
}

/// template-for-game) are missing. The returned map is the templates
/// keyed by template name (empty string = default).
pub fn templates_for_selection_public(
    state: &PlayState,
    scanners: &Scanners,
) -> Option<indexmap::IndexMap<String, Box<dyn tango_dataview::save::Save + Send + Sync>>> {
    templates_for_selection(state, scanners)
}

fn templates_for_selection<'a>(
    state: &PlayState,
    scanners: &'a Scanners,
) -> Option<indexmap::IndexMap<String, Box<dyn tango_dataview::save::Save + Send + Sync>>> {
    let game = state.local_game?;
    // IndexMap (not BTreeMap) so templates iterate in declaration order
    // — patch-provided first, then the game's bundled order — instead
    // of alphabetically by raw key.
    let mut out = indexmap::IndexMap::new();

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
            out.entry((*name).to_string()).or_insert_with(|| save.clone_box());
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
/// `rebuild_checksum()` is required before `to_sram_dump()` — without
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
    let sram = save.to_sram_dump();
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
    phase: &'a crate::netplay::Phase,
    local_game: Option<rom::GameRef>,
    scanners: &'a Scanners,
    has_save: bool,
    local_fallback: crate::net::protocol::Settings,
    streamer_mode: bool,
) -> Element<'a, Message> {
    // Compact "you / opponent" card — 2 lines max so the lobby
    // strip can fit in ~220 px without losing the ready button.
    // `ready` paints a green dot when that side has committed.
    let side =
        |label: String, settings: Option<&crate::net::protocol::Settings>, ready: bool| -> Element<'static, Message> {
            // 14 px dot with a soft primary-tinted glow when the
            // side is committed — reads as a "ready light" on a
            // console panel rather than a flat status pip.
            // Padded so the dot lines up with the nickname row of
            // the column to its right — the inner side row is
            // top-aligned (Alignment::Start) so the dot doesn't
            // drift when the card grows from a 2-line placeholder
            // to a 3-line populated card.
            let dot_color = |ready: bool| -> Element<'static, Message> {
                container(
                    container(
                        iced::widget::Space::new()
                            .width(Length::Fixed(14.0))
                            .height(Length::Fixed(14.0)),
                    )
                    .style(move |theme: &iced::Theme| {
                        let bg = if ready {
                            theme.palette().primary
                        } else {
                            iced::Color::from_rgb8(0x66, 0x66, 0x66)
                        };
                        iced::widget::container::Style {
                            background: Some(iced::Background::Color(bg)),
                            border: iced::Border {
                                radius: 7.0.into(),
                                ..Default::default()
                            },
                            shadow: if ready {
                                iced::Shadow {
                                    color: iced::Color {
                                        a: 0.7,
                                        ..theme.palette().primary
                                    },
                                    offset: iced::Vector::new(0.0, 0.0),
                                    blur_radius: 10.0,
                                }
                            } else {
                                iced::Shadow::default()
                            },
                            ..Default::default()
                        }
                    }),
                )
                .padding(iced::Padding {
                    top: 20.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                })
                .into()
            };
            let Some(settings) = settings else {
                return container(
                    row![
                        dot_color(false),
                        column![
                            text(label).size(TEXT_CAPTION).style(widgets::muted_text_style),
                            text(t!(lang, "lobby-waiting"))
                                .size(TEXT_TITLE)
                                .style(widgets::muted_text_style),
                        ]
                        .spacing(2),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Start),
                )
                .width(Length::Fill)
                .into();
            };
            let nickname = settings.nickname.clone();
            let game_label = settings
                .game_info
                .as_ref()
                .map(|gi| {
                    let family = gi.family_and_variant.0.as_str();
                    // Dynamic key (one per gamedb family) — bypass the
                    // literal-only macro and hit the Fluent loader directly.
                    use fluent_templates::Loader;
                    crate::i18n::LOCALES
                        .try_lookup(lang, &format!("game-{family}"))
                        .unwrap_or_else(|| format!("{} v{}", gi.family_and_variant.0, gi.family_and_variant.1))
                })
                .unwrap_or_else(|| t!(lang, "lobby-no-game"));
            let patch = settings
                .game_info
                .as_ref()
                .and_then(|gi| gi.patch.as_ref())
                .map(|p| format!(" · {} v{}", p.name, p.version));
            // Game line: "<game name> · <patch> · <match-type>" packed
            // onto a single caption row so the card stays 2 lines tall.
            // Match-type is meaningless without a game (no Game::match_types
            // table to look the name up against), so omit it then.
            let mut subline = game_label;
            if let Some(p) = patch {
                subline.push_str(&p);
            }
            if let Some(gi) = settings.game_info.as_ref() {
                let mt = crate::game::match_type_name(
                    lang,
                    gi.family_and_variant.0.as_str(),
                    settings.match_type.0,
                    settings.match_type.1,
                );
                subline.push_str(&format!(" · {mt}"));
            }
            // Nickname is the marquee — title-sized, primary
            // tinted when this side is ready so the card lights
            // up visibly as commitment lands.
            let nickname_style: fn(&iced::Theme) -> iced::widget::text::Style = if ready {
                |theme: &iced::Theme| iced::widget::text::Style {
                    color: Some(theme.palette().primary),
                }
            } else {
                |_theme: &iced::Theme| iced::widget::text::Style { color: None }
            };
            container(
                row![
                    dot_color(ready),
                    column![
                        text(label).size(TEXT_CAPTION).style(widgets::muted_text_style),
                        text(nickname).size(TEXT_TITLE).style(nickname_style),
                        text(subline).size(TEXT_CAPTION),
                    ]
                    .spacing(2),
                ]
                .spacing(10)
                .align_y(Alignment::Start),
            )
            .width(Length::Fill)
            .into()
        };

    // Pre-handshake we don't have a ping yet, but we always know
    // the connection identifier — show that instead of the generic
    // "Exchanging settings…" placeholder so the user sees the
    // identifier they're matched on. Streamer privacy mode
    // suppresses the matchmaking code so a viewer of the stream
    // can't scrape it off the screen and crash the lobby; direct
    // ports/addresses are equally sensitive on a public stream, so
    // hide them too.
    let ident: Option<&crate::netplay::LinkIdent> = if streamer_mode {
        None
    } else {
        match phase {
            crate::netplay::Phase::Connecting { ident, .. }
            | crate::netplay::Phase::Negotiating { ident }
            | crate::netplay::Phase::Lobby { ident } => Some(ident),
            _ => None,
        }
    };
    // Streamer mode is the only path that reaches the "no latency,
    // no identifier" state (the identifier is always available
    // otherwise); skip the header line entirely there rather than
    // reserving a slot for it.
    let header_line: Option<Element<'a, Message>> = if let Some(d) = lobby.latency {
        Some(
            text(t!(lang, "lobby-latency", ms = d.as_millis() as i64))
                .size(TEXT_BODY)
                .style(widgets::muted_text_style)
                .into(),
        )
    } else if let Some(ident) = ident {
        use crate::netplay::{DirectRole, LinkIdent};
        let label = match ident {
            LinkIdent::Matchmaking(code) => t!(lang, "lobby-link-code", code = code.clone()),
            LinkIdent::Direct(DirectRole::Host { port }) => {
                t!(lang, "lobby-direct-host", port = port.to_string())
            }
            LinkIdent::Direct(DirectRole::Connect { addr }) => {
                t!(lang, "lobby-direct-connect", target = addr.clone())
            }
        };
        Some(text(label).size(TEXT_BODY).style(widgets::muted_text_style).into())
    } else {
        None
    };

    // Match-type pick_list — options pulled from the current
    // local game's Game::match_types() table (mode + subtype
    // counts), labeled with the per-game Fluent strings via
    // game::match_type_name. Renders an empty disabled pick_list
    // when no game is selected (Game::match_types() can't be
    // queried until we know the game) — gives the row a stable
    // shape so the surrounding layout doesn't jump once the user
    // picks a game.
    let mt_picker: Element<'a, Message> = if let Some(g) = local_game {
        let game_impl = crate::game::from_gamedb_entry(g);
        let mt_table = game_impl.map(|gi| gi.match_types()).unwrap_or(&[]);
        let mut options = Vec::new();
        for (mode, subtype_count) in mt_table.iter().enumerate() {
            for sub in 0..*subtype_count {
                options.push(MatchTypeOption {
                    mode: mode as u8,
                    subtype: sub as u8,
                    label: crate::game::match_type_name(lang, g.family_and_variant().0, mode as u8, sub as u8),
                });
            }
        }
        let selected = options
            .iter()
            .find(|o| o.mode == lobby.match_type.0 && o.subtype == lobby.match_type.1)
            .cloned();
        // In Phase::Failed the dropdown's on_change reroutes
        // to Noop so picks are inert without touching layout.
        let failed = matches!(phase, crate::netplay::Phase::Failed { .. });
        let on_change: fn((u8, u8)) -> Message = if failed {
            |_| Message::Noop
        } else {
            Message::NetplaySetMatchType
        };
        if options.is_empty() {
            text(t!(lang, "lobby-no-match-types"))
                .style(widgets::muted_text_style)
                .into()
        } else {
            pick_list(options, selected, move |o| on_change((o.mode, o.subtype)))
                .padding(STANDARD_PADDING)
                .style(crate::widgets::chunky_pick_list)
                .into()
        }
    } else {
        let empty: Vec<MatchTypeOption> = Vec::new();
        pick_list(empty, None::<MatchTypeOption>, |o: MatchTypeOption| {
            Message::NetplaySetMatchType((o.mode, o.subtype))
        })
        .padding(STANDARD_PADDING)
        .style(crate::widgets::chunky_pick_list)
        .into()
    };

    let failed = matches!(phase, crate::netplay::Phase::Failed { .. });

    // Input delay slider — legacy app caps at 10 frames. Each
    // increment is one full GBA frame (~16.7 ms one-way) of
    // smoothing for jittery connections. Reroute through Noop
    // when Failed so dragging it doesn't do anything.
    let slider_on_change: fn(u8) -> Message = if failed {
        |_| Message::Noop
    } else {
        Message::NetplaySetInputDelay
    };
    let id_slider = iced::widget::slider(2..=10u8, lobby.input_delay, slider_on_change).width(Length::Fixed(160.0));

    // "Suggest" button: legacy formula = one-way frames + 1 - 2,
    // clamped to the slider range. Disabled until the first Pong
    // gives us a latency reading — and unconditionally disabled
    // in Failed phase.
    let suggest_msg = if failed {
        None
    } else {
        lobby.latency.map(|rtt| {
            let one_way_frames = (rtt.as_nanos() * 60 / 2 / std::time::Duration::from_secs(1).as_nanos()) as i32;
            let d = (one_way_frames + 1 - 2).clamp(2, 10) as u8;
            Message::NetplaySetInputDelay(d)
        })
    };
    let id_suggest = widgets::icon_button_maybe(
        Icon::Wand,
        t!(lang, "lobby-input-delay-suggest"),
        suggest_msg,
        STANDARD_PADDING,
    );

    // Reveal-setup checkbox. Mirrors the legacy app's
    // `play-details-reveal-setup` checkbox — each side picks
    // independently; the peer can see (read-only) what we picked
    // via the remote status next to the checkbox.
    // Peer's current "reveal my setup" flag — surfaced as a
    // standalone sentence under the checkbox so the parens-stuffed
    // label doesn't have to be locale-jammed into the checkbox text.
    // Color follows the state: green when peer is sharing,
    // muted/red when not / unknown.
    let (reveal_label, reveal_style): (String, fn(&iced::Theme) -> iced::widget::text::Style) =
        if let Some(r) = lobby.remote.as_ref() {
            if r.reveal_setup {
                (t!(lang, "lobby-reveal-peer-on"), widgets::success_text_style)
            } else {
                (t!(lang, "lobby-reveal-peer-off"), widgets::danger_text_style)
            }
        } else {
            (t!(lang, "lobby-reveal-peer-unknown"), widgets::muted_text_style)
        };

    // Settings table — one stacked row per setting, each shaped
    // `[fixed-width muted label] [control fills the rest]`. The
    // identical row shape is what makes the block read as a
    // single coherent settings group; visual weight differences
    // between picker / slider / checkbox stop mattering because
    // every control hangs off the same label column.
    let label_style: fn(&iced::Theme) -> iced::widget::text::Style = widgets::muted_text_style;
    let setting_row = |label_el: Element<'a, Message>, control: Element<'a, Message>| -> Element<'a, Message> {
        row![
            container(label_el).width(Length::Fixed(140.0)),
            container(control).width(Length::Fill),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .into()
    };

    let match_row = setting_row(
        text(t!(lang, "lobby-match-type"))
            .size(TEXT_BODY)
            .style(label_style)
            .into(),
        mt_picker,
    );

    let delay_row = setting_row(
        text(t!(lang, "lobby-input-delay"))
            .size(TEXT_BODY)
            .style(label_style)
            .into(),
        row![
            id_slider,
            // Live value rendered as a fixed-width monospaced
            // numeral so the slider's position has a readable
            // numeric counterpart that doesn't jiggle layout.
            text(format!("{}", lobby.input_delay))
                .size(TEXT_BODY)
                .font(iced::Font::MONOSPACE)
                .width(Length::Fixed(18.0)),
            id_suggest,
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into(),
    );

    let reveal_toggle = if failed {
        None
    } else {
        Some(Message::NetplaySetRevealSetup as fn(bool) -> Message)
    };
    let reveal_row = setting_row(
        text(t!(lang, "lobby-reveal-mine"))
            .size(TEXT_BODY)
            .style(label_style)
            .into(),
        row![
            iced::widget::checkbox(lobby.reveal_setup)
                .on_toggle_maybe(reveal_toggle)
                .size(TEXT_HEADING)
                .style(widgets::chunky_checkbox),
            text(reveal_label).size(TEXT_CAPTION).style(reveal_style),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .into(),
    );

    // Status / verdict line. While the netplay attempt is still
    // pre-Lobby (Connecting / Negotiating), this shows the
    // connection progress so the user has something to read
    // through the handshake. Once we're in Lobby with both
    // sides' settings on hand, it switches to the compat
    // verdict and gates the Ready button. Failed = sticky
    // banner with the cause, dismissed by the Cancel button in
    // the header.
    use crate::netplay::Phase;
    let (verdict_line, compat_ok): (Element<'a, Message>, bool) = match phase {
        Phase::Failed { error } => {
            // Route the netplay error tag through Fluent so each
            // failure mode can carry its own translated copy.
            // Anything we don't have a dedicated key for falls
            // back to the generic "Connection failed: <raw>".
            let label = match error.as_str() {
                "peer-disconnected" => t!(lang, "play-status-peer-disconnected"),
                "negotiate-expected-hello" => t!(lang, "play-status-negotiate-expected-hello"),
                "negotiate-version-too-old" => t!(lang, "play-status-negotiate-version-too-old"),
                "negotiate-version-too-new" => t!(lang, "play-status-negotiate-version-too-new"),
                other if other.starts_with("negotiate-other: ") => t!(
                    lang,
                    "play-status-negotiate-failed",
                    error = other.trim_start_matches("negotiate-other: ").to_string(),
                ),
                _ => t!(lang, "play-status-failed", error = error.clone()),
            };
            (
                text(label).size(TEXT_BODY).style(widgets::danger_text_style).into(),
                false,
            )
        }
        Phase::Connecting {
            ident,
            waiting_for_opponent: false,
        } => {
            // Matchmaking codes hit the server first ("Connecting
            // to matchmaking server…"); direct `/connect` codes
            // dial straight at the peer, so the matchmaking copy
            // is wrong — use the opponent-targeted string instead.
            let label = match ident {
                crate::netplay::LinkIdent::Direct(crate::netplay::DirectRole::Connect { .. }) => {
                    t!(lang, "play-status-direct-connecting")
                }
                _ => t!(lang, "play-status-connecting"),
            };
            (
                text(label).size(TEXT_BODY).style(widgets::muted_text_style).into(),
                false,
            )
        }
        Phase::Connecting {
            waiting_for_opponent: true,
            ..
        } => (
            text(t!(lang, "play-status-waiting-opponent"))
                .size(TEXT_BODY)
                .style(widgets::muted_text_style)
                .into(),
            false,
        ),
        Phase::Negotiating { .. } => (
            text(t!(lang, "play-status-negotiating"))
                .size(TEXT_BODY)
                .style(widgets::muted_text_style)
                .into(),
            false,
        ),
        _ => match (lobby.local.as_ref(), lobby.remote.as_ref()) {
            (Some(l), Some(r)) => {
                use crate::netplay::compat::Verdict;
                let patches = scanners.patches.read();
                let verdict = crate::netplay::compat::check(l, r, &*patches);
                let label = match verdict {
                    Verdict::Compatible => t!(lang, "lobby-compat-ok"),
                    Verdict::MissingGame => t!(lang, "lobby-compat-missing-game"),
                    Verdict::MissingRomOrPatch => t!(lang, "lobby-compat-missing-rom"),
                    Verdict::DifferentVersions => t!(lang, "lobby-compat-version-mismatch"),
                    Verdict::DifferentMatchTypes => t!(lang, "lobby-compat-match-mismatch"),
                };
                let ok = matches!(verdict, Verdict::Compatible);
                let style: fn(&iced::Theme) -> iced::widget::text::Style = if ok {
                    widgets::success_text_style
                } else {
                    widgets::danger_text_style
                };
                (text(label).size(TEXT_BODY).style(style).into(), ok)
            }
            _ => (
                text(t!(lang, "lobby-handshake"))
                    .size(TEXT_BODY)
                    .style(widgets::muted_text_style)
                    .into(),
                false,
            ),
        },
    };

    // Big single toggle: Ready → Unready → Starting…, switching
    // label + icon + color on click. Same button, same position;
    // clicking it always does the obvious next thing (ready up,
    // unready, or wait for match-start). A touch chunkier than
    // the regular CTAs in the strip, but not so big that it
    // blows the lobby layout — the glow shadow does the work of
    // "look at me" instead.
    const READY_TEXT: f32 = 16.0;
    const READY_PAD: [f32; 2] = [10.0, 22.0];
    let (ready_icon, ready_label, ready_msg, ready_palette): (Icon, String, Option<Message>, ReadyPalette) =
        if lobby.match_ready {
            // Both committed — match is spinning up. Button is purely
            // a status indicator; no click target until the session
            // actually opens.
            (
                Icon::Play,
                t!(lang, "lobby-match-starting"),
                None,
                ReadyPalette::Starting,
            )
        } else if lobby.local_ready {
            // Locally committed, waiting on peer. Action = unready.
            // Gray / neutral so it doesn't masquerade as a primary CTA.
            (
                Icon::X,
                t!(lang, "lobby-unready"),
                Some(Message::NetplayUnready),
                ReadyPalette::Committed,
            )
        } else {
            // Compat OK + a save loaded → click sends Commit. Either
            // missing → button disabled (the user can see WHY: the
            // verdict text covers compat, and the side card / save
            // selector covers "no save").
            let can_ready = compat_ok && has_save;
            (
                Icon::Check,
                t!(lang, "lobby-ready"),
                if can_ready { Some(Message::NetplayReady) } else { None },
                ReadyPalette::Idle,
            )
        };
    // Failed lobby: the only action is to dismiss via Cancel.
    // Force the Ready button off regardless of how the
    // pre-failure state looked.
    let ready_msg = if matches!(phase, Phase::Failed { .. }) {
        None
    } else {
        ready_msg
    };
    let ready_button: Element<'a, Message> = {
        let label_widget = row![ready_icon.widget().size(READY_TEXT), text(ready_label).size(READY_TEXT),]
            .spacing(8)
            .align_y(Alignment::Center);
        let mut btn = iced::widget::button(label_widget)
            .padding(READY_PAD)
            .style(move |theme: &iced::Theme, status| ready_button_style(theme, status, ready_palette));
        if let Some(m) = ready_msg {
            btn = btn.on_press(m);
        }
        btn.into()
    };

    // Settings stack on the left, Ready CTA floated to the right
    // of the pane and bottom-aligned against the stack — mirrors
    // the matchmaking screen's Fight button anchored to the
    // bottom-right of the hud bar.
    let controls = row![
        column![match_row, delay_row, reveal_row].spacing(8).width(Length::Fill),
        ready_button,
    ]
    .spacing(12)
    .align_y(Alignment::End);

    // Leave-lobby (Disconnect) button. Top-right of the header —
    // out of the way of the verdict line, and visually paired
    // with the Ready CTA in the bottom-right of the lobby pane
    // (same right edge, opposite corner).
    let leave_button: Element<'a, Message> = widgets::labeled_icon_button(
        Icon::LogOut,
        t!(lang, "play-cancel"),
        Message::NetplayDisconnect,
        STANDARD_PADDING,
        widgets::danger_button,
    );

    // Header row: verdict on the left, leave button on the right.
    let mut header_text_col = column![].spacing(2);
    if let Some(hl) = header_line {
        header_text_col = header_text_col.push(hl);
    }
    header_text_col = header_text_col.push(verdict_line);
    let header_row = row![header_text_col, horizontal_space(), leave_button]
        .spacing(12)
        .align_y(Alignment::Center);

    // Sides row: you / opponent cards with a wide gap so the
    // diagonal cut + VS badge from `widgets::vs_splitter` paints
    // through the middle. The splitter canvas (which also paints
    // the red/blue half tints) is layered *under* the row.
    let sides_row = iced::widget::row![
        side(
            t!(lang, "play-you"),
            Some(lobby.local.as_ref().unwrap_or(&local_fallback)),
            lobby.local_ready,
        ),
        side(t!(lang, "play-opponent"), lobby.remote.as_ref(), lobby.remote_ready),
    ]
    .spacing(56)
    // Top-align so the YOU slot doesn't bounce upward when the
    // opponent's settings land and their card grows from a 2-line
    // placeholder to a 3-line filled card.
    .align_y(Alignment::Start);
    let matchup_pane = container(
        iced::widget::Stack::new()
            .push(container(sides_row).padding(widgets::PANE_PADDING).width(Fill))
            .push_under(widgets::vs_splitter()),
    )
    .width(Fill)
    .style(widgets::pane);
    let controls_pane = container(controls)
        .padding(widgets::PANE_PADDING)
        .width(Fill)
        .style(widgets::pane);
    let header_pane = container(header_row)
        .padding(widgets::PANE_PADDING)
        .width(Fill)
        .style(widgets::pane);
    container(
        column![header_pane, matchup_pane, controls_pane]
            .spacing(widgets::PANE_GAP)
            .padding(widgets::PANE_GAP),
    )
    .width(Fill)
    .into()
}

/// One entry in the "new save" template pick_list. Carries the
/// raw template name (whatever was scanned off disk) plus a
/// display label resolved via `game-<family>.save-<name>` so the
/// user sees "MegaMan.EXE" / "Saito" / "Brother" instead of the
/// bare filename suffix.
#[derive(Clone)]
struct SaveTemplateOption {
    raw: String,
    display: String,
}

impl SaveTemplateOption {
    fn new(lang: &unic_langid::LanguageIdentifier, family: &str, raw: &str) -> Self {
        // Empty `raw` is the unnamed default-template file that
        // patches ship as `<rom>_<rev>.sav`. Patches' .save-megaman
        // attr usually carries the right label for that.
        let key_suffix = if raw.is_empty() { "megaman" } else { raw };
        // Dynamic key (one per family × template name) — bypass the
        // literal-only macro and hit the Fluent loader directly.
        use fluent_templates::Loader;
        let display = crate::i18n::LOCALES
            .try_lookup(lang, &format!("game-{family}.save-{key_suffix}"))
            .unwrap_or_else(|| {
                if raw.is_empty() {
                    t!(lang, "save-template-default")
                } else {
                    raw.to_string()
                }
            });
        Self {
            raw: raw.to_string(),
            display,
        }
    }
}

impl PartialEq for SaveTemplateOption {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}
impl Eq for SaveTemplateOption {}
impl std::hash::Hash for SaveTemplateOption {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.raw.hash(h);
    }
}
impl std::fmt::Display for SaveTemplateOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}

/// Which ready-button state we're painting. Drives
/// [`ready_button_style`]'s color choice.
#[derive(Clone, Copy)]
enum ReadyPalette {
    /// Pre-commit; the action is "ready up". Accent (primary) so
    /// it reads as the call-to-action in the strip.
    Idle,
    /// Locally committed; the action is "unready". Neutral / gray —
    /// the commitment isn't a celebration to surface in green;
    /// what matters is the user can un-commit.
    Committed,
    /// Both committed; match is spinning up. Rendered as a passive
    /// indicator: muted background, no click target, no border.
    /// Caller sets `on_press = None` to match the disabled look.
    Starting,
}

/// Custom style for the lobby's Ready toggle. Three discrete
/// moods — each one its own visual register so a glance at the
/// button tells the whole story of "where are we in the
/// handshake".
///
/// * Idle      — primary_button on steroids: brighter gradient,
///               huge primary glow, chunky 2 px border. This is
///               the moment the user is supposed to slam the
///               button, so it has to feel hot.
/// * Committed — neutral beveled plate. We've ack'd locally and
///               are waiting on the peer; the only useful action
///               is to take it back, which is not a celebration.
/// * Starting  — flat muted badge. Both sides committed; the
///               button is now purely a status indicator with no
///               click target.
fn ready_button_style(theme: &iced::Theme, status: button::Status, palette: ReadyPalette) -> button::Style {
    let p = theme.extended_palette();
    let primary = theme.palette().primary;
    match palette {
        ReadyPalette::Starting => button::Style {
            background: Some(iced::Background::Color(p.background.weak.color)),
            text_color: widgets::muted_color(theme),
            border: iced::Border {
                radius: 10.0.into(),
                width: 1.0,
                color: p.background.strong.color,
            },
            ..Default::default()
        },
        ReadyPalette::Committed => {
            // Defer to the shared beveled neutral so the
            // un-ready toggle looks like a sibling of the other
            // chunky neutral buttons in the lobby strip.
            crate::widgets::neutral(theme, status)
        }
        ReadyPalette::Idle => {
            // Disabled state defers to the standard neutral
            // button so it reads as a plainly-greyed-out button
            // — the dim-primary-fill version this used to
            // render looked like a corrupted variant of the
            // lit-up state rather than a disabled affordance.
            if matches!(status, button::Status::Disabled) {
                return crate::widgets::neutral(theme, status);
            }
            // Inline expansion of the battle-button kernel with
            // every dial cranked: bigger glow, brighter top stop,
            // 2 px border so the button reads as a console
            // affordance rather than a CSS rectangle.
            let lighter = mix(primary, iced::Color::WHITE, 0.30);
            let darker = mix(primary, iced::Color::BLACK, 0.25);
            let (top, bottom, glow_alpha, offset_y, blur) = match status {
                button::Status::Hovered => (
                    mix(lighter, iced::Color::WHITE, 0.18),
                    mix(primary, iced::Color::WHITE, 0.05),
                    0.95,
                    8.0,
                    28.0,
                ),
                button::Status::Pressed => (darker, mix(darker, iced::Color::BLACK, 0.12), 0.35, 2.0, 14.0),
                button::Status::Disabled => unreachable!("handled above"),
                button::Status::Active => (lighter, darker, 0.75, 6.0, 22.0),
            };
            button::Style {
                background: Some(iced::Background::Gradient(iced::Gradient::Linear(
                    iced::gradient::Linear::new(0.0)
                        .add_stop(0.0, top)
                        .add_stop(1.0, bottom),
                ))),
                text_color: iced::Color::WHITE,
                border: iced::Border {
                    radius: 10.0.into(),
                    width: 2.0,
                    color: mix(primary, iced::Color::WHITE, 0.45),
                },
                shadow: iced::Shadow {
                    color: iced::Color {
                        a: glow_alpha,
                        ..primary
                    },
                    offset: iced::Vector::new(0.0, offset_y),
                    blur_radius: blur,
                },
                snap: false,
            }
        }
    }
}

fn mix(a: iced::Color, b: iced::Color, t: f32) -> iced::Color {
    iced::Color {
        r: a.r * (1.0 - t) + b.r * t,
        g: a.g * (1.0 - t) + b.g * t,
        b: a.b * (1.0 - t) + b.b * t,
        a: 1.0,
    }
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

/// Full-width inline banner for after-the-fact action failures
/// (singleplayer launch, PvP session build). Softer styling than a
/// hard-bordered chrome: a danger-tinted wash, rounded corners, an
/// AlertTriangle glyph, danger-colored body text, and a quiet × the
/// user can click to dismiss. Auto-clears on the next Fight or Play
/// retry too, so the user isn't forced into the × path.
fn error_banner<'a>(lang: &'a LanguageIdentifier, err: &'a str) -> Element<'a, Message> {
    container(
        row![
            Icon::AlertTriangle.widget(),
            text(err.to_string())
                .size(TEXT_BODY)
                .style(widgets::danger_text_style),
            iced::widget::space::horizontal(),
            widgets::icon_button(
                Icon::X,
                t!(lang, "save-action-cancel"),
                Message::DismissError,
                [4.0, 8.0],
            ),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding([8, 16])
    .style(|theme: &iced::Theme| {
        let p = theme.extended_palette();
        // Soft danger-tinted wash — readable against both light and
        // dark themes without the hard border that made the old
        // banner feel like an OS-level dialog.
        let alpha = if p.is_dark { 0.18 } else { 0.10 };
        iced::widget::container::Style {
            background: Some(iced::Background::Color(iced::Color {
                a: alpha,
                ..p.danger.base.color
            })),
            text_color: Some(theme.palette().text),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .into()
}

/// Centered card used for the no-roms / no-saves hints. Title is
/// rendered larger, body lines stack underneath in muted text.
/// When `folder` is provided, appends an "Open Folder" button —
/// usually the same path as the body's last line, so the user
/// can click straight through instead of copy-pasting it into
/// their file manager.
fn empty_state_card(
    title: String,
    body_lines: Vec<String>,
    open_folder: Option<(String, std::path::PathBuf)>,
) -> Element<'static, Message> {
    let mut col = column![
        // Lucide "info" glyph sized up so the card has a clear
        // visual anchor — without it the empty state was just a
        // floating title + paragraph, which read as a flash of
        // text rather than a deliberate placeholder.
        Icon::Info.widget().size(28.0),
        text(title).size(TEXT_TITLE),
    ]
    .spacing(10)
    .align_x(Alignment::Center);
    for line in body_lines {
        col = col.push(text(line).size(TEXT_CAPTION).style(widgets::muted_text_style));
    }
    if let Some((label, path)) = open_folder {
        col = col.push(Space::new().height(4)).push(widgets::labeled_icon_button(
            Icon::Folder,
            label,
            Message::OpenSavesFolder(path),
            STANDARD_PADDING,
            widgets::neutral,
        ));
    }
    container(container(col.padding(28).max_width(520)).style(widgets::panel))
        .padding(24)
        .center(Fill)
        .into()
}

// ---------- File-level save helpers ----------

/// Copy `src` to a sibling file with " (copy)" inserted before the
/// extension (with " (copy 2)", " (copy 3)", ... on collisions).
pub fn duplicate_save(src: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let parent = src.parent().ok_or_else(|| anyhow::anyhow!("save has no parent dir"))?;
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
    let parent = src.parent().ok_or_else(|| anyhow::anyhow!("save has no parent dir"))?;
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

/// Recognise the direct-TCP link-code commands the user can type
/// in place of a matchmaking code:
///
/// - `/host`            — listen on [`crate::net::DEFAULT_LOCAL_PORT`]
/// - `/host <port>`     — listen on the given port
/// - `/connect <addr>`  — dial `<addr>`, appending the default
///                        port if the user didn't specify one
///
/// Returns `Ok(Some(role))` for a recognised direct command,
/// `Ok(None)` for an ordinary matchmaking link code, and `Err`
/// when the user typed something that started with `/` but
/// didn't parse — so the play-tab handler can surface the error
/// inline instead of silently routing to matchmaking with a
/// nonsense link code.
fn parse_direct_command(input: &str) -> Option<crate::netplay::DirectRole> {
    // The leading slash is the disambiguator — without it, any
    // input is a matchmaking link code (which can legitimately
    // contain letters, digits, and the random-code separators).
    if !input.starts_with('/') {
        return None;
    }
    let mut parts = input.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    let arg = parts.next().map(str::trim).unwrap_or("");
    match cmd {
        "/host" => {
            let port = if arg.is_empty() {
                crate::net::DEFAULT_LOCAL_PORT
            } else {
                arg.parse::<u16>().ok()?
            };
            Some(crate::netplay::DirectRole::Host { port })
        }
        "/connect" => {
            if arg.is_empty() {
                return None;
            }
            // Heuristic: if the user gave no colon (bare IP) or
            // their input ends with the IPv6 closing bracket
            // without a trailing colon, append the default port.
            // We deliberately don't try to validate the address
            // itself — TcpStream::connect's error surfaces well.
            let addr = if arg.contains(':') && !arg.ends_with(']') {
                arg.to_string()
            } else {
                format!("{arg}:{}", crate::net::DEFAULT_LOCAL_PORT)
            };
            Some(crate::netplay::DirectRole::Connect { addr })
        }
        _ => None,
    }
}

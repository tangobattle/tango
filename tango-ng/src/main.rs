mod config;
mod game;
mod i18n;
mod navicust;
mod patch;
mod replay_session;
mod replays;
mod singleplayer_session;
mod rom;
mod rom_overrides;
mod save;
mod save_view;
mod scanner;
mod selection;
mod tabs;

use i18n::{t, FALLBACK_LANG};
use iced::widget::{button, column, container, horizontal_rule, horizontal_space, row, text};
use iced::{Alignment, Element, Fill, Theme};
use tabs::patches::PatchesState;
use tabs::play::{create_new_save, duplicate_save, rename_save, PlayState, SaveAction};
use tabs::replays::ReplaysState;
use tabs::settings::{settings_panel, SettingsTab};
use tabs::welcome::welcome_view;
use unic_langid::LanguageIdentifier;

pub const SUPPORTED_LANGS: &[LanguageIdentifier] = &[
    unic_langid::langid!("en-US"),
    unic_langid::langid!("ja-JP"),
];

// Button sizing constants — three tiers that everything else maps onto.
// `NAV` for the top-level nav strip; `PRIMARY` for the single big
// call-to-action (Play); `STANDARD` for everything else.
pub const NAV_TEXT_SIZE: u16 = 14;
pub const NAV_PADDING: [u16; 2] = [8, 16];
pub const PRIMARY_TEXT_SIZE: u16 = 14;
pub const PRIMARY_PADDING: [u16; 2] = [10, 24];
pub const STANDARD_TEXT_SIZE: u16 = 13;
pub const STANDARD_PADDING: [u16; 2] = [6, 14];

// Bundled fonts. We reuse the main app's font files (a few MB total)
// so JP / SC / TC scripts render instead of tofuing out, and so the
// monospace chip-code badge matches the rest of the UI. cosmic-text
// automatically falls back to whichever registered font has the
// requested glyph when the default doesn't.
const FONT_NOTO_SANS: &[u8] = include_bytes!("../../tango/fonts/NotoSans-Regular.ttf");
const FONT_NOTO_SANS_JP: &[u8] = include_bytes!("../../tango/fonts/NotoSansJP-Regular.otf");
const FONT_NOTO_SANS_SC: &[u8] = include_bytes!("../../tango/fonts/NotoSansSC-Regular.otf");
const FONT_NOTO_SANS_TC: &[u8] = include_bytes!("../../tango/fonts/NotoSansTC-Regular.otf");
const FONT_NOTO_SANS_MONO: &[u8] = include_bytes!("../../tango/fonts/NotoSansMono-Regular.ttf");
const FONT_NOTO_EMOJI: &[u8] = include_bytes!("../../tango/fonts/NotoEmoji-Regular.ttf");

pub fn main() -> iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    iced::application(App::title, App::update, App::view)
        .theme(App::theme)
        .subscription(App::subscription)
        .window_size((1000.0, 640.0))
        .font(FONT_NOTO_SANS)
        .font(FONT_NOTO_SANS_JP)
        .font(FONT_NOTO_SANS_SC)
        .font(FONT_NOTO_SANS_TC)
        .font(FONT_NOTO_SANS_MONO)
        .font(FONT_NOTO_EMOJI)
        // cosmic-text in iced 0.13 doesn't reliably auto-fall-back from
        // a Latin-only family to a CJK one for missing glyphs, so we
        // default to Noto Sans JP whose Latin coverage is designed to
        // integrate with CJK. Hans/Hant get covered automatically by
        // the registered fallbacks; Latin reads fine inline.
        .default_font(iced::Font::with_name("Noto Sans JP"))
        .run_with(App::new)
}

#[derive(Clone)]
pub struct Scanners {
    pub roms: rom::Scanner,
    pub saves: save::Scanner,
    pub patches: patch::Scanner,
    pub replays: replays::Scanner,
}

impl Scanners {
    fn new() -> Self {
        Self {
            roms: rom::Scanner::new(),
            saves: save::Scanner::new(),
            patches: patch::Scanner::new(),
            replays: replays::Scanner::new(),
        }
    }

    fn rescan(&self, config: &config::Config) {
        let roms_path = config.roms_path();
        let saves_path = config.saves_path();
        let patches_path = config.patches_path();
        let replays_path = config.replays_path();
        self.roms.rescan(|| Some(rom::scan_roms(&roms_path)));
        self.saves.rescan(|| Some(save::scan_saves(&saves_path)));
        self.patches.rescan(|| patch::scan(&patches_path).ok());
        self.replays.rescan(|| Some(replays::scan_replays(&replays_path)));
    }
}

struct App {
    config: config::Config,
    tab: Tab,
    settings_tab: SettingsTab,
    /// Draft nickname for the welcome screen, before we commit it to
    /// config.nickname.
    welcome_nickname: String,
    scanners: Scanners,

    /// Owned game+save+assets for the current selection. Rebuilt only
    /// when game or save changes; per-frame view() borrows it.
    loaded: Option<selection::Loaded>,

    play: PlayState,
    replays: ReplaysState,
    patches: PatchesState,

    /// Active emulator session — at most one of replay playback or
    /// single-player play. While `Some`, the main body is replaced by
    /// the session view and a 60Hz subscription keeps `session_frame`
    /// in sync with the mgba framebuffer.
    session: Option<ActiveSession>,
    session_frame: Option<iced::widget::image::Handle>,
    /// Counter incremented each tick we consume a frame, used to drive
    /// fresh `image::Handle` ids — without distinct ids iced caches the
    /// texture and the picture stops updating.
    session_frame_counter: u64,
}

enum ActiveSession {
    Replay(replay_session::ReplaySession),
    SinglePlayer(singleplayer_session::SinglePlayerSession),
}

impl ActiveSession {
    fn snapshot_vbuf(&self) -> Vec<u8> {
        match self {
            Self::Replay(s) => s.snapshot_vbuf(),
            Self::SinglePlayer(s) => s.snapshot_vbuf(),
        }
    }
    fn request_close(&self) {
        match self {
            Self::Replay(s) => s.request_close(),
            Self::SinglePlayer(s) => s.request_close(),
        }
    }
    /// Progress text shown in the session view header. `(current, total)`
    /// for replays, `None` for single-player (no fixed length).
    fn progress(&self) -> Option<(u32, u32)> {
        match self {
            Self::Replay(s) => Some((s.current_tick(), s.total_ticks())),
            Self::SinglePlayer(_) => None,
        }
    }
}

impl App {
    fn new() -> (Self, iced::Task<Message>) {
        let config = config::Config::load_or_create();
        let _ = FALLBACK_LANG; // re-exported for use in config; suppress unused warning here

        let scanners = Scanners::new();
        scanners.rescan(&config);
        log::info!(
            "initial scan: {} rom(s), {} save game(s), {} patch(es), {} replay(s)",
            scanners.roms.read().len(),
            scanners.saves.read().values().map(|v| v.len()).sum::<usize>(),
            scanners.patches.read().len(),
            scanners.replays.read().len(),
        );

        // Restore the last play selection from config, but only the bits
        // that still resolve against the current scanners.
        let mut play = PlayState::default();
        if let Some((family, variant)) = config.last_game.as_ref() {
            if let Some(game) = tango_gamedb::find_by_family_and_variant(family, *variant) {
                if scanners.roms.read().contains_key(&game) {
                    play.local_game = Some(game);
                    if let Some(p) = config.last_save.as_ref() {
                        if scanners
                            .saves
                            .read()
                            .get(&game)
                            .map(|v| v.iter().any(|s| s.path == *p))
                            .unwrap_or(false)
                        {
                            play.local_save = Some(p.clone());
                        }
                    }
                    if let Some(n) = config.last_patch.as_ref() {
                        if let Some(p) = scanners.patches.read().get(n) {
                            let v = config.last_patch_version.as_ref().and_then(|v| {
                                if p.versions.contains_key(v) && p.versions.get(v).map(|vm| vm.supported_games.contains(&game)).unwrap_or(false) {
                                    Some(v.clone())
                                } else {
                                    None
                                }
                            });
                            if v.is_some() {
                                play.local_patch = Some(n.clone());
                                play.local_patch_version = v;
                            }
                        }
                    }
                }
            }
        }
        let welcome_nickname = config.nickname.clone().unwrap_or_default();

        let mut app = Self {
            config,
            tab: Tab::default(),
            settings_tab: SettingsTab::General,
            welcome_nickname,
            scanners,
            loaded: None,
            play,
            replays: ReplaysState::default(),
            patches: PatchesState::default(),
            session: None,
            session_frame: None,
            session_frame_counter: 0,
        };
        app.refresh_loaded();
        (app, iced::Task::none())
    }

    /// Persist `self.config` to disk. Failures are logged but otherwise
    /// swallowed so a transient write error doesn't crash the UI.
    fn persist_config(&self) {
        if let Err(e) = self.config.save() {
            log::error!("failed to save config: {e}");
        }
    }

    /// Record the current selection back to config.last_*; called after
    /// any selection change so the next launch restores it.
    fn persist_selection(&mut self) {
        self.config.last_game = self
            .play
            .local_game
            .map(|g| (g.family_and_variant().0.to_string(), g.family_and_variant().1));
        self.config.last_save = self.play.local_save.clone();
        self.config.last_patch = self.play.local_patch.clone();
        self.config.last_patch_version = self.play.local_patch_version.clone();
        self.persist_config();
    }

    /// Snapshot of the inputs that determine `loaded`, used to skip
    /// rebuilds when nothing relevant changed.
    fn loaded_key(
        &self,
    ) -> Option<(rom::GameRef, std::path::PathBuf, Option<(String, semver::Version)>)> {
        let game = self.play.local_game?;
        let save_path = self.play.local_save.clone()?;
        let patch = match (&self.play.local_patch, &self.play.local_patch_version) {
            (Some(n), Some(v)) => Some((n.clone(), v.clone())),
            _ => None,
        };
        Some((game, save_path, patch))
    }

    /// Recompute `self.loaded` from `play.local_game` + `play.local_save`
    /// + `play.local_patch[+version]`. Cheap when nothing's changed;
    /// expensive when ROM/assets need a fresh parse (BPS + asset
    /// parsing + icon decode), which is why we don't call it from view().
    fn refresh_loaded(&mut self) {
        let Some((game, save_path, patch)) = self.loaded_key() else {
            self.loaded = None;
            return;
        };

        // Reuse existing if all inputs still match.
        if let Some(l) = &self.loaded {
            let cur_patch = l
                .patch
                .as_ref()
                .map(|p| (p.name.clone(), p.version.clone()));
            if l.game == game && l.save_path == save_path && cur_patch == patch {
                return;
            }
        }

        let roms = self.scanners.roms.read();
        let saves = self.scanners.saves.read();
        let patches = self.scanners.patches.read();
        let Some(rom) = roms.get(&game).cloned() else {
            self.loaded = None;
            return;
        };
        let Some(scanned) = saves
            .get(&game)
            .and_then(|v| v.iter().find(|s| s.path == save_path))
        else {
            self.loaded = None;
            return;
        };
        let save = scanned.save.clone_box();
        let patch_meta = patch.and_then(|(name, version)| {
            patches
                .get(&name)
                .and_then(|p| p.versions.get(&version).map(|v| (name.clone(), version, v.clone())))
        });
        drop(patches);
        drop(saves);
        drop(roms);

        log::info!(
            "loading selection: {:?} {} {}",
            game.family_and_variant(),
            save_path.display(),
            patch_meta
                .as_ref()
                .map(|(n, v, _)| format!("[{n} v{v}]"))
                .unwrap_or_default(),
        );
        let patches_path = self.config.patches_path();
        self.loaded = Some(selection::Loaded::build(
            game,
            rom,
            save_path,
            save,
            &patches_path,
            patch_meta,
        ));
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    #[default]
    Play,
    Replays,
    Patches,
    Settings,
}

/// Top-level Message. Tab-specific messages live in each tab module
/// and are wrapped here; the dispatch in `App::update` routes them to
/// per-tab `update_*` methods below.
#[derive(Debug, Clone)]
enum Message {
    TabSelected(Tab),
    Play(tabs::play::Message),
    Patches(tabs::patches::Message),
    Replays(tabs::replays::Message),
    Settings(tabs::settings::Message),
    Welcome(tabs::welcome::Message),
    SessionTick,
    SessionClose,
    /// Key mapped to an mgba joypad bit went down.
    SessionKeyDown(u32),
    /// Key mapped to an mgba joypad bit went up.
    SessionKeyUp(u32),
}

impl App {
    fn title(&self) -> String {
        let base = t(&self.config.language, "window-title");
        // Append "<game name> (<save filename>)" when a selection is
        // active so multiple Tango windows stay distinguishable.
        let Some(game) = self.play.local_game else {
            return base;
        };
        let game_name = game::display_name(&self.config.language, game);
        if let Some(save_path) = self.play.local_save.as_ref() {
            let save_name = save_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            format!("{base} — {game_name} ({save_name})")
        } else {
            format!("{base} — {game_name}")
        }
    }

    fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::TabSelected(t) => {
                self.tab = t;
                iced::Task::none()
            }
            Message::Play(m) => self.update_play(m).map(Message::Play),
            Message::Patches(m) => self.update_patches(m).map(Message::Patches),
            Message::Replays(m) => self.update_replays(m).map(Message::Replays),
            Message::Settings(m) => self.update_settings(m).map(Message::Settings),
            Message::Welcome(m) => self.update_welcome(m).map(Message::Welcome),
            Message::SessionTick => {
                if let Some(session) = self.session.as_ref() {
                    let pixels = session.snapshot_vbuf();
                    self.session_frame = Some(iced::widget::image::Handle::from_rgba(
                        replay_session::SCREEN_WIDTH,
                        replay_session::SCREEN_HEIGHT,
                        pixels,
                    ));
                    self.session_frame_counter = self.session_frame_counter.wrapping_add(1);
                }
                iced::Task::none()
            }
            Message::SessionClose => {
                if let Some(s) = self.session.as_ref() {
                    s.request_close();
                }
                self.session = None;
                self.session_frame = None;
                iced::Task::none()
            }
            Message::SessionKeyDown(bit) => {
                if let Some(ActiveSession::SinglePlayer(s)) = self.session.as_ref() {
                    s.set_joyflag(bit, true);
                }
                iced::Task::none()
            }
            Message::SessionKeyUp(bit) => {
                if let Some(ActiveSession::SinglePlayer(s)) = self.session.as_ref() {
                    s.set_joyflag(bit, false);
                }
                iced::Task::none()
            }
        }
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        let mut subs: Vec<iced::Subscription<Message>> = Vec::new();
        if self.session.is_some() {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(16))
                    .map(|_| Message::SessionTick),
            );
        }
        if matches!(self.session, Some(ActiveSession::SinglePlayer(_))) {
            subs.push(iced::event::listen_with(map_keyboard_event));
        }
        iced::Subscription::batch(subs)
    }

    fn update_play(&mut self, msg: tabs::play::Message) -> iced::Task<tabs::play::Message> {
        use tabs::play::Message as M;
        match msg {
            M::LocalGameSelected(g) => {
                self.play.local_game = Some(g.game);
                self.play.local_save = self
                    .scanners
                    .saves
                    .read()
                    .get(&g.game)
                    .and_then(|v| v.first().map(|s| s.path.clone()));
                self.play.local_patch = None;
                self.play.local_patch_version = None;
                self.refresh_loaded();
                self.persist_selection();
            }
            M::LocalSaveSelected(s) => {
                self.play.local_save = Some(s.path);
                self.refresh_loaded();
                self.persist_selection();
            }
            M::LocalPatchSelected(p) => {
                if p == t(&self.config.language, "play-no-patch") {
                    self.play.local_patch = None;
                    self.play.local_patch_version = None;
                } else {
                    let v = self
                        .scanners
                        .patches
                        .read()
                        .get(&p)
                        .and_then(|patch| patch.versions.keys().max().cloned());
                    self.play.local_patch = Some(p);
                    self.play.local_patch_version = v;
                }
                self.refresh_loaded();
                self.persist_selection();
            }
            M::LocalPatchVersionSelected(v) => {
                self.play.local_patch_version = Some(v);
                self.refresh_loaded();
                self.persist_selection();
            }
            M::SaveTabSelected(t) => self.play.save_tab = Some(t),
            M::ToggleFolderGrouped(g) => self.play.folder_grouped = g,
            M::LinkCodeChanged(s) => self.play.link_code = s,
            M::PlayPressed => {
                // Single-player path only for now — netplay (when
                // link_code is non-empty) isn't wired up yet.
                if self.play.link_code.trim().is_empty() {
                    match self.spawn_singleplayer() {
                        Ok(session) => {
                            self.session = Some(ActiveSession::SinglePlayer(session));
                            self.session_frame = None;
                            self.play.playing = true;
                        }
                        Err(e) => log::warn!("singleplayer start failed: {e}"),
                    }
                } else {
                    log::warn!("netplay sessions not yet implemented");
                }
            }
            M::Rescan => {
                self.scanners.rescan(&self.config);
                self.refresh_loaded();
            }
            M::CopyTabAsText(tab) => {
                if let Some(loaded) = self.loaded.as_ref() {
                    if let Some(text) = save_view::tab_as_text(&self.config.language, tab, loaded) {
                        return iced::clipboard::write(text);
                    }
                }
            }
            M::SaveOpenFolder => {
                if let Some(p) = self.play.local_save.as_ref().and_then(|p| p.parent()) {
                    if let Err(e) = open::that(p) {
                        log::error!("open save folder: {e}");
                    }
                }
            }
            M::SaveDuplicate => {
                if let Some(src) = self.play.local_save.clone() {
                    match duplicate_save(&src) {
                        Ok(dst) => {
                            log::info!("duplicated save: {}", dst.display());
                            self.scanners.rescan(&self.config);
                            self.play.local_save = Some(dst);
                            self.refresh_loaded();
                            self.persist_selection();
                        }
                        Err(e) => log::error!("duplicate save: {e}"),
                    }
                }
            }
            M::SaveRenameStart => {
                let draft = self
                    .play
                    .local_save
                    .as_ref()
                    .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                self.play.save_action = SaveAction::Renaming { draft };
            }
            M::SaveRenameDraftChanged(s) => {
                if let SaveAction::Renaming { draft } = &mut self.play.save_action {
                    *draft = s;
                }
            }
            M::SaveRenameConfirm => {
                if let (Some(src), SaveAction::Renaming { draft }) =
                    (self.play.local_save.clone(), self.play.save_action.clone())
                {
                    match rename_save(&src, draft.trim()) {
                        Ok(dst) => {
                            log::info!("renamed save: {} → {}", src.display(), dst.display());
                            self.scanners.rescan(&self.config);
                            self.play.local_save = Some(dst);
                            self.refresh_loaded();
                            self.persist_selection();
                        }
                        Err(e) => log::error!("rename save: {e}"),
                    }
                }
                self.play.save_action = SaveAction::None;
            }
            M::SaveDeleteStart => {
                self.play.save_action = SaveAction::ConfirmDelete;
            }
            M::SaveDeleteConfirm => {
                if let Some(src) = self.play.local_save.clone() {
                    if let Err(e) = std::fs::remove_file(&src) {
                        log::error!("delete save: {e}");
                    } else {
                        log::info!("deleted save: {}", src.display());
                    }
                    self.scanners.rescan(&self.config);
                    self.play.local_save = self
                        .play
                        .local_game
                        .and_then(|g| {
                            self.scanners
                                .saves
                                .read()
                                .get(&g)
                                .and_then(|v| v.first().map(|s| s.path.clone()))
                        });
                    self.refresh_loaded();
                    self.persist_selection();
                }
                self.play.save_action = SaveAction::None;
            }
            M::SaveActionCancel => {
                self.play.save_action = SaveAction::None;
            }
            M::NavicustHover(idx) => {
                self.play.hovered_ncp_idx = idx;
            }
            M::SaveNewStart => {
                let saves_dir = self.config.saves_path();
                let mut draft = "new save".to_string();
                for n in 2..100 {
                    if !saves_dir.join(format!("{draft}.sav")).exists() {
                        break;
                    }
                    draft = format!("new save {n}");
                }
                self.play.save_action = SaveAction::NewSave {
                    draft,
                    template: String::new(),
                };
            }
            M::SaveNewDraftChanged(s) => {
                if let SaveAction::NewSave { draft, .. } = &mut self.play.save_action {
                    *draft = s;
                }
            }
            M::SaveNewTemplateSelected(name) => {
                if let SaveAction::NewSave { template, .. } = &mut self.play.save_action {
                    *template = name;
                }
            }
            M::SaveNewConfirm => {
                if let SaveAction::NewSave { draft, template } = self.play.save_action.clone() {
                    if let Some(game) = self.play.local_game {
                        if let Some(templates) =
                            tabs::play::templates_for_selection_public(&self.play, &self.scanners)
                        {
                            // Use the chosen template name; fall back to default
                            // ("") and then to whatever's first.
                            let chosen = templates
                                .get(template.as_str())
                                .or_else(|| templates.get(""))
                                .or_else(|| templates.values().next())
                                .map(|s| s.clone_box());
                            if let Some(template) = chosen {
                                match create_new_save(
                                    &self.config.saves_path(),
                                    draft.trim(),
                                    template.as_ref(),
                                ) {
                                    Ok(dst) => {
                                        log::info!(
                                            "created new save for {:?}: {}",
                                            game.family_and_variant(),
                                            dst.display()
                                        );
                                        self.scanners.rescan(&self.config);
                                        self.play.local_save = Some(dst);
                                        self.refresh_loaded();
                                        self.persist_selection();
                                    }
                                    Err(e) => log::error!("create save: {e}"),
                                }
                            }
                        }
                    }
                }
                self.play.save_action = SaveAction::None;
            }
        }
        iced::Task::none()
    }

    fn update_patches(&mut self, msg: tabs::patches::Message) -> iced::Task<tabs::patches::Message> {
        use tabs::patches::Message as M;
        match msg {
            M::Selected(p) => {
                let v = self
                    .scanners
                    .patches
                    .read()
                    .get(&p)
                    .and_then(|patch| patch.versions.keys().max().cloned());
                self.patches.selected = Some(p);
                self.patches.version = v;
                self.patches.refresh_readme(&self.scanners);
            }
            M::VersionSelected(v) => {
                self.patches.version = Some(v);
                self.patches.refresh_readme(&self.scanners);
            }
            M::OpenFolder(p) => {
                if let Err(e) = open::that(&p) {
                    log::error!("open folder {}: {e}", p.display());
                }
            }
            M::ReadmeLinkClicked(url) => {
                if let Err(e) = open::that(url.as_str()) {
                    log::error!("open url {url}: {e}");
                }
            }
            M::Rescan => {
                self.scanners.rescan(&self.config);
                self.refresh_loaded();
            }
            M::Update => {
                if !self.patches.updating {
                    self.patches.updating = true;
                    self.patches.last_update_error = None;
                    let url = self.config.patch_repo.clone();
                    let root = self.config.data_path.join("patches");
                    return iced::Task::perform(
                        async move { patch::update(url, root).await.map_err(|e| e.to_string()) },
                        M::UpdateFinished,
                    );
                }
            }
            M::UpdateFinished(res) => {
                self.patches.updating = false;
                match res {
                    Ok(()) => {
                        self.patches.last_update_error = None;
                        self.scanners.rescan(&self.config);
                        self.refresh_loaded();
                    }
                    Err(e) => {
                        log::warn!("patch update failed: {e}");
                        self.patches.last_update_error = Some(e);
                    }
                }
            }
        }
        iced::Task::none()
    }

    fn update_replays(&mut self, msg: tabs::replays::Message) -> iced::Task<tabs::replays::Message> {
        use tabs::replays::Message as M;
        match msg {
            M::FolderFilterSelected(f) => {
                self.replays.folder_filter = f.path;
                self.replays.selected = None;
            }
            M::Selected(p) => self.replays.selected = Some(p),
            M::OpenFolder(p) => {
                if let Err(e) = open::that(&p) {
                    log::error!("open folder {}: {e}", p.display());
                }
            }
            M::Watch(p) => match self.build_playback(&p) {
                Ok(session) => {
                    self.session = Some(ActiveSession::Replay(session));
                    self.session_frame = None;
                }
                Err(e) => log::warn!("failed to play replay {}: {e}", p.display()),
            },
            M::Rescan => {
                self.scanners.rescan(&self.config);
                self.refresh_loaded();
            }
        }
        iced::Task::none()
    }

    fn update_settings(&mut self, msg: tabs::settings::Message) -> iced::Task<tabs::settings::Message> {
        use tabs::settings::Message as M;
        match msg {
            M::TabSelected(t) => self.settings_tab = t,
            M::LanguageSelected(l) => {
                self.config.language = l;
                self.persist_config();
            }
            M::NicknameChanged(s) => {
                self.config.nickname = if s.is_empty() { None } else { Some(s) };
                self.persist_config();
            }
            M::ToggleStreamerMode(s) => {
                self.config.streamer_mode = s;
                self.persist_config();
            }
            M::MatchmakingEndpointChanged(s) => {
                self.config.matchmaking_endpoint = s;
                self.persist_config();
            }
            M::PatchRepoChanged(s) => {
                self.config.patch_repo = s;
                self.persist_config();
            }
            M::ThemeChanged(t) => {
                self.config.theme = t;
                self.persist_config();
            }
        }
        iced::Task::none()
    }

    fn update_welcome(&mut self, msg: tabs::welcome::Message) -> iced::Task<tabs::welcome::Message> {
        use tabs::welcome::Message as M;
        match msg {
            M::NicknameChanged(s) => self.welcome_nickname = s,
            M::Continue => {
                let trimmed = self.welcome_nickname.trim().to_string();
                if !trimmed.is_empty() {
                    self.config.nickname = Some(trimmed);
                    self.persist_config();
                }
            }
        }
        iced::Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let lang = &self.config.language;

        // First-run gate: no main UI until the user picks a nickname.
        if self.config.nickname.is_none() {
            return welcome_view(lang, &self.welcome_nickname).map(Message::Welcome);
        }

        if self.session.is_some() {
            return self.session_view(lang);
        }

        let body: Element<'_, Message> = match self.tab {
            Tab::Play => self
                .play
                .view(
                    lang,
                    &self.scanners,
                    self.loaded.as_ref(),
                    self.config.streamer_mode,
                    &self.config,
                )
                .map(Message::Play),
            Tab::Replays => self
                .replays
                .view(lang, &self.scanners, &self.config)
                .map(Message::Replays),
            Tab::Patches => self.patches.view(lang, &self.scanners).map(Message::Patches),
            Tab::Settings => settings_panel(lang, &self.config, self.settings_tab).map(Message::Settings),
        };

        column![top_bar(lang, self.tab), horizontal_rule(1), body]
            .spacing(0)
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn session_view(&self, lang: &LanguageIdentifier) -> Element<'_, Message> {
        use iced::widget::{image, Space};
        let session = self.session.as_ref().expect("session_view: no session");
        let frame: Element<'_, Message> = if let Some(handle) = self.session_frame.as_ref() {
            image(handle.clone())
                .width(Fill)
                .height(Fill)
                .filter_method(image::FilterMethod::Nearest)
                .content_fit(iced::ContentFit::Contain)
                .into()
        } else {
            Space::new(Fill, Fill).into()
        };

        let title_key = match session {
            ActiveSession::Replay(_) => "replays-watch",
            ActiveSession::SinglePlayer(_) => "play-play",
        };
        let mut header_row = row![text(t(lang, title_key)).size(14), horizontal_space(),]
            .spacing(8)
            .align_y(Alignment::Center);
        if let Some((cur, total)) = session.progress() {
            header_row = header_row.push(
                text(format!("{cur} / {total}"))
                    .size(12)
                    .style(save_view::muted_text_style),
            );
        }
        header_row = header_row.push(
            button(text(t(lang, "playback-close")).size(STANDARD_TEXT_SIZE))
                .padding(STANDARD_PADDING)
                .on_press(Message::SessionClose),
        );
        let header = container(header_row.padding(8)).width(Fill);

        column![header, horizontal_rule(1), container(frame).center(Fill).padding(8)]
            .spacing(0)
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// Decode a `.tangoreplay` and spin up an mgba playback thread for
    /// it. Resolves both sides' ROMs (and BPS patches, if any) from the
    /// current scanners. Returns Err if any side can't be resolved.
    fn build_playback(
        &self,
        path: &std::path::Path,
    ) -> anyhow::Result<replay_session::ReplaySession> {
        let f = std::fs::File::open(path)?;
        let replay = std::sync::Arc::new(tango_pvp::replay::Replay::decode(f)?);
        let resolve_rom = |side: Option<&tango_pvp::replay::metadata::Side>| -> anyhow::Result<(
            &'static (dyn game::Game + Send + Sync),
            std::sync::Arc<Vec<u8>>,
        )> {
            let gi = side
                .and_then(|s| s.game_info.as_ref())
                .ok_or_else(|| anyhow::anyhow!("replay side has no game info"))?;
            let variant = u8::try_from(gi.rom_variant).map_err(|_| {
                anyhow::anyhow!("variant {} out of range", gi.rom_variant)
            })?;
            let entry = tango_gamedb::find_by_family_and_variant(&gi.rom_family, variant)
                .ok_or_else(|| {
                    anyhow::anyhow!("unknown rom {}/{}", gi.rom_family, gi.rom_variant)
                })?;
            let g = game::from_gamedb_entry(entry)
                .ok_or_else(|| anyhow::anyhow!("no tango-ng impl for {}/{}", gi.rom_family, gi.rom_variant))?;
            let rom = self
                .scanners
                .roms
                .read()
                .get(&entry)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("rom for {}/{} not scanned", gi.rom_family, gi.rom_variant))?;
            let rom = if let Some(patch_info) = gi.patch.as_ref() {
                let v = semver::Version::parse(&patch_info.version)?;
                patch::apply_patch_from_disk(
                    &rom,
                    entry,
                    &self.config.patches_path(),
                    &patch_info.name,
                    &v,
                )?
            } else {
                rom
            };
            Ok((g, std::sync::Arc::new(rom)))
        };

        let (local_game, local_rom) = resolve_rom(replay.metadata.local_side.as_ref())?;
        let (remote_game, remote_rom) = resolve_rom(replay.metadata.remote_side.as_ref())?;
        replay_session::ReplaySession::new(local_game, local_rom, remote_game, remote_rom, replay)
    }

    /// Boot the currently-loaded ROM in single-player mode using the
    /// active save selection. Errors out if anything's missing —
    /// callers should gate the Play button on a complete selection.
    fn spawn_singleplayer(&self) -> anyhow::Result<singleplayer_session::SinglePlayerSession> {
        let loaded = self
            .loaded
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no game / save selected"))?;
        let game = game::from_gamedb_entry(loaded.game).ok_or_else(|| {
            anyhow::anyhow!(
                "no tango-ng game impl for {:?}",
                loaded.game.family_and_variant()
            )
        })?;
        // Loaded stashes the *parsed* ROM (assets), not the raw bytes —
        // grab them back from the scanner and re-apply the patch if any
        // so the emulator sees the same image it would in the legacy app.
        let raw = self
            .scanners
            .roms
            .read()
            .get(&loaded.game)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("rom not in scanner cache"))?;
        let rom_bytes = if let Some(p) = loaded.patch.as_ref() {
            patch::apply_patch_from_disk(
                &raw,
                loaded.game,
                &self.config.patches_path(),
                &p.name,
                &p.version,
            )?
        } else {
            raw
        };
        singleplayer_session::SinglePlayerSession::new(
            game,
            std::sync::Arc::new(rom_bytes),
            &loaded.save_path,
        )
    }

    fn theme(&self) -> Theme {
        // Custom palettes derived from the built-in Light/Dark, with the
        // accent (primary) swapped to the BN-green that the main egui
        // app uses for selection / accents.
        const TANGO_GREEN: iced::Color = iced::Color::from_rgb(
            0x4c as f32 / 255.0,
            0xaf as f32 / 255.0,
            0x50 as f32 / 255.0,
        );
        match self.config.theme {
            config::ThemeMode::Light => Theme::custom(
                "Tango Light".to_string(),
                iced::theme::Palette {
                    primary: TANGO_GREEN,
                    ..iced::theme::Palette::LIGHT
                },
            ),
            config::ThemeMode::Dark => Theme::custom(
                "Tango Dark".to_string(),
                iced::theme::Palette {
                    primary: TANGO_GREEN,
                    ..iced::theme::Palette::DARK
                },
            ),
        }
    }
}

/// `iced::event::listen_with` needs a free `fn` (no captures), so we
/// fold the key→mgba-bit translation into the subscription itself and
/// only emit messages for keys we actually bind.
fn map_keyboard_event(
    event: iced::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    use iced::keyboard::Event as Kb;
    match event {
        iced::Event::Keyboard(Kb::KeyPressed { key, .. }) => {
            singleplayer_session::key_to_mgba_bit(&key).map(Message::SessionKeyDown)
        }
        iced::Event::Keyboard(Kb::KeyReleased { key, .. }) => {
            singleplayer_session::key_to_mgba_bit(&key).map(Message::SessionKeyUp)
        }
        _ => None,
    }
}

fn top_bar(lang: &LanguageIdentifier, active: Tab) -> Element<'_, Message> {
    let tab_button = |label: String, tab: Tab| {
        let style = if tab == active { button::primary } else { button::text };
        button(text(label).size(NAV_TEXT_SIZE))
            .padding(NAV_PADDING)
            .style(style)
            .on_press(Message::TabSelected(tab))
    };

    container(
        row![
            tab_button(t(lang, "tab-play"), Tab::Play),
            tab_button(t(lang, "tab-replays"), Tab::Replays),
            tab_button(t(lang, "tab-patches"), Tab::Patches),
            horizontal_space(),
            tab_button(t(lang, "tab-settings"), Tab::Settings),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .padding(6),
    )
    .width(Fill)
    .into()
}

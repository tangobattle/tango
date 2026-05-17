mod audio;
mod config;
mod game;
mod i18n;
mod icons;
mod navicust;
mod patch;
mod replay_session;
mod replays;
mod singleplayer_session;
mod rom;
mod rom_overrides;
mod save;
mod save_view;
mod net;
mod netplay;
mod scanner;
mod scrubber;
mod selection;
mod session;
mod tabs;

use session::ActiveSession;

use i18n::{t, FALLBACK_LANG};
use iced::widget::{button, column, container, horizontal_rule, horizontal_space, row};
use iced::{Alignment, Element, Fill, Theme};
use tabs::patches::PatchesState;
use tabs::play::{create_new_save, duplicate_save, rename_save, PlayState, SaveAction};
use tabs::replays::ReplaysState;
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
/// Lucide icon font (https://lucide.dev). Mapped via Private Use Area
/// codepoints — see `icons.rs` for the per-glyph constants.
const FONT_LUCIDE: &[u8] = include_bytes!("../fonts/lucide.ttf");

pub fn main() -> iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    // Route mgba's global default logger through `c_log` too — without
    // this, the prefetcher's bare Core falls through to mgba's printf
    // stub and spams `GBA BIOS: SWI: …` lines straight to stdout.
    mgba::log::install_default_logger();

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
        .font(FONT_LUCIDE)
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
    scanners: Scanners,
    /// Cloned into every session so they can bind their MGBAStream
    /// without owning the cpal backend. The cpal Backend lives in
    /// `_audio_backend` so the underlying stream keeps playing.
    audio_binder: audio::LateBinder,
    /// Kept alive for the program's lifetime; dropping it would tear
    /// down the cpal output stream and the app would go silent.
    _audio_backend: Option<audio::cpal::Backend>,

    /// Owned game+save+assets for the current selection. Rebuilt only
    /// when game or save changes; per-frame view() borrows it.
    loaded: Option<selection::Loaded>,

    play: PlayState,
    replays: ReplaysState,
    patches: PatchesState,
    settings: tabs::settings::State,
    welcome: tabs::welcome::State,
    netplay: netplay::State,

    /// Active emulator session (replay playback or single-player) plus
    /// the cached framebuffer Handle. While `session.is_active()`, the
    /// main body is replaced by `session::view`.
    session: session::State,
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
        let welcome = tabs::welcome::State::from_nickname(config.nickname.as_deref());

        // Spin up cpal once at startup with the LateBinder as the
        // source. Sessions later bind their MGBAStream into the binder
        // and the cpal stream keeps going across selections.
        let mut audio_binder = audio::LateBinder::new();
        audio_binder.set_volume(config.volume);
        let audio_backend = match audio::cpal::Backend::new(audio_binder.clone()) {
            Ok(b) => {
                use audio::Backend;
                audio_binder.set_sample_rate(b.sample_rate());
                log::info!("audio: cpal backend up at {} Hz", b.sample_rate());
                Some(b)
            }
            Err(e) => {
                log::warn!("audio: cpal init failed, running silent: {e:?}");
                None
            }
        };

        let mut app = Self {
            config,
            tab: Tab::default(),
            welcome,
            settings: tabs::settings::State::default(),
            scanners,
            audio_binder,
            _audio_backend: audio_backend,
            loaded: None,
            play,
            replays: ReplaysState::default(),
            patches: PatchesState::default(),
            session: session::State::new(),
            netplay: netplay::State::new(),
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
    /// Build a `protocol::Settings` packet from the App's current
    /// state: nickname from config, match_type defaults to (0, 0),
    /// game_info from the Play tab's local selection, and the
    /// available_games / available_patches lists from the scanners
    /// so the peer can see what we have locally. Mirrors
    /// `tango/src/gui/play_pane.rs::make_local_settings`.
    fn make_local_settings(&self) -> net::protocol::Settings {
        use net::protocol::{GameInfo, PatchInfo, Settings};
        let roms = self.scanners.roms.read();
        let patches = self.scanners.patches.read();
        Settings {
            nickname: self.config.nickname.clone().unwrap_or_default(),
            match_type: (0, 0),
            game_info: self.play.local_game.map(|game| {
                let (family, variant) = game.family_and_variant();
                GameInfo {
                    family_and_variant: (family.to_string(), variant),
                    patch: match (&self.play.local_patch, &self.play.local_patch_version) {
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
            reveal_setup: false,
        }
    }

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
pub enum Tab {
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
pub enum Message {
    TabSelected(Tab),
    Play(tabs::play::Message),
    Patches(tabs::patches::Message),
    Replays(tabs::replays::Message),
    Settings(tabs::settings::Message),
    Welcome(tabs::welcome::Message),
    Session(session::Message),
    Netplay(netplay::Message),
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
            // PlayPressed branches to the netplay path when the user
            // typed a link code. We special-case it here because
            // update_play returns Task<play::Message>, not
            // Task<crate::Message> — kicking off a netplay::Connect
            // task needs the broader return type.
            Message::Play(tabs::play::Message::PlayPressed)
                if !self.play.link_code.trim().is_empty() =>
            {
                self.play.flash_status = None;
                let link_code = self.play.link_code.trim().to_string();
                let endpoint = self.config.matchmaking_endpoint.clone();
                self.netplay
                    .update(netplay::Message::Connect { link_code, endpoint })
                    .map(Message::Netplay)
            }
            Message::Play(tabs::play::Message::NetplayDisconnect) => {
                self.netplay
                    .update(netplay::Message::Disconnect)
                    .map(Message::Netplay)
            }
            Message::Play(m) => self.update_play(m).map(Message::Play),
            Message::Patches(m) => self.update_patches(m).map(Message::Patches),
            Message::Replays(m) => self.update_replays(m).map(Message::Replays),
            Message::Settings(m) => self.update_settings(m).map(Message::Settings),
            Message::Welcome(m) => self.update_welcome(m).map(Message::Welcome),
            Message::Session(m) => self.session.update(m).map(Message::Session),
            Message::Netplay(m) => {
                // Watch for Negotiating → Lobby. The first time we
                // land in Lobby we push a SendLocalSettings so the
                // peer sees our nickname / game / match type. Done
                // outside the netplay module because it needs the
                // App's view of the world (config + scanners +
                // PlayState selection).
                let was_lobby = matches!(self.netplay.phase, netplay::Phase::Lobby { .. });
                let task = self.netplay.update(m).map(Message::Netplay);
                let now_lobby = matches!(self.netplay.phase, netplay::Phase::Lobby { .. });
                if !was_lobby && now_lobby {
                    let settings = self.make_local_settings();
                    let send = self
                        .netplay
                        .update(netplay::Message::SendLocalSettings(Box::new(settings)))
                        .map(Message::Netplay);
                    iced::Task::batch([task, send])
                } else {
                    task
                }
            }
        }
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        iced::Subscription::batch([
            session::subscription(&self.session).map(Message::Session),
            netplay::subscription(&self.netplay).map(Message::Netplay),
        ])
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
            M::SaveViewAction(action) => {
                self.play.save_view.apply(&action);
                if let save_view::Action::CopyTab(tab) = action {
                    if let Some(loaded) = self.loaded.as_ref() {
                        if let Some(s) = save_view::tab_as_text(&self.config.language, tab, loaded) {
                            return iced::clipboard::write(s);
                        }
                    }
                }
            }
            M::LinkCodeChanged(s) => {
                self.play.link_code = s;
                self.play.flash_status = None;
            }
            M::PlayPressed => {
                // Netplay branch is handled in App::update before this
                // (it needs to return Task<Message::Netplay>); we only
                // see PlayPressed here when link_code is empty, i.e.
                // the single-player path.
                self.play.flash_status = None;
                let Some(loaded) = self.loaded.as_ref() else {
                    self.play.flash_status =
                        Some(t(&self.config.language, "play-no-selection"));
                    return iced::Task::none();
                };
                match session::spawn_singleplayer(
                    &self.scanners,
                    &self.config,
                    &self.audio_binder,
                    loaded,
                ) {
                    Ok(s) => {
                        self.session.active = Some(ActiveSession::SinglePlayer(s));
                        self.session.frame = None;
                    }
                    Err(e) => {
                        log::warn!("singleplayer start failed: {e}");
                        self.play.flash_status = Some(format!("{e}"));
                    }
                }
            }
            M::NetplayDisconnect => {
                // Handled at App::update with the broader return type.
                // No-op here; we should never actually land in this arm.
            }
            M::Rescan => {
                self.scanners.rescan(&self.config);
                self.refresh_loaded();
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
                self.replays.loaded = None;
                self.replays.loaded_cache_path = None;
            }
            M::Selected(p) => {
                self.replays.selected = Some(p);
                self.refresh_replay_loaded();
            }
            M::OpenFolder(p) => {
                if let Err(e) = open::that(&p) {
                    log::error!("open folder {}: {e}", p.display());
                }
            }
            M::Watch(p) => match session::build_playback(
                &self.scanners,
                &self.config,
                &self.audio_binder,
                &p,
            ) {
                Ok(s) => {
                    self.session.active = Some(ActiveSession::Replay(s));
                    self.session.frame = None;
                }
                Err(e) => log::warn!("failed to play replay {}: {e}", p.display()),
            },
            M::Rescan => {
                self.scanners.rescan(&self.config);
                self.refresh_loaded();
            }
            M::SaveViewAction(action) => {
                self.replays.save_view.apply(&action);
                if let save_view::Action::CopyTab(tab) = action {
                    if let Some(loaded) = self.replays.loaded.as_ref() {
                        if let Some(s) = save_view::tab_as_text(&self.config.language, tab, loaded) {
                            return iced::clipboard::write(s);
                        }
                    }
                }
            }
        }
        iced::Task::none()
    }

    /// Lazily rebuild `replays.loaded` for the currently-selected
    /// replay's local side. No-op when the cache path already matches.
    /// Failures log + clear the cache so the detail panel falls back
    /// to the metadata-only summary.
    fn refresh_replay_loaded(&mut self) {
        let Some(path) = self.replays.selected.clone() else {
            self.replays.loaded = None;
            self.replays.loaded_cache_path = None;
            return;
        };
        if self.replays.loaded_cache_path.as_ref() == Some(&path) {
            return;
        }
        let res = (|| -> anyhow::Result<selection::Loaded> {
            let f = std::fs::File::open(&path)?;
            let replay = tango_pvp::replay::Replay::decode(f)?;
            selection::Loaded::for_replay_local(&self.scanners, &self.config, &replay)
        })();
        match res {
            Ok(loaded) => {
                self.replays.loaded = Some(loaded);
                self.replays.loaded_cache_path = Some(path);
            }
            Err(e) => {
                log::warn!("replay save preview failed: {e}");
                self.replays.loaded = None;
                self.replays.loaded_cache_path = None;
            }
        }
    }

    fn update_settings(&mut self, msg: tabs::settings::Message) -> iced::Task<tabs::settings::Message> {
        use tabs::settings::ConfigChange as C;
        let Some(change) = self.settings.update(msg) else {
            return iced::Task::none();
        };
        match change {
            C::Language(l) => self.config.language = l,
            C::Nickname(s) => self.config.nickname = if s.is_empty() { None } else { Some(s) },
            C::StreamerMode(b) => self.config.streamer_mode = b,
            C::MatchmakingEndpoint(s) => self.config.matchmaking_endpoint = s,
            C::PatchRepo(s) => self.config.patch_repo = s,
            C::Theme(t) => self.config.theme = t,
            C::Volume(v) => {
                self.config.volume = v;
                self.audio_binder.set_volume(v);
            }
        }
        self.persist_config();
        iced::Task::none()
    }

    fn update_welcome(&mut self, msg: tabs::welcome::Message) -> iced::Task<tabs::welcome::Message> {
        use tabs::welcome::Message as M;
        match msg {
            M::NicknameChanged(s) => {
                self.welcome.nickname_draft = s;
            }
            M::Continue => {
                if let Some(nickname) = self.welcome.finalize_nickname() {
                    self.config.nickname = Some(nickname);
                    self.persist_config();
                }
            }
            M::LanguageSelected(l) => {
                self.config.language = l;
                self.persist_config();
            }
            M::OpenRomsFolder => {
                let p = self.config.roms_path();
                let _ = std::fs::create_dir_all(&p);
                if let Err(e) = open::that(&p) {
                    log::error!("open roms folder: {e}");
                }
            }
            M::RescanRoms => {
                self.scanners.rescan(&self.config);
                self.refresh_loaded();
            }
        }
        iced::Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let lang = &self.config.language;

        // First-run gate: no main UI until the user picks a nickname.
        if self.config.nickname.is_none() {
            let roms_count = self.scanners.roms.read().len();
            return tabs::welcome::view(lang, &self.welcome, roms_count, &self.config.roms_path())
                .map(Message::Welcome);
        }

        if self.session.is_active() {
            return session::view(lang, &self.session).map(Message::Session);
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
                    &self.netplay.phase,
                    &self.netplay.lobby,
                )
                .map(Message::Play),
            Tab::Replays => self
                .replays
                .view(lang, &self.scanners, &self.config)
                .map(Message::Replays),
            Tab::Patches => self.patches.view(lang, &self.scanners).map(Message::Patches),
            Tab::Settings => tabs::settings::view(lang, &self.config, &self.settings).map(Message::Settings),
        };

        column![top_bar(lang, self.tab), horizontal_rule(1), body]
            .spacing(0)
            .width(Fill)
            .height(Fill)
            .into()
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

fn top_bar(lang: &LanguageIdentifier, active: Tab) -> Element<'_, Message> {
    let tab_button = |icon: &'static str, label: String, tab: Tab| {
        let style = if tab == active { button::primary } else { button::text };
        icons::labeled_icon_button(
            icon,
            label,
            Message::TabSelected(tab),
            NAV_TEXT_SIZE,
            NAV_PADDING,
            style,
        )
    };

    container(
        row![
            tab_button(icons::TAB_PLAY, t(lang, "tab-play"), Tab::Play),
            tab_button(icons::TAB_REPLAYS, t(lang, "tab-replays"), Tab::Replays),
            tab_button(icons::TAB_PATCHES, t(lang, "tab-patches"), Tab::Patches),
            horizontal_space(),
            tab_button(icons::TAB_SETTINGS, t(lang, "tab-settings"), Tab::Settings),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .padding(6),
    )
    .width(Fill)
    .into()
}

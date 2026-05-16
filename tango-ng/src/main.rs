mod config;
mod game;
mod i18n;
mod navicust;
mod patch;
mod replays;
mod rom;
mod rom_overrides;
mod save;
mod save_view;
mod scanner;
mod selection;

use i18n::{t, FALLBACK_LANG};
use iced::widget::{
    button, column, container, horizontal_rule, horizontal_space, pick_list, row, scrollable, text, text_input,
    vertical_rule, Space,
};
use iced::{Alignment, Element, Fill, Length, Theme};
use unic_langid::LanguageIdentifier;

const SUPPORTED_LANGS: &[LanguageIdentifier] = &[
    unic_langid::langid!("en-US"),
    unic_langid::langid!("ja-JP"),
];

pub fn main() -> iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    iced::application(App::title, App::update, App::view)
        .theme(App::theme)
        .window_size((1000.0, 640.0))
        .run_with(App::new)
}

#[derive(Clone)]
struct Scanners {
    roms: rom::Scanner,
    saves: save::Scanner,
    patches: patch::Scanner,
    replays: replays::Scanner,
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

    fn rescan(&self, paths: &config::Paths) {
        let roms_path = paths.roms();
        let saves_path = paths.saves();
        let patches_path = paths.patches();
        let replays_path = paths.replays();
        self.roms.rescan(|| Some(rom::scan_roms(&roms_path)));
        self.saves.rescan(|| Some(save::scan_saves(&saves_path)));
        self.patches.rescan(|| patch::scan(&patches_path).ok());
        self.replays.rescan(|| Some(replays::scan_replays(&replays_path)));
    }
}

struct App {
    language: LanguageIdentifier,
    tab: Tab,
    settings_open: bool,
    streamer_mode: bool,
    paths: config::Paths,
    scanners: Scanners,

    /// Owned game+save+assets for the current selection. Rebuilt only
    /// when game or save changes; per-frame view() borrows it.
    loaded: Option<selection::Loaded>,

    play: PlayState,
    replays: ReplaysState,
    patches: PatchesState,
}

impl App {
    fn new() -> (Self, iced::Task<Message>) {
        let paths = config::Paths::system_default().unwrap_or_else(|e| {
            log::error!("failed to resolve data paths: {e}; falling back to ./tango-data");
            config::Paths {
                data: std::path::PathBuf::from("./tango-data"),
            }
        });

        let scanners = Scanners::new();
        scanners.rescan(&paths);
        log::info!(
            "initial scan: {} rom(s), {} save game(s), {} patch(es), {} replay(s)",
            scanners.roms.read().len(),
            scanners.saves.read().values().map(|v| v.len()).sum::<usize>(),
            scanners.patches.read().len(),
            scanners.replays.read().len(),
        );

        let app = Self {
            language: FALLBACK_LANG,
            tab: Tab::default(),
            settings_open: false,
            streamer_mode: false,
            paths,
            scanners,
            loaded: None,
            play: PlayState::default(),
            replays: ReplaysState::default(),
            patches: PatchesState::default(),
        };
        (app, iced::Task::none())
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
        let patches_path = self.paths.patches();
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
}

#[derive(Debug, Clone)]
enum Message {
    TabSelected(Tab),
    ToggleSettings,
    LanguageSelected(LanguageIdentifier),
    Rescan,

    LocalGameSelected(GameOption),
    LocalSaveSelected(SaveOption),
    LocalPatchSelected(String),
    LocalPatchVersionSelected(semver::Version),
    SaveTabSelected(save_view::Tab),
    ToggleFolderGrouped(bool),
    ToggleStreamerMode(bool),
    LinkCodeChanged(String),
    PlayPressed,

    FolderFilterSelected(FolderOption),
    ReplaySelected(std::path::PathBuf),

    PatchSelected(String),
    PatchVersionSelected(semver::Version),
    OpenFolder(std::path::PathBuf),
    ReadmeLinkClicked(iced::widget::markdown::Url),
}

impl App {
    fn title(&self) -> String {
        t(&self.language, "window-title")
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::TabSelected(t) => self.tab = t,
            Message::ToggleSettings => self.settings_open = !self.settings_open,
            Message::LanguageSelected(l) => self.language = l,
            Message::Rescan => {
                self.scanners.rescan(&self.paths);
                self.refresh_loaded();
            }

            Message::LocalGameSelected(g) => {
                self.play.local_game = Some(g.game);
                // Pick the first save for this game if any.
                self.play.local_save = self
                    .scanners
                    .saves
                    .read()
                    .get(&g.game)
                    .and_then(|v| v.first().map(|s| s.path.clone()));
                self.play.local_patch = None;
                self.play.local_patch_version = None;
                self.refresh_loaded();
            }
            Message::LocalSaveSelected(s) => {
                self.play.local_save = Some(s.path);
                self.refresh_loaded();
            }
            Message::LocalPatchSelected(p) => {
                if p == t(&self.language, "play-no-patch") {
                    self.play.local_patch = None;
                    self.play.local_patch_version = None;
                } else {
                    // Default to the highest version of the chosen patch.
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
            }
            Message::LocalPatchVersionSelected(v) => {
                self.play.local_patch_version = Some(v);
                self.refresh_loaded();
            }
            Message::SaveTabSelected(t) => self.play.save_tab = Some(t),
            Message::ToggleFolderGrouped(g) => self.play.folder_grouped = g,
            Message::ToggleStreamerMode(s) => self.streamer_mode = s,
            Message::LinkCodeChanged(s) => self.play.link_code = s,
            Message::PlayPressed => self.play.playing = !self.play.playing,

            Message::FolderFilterSelected(f) => {
                self.replays.folder_filter = f.path;
                self.replays.selected = None;
            }
            Message::ReplaySelected(p) => self.replays.selected = Some(p),

            Message::PatchSelected(p) => {
                // Initial version: highest available for this patch.
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
            Message::PatchVersionSelected(v) => {
                self.patches.version = Some(v);
                self.patches.refresh_readme(&self.scanners);
            }
            Message::ReadmeLinkClicked(url) => {
                if let Err(e) = open::that(url.as_str()) {
                    log::error!("failed to open url {url}: {e}");
                }
            }
            Message::OpenFolder(p) => {
                if let Err(e) = open::that(&p) {
                    log::error!("failed to open folder {}: {e}", p.display());
                }
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let lang = &self.language;
        let body: Element<'_, _> = match self.tab {
            Tab::Play => self
                .play
                .view(lang, &self.scanners, self.loaded.as_ref(), self.streamer_mode),
            Tab::Replays => self.replays.view(lang, &self.scanners, &self.paths),
            Tab::Patches => self.patches.view(lang, &self.scanners),
        };

        let main = column![top_bar(lang, self.tab, self.settings_open), horizontal_rule(1), body]
            .spacing(0)
            .width(Fill)
            .height(Fill);

        if self.settings_open {
            row![
                main,
                vertical_rule(1),
                settings_panel(lang, &self.paths, self.streamer_mode)
            ]
            .width(Fill)
            .height(Fill)
            .into()
        } else {
            main.into()
        }
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }
}

fn top_bar(lang: &LanguageIdentifier, active: Tab, settings_open: bool) -> Element<'_, Message> {
    let tab_button = |label: String, tab: Tab| {
        let style = if tab == active { button::primary } else { button::text };
        button(text(label).size(16))
            .padding([6, 14])
            .style(style)
            .on_press(Message::TabSelected(tab))
    };

    let settings_style = if settings_open { button::primary } else { button::text };

    container(
        row![
            tab_button(t(lang, "tab-play"), Tab::Play),
            tab_button(t(lang, "tab-replays"), Tab::Replays),
            tab_button(t(lang, "tab-patches"), Tab::Patches),
            horizontal_space(),
            button(text(t(lang, "tab-settings")).size(14))
                .padding([6, 12])
                .style(settings_style)
                .on_press(Message::ToggleSettings),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .padding(6),
    )
    .width(Fill)
    .into()
}

fn settings_panel<'a>(
    lang: &'a LanguageIdentifier,
    paths: &'a config::Paths,
    streamer_mode: bool,
) -> Element<'a, Message> {
    let kv_str = |label: String, value: String| -> Element<'a, Message> {
        row![
            text(label).size(13).width(Length::Fill),
            text(value).size(13).style(text::primary),
        ]
        .into()
    };

    container(
        column![
            text(t(lang, "tab-settings")).size(20),
            horizontal_rule(1),
            section_label_str(t(lang, "settings-section-general")),
            kv_str(t(lang, "settings-nickname"), "bigfarts".to_string()),
            row![
                text(t(lang, "settings-language")).size(13).width(Length::Fill),
                pick_list(
                    SUPPORTED_LANGS.to_vec(),
                    Some(lang.clone()),
                    Message::LanguageSelected,
                ),
            ]
            .align_y(Alignment::Center),
            iced::widget::checkbox(t(lang, "settings-streamer-mode"), streamer_mode)
                .on_toggle(Message::ToggleStreamerMode),
            Space::with_height(8),
            section_label_str(t(lang, "settings-data-path")),
            text(paths.data.display().to_string()).size(11),
            Space::with_height(8),
            section_label_str(t(lang, "settings-section-graphics")),
            kv_str(t(lang, "settings-renderer"), "wgpu".to_string()),
            kv_str(t(lang, "settings-scale"), "3×".to_string()),
            Space::with_height(8),
            section_label_str(t(lang, "settings-section-audio")),
            kv_str(t(lang, "settings-audio-backend"), "cpal".to_string()),
            Space::with_height(8),
            section_label_str(t(lang, "settings-section-netplay")),
            kv_str(t(lang, "settings-signaling"), "signaling.tango.nyc".to_string()),
        ]
        .spacing(6)
        .padding(16),
    )
    .width(Length::Fixed(320.0))
    .height(Fill)
    .into()
}

fn section_label_str(s: String) -> Element<'static, Message> {
    text(s).size(13).style(text::primary).into()
}

// ---------- Play tab ----------

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

struct PlayState {
    local_game: Option<rom::GameRef>,
    local_save: Option<std::path::PathBuf>,
    local_patch: Option<String>,
    local_patch_version: Option<semver::Version>,
    /// Explicit save-tab pick; `None` means "auto-pick from available".
    save_tab: Option<save_view::Tab>,
    folder_grouped: bool,
    link_code: String,
    playing: bool,
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
            link_code: String::new(),
            playing: false,
        }
    }
}

impl PlayState {
    fn view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        loaded: Option<&'a selection::Loaded>,
        streamer_mode: bool,
    ) -> Element<'a, Message> {
        column![
            self.selector_strip(lang, scanners),
            horizontal_rule(1),
            self.save_view(lang, loaded, streamer_mode),
            horizontal_rule(1),
            self.bottom_strip(lang),
        ]
        .width(Fill)
        .height(Fill)
        .into()
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

        // Patches: only those that explicitly support the selected game
        // for at least one version. Listed alphabetically by name with
        // a localized "no patch" sentinel as the first option.
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

        // Versions: those of the selected patch that support the
        // selected game.
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

        let refresh = button(text(t(lang, "rescan")).size(12))
            .padding([6, 10])
            .on_press(Message::Rescan);

        let game_row = row![game, patch, version, refresh]
            .spacing(8)
            .align_y(Alignment::Center);
        let save_row = row![save].align_y(Alignment::Center);

        container(
            column![game_row, save_row]
                .spacing(6)
                .padding(8),
        )
        .width(Fill)
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

        // Auto-pick first available if the user-picked tab isn't in the
        // set this save supports. In streamer mode the first tab is
        // Cover, which keeps save data hidden until the user clicks
        // another tab explicitly.
        let active = self
            .save_tab
            .filter(|t| available.contains(t))
            .unwrap_or(available[0]);

        let tab_button = |label: String, tab: save_view::Tab| {
            let style = if tab == active { button::primary } else { button::text };
            button(text(label).size(13))
                .padding([5, 12])
                .style(style)
                .on_press(Message::SaveTabSelected(tab))
        };

        let mut tab_row = row![].spacing(4).padding([4, 8]);
        for tab in &available {
            tab_row = tab_row.push(tab_button(t(lang, save_view::tab_key(*tab)), *tab));
        }
        let tabs = container(tab_row).width(Fill);

        let body = save_view::render(
            lang,
            active,
            loaded,
            save_view::RenderOpts {
                folder_grouped: self.folder_grouped,
            },
        );

        column![tabs, horizontal_rule(1), body]
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn bottom_strip<'a>(&'a self, lang: &'a LanguageIdentifier) -> Element<'a, Message> {
        let play_button = if self.playing {
            button(text(t(lang, "play-cancel")).size(14))
                .padding([8, 18])
                .style(button::danger)
                .on_press(Message::PlayPressed)
        } else {
            button(text(t(lang, "play-play")).size(14))
                .padding([8, 18])
                .style(button::success)
                .on_press(Message::PlayPressed)
        };

        let status: Element<'_, _> = if self.playing {
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

// ---------- Replays tab ----------

#[derive(Default)]
struct ReplaysState {
    /// `None` = no folder filter (show all); `Some` = restrict to direct
    /// children of this dir.
    folder_filter: Option<std::path::PathBuf>,
    selected: Option<std::path::PathBuf>,
}

impl ReplaysState {
    fn view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        paths: &'a config::Paths,
    ) -> Element<'a, Message> {
        let replays_path = paths.replays();
        let replays = scanners.replays.read();

        // Top: folder filter dropdown. Default option is "all".
        let all_label = t(lang, "replays-all-replays");
        let mut folder_options = vec![FolderOption::all(all_label.clone())];
        {
            use itertools::Itertools;
            let mut parents: Vec<std::path::PathBuf> = replays
                .iter()
                .flat_map(|r| r.path.parent().map(|p| p.to_path_buf()))
                .unique()
                .collect();
            parents.sort();
            for p in parents {
                let display = replays::format_rel_path(&replays_path, &p);
                folder_options.push(FolderOption {
                    path: Some(p),
                    display,
                });
            }
        }
        let selected_folder = folder_options
            .iter()
            .find(|f| f.path == self.folder_filter)
            .cloned()
            .unwrap_or_else(|| folder_options[0].clone());
        let top = container(
            row![
                text(format!("{}:", t(lang, "replays-folder-label"))),
                pick_list(folder_options, Some(selected_folder), Message::FolderFilterSelected),
                horizontal_space(),
                button(text(t(lang, "rescan")).size(12))
                    .padding([6, 10])
                    .on_press(Message::Rescan),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .padding(8),
        )
        .width(Fill);

        // Left list. Pre-filter by folder, then build rows.
        let folder_filter = self.folder_filter.as_ref();
        let filtered: Vec<&replays::ScannedReplay> = replays
            .iter()
            .filter(|r| {
                folder_filter
                    .map(|f| r.path.parent().map(|p| p == f.as_path()).unwrap_or(false))
                    .unwrap_or(true)
            })
            .collect();

        let mut list = column![].spacing(0).padding(8);
        let mut last_fp: Option<(String, String, String)> = None;
        let mut alternate = true;
        for r in &filtered {
            let md = &r.metadata;
            let local_nick = md.local_side.as_ref().map(|s| s.nickname.clone()).unwrap_or_default();
            let remote_nick = md.remote_side.as_ref().map(|s| s.nickname.clone()).unwrap_or_default();
            let fp = (md.link_code.clone(), local_nick.clone(), remote_nick.clone());
            if Some(&fp) != last_fp.as_ref() {
                alternate = !alternate;
                last_fp = Some(fp);
            }

            let ts_str = std::time::UNIX_EPOCH
                .checked_add(std::time::Duration::from_millis(md.ts))
                .map(|t| {
                    let dt: chrono::DateTime<chrono::Local> = t.into();
                    dt.format("%Y-%m-%d %H:%M:%S").to_string()
                })
                .unwrap_or_else(|| "(?)".to_string());

            let game_family = md
                .local_side
                .as_ref()
                .and_then(|s| s.game_info.as_ref())
                .map(|g| g.rom_family.clone())
                .unwrap_or_default();
            let nick_pair = if remote_nick.is_empty() && local_nick.is_empty() {
                md.link_code.clone()
            } else {
                format!("{local_nick} vs {remote_nick}")
            };

            let selected = self.selected.as_ref() == Some(&r.path);
            let style = if selected {
                button::primary
            } else if alternate {
                button::secondary
            } else {
                button::text
            };
            list = list.push(
                button(
                    column![
                        text(ts_str).size(13),
                        text(format!(
                            "{game_family} @ {}  ·  {nick_pair}  ·  round {}",
                            md.link_code, md.round
                        ))
                        .size(11)
                        .color(iced::Color::from_rgb8(0x90, 0x90, 0x90)),
                    ]
                    .spacing(2),
                )
                .padding(6)
                .width(Fill)
                .style(style)
                .on_press(Message::ReplaySelected(r.path.clone())),
            );
        }
        let left = container(scrollable(list).height(Fill))
            .width(Length::Fixed(360.0))
            .height(Fill);

        // Right panel.
        let right: Element<'_, Message> = if let Some(sel_path) = self.selected.as_ref() {
            if let Some(r) = filtered.iter().find(|r| &r.path == sel_path) {
                replay_detail(lang, r, &replays_path)
            } else {
                container(text(t(lang, "replays-select-prompt")).size(13))
                    .center(Fill)
                    .into()
            }
        } else {
            container(text(t(lang, "replays-select-prompt")).size(13))
                .center(Fill)
                .into()
        };

        column![
            top,
            horizontal_rule(1),
            row![left, vertical_rule(1), right].height(Fill),
        ]
        .height(Fill)
        .into()
    }
}

fn replay_detail<'a>(
    lang: &'a LanguageIdentifier,
    r: &'a replays::ScannedReplay,
    replays_path: &'a std::path::Path,
) -> Element<'static, Message> {
    let md = &r.metadata;
    let ts_str = std::time::UNIX_EPOCH
        .checked_add(std::time::Duration::from_millis(md.ts))
        .map(|t| {
            let dt: chrono::DateTime<chrono::Local> = t.into();
            dt.format("%Y-%m-%d %H:%M:%S %z").to_string()
        })
        .unwrap_or_else(|| "(?)".to_string());

    let row_for_side = |label: String, side: Option<&tango_pvp::replay::metadata::Side>| -> Element<'static, Message> {
        let nick = side.map(|s| s.nickname.clone()).unwrap_or_default();
        let gi = side.and_then(|s| s.game_info.as_ref());
        let game = gi.map(|g| format!("{} v{}", g.rom_family, g.rom_variant)).unwrap_or_default();
        let patch = gi.and_then(|g| g.patch.as_ref()).map(|p| format!("{} v{}", p.name, p.version));
        let mut col = column![
            text(label).size(11).color(iced::Color::from_rgb8(0x90, 0x90, 0x90)),
            text(nick).size(14),
            text(game).size(12),
        ]
        .spacing(2);
        if let Some(p) = patch {
            col = col.push(text(p).size(11).color(iced::Color::from_rgb8(0xa0, 0xa0, 0xff)));
        }
        container(col).width(Length::Fill).into()
    };

    let parent_str = r
        .path
        .parent()
        .map(|p| replays::format_rel_path(replays_path, p))
        .unwrap_or_else(|| "/".to_string());
    let filename = r
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    container(
        column![
            row![
                text(format!("{} #{}", t(lang, "replays-round"), md.round)).size(18),
                horizontal_space(),
                button(text(t(lang, "replays-watch")))
                    .padding([8, 14])
                    .style(button::primary),
                button(text(t(lang, "replays-export"))).padding([8, 14]),
                button(text(t(lang, "patches-open-folder")).size(12))
                    .padding([6, 10])
                    .on_press(Message::OpenFolder(
                        r.path.parent().map(|p| p.to_path_buf()).unwrap_or_default(),
                    )),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            text(ts_str).size(12).color(iced::Color::from_rgb8(0xa0, 0xa0, 0xa0)),
            text(format!("{parent_str}{filename}")).size(11).color(iced::Color::from_rgb8(0x80, 0x80, 0x80)),
            Space::with_height(8),
            horizontal_rule(1),
            Space::with_height(8),
            row![
                row_for_side(t(lang, "play-you"), md.local_side.as_ref()),
                vertical_rule(1),
                row_for_side(t(lang, "replays-opponent"), md.remote_side.as_ref()),
            ]
            .spacing(12)
            .height(Length::Shrink),
            Space::with_height(8),
            text(format!(
                "{}: {}.{}",
                t(lang, "replays-match-type"),
                md.match_type,
                md.match_subtype
            ))
            .size(12),
        ]
        .spacing(6)
        .padding(16),
    )
    .width(Fill)
    .height(Fill)
    .into()
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FolderOption {
    path: Option<std::path::PathBuf>,
    display: String,
}
impl FolderOption {
    fn all(label: String) -> Self {
        Self { path: None, display: label }
    }
}
impl std::fmt::Display for FolderOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}

// ---------- Patches tab ----------

#[derive(Default)]
struct PatchesState {
    selected: Option<String>,
    version: Option<semver::Version>,
    /// Parsed markdown items for the current selection's README. Cached
    /// so we don't re-parse the whole README on every frame.
    readme_items: Vec<iced::widget::markdown::Item>,
    /// Name + version the cache was built for, so we know when to
    /// invalidate.
    readme_cache_key: Option<(String, semver::Version)>,
}

impl PatchesState {
    /// Rebuild the parsed-markdown cache for the currently selected
    /// patch+version. No-op if the cache already matches.
    fn refresh_readme(&mut self, scanners: &Scanners) {
        let key = match (&self.selected, &self.version) {
            (Some(n), Some(v)) => Some((n.clone(), v.clone())),
            _ => None,
        };
        if self.readme_cache_key == key {
            return;
        }
        self.readme_cache_key = key.clone();
        self.readme_items = key
            .as_ref()
            .and_then(|(n, _)| {
                scanners
                    .patches
                    .read()
                    .get(n)
                    .and_then(|p| p.readme.clone())
            })
            .map(|md| iced::widget::markdown::parse(&md).collect())
            .unwrap_or_default();
    }

    fn view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
    ) -> Element<'a, Message> {
        let patches = scanners.patches.read();

        let top = container(
            row![
                button(text(t(lang, "rescan")).size(14))
                    .padding([6, 12])
                    .on_press(Message::Rescan),
                horizontal_space(),
                text(format!(
                    "{}: {}",
                    t(lang, "patches-installed"),
                    patches.len()
                ))
                .size(11),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .padding(8),
        )
        .width(Fill);

        let mut list = column![].spacing(2).padding(8);
        for (name, patch) in patches.iter() {
            let selected = self.selected.as_deref() == Some(name.as_str());
            let style = if selected { button::primary } else { button::text };
            list = list.push(
                button(
                    column![
                        text(patch.title.clone()).size(14),
                        text(name.clone()).size(10).color(iced::Color::from_rgb8(0x80, 0x80, 0x80)),
                    ]
                    .spacing(2),
                )
                .padding(8)
                .width(Fill)
                .style(style)
                .on_press(Message::PatchSelected(name.clone())),
            );
        }
        let left = container(scrollable(list).height(Fill))
            .width(Length::Fixed(280.0))
            .height(Fill);

        let right: Element<_> = if let Some(patch) = self.selected.as_ref().and_then(|n| patches.get(n)) {
            let mut versions: Vec<semver::Version> = patch.versions.keys().cloned().collect();
            versions.sort_by(|a, b| b.cmp(a));
            let selected_version = self
                .version
                .clone()
                .filter(|v| patch.versions.contains_key(v))
                .or_else(|| versions.first().cloned());

            let version_info = selected_version
                .as_ref()
                .and_then(|v| patch.versions.get(v))
                .cloned();

            let supported_games_str = version_info
                .as_ref()
                .map(|v| {
                    let mut names: Vec<String> = v
                        .supported_games
                        .iter()
                        .map(|g| game::display_name(lang, *g))
                        .collect();
                    names.sort();
                    if names.is_empty() {
                        "—".to_string()
                    } else {
                        names.join(", ")
                    }
                })
                .unwrap_or_else(|| "—".to_string());

            let netplay_compat = version_info
                .as_ref()
                .map(|v| v.netplay_compatibility.clone())
                .unwrap_or_default();

            let header = row![
                text(patch.title.clone()).size(20),
                horizontal_space(),
                pick_list(versions, selected_version, Message::PatchVersionSelected),
                {
                    let path = patch.path.clone();
                    button(text(t(lang, "patches-open-folder")).size(12))
                        .padding([6, 10])
                        .on_press(Message::OpenFolder(path))
                },
            ]
            .spacing(8)
            .align_y(Alignment::Center);

            let mut details = column![].spacing(4);
            if !patch.authors.is_empty() {
                details = details.push(
                    text(format!(
                        "{}: {}",
                        t(lang, "patches-details-authors"),
                        patch.authors.join(", ")
                    ))
                    .size(12),
                );
            }
            if let Some(license) = &patch.license {
                details = details.push(
                    text(format!("{}: {}", t(lang, "patches-details-license"), license)).size(12),
                );
            }
            if let Some(source) = &patch.source {
                details = details.push(
                    text(format!("{}: {}", t(lang, "patches-details-source"), source)).size(12),
                );
            }
            details = details.push(
                text(format!(
                    "{}: {}",
                    t(lang, "patches-details-games"),
                    supported_games_str
                ))
                .size(12),
            );
            if !netplay_compat.is_empty() {
                details = details.push(
                    text(format!(
                        "{}: {}",
                        t(lang, "patches-netplay-compatibility"),
                        netplay_compat
                    ))
                    .size(12),
                );
            }

            // Markdown README, parsed and cached in self.readme_items.
            let readme_body: Element<'_, Message> = if self.readme_items.is_empty() {
                text(t(lang, "patches-readme-placeholder")).size(12).into()
            } else {
                let theme = iced::Theme::Dark;
                iced::widget::markdown::view(
                    &self.readme_items,
                    iced::widget::markdown::Settings::default(),
                    iced::widget::markdown::Style::from_palette(theme.palette()),
                )
                .map(Message::ReadmeLinkClicked)
            };

            container(
                column![
                    header,
                    Space::with_height(8),
                    horizontal_rule(1),
                    Space::with_height(8),
                    details,
                    Space::with_height(12),
                    text(t(lang, "patches-readme")).size(13).style(text::primary),
                    horizontal_rule(1),
                    scrollable(container(readme_body).padding(4)).height(Fill),
                ]
                .spacing(6)
                .padding(16),
            )
            .width(Fill)
            .height(Fill)
            .into()
        } else {
            container(text(t(lang, "patches-select-prompt")).size(13))
                .center(Fill)
                .into()
        };

        column![
            top,
            horizontal_rule(1),
            row![left, vertical_rule(1), right].height(Fill),
        ]
        .height(Fill)
        .into()
    }
}

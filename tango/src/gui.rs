use fluent_templates::Loader;

use crate::{audio, config, discord, fonts, game, graphics, i18n, input, patch, rom, save, session, stats, updater};

mod debug_window;
mod escape_window;
mod language_select;
mod main_view;
mod memoize;
mod patches_pane;
mod play_pane;
mod replay_dump_window;
mod replays_pane;
mod save_select_view;
mod save_view;
mod session_view;
mod settings_window;
mod steal_input_window;
mod ui_windows;
mod updater_window;
mod warning;
mod welcome;

pub struct Selection {
    pub game: &'static (dyn game::Game + Send + Sync),
    pub assets: Option<Box<dyn tango_dataview::rom::Assets + Send + Sync>>,
    pub save: save::ScannedSave,
    pub rom: Vec<u8>,
    pub patch: Option<(String, semver::Version, std::sync::Arc<patch::Version>)>,
    pub save_view_state: save_view::State,
}

impl Selection {
    pub fn new(
        game: &'static (dyn game::Game + Send + Sync),
        save: save::ScannedSave,
        patch: Option<(String, semver::Version, std::sync::Arc<patch::Version>)>,
        rom: Vec<u8>,
    ) -> Self {
        let assets = game
            .load_rom_assets(
                &rom,
                &save.save.as_raw_wram(),
                &patch
                    .as_ref()
                    .map(|(_, _, metadata)| metadata.rom_overrides.clone())
                    .unwrap_or_default(),
            )
            .ok();
        Self {
            game,
            assets,
            save,
            patch,
            rom,
            save_view_state: save_view::State::new(),
        }
    }

    pub fn reload_save(&mut self, saves_scanner: &mut save::Scanner) -> anyhow::Result<()> {
        let raw = std::fs::read(&self.save.path)?;
        self.save.save = self.game.parse_save(&raw)?;
        self.save_view_state = save_view::State::new();

        saves_scanner.modify(|saves_map| {
            let Some(saves) = saves_map.get_mut(&self.game) else {
                return;
            };
            let Some(save) = saves.iter_mut().find(|save| save.path == self.save.path) else {
                return;
            };
            *save = self.save.clone();
        });

        Ok(())
    }
}

#[derive(Clone)]
pub struct Scanners {
    pub roms: rom::Scanner,
    pub saves: save::Scanner,
    pub patches: patch::Scanner,
}

impl Scanners {
    pub fn new(config: &config::Config) -> Self {
        let roms_scanner = rom::Scanner::new();
        let saves_scanner = save::Scanner::new();
        let patches_scanner = patch::Scanner::new();

        let roms_path = config.roms_path();
        let saves_path = config.saves_path();
        let patches_path = config.patches_path();

        roms_scanner.rescan(move || Some(game::scan_roms(&roms_path)));
        saves_scanner.rescan(move || Some(save::scan_saves(&saves_path)));
        patches_scanner.rescan(move || Some(patch::scan(&patches_path).unwrap_or_default()));

        Self {
            roms: roms_scanner,
            saves: saves_scanner,
            patches: patches_scanner,
        }
    }
}

pub struct SharedRootState {
    pub offscreen_ui: graphics::offscreen::OffscreenUi,
    pub event_loop_proxy: winit::event_loop::EventLoopProxy<crate::WindowRequest>,
    pub config: std::sync::Arc<parking_lot::RwLock<config::Config>>,
    pub session: std::sync::Arc<parking_lot::Mutex<Option<session::Session>>>,
    pub clipboard: arboard::Clipboard,
    pub scanners: Scanners,
    pub audio_binder: audio::LateBinder,
    pub fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    pub emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    pub discord_client: discord::Client,
    pub font_families: fonts::FontFamilies,
    pub ui_windows: ui_windows::UiWindows,
    pub selection: Option<Selection>,
}

impl SharedRootState {
    pub fn send_window_request(&self, request: crate::WindowRequest) {
        let _ = self.event_loop_proxy.send_event(request);
    }
}

pub struct State {
    pub shared: SharedRootState,
    pub steal_input: Option<steal_input_window::State>,
    pub last_mouse_motion_time: Option<std::time::Instant>,
    main_view: main_view::State,
    show_escape_window: Option<escape_window::State>,
    show_settings: Option<settings_window::State>,
    themes: Themes,
    current_language: Option<unic_langid::LanguageIdentifier>,
    session_view: Option<session_view::State>,
    welcome: Option<welcome::State>,
    init_link_code: Option<String>,
}

impl State {
    pub fn new(
        event_loop_proxy: winit::event_loop::EventLoopProxy<crate::WindowRequest>,
        ctx: &egui::Context,
        show_updater: bool,
        config: std::sync::Arc<parking_lot::RwLock<config::Config>>,
        discord_client: discord::Client,
        audio_binder: audio::LateBinder,
        fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
        emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
        scanners: Scanners,
        init_link_code: Option<String>,
    ) -> Result<Self, anyhow::Error> {
        let offscreen_ui = graphics::offscreen::OffscreenUi::new();

        let font_families = fonts::FontFamilies::new();
        let font_definitions = font_families.resolve_definitions(config.read().language.clone());
        ctx.set_fonts(font_definitions.clone());
        offscreen_ui.ctx().set_fonts(font_definitions);

        ctx.style_mut(|style| {
            style.spacing.scroll = egui::style::ScrollStyle::solid();
            // animation_time > 0 causes panics as egui requires us to keep data around for closing animations
            // to see what i mean, open the settings window and close it with this set to anything other than 0
            // disabling the fade_out animation on specific windows does not appear to stop egui from attempting to rerender old data
            style.animation_time = 0.0;
        });

        // load previous selection
        let working_selection = crate::gui::save_select_view::Selection::resolve_from_config(&scanners, &config.read());

        let committed_selection = working_selection
            .as_ref()
            .and_then(|selection| selection.commit(&scanners, &config.read()));

        let main_view = main_view::State::new(working_selection, show_updater);

        Ok(Self {
            shared: SharedRootState {
                offscreen_ui,
                event_loop_proxy,
                config,
                session: std::sync::Arc::new(parking_lot::Mutex::new(None)),
                clipboard: arboard::Clipboard::new().unwrap(),
                scanners,
                audio_binder,
                fps_counter,
                emu_tps_counter,
                font_families,
                ui_windows: Default::default(),
                discord_client,
                selection: committed_selection,
            },
            last_mouse_motion_time: None,
            main_view,
            steal_input: None,
            show_settings: None,
            show_escape_window: None,
            session_view: None,
            welcome: None,
            themes: Themes {
                light: {
                    let mut visuals = egui::style::Visuals::light();
                    visuals.selection.bg_fill = egui::Color32::from_rgb(0x4c, 0xaf, 0x50);
                    visuals.selection.stroke.color = egui::Color32::BLACK;
                    visuals.faint_bg_color = egui::Color32::LIGHT_GRAY;
                    visuals
                },
                dark: {
                    let mut visuals = egui::style::Visuals::dark();
                    visuals.selection.bg_fill = egui::Color32::from_rgb(0x4c, 0xaf, 0x50);
                    visuals.selection.stroke.color = egui::Color32::WHITE;
                    visuals.faint_bg_color = egui::Color32::from_gray(14);
                    visuals.extreme_bg_color = egui::Color32::BLACK;
                    visuals
                },
            },
            current_language: None,
            init_link_code,
        })
    }
}

struct Themes {
    light: egui::style::Visuals,
    dark: egui::style::Visuals,
}

pub fn show(
    ctx: &egui::Context,
    config: &mut config::Config,
    input_state: &input::State,
    state: &mut State,
    updater: &updater::Updater,
) {
    {
        let mut session = state.shared.session.lock();
        if let Some(s) = session.as_ref() {
            if s.completed() {
                *session = None;
            }
        }
    }

    if state.current_language.as_ref() != Some(&config.language) {
        let language = config.language.clone();

        let font_definitions = state.shared.font_families.resolve_definitions(language);
        state.shared.offscreen_ui.ctx().set_fonts(font_definitions.clone());
        ctx.set_fonts(font_definitions);

        state.current_language = Some(config.language.clone());
        log::info!("language was changed to {}", state.current_language.as_ref().unwrap());
    }

    let is_dark = match config.theme {
        config::Theme::System => match dark_light::detect() {
            dark_light::Mode::Light => false,
            dark_light::Mode::Default | dark_light::Mode::Dark => true,
        },
        config::Theme::Light => false,
        config::Theme::Dark => true,
    };

    let visuals = if is_dark {
        state.themes.dark.clone()
    } else {
        state.themes.light.clone()
    };

    ctx.set_visuals(visuals.clone());
    state.shared.offscreen_ui.ctx().set_visuals(visuals);

    if config.nickname.is_none() {
        welcome::show(
            ctx,
            &state.shared,
            config,
            state.welcome.get_or_insert_with(welcome::State::new),
        );
        return;
    } else {
        state.welcome = None;
    }

    settings_window::show(
        ctx,
        &mut state.show_settings,
        &state.shared,
        config,
        &mut state.steal_input,
    );
    steal_input_window::show(ctx, &config.language, &mut state.steal_input);
    escape_window::show(
        ctx,
        &mut state.shared,
        &mut state.show_escape_window,
        &config.language,
        &mut state.show_settings,
    );

    // take ui windows to allow state to be passed to each window
    let mut ui_windows = std::mem::take(&mut state.shared.ui_windows);
    ui_windows.show(ctx, config, &mut state.shared);
    // store original ui windows, append any new ui windows
    std::mem::swap(&mut state.shared.ui_windows, &mut ui_windows);
    state.shared.ui_windows.merge(ui_windows);

    let session = state.shared.session.clone();
    let session_guard = session.lock();

    if let Some(session) = session_guard.as_ref() {
        let title = i18n::LOCALES.lookup(&config.language, "window-title.running").unwrap();
        let window_request = crate::WindowRequest::SetTitle(title);
        state.shared.send_window_request(window_request);

        session_view::show(
            ctx,
            config,
            &mut state.shared,
            input_state,
            session,
            &state.last_mouse_motion_time,
            &mut state.show_escape_window,
            state.session_view.get_or_insert_with(session_view::State::new),
        );
    } else {
        state.session_view = None;

        let title = i18n::LOCALES.lookup(&config.language, "window-title").unwrap();
        let window_request = crate::WindowRequest::SetTitle(title);
        state.shared.send_window_request(window_request);

        main_view::show(
            ctx,
            config,
            &mut state.shared,
            &mut state.show_settings,
            &mut state.main_view,
            &mut state.init_link_code,
            updater,
        );
    }
}

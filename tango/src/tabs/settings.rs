use crate::app::{STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION};
use crate::i18n::{t, SUPPORTED_LANGS};
use crate::widgets;
use crate::{config, input};
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{container, scrollable, text, Space};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use sweeten::widget::{button, column, pick_list, row, text_input};
use unic_langid::LanguageIdentifier;

/// A [`config::ThemeMode`] paired with its localized label, for the theme
/// pick_list — the picker renders options via `Display`, which can't reach
/// the language, so the label is resolved up front (mirrors
/// [`crate::i18n::LanguageChoice`]).
#[derive(Clone)]
struct ThemeChoice {
    mode: config::ThemeMode,
    label: String,
}

impl ThemeChoice {
    fn new(lang: &LanguageIdentifier, mode: config::ThemeMode) -> Self {
        let label = match mode {
            config::ThemeMode::Dark => t!(lang, "settings-theme-dark"),
            config::ThemeMode::Light => t!(lang, "settings-theme-light"),
        };
        Self { mode, label }
    }
}

impl PartialEq for ThemeChoice {
    fn eq(&self, other: &Self) -> bool {
        self.mode == other.mode
    }
}

impl std::fmt::Display for ThemeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    #[default]
    General,
    Graphics,
    Audio,
    Input,
    Netplay,
    About,
}

/// Settings tab UI state. Just the active sub-tab today — anything
/// configurable lives in `config::Config`, not here, since it has to
/// survive a restart.
#[derive(Default)]
pub struct State {
    pub active_tab: SettingsTab,
    /// When `Some(k)`, the next keyboard/gamepad event captured by
    /// the settings subscription is appended to the bindings list
    /// for `k`. UI displays a "press a key…" hint on the matching
    /// row.
    pub capture_target: Option<input::MappedKey>,
    /// Cached parsed markdown for the About tab. Lives here
    /// (rather than as a `static`) because `markdown::Content`
    /// is `!Sync`.
    pub about: AboutMarkdown,
}

#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(SettingsTab),
    LanguageSelected(LanguageIdentifier),
    NicknameChanged(String),
    ToggleStreamerMode(bool),
    MatchmakingEndpointChanged(String),
    /// Netplay frame-delay slider moved. Persisted to `config.frame_delay`;
    /// it's this side's local presentation lag, applied at the next match start
    /// (or live via the in-match footer slider).
    FrameDelayChanged(u32),
    PatchRepoChanged(String),
    TogglePatchAutoupdate(bool),
    VideoFilterChanged(String),
    ToggleFractionalScaling(bool),
    ToggleHideEmulatorBorder(bool),
    ToggleFullscreen(bool),
    ResolutionChanged(ResolutionChoice),
    UiScaleChanged(UiScaleChoice),
    ToggleEnableUpdater(bool),
    ToggleAllowPrereleaseUpgrades(bool),
    VolumeChanged(f32),
    /// User clicked "Update Now" on the About panel. App's
    /// settings handler calls `updater.finish_update()` which
    /// hands off to the installer + exits the process.
    UpdateNow,
    ThemeChanged(config::ThemeMode),
    /// User clicked "Add binding" for `k`. The next key/button
    /// event captured by the settings subscription is appended.
    BindingCaptureStart(input::MappedKey),
    /// User clicked × to abort the current capture.
    BindingCaptureCancel,
    /// Settings subscription saw an input event while
    /// `capture_target.is_some()`. Append + clear the target.
    BindingCaptured(input::PhysicalInput),
    /// User clicked Remove on the Nth binding for `k`.
    BindingRemove(input::MappedKey, usize),
    /// User clicked Reset to defaults.
    BindingsReset,
    /// User clicked an external link in the About panel. Side
    /// effect only — opens the URL via `open::that` and returns
    /// `None` from update. `String` (not `&'static str`) so the
    /// iced markdown widget can pass its parsed `Uri` straight
    /// in via `.map(Message::OpenUrl)`.
    OpenUrl(String),
}

/// Messages the settings panel emits that affect persisted
/// config — used by App's update handler to apply to its
/// `config::Config` and call `persist_config()`. The TabSelected
/// variant is handled internally and never appears here.
#[derive(Debug, Clone)]
pub enum ConfigChange {
    Language(LanguageIdentifier),
    Nickname(String),
    StreamerMode(bool),
    MatchmakingEndpoint(String),
    FrameDelay(u32),
    PatchRepo(String),
    PatchAutoupdate(bool),
    VideoFilter(String),
    FractionalScaling(bool),
    HideEmulatorBorder(bool),
    Fullscreen(bool),
    Resolution(f32, f32),
    UiScale(f32),
    EnableUpdater(bool),
    AllowPrereleaseUpgrades(bool),
    Volume(f32),
    Theme(config::ThemeMode),
    AddInputBinding(input::MappedKey, input::PhysicalInput),
    RemoveInputBinding(input::MappedKey, usize),
    ResetInputBindings,
}

impl State {
    /// Apply a settings message to local UI state. Returns
    /// `Some(ConfigChange)` for variants the caller needs to persist
    /// to disk; `None` for purely-local navigation like TabSelected.
    pub fn update(&mut self, msg: Message) -> Option<ConfigChange> {
        match msg {
            Message::TabSelected(t) => {
                self.active_tab = t;
                None
            }
            Message::LanguageSelected(l) => Some(ConfigChange::Language(l)),
            Message::NicknameChanged(s) => Some(ConfigChange::Nickname(s)),
            Message::ToggleStreamerMode(b) => Some(ConfigChange::StreamerMode(b)),
            Message::MatchmakingEndpointChanged(s) => Some(ConfigChange::MatchmakingEndpoint(s)),
            Message::FrameDelayChanged(v) => Some(ConfigChange::FrameDelay(v)),
            Message::PatchRepoChanged(s) => Some(ConfigChange::PatchRepo(s)),
            Message::TogglePatchAutoupdate(b) => Some(ConfigChange::PatchAutoupdate(b)),
            Message::VideoFilterChanged(s) => Some(ConfigChange::VideoFilter(s)),
            Message::ToggleFractionalScaling(b) => Some(ConfigChange::FractionalScaling(b)),
            Message::ToggleHideEmulatorBorder(b) => Some(ConfigChange::HideEmulatorBorder(b)),
            Message::ToggleFullscreen(b) => Some(ConfigChange::Fullscreen(b)),
            Message::ResolutionChanged(c) => Some(ConfigChange::Resolution(c.width, c.height)),
            Message::UiScaleChanged(c) => Some(ConfigChange::UiScale(c.0)),
            Message::ToggleEnableUpdater(b) => Some(ConfigChange::EnableUpdater(b)),
            Message::ToggleAllowPrereleaseUpgrades(b) => Some(ConfigChange::AllowPrereleaseUpgrades(b)),
            Message::VolumeChanged(v) => Some(ConfigChange::Volume(v)),
            // App handles UpdateNow as a top-level effect — it
            // calls `updater.finish_update()` which exits the
            // process on success. Nothing to fold into config.
            Message::UpdateNow => None,
            Message::ThemeChanged(t) => Some(ConfigChange::Theme(t)),
            Message::BindingCaptureStart(k) => {
                self.capture_target = Some(k);
                None
            }
            Message::BindingCaptureCancel => {
                self.capture_target = None;
                None
            }
            Message::BindingCaptured(p) => {
                let Some(target) = self.capture_target.take() else {
                    return None;
                };
                Some(ConfigChange::AddInputBinding(target, p))
            }
            Message::BindingRemove(k, idx) => Some(ConfigChange::RemoveInputBinding(k, idx)),
            Message::BindingsReset => Some(ConfigChange::ResetInputBindings),
            Message::OpenUrl(url) => {
                if let Err(e) = open::that(&url) {
                    log::warn!("open url {url}: {e}");
                }
                None
            }
        }
    }
}

pub fn view<'a>(
    lang: &'a LanguageIdentifier,
    config: &'a config::Config,
    state: &'a State,
    updater_status: crate::updater::Status,
) -> Element<'a, Message> {
    let active = state.active_tab;
    // Vertical tab strip on the left; selected pane on the right.
    // Pill style matches the global top nav + save_view sub-nav
    // so every tab affordance in the app reads as the same
    // widget family.
    let side_btn = |icon: Icon, label: String, tab: SettingsTab| {
        button(row![icon.widget(), text(label)].spacing(8).align_y(Alignment::Center))
            .padding(STANDARD_PADDING)
            .width(Fill)
            .style(widgets::pill_tab_style(tab == active))
            .on_press(Message::TabSelected(tab))
    };
    let sidebar = container(
        column![
            side_btn(
                Icon::SlidersHorizontal,
                t!(lang, "settings-section-general"),
                SettingsTab::General
            ),
            side_btn(
                Icon::Monitor,
                t!(lang, "settings-section-graphics"),
                SettingsTab::Graphics
            ),
            side_btn(Icon::Volume2, t!(lang, "settings-section-audio"), SettingsTab::Audio),
            side_btn(Icon::Gamepad2, t!(lang, "settings-section-input"), SettingsTab::Input),
            side_btn(Icon::Globe, t!(lang, "settings-section-netplay"), SettingsTab::Netplay),
            side_btn(Icon::Info, t!(lang, "settings-section-about"), SettingsTab::About),
        ]
        .spacing(4)
        .padding(8),
    )
    .width(Length::Fixed(160.0))
    .height(Fill)
    .style(widgets::pane);

    let body: Element<'a, Message> = match active {
        SettingsTab::General => settings_general(lang, config),
        SettingsTab::Graphics => settings_graphics(lang, config),
        SettingsTab::Audio => settings_audio(lang, config),
        SettingsTab::Input => settings_input(lang, config, state),
        SettingsTab::Netplay => settings_netplay(lang, config),
        SettingsTab::About => settings_about(lang, config, &state.about, updater_status),
        // The status arg is consumed by About's call here; iced
        // discards the unused-on-other-tabs branches at runtime
        // so no double-clone is needed.
    };

    // Scrollable wraps the body once at the dispatch layer —
    // each settings_* pane returns a plain column with its own
    // inner padding so the scrollbar hugs the right edge.
    let body_wrap = container(scrollable(body).width(Fill).height(Fill))
        .width(Fill)
        .height(Fill)
        .style(widgets::pane);

    let root = row![sidebar, body_wrap]
        .spacing(widgets::PANE_GAP)
        .padding(widgets::PANE_GAP)
        .width(Fill)
        .height(Fill);

    // Binding capture: while the user is rebinding a key, wrap the
    // settings UI in `InputCapture` so both keyboard and gamepad
    // events flow through one synchronous path. Without the wrapper
    // we'd be idle and SDL3's pump (main-thread only) wouldn't get
    // drained. The callback publishes a `BindingCaptured` for the
    // first key press, button press, or axis-past-threshold event.
    if state.capture_target.is_some() {
        crate::input_capture::InputCapture::new(root, |input| {
            let captured = match input {
                crate::input_capture::Input::Keyboard(iced::keyboard::Event::KeyPressed { physical_key, .. }) => {
                    Some(input::PhysicalInput::Key(input::KeyPhysical(*physical_key)))
                }
                crate::input_capture::Input::Keyboard(_) => None,
                crate::input_capture::Input::Gamepad(ev) => match *ev {
                    crate::gamepad::GamepadEvent::ButtonDown(b) => {
                        Some(input::PhysicalInput::Button(input::GamepadButton::from_sdl3(b)))
                    }
                    crate::gamepad::GamepadEvent::AxisMotion { axis, value } => (value.abs() > input::AXIS_THRESHOLD)
                        .then(|| input::PhysicalInput::Axis {
                            axis,
                            dir: if value > 0.0 {
                                input::AxisDir::Positive
                            } else {
                                input::AxisDir::Negative
                            },
                        }),
                    _ => None,
                },
            };
            captured.map(Message::BindingCaptured)
        })
        .into()
    } else {
        root.into()
    }
}

/// Generic over Message so the welcome screen can use it too with its
/// own Message type.
pub fn labeled<'a, M: Clone + 'a>(label: String, ctrl: impl Into<Element<'a, M>>) -> Element<'a, M> {
    column![
        text(label).size(TEXT_CAPTION).style(widgets::muted_text_style),
        ctrl.into(),
    ]
    .spacing(4)
    .into()
}

fn settings_general<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
    column![
        labeled::<Message>(
            t!(lang, "settings-nickname"),
            text_input("", config.nickname.as_deref().unwrap_or(""))
                .on_input(Message::NicknameChanged)
                .padding(STANDARD_PADDING)
                .width(Length::Fixed(240.0))
                .style(widgets::chunky_text_input),
        ),
        labeled::<Message>(t!(lang, "settings-language"), {
            // Build the picker options as `LanguageChoice`
            // wrappers — they Display the endonym from each
            // locale's `LANGUAGE` Fluent key instead of the
            // bare locale code.
            let options: Vec<crate::i18n::LanguageChoice> = SUPPORTED_LANGS
                .iter()
                .map(|id| crate::i18n::LanguageChoice::new(id.clone()))
                .collect();
            let selected = options.iter().find(|c| c.id == config.language).cloned();
            pick_list(options, selected, |c: crate::i18n::LanguageChoice| {
                Message::LanguageSelected(c.id)
            })
            .padding(STANDARD_PADDING)
            .style(widgets::chunky_pick_list)
        },),
        labeled::<Message>(t!(lang, "settings-theme"), {
            let options = vec![
                ThemeChoice::new(lang, config::ThemeMode::Dark),
                ThemeChoice::new(lang, config::ThemeMode::Light),
            ];
            let selected = options.iter().find(|c| c.mode == config.theme).cloned();
            pick_list(options, selected, |c: ThemeChoice| Message::ThemeChanged(c.mode))
                .padding(STANDARD_PADDING)
                .style(widgets::chunky_pick_list)
        }),
        iced::widget::checkbox(config.streamer_mode)
            .label(t!(lang, "settings-streamer-mode"))
            .on_toggle(Message::ToggleStreamerMode)
            .style(widgets::chunky_checkbox),
        labeled::<Message>(
            t!(lang, "settings-patch-repo"),
            text_input("", &config.patch_repo)
                .on_input(Message::PatchRepoChanged)
                .padding(STANDARD_PADDING)
                .width(Length::Fixed(480.0))
                .style(widgets::chunky_text_input),
        ),
        iced::widget::checkbox(config.enable_patch_autoupdate)
            .label(t!(lang, "settings-enable-patch-autoupdate"))
            .on_toggle(Message::TogglePatchAutoupdate)
            .style(widgets::chunky_checkbox),
        iced::widget::checkbox(config.enable_updater)
            .label(t!(lang, "settings-enable-updater"))
            .on_toggle(Message::ToggleEnableUpdater)
            .style(widgets::chunky_checkbox),
        iced::widget::checkbox(config.allow_prerelease_upgrades)
            .label(t!(lang, "settings-allow-prerelease-upgrades"))
            .on_toggle(Message::ToggleAllowPrereleaseUpgrades)
            .style(widgets::chunky_checkbox),
    ]
    .spacing(14)
    .padding(widgets::PANE_PADDING)
    .into()
}

fn settings_audio<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
    column![labeled::<Message>(
        t!(lang, "settings-volume"),
        row![
            // Bounded slider width — Fill would stretch all the way
            // across the pane, which looks silly for a volume bar.
            container(iced::widget::slider(0.0..=1.0, config.volume, Message::VolumeChanged).step(0.01))
                .width(Length::Fixed(220.0)),
            // Compact percent readout next to the track so the user
            // can see exactly where the slider sits.
            text(format!("{:.0}%", config.volume * 100.0)).size(TEXT_CAPTION),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )]
    .spacing(14)
    .padding(widgets::PANE_PADDING)
    .into()
}

/// Pick-list adapter for the video filter choice. `key` is the
/// `config.video_filter` value (`""`, `"hq2x"`, …) and `display`
/// is the human-readable label the dropdown shows.
#[derive(Clone, PartialEq, Eq)]
struct VideoFilterChoice {
    key: String,
    display: String,
}
impl std::fmt::Display for VideoFilterChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}

/// Standard windowed resolutions surfaced in the graphics settings
/// pick-list. Selecting one resizes the live window and updates
/// `config.last_window_size`. Skips anything smaller than the
/// min_size enforced in `main.rs` (800×600).
const STANDARD_RESOLUTIONS: &[(u32, u32)] = &[
    (800, 600),
    (1024, 768),
    (1280, 720),
    (1280, 800),
    (1366, 768),
    (1440, 900),
    (1600, 900),
    (1680, 1050),
    (1920, 1080),
    (2560, 1440),
    (3840, 2160),
];

/// Pick-list adapter for a window resolution. PartialEq is exact
/// f32 — fine since the values come straight from
/// `STANDARD_RESOLUTIONS` constants and matched by equality.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolutionChoice {
    pub width: f32,
    pub height: f32,
}
impl Eq for ResolutionChoice {}
impl std::fmt::Display for ResolutionChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}×{}", self.width as u32, self.height as u32)
    }
}

/// UI scale presets surfaced in the graphics-settings pick-list.
/// Multiplies on top of the OS DPI scale.
const UI_SCALE_PRESETS: &[f32] = &[0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

/// Pick-list adapter for the UI scale multiplier. `PartialEq` is
/// exact on f32, which is fine since values come from the
/// `UI_SCALE_PRESETS` constants and matched by equality.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiScaleChoice(pub f32);
impl Eq for UiScaleChoice {}
impl std::fmt::Display for UiScaleChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Print integer percent for the round 25%-step presets,
        // 25%-step decimals would just clutter the dropdown.
        write!(f, "{}%", (self.0 * 100.0).round() as u32)
    }
}

fn settings_graphics<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
    let resolution_options: Vec<ResolutionChoice> = STANDARD_RESOLUTIONS
        .iter()
        .map(|(w, h)| ResolutionChoice {
            width: *w as f32,
            height: *h as f32,
        })
        .collect();
    // Match the current windowed size against the preset list so
    // the picker shows a selected value when it lines up exactly.
    // No match (custom drag-resized size) renders as blank.
    let current_size = config
        .last_window_size
        .map(|(w, h)| ResolutionChoice { width: w, height: h });
    let selected_resolution = current_size.and_then(|cur| resolution_options.iter().find(|o| **o == cur).copied());
    // Disable the window-size picker while fullscreen is on:
    // picking a sub-monitor size while fullscreen is meaningless
    // (the live window stays at monitor resolution). Render the
    // shared disabled-dropdown placeholder so it reads as the
    // same control family as the live picker.
    let window_size_picker: Element<'a, Message> = if config.fullscreen {
        let label = selected_resolution.map(|r| r.to_string()).unwrap_or_else(|| "—".into());
        widgets::disabled_pick_list(label).into()
    } else {
        pick_list(resolution_options, selected_resolution, Message::ResolutionChanged)
            .padding(STANDARD_PADDING)
            .style(widgets::chunky_pick_list)
            .into()
    };
    let ui_scale_options: Vec<UiScaleChoice> = UI_SCALE_PRESETS.iter().copied().map(UiScaleChoice).collect();
    let selected_ui_scale = ui_scale_options
        .iter()
        .find(|c| (c.0 - config.ui_scale).abs() < f32::EPSILON)
        .copied();
    column![
        // Label hovers over just the dropdown; the row beneath
        // it centers the fullscreen checkbox with the dropdown
        // box itself (not the dropdown+label column).
        column![
            text(t!(lang, "settings-window-size"))
                .size(TEXT_CAPTION)
                .style(widgets::muted_text_style),
            row![
                window_size_picker,
                iced::widget::checkbox(config.fullscreen)
                    .label(t!(lang, "settings-fullscreen"))
                    .on_toggle(Message::ToggleFullscreen)
                    .style(widgets::chunky_checkbox),
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        ]
        .spacing(4),
        labeled::<Message>(
            t!(lang, "settings-ui-scale"),
            pick_list(ui_scale_options, selected_ui_scale, Message::UiScaleChanged)
                .padding(STANDARD_PADDING)
                .style(widgets::chunky_pick_list),
        ),
        labeled::<Message>(t!(lang, "settings-video-filter"), {
            let options: Vec<VideoFilterChoice> = crate::video::effects::EFFECTS
                .iter()
                .map(|effect| VideoFilterChoice {
                    key: effect.id.into(),
                    display: effect.name.into(),
                })
                .collect();
            let selected = options.iter().find(|c| c.key == config.video_filter).cloned();
            pick_list(options, selected, |c: VideoFilterChoice| {
                Message::VideoFilterChanged(c.key)
            })
            .padding(STANDARD_PADDING)
            .style(widgets::chunky_pick_list)
        },),
        iced::widget::checkbox(config.fractional_scaling)
            .label(t!(lang, "settings-fractional-scaling"))
            .on_toggle(Message::ToggleFractionalScaling)
            .style(widgets::chunky_checkbox),
        iced::widget::checkbox(config.hide_emulator_border)
            .label(t!(lang, "settings-hide-emulator-border"))
            .on_toggle(Message::ToggleHideEmulatorBorder)
            .style(widgets::chunky_checkbox),
    ]
    .spacing(14)
    .padding(widgets::PANE_PADDING)
    .into()
}

fn settings_netplay<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
    // Clamp the displayed value so a stale out-of-range config still lands the
    // slider handle inside the track.
    let frame_delay = config
        .frame_delay
        .clamp(tango_pvp::battle::MIN_FRAME_DELAY, tango_pvp::battle::MAX_FRAME_DELAY);
    column![
        labeled::<Message>(
            t!(lang, "settings-matchmaking-endpoint"),
            text_input("", &config.matchmaking_endpoint)
                .on_input(Message::MatchmakingEndpointChanged)
                .padding(STANDARD_PADDING)
                .width(Length::Fixed(480.0))
                .style(widgets::chunky_text_input),
        ),
        labeled::<Message>(
            t!(lang, "settings-netplay-frame-delay"),
            row![
                container(iced::widget::slider(
                    tango_pvp::battle::MIN_FRAME_DELAY..=tango_pvp::battle::MAX_FRAME_DELAY,
                    frame_delay,
                    Message::FrameDelayChanged,
                ))
                .width(Length::Fixed(220.0)),
                // Compact numeric readout next to the track, mirroring the
                // volume slider's percent display.
                text(format!("{}", frame_delay)).size(TEXT_CAPTION),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ),
    ]
    .spacing(14)
    .padding(widgets::PANE_PADDING)
    .into()
}

fn settings_input<'a>(
    lang: &'a LanguageIdentifier,
    config: &'a config::Config,
    state: &'a State,
) -> Element<'a, Message> {
    let mut col = column![].spacing(2);
    let slots = config.input_mapping.slots();
    for (idx, (k, bindings)) in slots.iter().enumerate() {
        let k = *k;
        let label = match k {
            input::MappedKey::Up => t!(lang, "input-key-up"),
            input::MappedKey::Down => t!(lang, "input-key-down"),
            input::MappedKey::Left => t!(lang, "input-key-left"),
            input::MappedKey::Right => t!(lang, "input-key-right"),
            input::MappedKey::A => t!(lang, "input-key-a"),
            input::MappedKey::B => t!(lang, "input-key-b"),
            input::MappedKey::L => t!(lang, "input-key-l"),
            input::MappedKey::R => t!(lang, "input-key-r"),
            input::MappedKey::Start => t!(lang, "input-key-start"),
            input::MappedKey::Select => t!(lang, "input-key-select"),
            input::MappedKey::SpeedUp => t!(lang, "input-key-speed-up"),
        };
        let bindings: &Vec<input::PhysicalInput> = bindings;
        // Capture-mode hint for the currently-targeted slot;
        // matches show "press a key…" + a cancel chip instead
        // of the usual Add button.
        let action: Element<'a, Message> = if state.capture_target == Some(k) {
            row![
                text(t!(lang, "settings-input-press-key"))
                    .size(TEXT_BODY)
                    .style(|theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(theme.palette().primary),
                    }),
                widgets::icon_button(
                    Icon::X,
                    t!(lang, "save-action-cancel"),
                    Message::BindingCaptureCancel,
                    STANDARD_PADDING,
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            // Icon-only + tooltip — "Add binding" text on every
            // row was visual noise; the + glyph is universal.
            widgets::icon_button(
                Icon::Plus,
                t!(lang, "settings-input-add"),
                Message::BindingCaptureStart(k),
                STANDARD_PADDING,
            )
        };
        let mut chips = row![].spacing(6).align_y(iced::Alignment::Center);
        for (i, b) in bindings.iter().enumerate() {
            chips = chips.push(binding_chip(lang, b, k, i));
        }
        // Wrap chips onto new lines when a key has more bindings
        // than fit on one row. The Fill container bounds the wrap
        // width (sweeten's `Wrapping` is Shrink by default) and
        // pushes the Add/cancel action to the right edge — same
        // pattern as the save_view tab strip.
        let row_inner = row![
            container(text(label).size(TEXT_BODY)).width(Length::Fixed(140.0)),
            container(chips.wrap()).width(Fill),
            action,
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center);
        // Zebra-stripe the rows so the eye can scan them as a
        // table without a chunky border on every row. Same
        // helper save_view uses for its chip table.
        col = col.push(
            container(row_inner)
                .padding([8, 12])
                .width(Fill)
                .style(widgets::zebra_row(idx)),
        );
    }
    // Reset sits in a row padded identically to the table rows
    // so its right edge aligns with the per-row Add buttons.
    let reset = widgets::labeled_icon_button(
        Icon::RefreshCw,
        t!(lang, "settings-input-reset"),
        Message::BindingsReset,
        STANDARD_PADDING,
        widgets::neutral,
    );
    col = col
        .push(Space::new().height(12))
        .push(container(row![horizontal_space(), reset]).padding([0, 12]).width(Fill));
    // Table is flush left/right against the scrollable's
    // visible edges — rows carry their own internal padding so
    // the zebra stripes extend full width without any outer
    // column padding. Vertical padding still applies so the
    // first/last row don't slam the scanline above/below.
    col.padding(iced::Padding {
        top: 0.0,
        right: 0.0,
        bottom: 20.0,
        left: 0.0,
    })
    .into()
}

fn binding_chip<'a>(
    lang: &'a LanguageIdentifier,
    binding: &input::PhysicalInput,
    key: input::MappedKey,
    idx: usize,
) -> Element<'a, Message> {
    let (kind, label) = input::describe(lang, binding);
    let kind_glyph = match kind {
        input::DescribeKind::Keyboard => Icon::Keyboard,
        input::DescribeKind::Gamepad => Icon::Gamepad2,
    };
    // Primary-tinted rounded pill matching the rest of the
    // app's chip + badge chrome. × button is subdued (neutral
    // chrome) so it doesn't shout against the small label.
    container(
        row![
            kind_glyph.widget().size(TEXT_BODY),
            text(label).size(TEXT_BODY),
            button(Icon::X.widget().size(TEXT_CAPTION))
                .padding([2, 4])
                .style(widgets::neutral)
                .on_press(Message::BindingRemove(key, idx)),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
    )
    .padding([3, 8])
    .style(|theme: &iced::Theme| {
        let primary = theme.palette().primary;
        iced::widget::container::Style {
            background: Some(iced::Background::Color(iced::Color { a: 0.12, ..primary })),
            text_color: Some(theme.palette().text),
            border: iced::Border {
                radius: 999.0.into(),
                width: 1.0,
                color: iced::Color { a: 0.45, ..primary },
            },
            ..Default::default()
        }
    })
    .into()
}

/// Holder for the parsed-once markdown Content. Tab state field
/// because `markdown::Content` is `!Sync` (interior mutability
/// for incremental parsing) and the parsed Items must outlive
/// the `Element<'_>` that `markdown::view` borrows from.
/// `OnceCell` so the parse only runs the first time the About
/// tab renders.
#[derive(Default)]
pub struct AboutMarkdown(std::cell::OnceCell<iced::widget::markdown::Content>);

impl AboutMarkdown {
    fn content(&self) -> &iced::widget::markdown::Content {
        self.0.get_or_init(|| {
            iced::widget::markdown::Content::parse(&format!(
                "# Tango {}\n{}",
                env!("CARGO_PKG_VERSION"),
                include_str!("../../../CREDITS.md")
            ))
        })
    }
}

fn settings_about<'a>(
    lang: &'a LanguageIdentifier,
    config: &'a config::Config,
    about: &'a AboutMarkdown,
    updater_status: crate::updater::Status,
) -> Element<'a, Message> {
    use iced::widget::image::{Handle, Image};
    use iced::widget::markdown;
    use std::sync::LazyLock;

    static EMBLEM: LazyLock<Handle> = LazyLock::new(|| {
        let raw: &'static [u8] = include_bytes!("../emblem.png");
        Handle::from_bytes(raw)
    });

    let _ = lang; // about screen is English-only, matching legacy.

    let emblem = iced::widget::container(Image::new(EMBLEM.clone()).width(Length::Fixed(200.0)))
        .width(Fill)
        .align_x(iced::alignment::Horizontal::Center);

    // Pull the live Theme from the same `crate::theme::theme_for` the
    // App's theme callback uses — keeps link color in sync with
    // the rest of the app instead of pinning to DARK + TANGO_GREEN
    // by hand. `Settings::from(&Theme)` defaults to text-size 16,
    // so wrap to also pin the app's body text size.
    let theme = crate::theme::theme_for(config);
    let style = crate::theme::markdown_style(&theme);
    let settings = markdown::Settings::with_text_size(TEXT_BODY, style);
    let body: Element<'a, Message> = markdown::view(about.content().items(), settings).map(Message::OpenUrl);

    column![
        emblem,
        body,
        updater_section(lang, updater_status, config.enable_updater)
    ]
    .spacing(12)
    // Symmetric inner padding so the emblem doesn't rub
    // the nav scanline at the top, the updater section
    // breathes at the page end, and link text doesn't slam
    // the left edge.
    .padding(widgets::PANE_PADDING)
    .into()
}

/// Bottom-of-About updater status panel. Current version
/// vs. latest, a one-line status, and an Update Now button
/// when a download is ready. Hidden release notes/download
/// progress until they're actually relevant.
fn updater_section<'a>(
    lang: &'a LanguageIdentifier,
    status: crate::updater::Status,
    enable_updater: bool,
) -> Element<'a, Message> {
    use crate::updater::Status as S;

    let current = env!("CARGO_PKG_VERSION");
    let is_ready = matches!(status, S::ReadyToUpdate { .. });

    let latest_label: String = match &status {
        S::UpToDate { release: None } | S::UpToDate { release: Some(None) } => t!(lang, "updater-loading"),
        S::UpToDate { release: Some(Some(r)) } => t!(lang, "updater-up-to-date", version = r.version.to_string()),
        S::UpdateAvailable { release: r } | S::Downloading { release: r, .. } | S::ReadyToUpdate { release: r } => {
            format!("v{}", r.version)
        }
    };

    let status_line: Option<Element<'a, Message>> = match &status {
        S::Downloading { current, total, .. } => {
            let pct = if *total > 0 {
                (*current as f32 / *total as f32 * 100.0).round() as u32
            } else {
                0
            };
            Some(
                text(t!(lang, "updater-downloading", pct = pct as i64))
                    .size(TEXT_CAPTION)
                    .style(widgets::muted_text_style)
                    .into(),
            )
        }
        S::ReadyToUpdate { .. } => Some(
            text(t!(lang, "updater-ready-to-update"))
                .size(TEXT_CAPTION)
                .style(widgets::muted_text_style)
                .into(),
        ),
        _ => None,
    };

    let action: Option<Element<'a, Message>> = is_ready.then(|| {
        widgets::labeled_icon_button(
            Icon::Download,
            t!(lang, "updater-update-now"),
            Message::UpdateNow,
            STANDARD_PADDING,
            widgets::primary_button,
        )
    });

    // The "latest version" readout only makes sense when the updater is on; with
    // it disabled we never fetch a release, so show just the current version and
    // drop the latest-version line.
    let mut version_row =
        row![text(t!(lang, "updater-current-version", version = format!("v{current}"))).size(TEXT_CAPTION),]
            .spacing(8)
            .align_y(Alignment::Center);
    if enable_updater {
        version_row = version_row.push(horizontal_space());
        version_row =
            version_row.push(text(t!(lang, "updater-latest-version", version = latest_label)).size(TEXT_CAPTION));
    }

    let mut col = column![iced::widget::rule::horizontal(1), version_row].spacing(8);
    match (status_line, action) {
        // Update ready: the button sits to the right of the status message on the
        // same row, rather than on its own row below it.
        (Some(s), Some(a)) => {
            col = col.push(row![s, horizontal_space(), a].spacing(8).align_y(Alignment::Center));
        }
        (Some(s), None) => col = col.push(s),
        (None, Some(a)) => col = col.push(row![horizontal_space(), a].spacing(8)),
        (None, None) => {}
    }
    col.into()
}

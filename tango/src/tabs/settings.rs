use crate::i18n::{t, SUPPORTED_LANGS};
use crate::style::{self, STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION};
use crate::widgets;
use crate::widgets::{option_row, Choice};
use crate::{config, input};
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{button, container, scrollable, text, Space};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use sweeten::widget::{column, row, text_input};
use unic_langid::LanguageIdentifier;

/// A [`config::ThemeMode`] as a pick_list [`Choice`], labeled in the
/// UI language (mirrors [`crate::i18n::LanguageChoice`]).
fn theme_choice(lang: &LanguageIdentifier, mode: config::ThemeMode) -> Choice<config::ThemeMode> {
    Choice::new(
        mode,
        match mode {
            config::ThemeMode::Dark => t!(lang, "settings-theme-dark"),
            config::ThemeMode::Light => t!(lang, "settings-theme-light"),
        },
    )
}

/// A [`config::AccentColor`] as a pick_list [`Choice`], labeled in
/// the UI language.
fn accent_choice(lang: &LanguageIdentifier, accent: config::AccentColor) -> Choice<config::AccentColor> {
    Choice::new(
        accent,
        match accent {
            config::AccentColor::TangoGreen => t!(lang, "settings-accent-tango-green"),
            config::AccentColor::MegaManBlue => t!(lang, "settings-accent-megaman-blue"),
            config::AccentColor::ProtoManRed => t!(lang, "settings-accent-protoman-red"),
            config::AccentColor::RollPink => t!(lang, "settings-accent-roll-pink"),
            config::AccentColor::BassGold => t!(lang, "settings-accent-bass-gold"),
        },
    )
}

/// A [`config::RelayMode`] as a pick_list [`Choice`], labeled in the
/// UI language.
fn relay_mode_choice(lang: &LanguageIdentifier, mode: config::RelayMode) -> Choice<config::RelayMode> {
    Choice::new(
        mode,
        match mode {
            config::RelayMode::Auto => t!(lang, "settings-use-relay-auto"),
            config::RelayMode::Always => t!(lang, "settings-use-relay-always"),
            config::RelayMode::Never => t!(lang, "settings-use-relay-never"),
        },
    )
}

/// Faint accent hairline — the divider register everywhere in the
/// app (group headers here, the About footer). Never a stock
/// `rule`, whose full-opacity gray line reads as dialog chrome.
fn accent_hairline<'a>() -> Element<'a, Message> {
    container(Space::new().width(Fill).height(Length::Fixed(1.0)))
        .style(|theme: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(iced::Color {
                a: 0.25,
                ..theme.palette().primary
            })),
            ..Default::default()
        })
        .into()
}

/// A titled cluster of related rows inside a settings pane. The
/// flat column-of-everything panes read as a mishmash once they
/// pass a handful of controls; headers + tight intra-group spacing
/// (vs. the wide inter-group gap the caller sets) give the pane a
/// scannable shape. The header is a caption in the accent color
/// with a hairline rule running off it — text hierarchy, not more
/// panel chrome, since these panes also render inside the
/// in-session settings modal where a framed card per group would
/// fight the modal's own frame.
fn settings_group<'a>(title: String, rows: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    // Accent tick ahead of the title — the small "this is a
    // section" mark game menus hang their headers on.
    let tick = container(Space::new().width(Length::Fixed(3.0)).height(Length::Fixed(11.0))).style(
        |theme: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(theme.palette().primary)),
            ..Default::default()
        },
    );
    // Uppercased in code so every locale's Fluent string stays
    // natural-case for reuse elsewhere; CJK passes through
    // unchanged.
    let header = row![
        tick,
        text(title.to_uppercase())
            .size(TEXT_CAPTION)
            .style(widgets::primary_text_style),
        accent_hairline(),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    // Rows sit tight (they're full-width plates that light up on
    // hover — see [`widgets::option_row`]); the wide gap between
    // GROUPS is the caller's spacing.
    column![header, column(rows).spacing(2)].spacing(8).into()
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
    /// for `k`. UI displays a "press a key…" hint on the console
    /// screen.
    pub capture_target: Option<input::MappedKey>,
    /// The console key whose bindings the Input pane's screen is
    /// showing — set by clicking a key on the drawn GBA. `None`
    /// (fresh state) shows a "click a button" hint instead.
    pub selected_key: Option<input::MappedKey>,
    /// Cached parsed markdown for the About tab. Lives here
    /// (rather than as a `static`) because `markdown::Content`
    /// is `!Sync`.
    pub about: AboutMarkdown,
    /// Entrance restarted on each section switch — the section
    /// pane slides in vertically, mirroring the sidebar's order
    /// (moving down the list enters from below, moving up from
    /// above) the same way the top tabs mirror their horizontal
    /// order. Owned here (not by the App's screen-enter
    /// machinery) so it also plays inside the in-session settings
    /// modal.
    pub pane_enter: crate::anim::Enter,
    /// Starting vertical offset for `pane_enter` — sign picked
    /// from the direction of travel along the sidebar. The
    /// `Default` of 0.0 is never seen: a direction is always set
    /// before the first entrance starts.
    pub pane_enter_dy: f32,
    /// Live held keys/buttons while the Input pane is on screen out
    /// of a session, fed by the pane's own `InputCapture` wrapper via
    /// [`Message::LiveInput`] — pressing a bound key/button lights up
    /// its chip. Inside the in-session settings modal the highlight
    /// reads the session's `input_held` instead (see [`view`]'s
    /// `session_held`), so this stays untouched there. Reset whenever
    /// the pane leaves the screen (its wrapper unmounts mid-hold and
    /// the releases would never arrive).
    pub held: input::HeldState,
}

#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(SettingsTab),
    LanguageSelected(LanguageIdentifier),
    NicknameChanged(String),
    ToggleStreamerMode(bool),
    MatchmakingEndpointChanged(String),
    /// Relay (TURN) usage policy picked: auto / always / never.
    /// Sampled at the next Connect; doesn't affect an in-flight
    /// connection.
    RelayModeChanged(config::RelayMode),
    /// "Show opponent's setup at match start" checkbox toggled.
    /// Persisted to `config.show_opponent_setup`; sampled when the
    /// next PvP session is installed.
    ToggleShowOpponentSetup(bool),
    PatchRepoChanged(String),
    /// "Change…" clicked next to the data folder. The App intercepts this
    /// (before `State::update`) to open an async folder picker, which comes
    /// back as `DataFolderPicked`.
    OpenDataFolderPicker,
    /// Folder picker resolved: `Some(path)` if the user chose one, `None` if
    /// they dismissed it.
    DataFolderPicked(Option<std::path::PathBuf>),
    TogglePatchAutoupdate(bool),
    VideoFilterChanged(String),
    ToggleFractionalScaling(bool),
    ToggleHideEmulatorBorder(bool),
    ToggleFullscreen(bool),
    /// New windowed size picked, as `(width, height)`.
    ResolutionChanged((f32, f32)),
    UiScaleChanged(f32),
    ToggleEnableUpdater(bool),
    ToggleAllowPrereleaseUpgrades(bool),
    VolumeChanged(f32),
    /// "Mute music in netplay" checkbox toggled. Persisted to
    /// `config.disable_bgm_in_pvp`; sampled at the next match start.
    ToggleDisableBgmInPvp(bool),
    /// User clicked "Update Now" on the About panel. App's
    /// settings handler calls `updater.finish_update()` which
    /// hands off to the installer + exits the process.
    UpdateNow,
    ThemeChanged(config::ThemeMode),
    AccentChanged(config::AccentColor),
    /// User clicked key `k` on the drawn console — the screen
    /// switches to showing its bindings.
    BindingSlotSelected(input::MappedKey),
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
    /// Raw key/button/axis event seen by the Input pane's capture
    /// wrapper while NOT rebinding — folded into [`State::held`] for
    /// the live binding-chip highlight.
    LiveInput(input::Event),
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
    RelayMode(config::RelayMode),
    ShowOpponentSetup(bool),
    PatchRepo(String),
    /// New root data folder picked. The App points `config.data_path` at it,
    /// creates the standard subfolders, re-scans, and re-points the patch
    /// autoupdater.
    DataPath(std::path::PathBuf),
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
    DisableBgmInPvp(bool),
    Theme(config::ThemeMode),
    Accent(config::AccentColor),
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
                if self.active_tab != t {
                    // The sidebar lists sections in declaration
                    // order, so the discriminants double as
                    // positions: moving down brings the pane in
                    // from below, moving up from above.
                    self.pane_enter_dy = if (t as u8) > (self.active_tab as u8) {
                        32.0
                    } else {
                        -32.0
                    };
                    self.active_tab = t;
                    self.pane_enter.start(iced::time::Instant::now());
                    // Leaving the Input pane unmounts its capture
                    // wrapper mid-hold; drop the held set so nothing
                    // shows stale-lit on the way back.
                    self.held = Default::default();
                }
                None
            }
            Message::LanguageSelected(l) => Some(ConfigChange::Language(l)),
            Message::NicknameChanged(s) => Some(ConfigChange::Nickname(s)),
            Message::ToggleStreamerMode(b) => Some(ConfigChange::StreamerMode(b)),
            Message::MatchmakingEndpointChanged(s) => Some(ConfigChange::MatchmakingEndpoint(s)),
            Message::RelayModeChanged(m) => Some(ConfigChange::RelayMode(m)),
            Message::ToggleShowOpponentSetup(b) => Some(ConfigChange::ShowOpponentSetup(b)),
            Message::PatchRepoChanged(s) => Some(ConfigChange::PatchRepo(s)),
            // Intercepted by the App before it reaches here (it opens the
            // folder picker); the arm exists only for exhaustiveness.
            Message::OpenDataFolderPicker => None,
            Message::DataFolderPicked(Some(path)) => Some(ConfigChange::DataPath(path)),
            // Dialog dismissed — nothing to change.
            Message::DataFolderPicked(None) => None,
            Message::TogglePatchAutoupdate(b) => Some(ConfigChange::PatchAutoupdate(b)),
            Message::VideoFilterChanged(s) => Some(ConfigChange::VideoFilter(s)),
            Message::ToggleFractionalScaling(b) => Some(ConfigChange::FractionalScaling(b)),
            Message::ToggleHideEmulatorBorder(b) => Some(ConfigChange::HideEmulatorBorder(b)),
            Message::ToggleFullscreen(b) => Some(ConfigChange::Fullscreen(b)),
            Message::ResolutionChanged((w, h)) => Some(ConfigChange::Resolution(w, h)),
            Message::UiScaleChanged(s) => Some(ConfigChange::UiScale(s)),
            Message::ToggleEnableUpdater(b) => Some(ConfigChange::EnableUpdater(b)),
            Message::ToggleAllowPrereleaseUpgrades(b) => Some(ConfigChange::AllowPrereleaseUpgrades(b)),
            Message::VolumeChanged(v) => Some(ConfigChange::Volume(v)),
            Message::ToggleDisableBgmInPvp(b) => Some(ConfigChange::DisableBgmInPvp(b)),
            // App handles UpdateNow as a top-level effect — it
            // calls `updater.finish_update()` which exits the
            // process on success. Nothing to fold into config.
            Message::UpdateNow => None,
            Message::ThemeChanged(t) => Some(ConfigChange::Theme(t)),
            Message::AccentChanged(a) => Some(ConfigChange::Accent(a)),
            Message::BindingSlotSelected(k) => {
                // Clicking a console key retargets the screen; any
                // in-flight capture is dropped rather than silently
                // rebound to the newly-selected key.
                self.selected_key = Some(k);
                self.capture_target = None;
                None
            }
            Message::BindingCaptureStart(k) => {
                self.capture_target = Some(k);
                None
            }
            Message::BindingCaptureCancel => {
                self.capture_target = None;
                None
            }
            Message::BindingCaptured(p) => {
                let target = self.capture_target.take()?;
                Some(ConfigChange::AddInputBinding(target, p))
            }
            Message::BindingRemove(k, idx) => Some(ConfigChange::RemoveInputBinding(k, idx)),
            Message::BindingsReset => Some(ConfigChange::ResetInputBindings),
            Message::LiveInput(ev) => {
                self.held.apply(&ev);
                None
            }
            Message::OpenUrl(url) => {
                if let Err(e) = open::that(&url) {
                    log::warn!("open url {url}: {e}");
                }
                None
            }
        }
    }
}

/// `session_held`: inside the in-session settings modal, the session's
/// live held-state — its own `InputCapture` wrapper + vblank-paced pump
/// already see every key/button, so the Input pane's binding highlight
/// reads from it and this view adds no wrapper of its own (a second
/// pump would steal the session's gamepad events). `None` outside a
/// session; the pane then wraps itself and feeds [`State::held`].
pub fn view<'a>(
    lang: &'a LanguageIdentifier,
    config: &'a config::Config,
    state: &'a State,
    updater_status: crate::updater::Status,
    session_held: Option<&'a input::HeldState>,
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
        SettingsTab::Input => settings_input(lang, config, state, session_held.unwrap_or(&state.held)),
        SettingsTab::Netplay => settings_netplay(lang, config),
        SettingsTab::About => settings_about(lang, config, &state.about, updater_status),
        // The status arg is consumed by About's call here; iced
        // discards the unused-on-other-tabs branches at runtime
        // so no double-clone is needed.
    };

    // Scrollable wraps the body once at the dispatch layer —
    // each settings_* pane returns a plain column with its own
    // inner padding so the scrollbar hugs the right edge.
    let body_wrap = container(
        scrollable(body)
            .style(widgets::chunky_scrollable)
            .width(Fill)
            .height(Fill),
    )
    .width(Fill)
    .height(Fill)
    .style(widgets::pane);
    // Section-switch entrance: just this pane glides in,
    // vertically along the direction of travel in the sidebar.
    let body_wrap: Element<'a, Message> = crate::anim::slide_in_opt(
        body_wrap,
        state.pane_enter.progress(iced::time::Instant::now()),
        iced::Vector::new(0.0, state.pane_enter_dy),
    );

    let root = row![sidebar, body_wrap]
        .spacing(style::PANE_GAP)
        .padding(style::PANE_GAP)
        .width(Fill)
        .height(Fill);

    // Input pane: wrap the settings UI in `InputCapture` so both
    // keyboard and gamepad events flow through one synchronous path.
    // Without the wrapper we'd be idle and SDL3's pump (main-thread
    // only) wouldn't get drained. While the user is rebinding a key
    // the callback publishes a `BindingCaptured` for the first key
    // press, button press, or axis-past-threshold event; everything
    // else (releases included) folds into `State::held` as `LiveInput`
    // so the binding chips light up while their key is down. In a
    // session the outer session wrapper already tracks held state, so
    // no wrapper here (see `session_held` on [`view`]).
    let capturing = state.capture_target.is_some();
    if capturing || (state.active_tab == SettingsTab::Input && session_held.is_none()) {
        crate::input_capture::InputCapture::new(root, move |input| {
            if capturing {
                let captured = match input {
                    crate::input_capture::Input::Keyboard(iced::keyboard::Event::KeyPressed {
                        physical_key, ..
                    }) => Some(input::PhysicalInput::Key(input::KeyPhysical(*physical_key))),
                    crate::input_capture::Input::Keyboard(_) => None,
                    crate::input_capture::Input::Gamepad(ev) => match *ev {
                        crate::gamepad::GamepadEvent::ButtonDown(b) => {
                            Some(input::PhysicalInput::Button(input::GamepadButton::from_sdl3(b)))
                        }
                        crate::gamepad::GamepadEvent::AxisMotion { axis, value } => {
                            (value.abs() > input::AXIS_THRESHOLD).then_some(input::PhysicalInput::Axis {
                                axis,
                                dir: if value > 0.0 {
                                    input::AxisDir::Positive
                                } else {
                                    input::AxisDir::Negative
                                },
                            })
                        }
                        _ => None,
                    },
                };
                if let Some(captured) = captured {
                    return Some(Message::BindingCaptured(captured));
                }
            }
            input.to_event().map(Message::LiveInput)
        })
        .into()
    } else {
        root.into()
    }
}

/// A bare chunky checkbox for the right slot of an [`option_row`] —
/// the row carries the label, so the box doesn't.
fn toggle<'a>(checked: bool, msg: fn(bool) -> Message) -> Element<'a, Message> {
    iced::widget::checkbox(checked)
        .on_toggle(msg)
        .size(style::TEXT_HEADING)
        .style(widgets::chunky_checkbox)
        .into()
}

fn settings_general<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
    column![
        settings_group(
            t!(lang, "settings-group-profile"),
            vec![
                option_row::<Message>(
                    t!(lang, "settings-nickname"),
                    text_input("", config.nickname.as_deref().unwrap_or(""))
                        .on_input(Message::NicknameChanged)
                        .padding(STANDARD_PADDING)
                        .width(Length::Fixed(240.0))
                        .style(widgets::chunky_text_input),
                ),
                option_row(
                    t!(lang, "settings-streamer-mode"),
                    toggle(config.streamer_mode, Message::ToggleStreamerMode),
                ),
            ],
        ),
        settings_group(
            t!(lang, "settings-group-interface"),
            vec![
                option_row::<Message>(t!(lang, "settings-language"), {
                    // Build the picker options as `LanguageChoice`
                    // wrappers — they Display the endonym from each
                    // locale's `LANGUAGE` Fluent key instead of the
                    // bare locale code.
                    let options: Vec<crate::i18n::LanguageChoice> = SUPPORTED_LANGS
                        .iter()
                        .map(|id| crate::i18n::LanguageChoice::new(id.clone()))
                        .collect();
                    let selected = options.iter().find(|c| c.id == config.language).cloned();
                    widgets::picker(options, selected, |c: crate::i18n::LanguageChoice| {
                        Message::LanguageSelected(c.id)
                    })
                }),
                option_row::<Message>(t!(lang, "settings-theme"), {
                    let options = vec![
                        theme_choice(lang, config::ThemeMode::Dark),
                        theme_choice(lang, config::ThemeMode::Light),
                    ];
                    let selected = options.iter().find(|c| c.value == config.theme).cloned();
                    widgets::picker(options, selected, |c: Choice<config::ThemeMode>| {
                        Message::ThemeChanged(c.value)
                    })
                }),
                option_row::<Message>(t!(lang, "settings-accent"), {
                    let options = vec![
                        accent_choice(lang, config::AccentColor::TangoGreen),
                        accent_choice(lang, config::AccentColor::MegaManBlue),
                        accent_choice(lang, config::AccentColor::ProtoManRed),
                        accent_choice(lang, config::AccentColor::RollPink),
                        accent_choice(lang, config::AccentColor::BassGold),
                    ];
                    let selected = options.iter().find(|c| c.value == config.accent).cloned();
                    widgets::picker(options, selected, |c: Choice<config::AccentColor>| {
                        Message::AccentChanged(c.value)
                    })
                }),
            ],
        ),
        settings_group(
            t!(lang, "settings-group-storage"),
            vec![option_row::<Message>(
                t!(lang, "settings-data-folder"),
                row![
                    // The path is supporting detail next to its
                    // Change action, so it rides muted at caption
                    // size instead of competing with the row label.
                    text(config.data_path.to_string_lossy().into_owned())
                        .size(TEXT_CAPTION)
                        .style(widgets::muted_text_style),
                    button(text(t!(lang, "settings-data-folder-change")))
                        .on_press(Message::OpenDataFolderPicker)
                        .padding(STANDARD_PADDING)
                        .style(widgets::neutral),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )],
        ),
        settings_group(
            t!(lang, "settings-group-patches"),
            vec![
                option_row::<Message>(
                    t!(lang, "settings-patch-repo"),
                    text_input("", &config.patch_repo)
                        .on_input(Message::PatchRepoChanged)
                        .padding(STANDARD_PADDING)
                        .width(Length::Fixed(380.0))
                        .style(widgets::chunky_text_input),
                ),
                option_row(
                    t!(lang, "settings-enable-patch-autoupdate"),
                    toggle(config.enable_patch_autoupdate, Message::TogglePatchAutoupdate),
                ),
            ],
        ),
        settings_group(
            t!(lang, "settings-group-updates"),
            vec![
                option_row(
                    t!(lang, "settings-enable-updater"),
                    toggle(config.enable_updater, Message::ToggleEnableUpdater),
                ),
                option_row(
                    t!(lang, "settings-allow-prerelease-upgrades"),
                    toggle(config.allow_prerelease_upgrades, Message::ToggleAllowPrereleaseUpgrades),
                ),
            ],
        ),
    ]
    .spacing(24)
    .padding(style::PANE_PADDING)
    .into()
}

fn settings_audio<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
    column![
        option_row::<Message>(
            t!(lang, "settings-volume"),
            row![
                // Compact percent readout next to the track so the user
                // can see exactly where the slider sits.
                text(format!("{:.0}%", config.volume * 100.0)).size(TEXT_CAPTION),
                // Bounded slider width — Fill would stretch all the way
                // across the pane, which looks silly for a volume bar.
                container(
                    iced::widget::slider(0.0..=1.0, config.volume, Message::VolumeChanged)
                        .step(0.01)
                        .style(widgets::chunky_slider)
                )
                .width(Length::Fixed(220.0)),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ),
        option_row(
            t!(lang, "settings-disable-bgm-in-pvp"),
            toggle(config.disable_bgm_in_pvp, Message::ToggleDisableBgmInPvp),
        ),
    ]
    .spacing(2)
    .padding(style::PANE_PADDING)
    .into()
}

/// Standard windowed resolutions surfaced in the graphics settings
/// pick-list. Selecting one resizes the live window and updates
/// `config.last_window_size`. Skips anything smaller than the
/// min_size enforced in `main.rs` (1280×720).
const STANDARD_RESOLUTIONS: &[(u32, u32)] = &[
    (960, 720),
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

pub const MINIMUM_RESOLUTION: (u32, u32) = STANDARD_RESOLUTIONS[0];

/// A window resolution as a pick_list [`Choice`]. PartialEq is exact
/// f32 — fine since the values come straight from
/// `STANDARD_RESOLUTIONS` constants and matched by equality.
fn resolution_choice(width: f32, height: f32) -> Choice<(f32, f32)> {
    Choice::new((width, height), format!("{}×{}", width as u32, height as u32))
}

/// UI scale presets surfaced in the graphics-settings pick-list.
/// Multiplies on top of the OS DPI scale.
const UI_SCALE_PRESETS: &[f32] = &[0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

/// A UI scale multiplier as a pick_list [`Choice`]. Integer percent —
/// 25%-step decimals would just clutter the dropdown. `PartialEq` is
/// exact on f32, which is fine since values come from the
/// `UI_SCALE_PRESETS` constants and matched by equality.
fn ui_scale_choice(scale: f32) -> Choice<f32> {
    Choice::new(scale, format!("{}%", (scale * 100.0).round() as u32))
}

fn settings_graphics<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
    let resolution_options: Vec<Choice<(f32, f32)>> = STANDARD_RESOLUTIONS
        .iter()
        .map(|&(w, h)| resolution_choice(w as f32, h as f32))
        .collect();
    // Match the current windowed size against the preset list so
    // the picker shows a selected value when it lines up exactly.
    // No match (custom drag-resized size) renders as blank.
    let selected_resolution = config
        .last_window_size
        .and_then(|size| resolution_options.iter().find(|o| o.value == size).cloned());
    // Disable the window-size picker while fullscreen is on:
    // picking a sub-monitor size while fullscreen is meaningless
    // (the live window stays at monitor resolution). Render the
    // shared disabled-dropdown placeholder so it reads as the
    // same control family as the live picker.
    let window_size_picker: Element<'a, Message> = if config.fullscreen {
        let label = selected_resolution.map(|r| r.label).unwrap_or_else(|| "—".into());
        widgets::disabled_pick_list(label).into()
    } else {
        widgets::picker(resolution_options, selected_resolution, |c: Choice<(f32, f32)>| {
            Message::ResolutionChanged(c.value)
        })
        .into()
    };
    let ui_scale_options: Vec<Choice<f32>> = UI_SCALE_PRESETS.iter().copied().map(ui_scale_choice).collect();
    let selected_ui_scale = ui_scale_options
        .iter()
        .find(|c| (c.value - config.ui_scale).abs() < f32::EPSILON)
        .cloned();
    column![
        settings_group(
            t!(lang, "settings-group-window"),
            vec![
                option_row::<Message>(t!(lang, "settings-window-size"), window_size_picker),
                option_row(
                    t!(lang, "settings-fullscreen"),
                    toggle(config.fullscreen, Message::ToggleFullscreen),
                ),
                option_row::<Message>(
                    t!(lang, "settings-ui-scale"),
                    widgets::picker(ui_scale_options, selected_ui_scale, |c: Choice<f32>| {
                        Message::UiScaleChanged(c.value)
                    }),
                ),
            ],
        ),
        settings_group(
            t!(lang, "settings-group-emulator"),
            vec![
                option_row::<Message>(t!(lang, "settings-video-filter"), {
                    // `value` is the `config.video_filter` key (`""`, `"hq2x"`, …).
                    let options: Vec<Choice<String>> = crate::video::effects::EFFECTS
                        .iter()
                        .map(|effect| Choice::new(effect.id.into(), effect.name))
                        .collect();
                    let selected = options.iter().find(|c| c.value == config.video_filter).cloned();
                    widgets::picker(options, selected, |c: Choice<String>| {
                        Message::VideoFilterChanged(c.value)
                    })
                }),
                option_row(
                    t!(lang, "settings-fractional-scaling"),
                    toggle(config.fractional_scaling, Message::ToggleFractionalScaling),
                ),
                option_row(
                    t!(lang, "settings-hide-emulator-border"),
                    toggle(config.hide_emulator_border, Message::ToggleHideEmulatorBorder),
                ),
            ],
        ),
    ]
    .spacing(24)
    .padding(style::PANE_PADDING)
    .into()
}

fn settings_netplay<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
    // No frame-delay control here — that knob lives in the lobby
    // (next to the latency it's tuned against, with the Suggest
    // button) and in the in-match settings; both persist to the
    // same `config.frame_delay`.
    column![
        option_row::<Message>(
            t!(lang, "settings-matchmaking-endpoint"),
            text_input("", &config.matchmaking_endpoint)
                .on_input(Message::MatchmakingEndpointChanged)
                .padding(STANDARD_PADDING)
                .width(Length::Fixed(380.0))
                .style(widgets::chunky_text_input),
        ),
        option_row::<Message>(t!(lang, "settings-use-relay"), {
            let options = vec![
                relay_mode_choice(lang, config::RelayMode::Auto),
                relay_mode_choice(lang, config::RelayMode::Always),
                relay_mode_choice(lang, config::RelayMode::Never),
            ];
            let selected = options.iter().find(|c| c.value == config.relay_mode).cloned();
            widgets::picker(options, selected, |c: Choice<config::RelayMode>| {
                Message::RelayModeChanged(c.value)
            })
        }),
        option_row(
            t!(lang, "settings-show-opponent-setup"),
            toggle(config.show_opponent_setup, Message::ToggleShowOpponentSetup),
        ),
    ]
    .spacing(2)
    .padding(style::PANE_PADDING)
    .into()
}

/// Fixed width of the drawn console shell. Sized to fit the
/// in-session settings modal's body (~620px wide); in the full
/// settings page it centers in the pane.
const GBA_SHELL_WIDTH: f32 = 560.0;

fn settings_input<'a>(
    lang: &'a LanguageIdentifier,
    config: &'a config::Config,
    state: &'a State,
    held: &input::HeldState,
) -> Element<'a, Message> {
    let mapping = &config.input_mapping;
    // A console key lights up while any of its bindings is physically
    // held — the same test-your-bindings affordance the old chip table
    // had, moved onto the drawn button itself.
    let lit = |k: input::MappedKey| mapping.slot(k).iter().any(|b| held.is_active(b));
    // One clickable console key: fixed box, centered face label,
    // selected/lit chrome. Clicking brings the key up on the screen.
    let key_btn = |content: Element<'a, Message>, k: input::MappedKey, w: f32, h: f32, radius: iced::border::Radius| {
        button(container(content).center(Fill))
            .width(Length::Fixed(w))
            .height(Length::Fixed(h))
            .padding(0)
            .style(gba_key(state.selected_key == Some(k), lit(k), radius))
            .on_press(Message::BindingSlotSelected(k))
    };

    // D-pad: four arms around an inert hub. Outer corners take the
    // big radius so the cross reads as one molded pad; the 2px seams
    // keep each arm clickable as its own key.
    let cell = 34.0;
    let arm = |icon: Icon, k: input::MappedKey, corners: [f32; 4]| {
        key_btn(
            icon.widget().size(16.0).into(),
            k,
            cell,
            cell,
            iced::border::Radius {
                top_left: corners[0],
                top_right: corners[1],
                bottom_right: corners[2],
                bottom_left: corners[3],
            },
        )
    };
    let corner = || Space::new().width(cell).height(cell);
    let hub = container(Space::new())
        .width(Length::Fixed(cell))
        .height(Length::Fixed(cell))
        .style(|theme: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(widgets::gba_key_plate(theme))),
            border: iced::Border {
                radius: 4.0.into(),
                width: 1.0,
                color: theme.extended_palette().background.strong.color,
            },
            ..Default::default()
        });
    let (ro, ri) = (10.0, 4.0);
    let dpad = column![
        row![
            corner(),
            arm(Icon::ChevronUp, input::MappedKey::Up, [ro, ro, ri, ri]),
            corner()
        ]
        .spacing(2),
        row![
            arm(Icon::ChevronLeft, input::MappedKey::Left, [ro, ri, ri, ro]),
            hub,
            arm(Icon::ChevronRight, input::MappedKey::Right, [ri, ro, ro, ri]),
        ]
        .spacing(2),
        row![
            corner(),
            arm(Icon::ChevronDown, input::MappedKey::Down, [ri, ri, ro, ro]),
            corner()
        ]
        .spacing(2),
    ]
    .spacing(2);

    // Start/Select: small pills below the D-pad, Start on top and
    // nudged right — flattening the real console's slanted pair.
    // Face labels stay literal like the silkscreen (localized names
    // appear on the bezel caption when selected).
    let pill = |label: &'static str, k: input::MappedKey| {
        key_btn(text(label).size(TEXT_CAPTION).into(), k, 64.0, 20.0, 999.0.into())
    };
    let start_select = column![
        row![Space::new().width(12.0), pill("START", input::MappedKey::Start)],
        pill("SELECT", input::MappedKey::Select),
    ]
    .spacing(5);
    let left_col = column![dpad, start_select].spacing(16);

    // A/B: round keys on a diagonal, A raised on the right. The
    // little always-on dot above them is the power LED.
    let ab_d = 46.0;
    let ab = row![
        column![
            Space::new().height(20.0),
            key_btn(
                text("B").size(style::TEXT_HEADING).into(),
                input::MappedKey::B,
                ab_d,
                ab_d,
                999.0.into()
            ),
        ],
        column![
            key_btn(
                text("A").size(style::TEXT_HEADING).into(),
                input::MappedKey::A,
                ab_d,
                ab_d,
                999.0.into()
            ),
            Space::new().height(20.0),
        ],
    ]
    .spacing(8);
    let right_col = column![ab].spacing(18).align_x(iced::Alignment::End);

    // The screen shows the selected key's bindings — chips + the
    // add/capture flow the old table rows carried — or a hint when
    // nothing is selected yet.
    let screen_body: Element<'a, Message> = if let Some(k) = state.selected_key {
        let mut chips = row![].spacing(6).align_y(iced::Alignment::Center);
        for (i, b) in mapping.slot(k).iter().enumerate() {
            chips = chips.push(binding_chip(lang, b, k, i, held.is_active(b)));
        }
        // Capture mode swaps the Add button for "press a key…" + a
        // cancel chip, same as the old per-row treatment.
        let action: Element<'a, Message> = if state.capture_target == Some(k) {
            row![
                text(t!(lang, "settings-input-press-key"))
                    .size(TEXT_BODY)
                    .style(widgets::primary_text_style),
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
            widgets::icon_button(
                Icon::Plus,
                t!(lang, "settings-input-add"),
                Message::BindingCaptureStart(k),
                STANDARD_PADDING,
            )
        };
        container(
            column![chips.wrap(), action]
                .spacing(10)
                .align_x(iced::Alignment::Center),
        )
        .center(Fill)
        .into()
    } else {
        container(
            text(t!(lang, "settings-input-select-hint"))
                .size(TEXT_BODY)
                .style(widgets::muted_text_style)
                .align_x(iced::Alignment::Center),
        )
        .center(Fill)
        .into()
    };
    let screen = container(screen_body)
        .width(Length::Fixed(252.0))
        .height(Length::Fixed(168.0))
        .padding(8)
        .style(gba_screen);
    // Silkscreen line under the glass names the selected key in the
    // UI language. Fixed-height slot so selecting never reflows the
    // bezel.
    let caption = container(
        text(
            state
                .selected_key
                .map(|k| mapped_key_label(lang, k))
                .unwrap_or_default(),
        )
        .size(TEXT_CAPTION)
        .style(|_: &iced::Theme| iced::widget::text::Style {
            color: Some(iced::Color::from_rgba(0.82, 0.82, 0.86, 0.9)),
        }),
    )
    .height(Length::Fixed(16.0));
    let bezel = container(column![screen, caption].spacing(4).align_x(iced::Alignment::Center))
        .padding(iced::Padding {
            top: 12.0,
            right: 14.0,
            bottom: 4.0,
            left: 14.0,
        })
        .style(gba_bezel);

    let shoulders = row![
        key_btn(
            text("L").size(TEXT_BODY).into(),
            input::MappedKey::L,
            84.0,
            22.0,
            999.0.into()
        ),
        horizontal_space(),
        key_btn(
            text("R").size(TEXT_BODY).into(),
            input::MappedKey::R,
            84.0,
            22.0,
            999.0.into()
        ),
    ];
    let face =
        row![left_col, horizontal_space(), bezel, horizontal_space(), right_col].align_y(iced::Alignment::Center);
    let shell = container(column![shoulders, face].spacing(10))
        .width(Length::Fixed(GBA_SHELL_WIDTH))
        .padding(iced::Padding {
            top: 10.0,
            right: 18.0,
            bottom: 24.0,
            left: 18.0,
        })
        .style(gba_shell);

    // Fast-forward isn't a console key; it sits under the shell as a
    // pill sharing the key chrome, with Reset on the opposite edge.
    let speed = button(
        row![
            Icon::FastForward.widget().size(TEXT_BODY),
            text(t!(lang, "input-key-speed-up")).size(TEXT_BODY),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
    )
    .padding([5.0, 12.0])
    .style(gba_key(
        state.selected_key == Some(input::MappedKey::SpeedUp),
        lit(input::MappedKey::SpeedUp),
        999.0.into(),
    ))
    .on_press(Message::BindingSlotSelected(input::MappedKey::SpeedUp));
    let reset = widgets::labeled_icon_button(
        Icon::RefreshCw,
        t!(lang, "settings-input-reset"),
        Message::BindingsReset,
        STANDARD_PADDING,
        widgets::neutral,
    );
    let below = row![speed, horizontal_space(), reset]
        .width(Length::Fixed(GBA_SHELL_WIDTH))
        .align_y(iced::Alignment::Center);

    container(column![shell, below].spacing(14))
        .width(Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .padding(style::PANE_PADDING)
        .into()
}

/// Localized display name for a mapped key — shown on the bezel
/// caption under the screen.
fn mapped_key_label(lang: &LanguageIdentifier, k: input::MappedKey) -> String {
    match k {
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
    }
}

/// Chrome for one console key. `selected` = the screen is showing
/// this key (primary ring); `lit` = a bound physical input is held
/// right now (primary flush, the live binding test). Both are
/// color-only so live input never shifts the layout.
fn gba_key(
    selected: bool,
    lit: bool,
    radius: iced::border::Radius,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |theme: &iced::Theme, status: button::Status| {
        let p = theme.extended_palette();
        let primary = theme.palette().primary;
        let plate = widgets::gba_key_plate(theme);
        let base = match status {
            button::Status::Hovered => widgets::mix(plate, iced::Color::WHITE, if p.is_dark { 0.12 } else { 0.2 }),
            button::Status::Pressed => widgets::mix(plate, iced::Color::BLACK, 0.10),
            _ => plate,
        };
        let background = if lit { widgets::mix(base, primary, 0.55) } else { base };
        button::Style {
            background: Some(iced::Background::Color(background)),
            text_color: theme.palette().text,
            border: iced::Border {
                radius,
                width: if selected { 2.0 } else { 1.0 },
                color: if selected {
                    primary
                } else if matches!(status, button::Status::Hovered) {
                    iced::Color { a: 0.7, ..primary }
                } else {
                    p.background.strong.color
                },
            },
            shadow: iced::Shadow {
                color: iced::Color {
                    a: if p.is_dark { 0.35 } else { 0.12 },
                    ..iced::Color::BLACK
                },
                offset: iced::Vector::new(
                    0.0,
                    if matches!(status, button::Status::Pressed) {
                        1.0
                    } else {
                        2.0
                    },
                ),
                blur_radius: 6.0,
            },
            snap: false,
        }
    }
}

/// The console shell — one lifted plate with the GBA's silhouette
/// (bottom grip corners fuller than the top). The accent stays on
/// the keys; the shell frames quietly.
fn gba_shell(theme: &iced::Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    iced::widget::container::Style {
        background: Some(iced::Background::Color(widgets::plate_color(theme))),
        text_color: Some(theme.palette().text),
        border: iced::Border {
            radius: iced::border::Radius {
                top_left: 24.0,
                top_right: 24.0,
                bottom_right: 34.0,
                bottom_left: 34.0,
            },
            width: 1.5,
            color: p.background.strong.color,
        },
        ..Default::default()
    }
}

/// The glass bezel around the screen — near-black in both themes,
/// like the real console's glass regardless of shell color.
fn gba_bezel(theme: &iced::Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    let glass = if p.is_dark {
        widgets::mix(theme.palette().background, iced::Color::BLACK, 0.55)
    } else {
        iced::Color::from_rgb(0.15, 0.15, 0.18)
    };
    iced::widget::container::Style {
        background: Some(iced::Background::Color(glass)),
        border: iced::Border {
            radius: 14.0.into(),
            width: 1.0,
            color: iced::Color {
                a: 0.5,
                ..iced::Color::BLACK
            },
        },
        ..Default::default()
    }
}

/// The LCD inside the bezel. Follows the theme (pale lit panel on
/// light, near-black on dark) so the binding chips and hint text
/// drawn "on screen" keep their normal contrast.
fn gba_screen(theme: &iced::Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    let bg = theme.palette().background;
    let lcd = if p.is_dark {
        widgets::mix(bg, iced::Color::BLACK, 0.3)
    } else {
        widgets::mix(bg, iced::Color::WHITE, 0.25)
    };
    iced::widget::container::Style {
        background: Some(iced::Background::Color(lcd)),
        text_color: Some(theme.palette().text),
        border: iced::Border {
            radius: 4.0.into(),
            width: 1.0,
            color: iced::Color {
                a: 0.35,
                ..iced::Color::BLACK
            },
        },
        ..Default::default()
    }
}

fn binding_chip<'a>(
    lang: &'a LanguageIdentifier,
    binding: &input::PhysicalInput,
    key: input::MappedKey,
    idx: usize,
    lit: bool,
) -> Element<'a, Message> {
    let (kind, label) = input::describe(lang, binding);
    let kind_glyph = match kind {
        input::DescribeKind::Keyboard => Icon::Keyboard,
        input::DescribeKind::Gamepad => Icon::Gamepad2,
    };
    // Primary-tinted rounded pill matching the rest of the
    // app's chip + badge chrome. × button is borderless (flat
    // chrome) so it doesn't shout against the small label.
    container(
        row![
            kind_glyph.widget().size(TEXT_BODY),
            text(label).size(TEXT_BODY),
            button(Icon::X.widget().size(TEXT_CAPTION))
                .padding([2, 4])
                .style(widgets::flat)
                .on_press(Message::BindingRemove(key, idx)),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
    )
    .padding([3, 8])
    .style(move |theme: &iced::Theme| {
        let primary = theme.palette().primary;
        // `lit` = the bound key/button is physically held right now —
        // the chip brightens so the user can test their bindings from
        // this screen. Colors only; the geometry never moves.
        let (bg_a, border_a) = if lit { (0.45, 1.0) } else { (0.12, 0.45) };
        iced::widget::container::Style {
            background: Some(iced::Background::Color(iced::Color { a: bg_a, ..primary })),
            text_color: Some(theme.palette().text),
            border: iced::Border {
                radius: 999.0.into(),
                width: 1.0,
                color: iced::Color { a: border_a, ..primary },
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
    .padding(style::PANE_PADDING)
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

    let mut col = column![accent_hairline(), version_row].spacing(8);
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

use crate::config;
use crate::i18n::{t, SUPPORTED_LANGS};
use crate::platform::input;
use crate::ui::style::{self, STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION};
use crate::ui::widgets;
use crate::ui::widgets::{option_row, Choice};
use crate::window;
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{button, container, scrollable, text, Space};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use sweeten::widget::{column, row, text_input};
use unic_langid::LanguageIdentifier;

mod about;
mod audio;
mod bindings;
mod general;
mod graphics;
mod netplay;
use about::settings_about;
pub use about::AboutMarkdown;
use audio::settings_audio;
use bindings::settings_input;
use general::settings_general;
use graphics::settings_graphics;
use netplay::settings_netplay;

/// Faint accent hairline — the divider register everywhere in the
/// app (group headers here, the About footer). Never a stock
/// `rule`, whose full-opacity gray line reads as dialog chrome.
pub(super) fn accent_hairline<'a>() -> Element<'a, Message> {
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
pub(super) fn settings_group<'a>(title: String, rows: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    // Accent tick ahead of the title — the small "this is a
    // section" mark game menus hang their headers on.
    let tick =
        container(Space::new().width(Length::Fixed(3.0)).height(Length::Fixed(11.0))).style(|theme: &iced::Theme| {
            iced::widget::container::Style {
                background: Some(iced::Background::Color(theme.palette().primary)),
                ..Default::default()
            }
        });
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
    pub pane_enter: crate::ui::anim::Enter,
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
    /// New DS screen arrangement picked. Applied at draw time, so an
    /// active session re-lays out immediately.
    DsScreenStackingChanged(config::DsScreenStacking),
    /// New DS primary screen picked — same draw-time application.
    DsPrimaryScreenChanged(config::DsPrimaryScreen),
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
    /// User clicked an external link in the About panel — an
    /// [`Effect::OpenUrl`]. `String` (not `&'static str`) so the
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
    DsScreenStacking(config::DsScreenStacking),
    DsPrimaryScreen(config::DsPrimaryScreen),
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

/// What a settings message asks of the application.
#[derive(Debug)]
pub enum Effect {
    /// A preference to apply and persist.
    Change(ConfigChange),
    /// Open an external link from the About panel.
    OpenUrl(String),
    /// Ask for a new data folder with the native folder picker; the
    /// answer comes back as [`Message::DataFolderPicked`].
    PickDataFolder,
    /// Install the downloaded update, which exits the process on success.
    InstallUpdate,
}

impl State {
    /// Apply a settings message to local UI state. Returns the
    /// [`Effect`] the caller performs; `None` for purely-local navigation
    /// like TabSelected.
    pub fn update(&mut self, msg: Message) -> Option<Effect> {
        match msg {
            Message::OpenUrl(url) => Some(Effect::OpenUrl(url)),
            Message::OpenDataFolderPicker => Some(Effect::PickDataFolder),
            Message::UpdateNow => Some(Effect::InstallUpdate),
            msg => self.change(msg).map(Effect::Change),
        }
    }

    /// The preference a message changes, if any, after applying its
    /// local UI state.
    fn change(&mut self, msg: Message) -> Option<ConfigChange> {
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
            // Effects, taken by `update` before this.
            Message::OpenDataFolderPicker | Message::UpdateNow | Message::OpenUrl(_) => None,
            Message::DataFolderPicked(Some(path)) => Some(ConfigChange::DataPath(path)),
            // Dialog dismissed — nothing to change.
            Message::DataFolderPicked(None) => None,
            Message::TogglePatchAutoupdate(b) => Some(ConfigChange::PatchAutoupdate(b)),
            Message::VideoFilterChanged(s) => Some(ConfigChange::VideoFilter(s)),
            Message::ToggleFractionalScaling(b) => Some(ConfigChange::FractionalScaling(b)),
            Message::DsScreenStackingChanged(s) => Some(ConfigChange::DsScreenStacking(s)),
            Message::DsPrimaryScreenChanged(s) => Some(ConfigChange::DsPrimaryScreen(s)),
            Message::ToggleHideEmulatorBorder(b) => Some(ConfigChange::HideEmulatorBorder(b)),
            Message::ToggleFullscreen(b) => Some(ConfigChange::Fullscreen(b)),
            Message::ResolutionChanged((w, h)) => Some(ConfigChange::Resolution(w, h)),
            Message::UiScaleChanged(s) => Some(ConfigChange::UiScale(s)),
            Message::ToggleEnableUpdater(b) => Some(ConfigChange::EnableUpdater(b)),
            Message::ToggleAllowPrereleaseUpgrades(b) => Some(ConfigChange::AllowPrereleaseUpgrades(b)),
            Message::VolumeChanged(v) => Some(ConfigChange::Volume(v)),
            Message::ToggleDisableBgmInPvp(b) => Some(ConfigChange::DisableBgmInPvp(b)),
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
    // Pill style matches the global top nav + save_editor sub-nav
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
    let body_wrap: Element<'a, Message> = crate::ui::anim::slide_in_opt(
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
        crate::platform::input_capture::InputCapture::new(root, move |input| {
            if capturing {
                let captured = match input {
                    crate::platform::input_capture::Input::Keyboard(iced::keyboard::Event::KeyPressed {
                        physical_key,
                        ..
                    }) => Some(input::PhysicalInput::Key(input::KeyPhysical(*physical_key))),
                    crate::platform::input_capture::Input::Keyboard(_) => None,
                    // Binding capture coalesces every pad: the first
                    // button/axis from any controller wins, so the source
                    // `id` is ignored here.
                    crate::platform::input_capture::Input::Gamepad(ev) => match ev.kind {
                        gamepad_facade::EventKind::ButtonDown(b) => {
                            Some(input::PhysicalInput::Button(input::GamepadButton::from_gamepad(b)))
                        }
                        gamepad_facade::EventKind::AxisMotion { axis, value } => (value.abs() > input::AXIS_THRESHOLD)
                            .then_some(input::PhysicalInput::Axis {
                                axis: input::GamepadAxis::from_gamepad(axis),
                                dir: if value > 0.0 {
                                    input::AxisDir::Positive
                                } else {
                                    input::AxisDir::Negative
                                },
                            }),
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
pub(super) fn toggle<'a>(checked: bool, msg: fn(bool) -> Message) -> Element<'a, Message> {
    iced::widget::checkbox(checked)
        .on_toggle(msg)
        .size(style::TEXT_HEADING)
        .style(widgets::chunky_checkbox)
        .into()
}

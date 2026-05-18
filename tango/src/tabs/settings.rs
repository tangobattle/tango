use crate::i18n::t;
use crate::widgets;
use crate::{config, input, save_view, STANDARD_PADDING, SUPPORTED_LANGS, TEXT_BODY, TEXT_CAPTION};
use iced::widget::rule::vertical as vertical_rule;
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use unic_langid::LanguageIdentifier;

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    #[default]
    General,
    Graphics,
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
    PatchRepoChanged(String),
    TogglePatchAutoupdate(bool),
    VideoFilterChanged(String),
    ToggleIntegerScaling(bool),
    ToggleEnableUpdater(bool),
    ToggleAllowPrereleaseUpgrades(bool),
    /// User clicked "Update Now" on the About panel. App's
    /// settings handler calls `updater.finish_update()` which
    /// hands off to the installer + exits the process.
    UpdateNow,
    /// User clicked "Open folder" next to the data path
    /// readout in General settings. Pure side effect — opens
    /// the OS file manager at the carried path
    /// (config.data_path; sourced from the view).
    OpenDataFolder(std::path::PathBuf),
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
    PatchRepo(String),
    PatchAutoupdate(bool),
    VideoFilter(String),
    IntegerScaling(bool),
    EnableUpdater(bool),
    AllowPrereleaseUpgrades(bool),
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
            Message::PatchRepoChanged(s) => Some(ConfigChange::PatchRepo(s)),
            Message::TogglePatchAutoupdate(b) => Some(ConfigChange::PatchAutoupdate(b)),
            Message::VideoFilterChanged(s) => Some(ConfigChange::VideoFilter(s)),
            Message::ToggleIntegerScaling(b) => Some(ConfigChange::IntegerScaling(b)),
            Message::ToggleEnableUpdater(b) => Some(ConfigChange::EnableUpdater(b)),
            Message::ToggleAllowPrereleaseUpgrades(b) => Some(ConfigChange::AllowPrereleaseUpgrades(b)),
            // App handles UpdateNow as a top-level effect — it
            // calls `updater.finish_update()` which exits the
            // process on success. Nothing to fold into config.
            Message::UpdateNow => None,
            Message::OpenDataFolder(p) => {
                let _ = std::fs::create_dir_all(&p);
                if let Err(e) = open::that(&p) {
                    log::warn!("open data folder {}: {e}", p.display());
                }
                None
            }
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

/// Subscription that listens for the *next* key/button event when
/// we're in binding-capture mode. Silent otherwise. Modifier
/// keys are filtered out so a user trying to bind "A" doesn't
/// accidentally bind Shift.
pub fn subscription(state: &State) -> iced::Subscription<Message> {
    if state.capture_target.is_none() {
        return iced::Subscription::none();
    }
    let kbd = iced::event::listen_with(|event, _, _| match event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) => {
            input::KeyId::from_iced(&key).map(|k| Message::BindingCaptured(input::PhysicalInput::Key(k)))
        }
        _ => None,
    });
    let pad = iced::Subscription::run(pad_capture_stream);
    iced::Subscription::batch([kbd, pad])
}

/// Builds the binding-capture gamepad polling stream. Stateless
/// — iced 0.14 requires subscription builders to be plain
/// functions (not closures), so this lives outside `subscription`.
fn pad_capture_stream() -> impl futures::Stream<Item = Message> {
    iced::stream::channel(8, |mut tx: futures::channel::mpsc::Sender<Message>| async move {
        use futures::SinkExt;
        let Ok(mut gilrs) = gilrs::Gilrs::new() else { return };
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(4)).await;
            while let Some(event) = gilrs.next_event() {
                let captured = match event.event {
                    gilrs::EventType::ButtonPressed(b, _) => {
                        input::GamepadButton::from_gilrs(b).map(input::PhysicalInput::Button)
                    }
                    gilrs::EventType::AxisChanged(a, v, _) => {
                        // Treat an axis as captured when it
                        // crosses the activation threshold —
                        // mirrors the runtime activation rule
                        // so the binding will fire next time.
                        if v.abs() > input::AXIS_THRESHOLD {
                            input::GamepadAxis::from_gilrs(a).map(|axis| input::PhysicalInput::Axis {
                                axis,
                                dir: if v > 0.0 {
                                    input::AxisDir::Positive
                                } else {
                                    input::AxisDir::Negative
                                },
                            })
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                if let Some(p) = captured {
                    if tx.send(Message::BindingCaptured(p)).await.is_err() {
                        return;
                    }
                }
            }
        }
    })
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
    let side_btn = |key: &'static str, tab: SettingsTab| {
        button(text(t(lang, key)))
            .padding(STANDARD_PADDING)
            .width(Fill)
            .style(widgets::pill_tab_style(tab == active))
            .on_press(Message::TabSelected(tab))
    };
    let sidebar = container(
        column![
            side_btn("settings-section-general", SettingsTab::General),
            side_btn("settings-section-graphics", SettingsTab::Graphics),
            side_btn("settings-section-input", SettingsTab::Input),
            side_btn("settings-section-network", SettingsTab::Netplay),
            side_btn("settings-section-about", SettingsTab::About),
        ]
        .spacing(4)
        .padding(12),
    )
    .width(Length::Fixed(140.0))
    .height(Fill);

    let body: Element<'a, Message> = match active {
        SettingsTab::General => settings_general(lang, config),
        SettingsTab::Graphics => settings_graphics(lang, config),
        SettingsTab::Input => settings_input(lang, config, state),
        SettingsTab::Netplay => settings_network(lang, config),
        SettingsTab::About => settings_about(lang, config, &state.about, updater_status),
        // The status arg is consumed by About's call here; iced
        // discards the unused-on-other-tabs branches at runtime
        // so no double-clone is needed.
    };

    // Scrollable wraps the body once at the dispatch layer —
    // each settings_* pane returns a plain column with its own
    // inner padding so the scrollbar hugs the right edge.
    let body_wrap = scrollable(body).width(Fill).height(Fill);

    row![sidebar, vertical_rule(1), body_wrap]
        .width(Fill)
        .height(Fill)
        .into()
}

/// Generic over Message so the welcome screen can use it too with its
/// own Message type.
pub fn labeled<'a, M: Clone + 'a>(label: String, ctrl: impl Into<Element<'a, M>>) -> Element<'a, M> {
    column![
        text(label).size(TEXT_CAPTION).style(save_view::muted_text_style),
        ctrl.into(),
    ]
    .spacing(4)
    .into()
}

fn settings_general<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
    column![
        labeled::<Message>(
            t(lang, "settings-nickname"),
            text_input("", config.nickname.as_deref().unwrap_or(""))
                .on_input(Message::NicknameChanged)
                .padding(STANDARD_PADDING)
                .style(widgets::chunky_text_input),
        ),
        labeled::<Message>(t(lang, "settings-language"), {
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
            .width(Fill)
            .style(widgets::chunky_pick_list)
        },),
        labeled::<Message>(
            t(lang, "settings-theme"),
            pick_list(
                vec![config::ThemeMode::Dark, config::ThemeMode::Light],
                Some(config.theme),
                Message::ThemeChanged,
            )
            .padding(STANDARD_PADDING)
            .width(Fill)
            .style(widgets::chunky_pick_list),
        ),
        iced::widget::checkbox(config.streamer_mode)
            .label(t(lang, "settings-streamer-mode"))
            .on_toggle(Message::ToggleStreamerMode)
            .style(widgets::chunky_checkbox),
        labeled::<Message>(
            t(lang, "settings-data-path"),
            row![
                // Path in a Fill container so a long Windows
                // path (e.g. nested under Documents) wraps
                // instead of squashing the Open Folder button.
                container(text(config.data_path.display().to_string()).size(TEXT_CAPTION)).width(Fill),
                widgets::icon_button(
                    Icon::Folder,
                    t(lang, "save-open-folder"),
                    Message::OpenDataFolder(config.data_path.clone()),
                    STANDARD_PADDING,
                ),
            ]
            .spacing(8)
            // Top-align so the Open Folder button stays put when
            // a long data path wraps to a second line.
            .align_y(Alignment::Start),
        ),
    ]
    .spacing(14)
    .padding(20)
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

fn settings_graphics<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
    column![
        labeled::<Message>(t(lang, "settings-video-filter"), {
            let options: Vec<VideoFilterChoice> = crate::video::FILTERS
                .iter()
                .map(|(k, d)| VideoFilterChoice {
                    key: (*k).into(),
                    display: (*d).into(),
                })
                .collect();
            let selected = options.iter().find(|c| c.key == config.video_filter).cloned();
            pick_list(options, selected, |c: VideoFilterChoice| {
                Message::VideoFilterChanged(c.key)
            })
            .padding(STANDARD_PADDING)
            .width(Fill)
            .style(widgets::chunky_pick_list)
        },),
        iced::widget::checkbox(config.integer_scaling)
            .label(t(lang, "settings-integer-scaling"))
            .on_toggle(Message::ToggleIntegerScaling)
            .style(widgets::chunky_checkbox),
    ]
    .spacing(14)
    .padding(20)
    .into()
}

fn settings_network<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
    column![
        labeled::<Message>(
            t(lang, "settings-matchmaking-endpoint"),
            text_input("", &config.matchmaking_endpoint)
                .on_input(Message::MatchmakingEndpointChanged)
                .padding(STANDARD_PADDING)
                .style(widgets::chunky_text_input),
        ),
        labeled::<Message>(
            t(lang, "settings-patch-repo"),
            text_input("", &config.patch_repo)
                .on_input(Message::PatchRepoChanged)
                .padding(STANDARD_PADDING)
                .style(widgets::chunky_text_input),
        ),
        iced::widget::checkbox(config.enable_patch_autoupdate)
            .label(t(lang, "settings-enable-patch-autoupdate"))
            .on_toggle(Message::TogglePatchAutoupdate)
            .style(widgets::chunky_checkbox),
        iced::widget::checkbox(config.enable_updater)
            .label(t(lang, "settings-enable-updater"))
            .on_toggle(Message::ToggleEnableUpdater)
            .style(widgets::chunky_checkbox),
        iced::widget::checkbox(config.allow_prerelease_upgrades)
            .label(t(lang, "settings-allow-prerelease-upgrades"))
            .on_toggle(Message::ToggleAllowPrereleaseUpgrades)
            .style(widgets::chunky_checkbox),
    ]
    .spacing(14)
    .padding(20)
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
        let label = t(lang, k.label_key());
        let bindings: &Vec<input::PhysicalInput> = bindings;
        // Capture-mode hint for the currently-targeted slot;
        // matches show "press a key…" + a cancel chip instead
        // of the usual Add button.
        let action: Element<'a, Message> = if state.capture_target == Some(k) {
            row![
                text(t(lang, "settings-input-press-key"))
                    .size(TEXT_BODY)
                    .style(|theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(theme.palette().primary),
                    }),
                widgets::icon_button(
                    Icon::X,
                    t(lang, "save-action-cancel"),
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
                t(lang, "settings-input-add"),
                Message::BindingCaptureStart(k),
                STANDARD_PADDING,
            )
        };
        let mut chips = row![].spacing(6).align_y(iced::Alignment::Center);
        for (i, b) in bindings.iter().enumerate() {
            chips = chips.push(binding_chip(b, k, i));
        }
        let row_inner = row![
            container(text(label).size(TEXT_BODY)).width(Length::Fixed(140.0)),
            chips,
            horizontal_space(),
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
        t(lang, "settings-input-reset"),
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

fn binding_chip<'a>(binding: &input::PhysicalInput, key: input::MappedKey, idx: usize) -> Element<'a, Message> {
    let (kind, label) = input::describe(binding);
    let kind_glyph = match kind {
        input::DescribeKind::Keyboard => Icon::Keyboard,
        input::DescribeKind::Gamepad => Icon::Gamepad,
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

    // Pull the live Theme from the same `crate::theme_for` the
    // App's theme callback uses — keeps link color in sync with
    // the rest of the app instead of pinning to DARK + TANGO_GREEN
    // by hand. `Settings::from(&Theme)` defaults to text-size 16,
    // so wrap to also pin the app's body text size.
    let theme = crate::theme_for(config);
    let style = markdown::Style::from(&theme);
    let settings = markdown::Settings::with_text_size(crate::TEXT_BODY, style);
    let body: Element<'a, Message> = markdown::view(about.content().items(), settings).map(Message::OpenUrl);

    column![emblem, body, updater_section(lang, updater_status)]
        .spacing(12)
        // Symmetric inner padding so the emblem doesn't rub
        // the nav scanline at the top, the updater section
        // breathes at the page end, and link text doesn't slam
        // the left edge.
        .padding(20)
        .into()
}

/// Bottom-of-About updater status panel. Current version
/// vs. latest, a one-line status, and an Update Now button
/// when a download is ready. Hidden release notes/download
/// progress until they're actually relevant.
fn updater_section<'a>(lang: &'a LanguageIdentifier, status: crate::updater::Status) -> Element<'a, Message> {
    use crate::updater::Status as S;

    let current = env!("CARGO_PKG_VERSION");
    let is_ready = matches!(status, S::ReadyToUpdate { .. });

    let latest_label: String = match &status {
        S::UpToDate { release: None } => t(lang, "updater-loading"),
        S::UpToDate { release: Some(Some(r)) } => format!("v{} ({})", r.version, t(lang, "updater-up-to-date")),
        S::UpToDate { release: Some(None) } => t(lang, "updater-up-to-date"),
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
                text(format!("{}: {pct}%", t(lang, "updater-downloading")))
                    .size(TEXT_CAPTION)
                    .style(save_view::muted_text_style)
                    .into(),
            )
        }
        S::ReadyToUpdate { .. } => Some(
            text(t(lang, "updater-ready-to-update"))
                .size(TEXT_CAPTION)
                .style(save_view::muted_text_style)
                .into(),
        ),
        _ => None,
    };

    let action: Option<Element<'a, Message>> = is_ready.then(|| {
        widgets::labeled_icon_button(
            Icon::Download,
            t(lang, "updater-update-now"),
            Message::UpdateNow,
            STANDARD_PADDING,
            widgets::primary_button,
        )
    });

    let mut col = column![
        iced::widget::rule::horizontal(1),
        row![
            text(format!("{}: v{current}", t(lang, "updater-current-version"))).size(TEXT_CAPTION),
            horizontal_space(),
            text(format!("{}: {}", t(lang, "updater-latest-version"), latest_label)).size(TEXT_CAPTION),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(8);
    if let Some(s) = status_line {
        col = col.push(s);
    }
    if let Some(a) = action {
        col = col.push(row![horizontal_space(), a].spacing(8));
    }
    col.into()
}

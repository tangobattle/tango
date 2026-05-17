use crate::i18n::t;
use crate::icons;
use crate::{
    config, input, save_view, STANDARD_PADDING, SUPPORTED_LANGS, TEXT_CAPTION, TEXT_DISPLAY,
};
use iced::widget::rule::vertical as vertical_rule;
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input, Space};
use iced::{Element, Fill, Length};
use unic_langid::LanguageIdentifier;

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    #[default]
    General,
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
}

#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(SettingsTab),
    LanguageSelected(LanguageIdentifier),
    NicknameChanged(String),
    ToggleStreamerMode(bool),
    MatchmakingEndpointChanged(String),
    PatchRepoChanged(String),
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

pub fn view<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config, state: &'a State) -> Element<'a, Message> {
    let active = state.active_tab;
    // Vertical tab strip on the left; selected pane on the right.
    let side_btn = |key: &'static str, tab: SettingsTab| {
        // Settings sidebar uses the bright primary fill for the
        // selected tab — the row contains a single line of text
        // (no muted subtitle) so the brighter accent doesn't have
        // a contrast clash to worry about, and it's what the
        // user wants the tab affordance to look like.
        let style: fn(&iced::Theme, button::Status) -> button::Style =
            if tab == active { button::primary } else { button::text };
        button(text(t(lang, key)).size(13.0))
            .padding(STANDARD_PADDING)
            .width(Fill)
            .style(style)
            .on_press(Message::TabSelected(tab))
    };
    let sidebar = container(
        column![
            side_btn("settings-section-general", SettingsTab::General),
            side_btn("settings-section-input", SettingsTab::Input),
            side_btn("settings-section-netplay", SettingsTab::Netplay),
            side_btn("settings-section-about", SettingsTab::About),
        ]
        .spacing(4)
        .padding(12),
    )
    .width(Length::Fixed(140.0))
    .height(Fill);

    let body: Element<'a, Message> = match active {
        SettingsTab::General => settings_general(lang, config),
        SettingsTab::Input => settings_input(lang, config, state),
        SettingsTab::Netplay => settings_netplay(lang, config),
        SettingsTab::About => settings_about(lang),
    };

    row![
        sidebar,
        vertical_rule(1),
        container(body).width(Fill).height(Fill).padding(20),
    ]
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
                .size(13.0)
                .padding(STANDARD_PADDING),
        ),
        labeled::<Message>(
            t(lang, "settings-language"),
            pick_list(
                SUPPORTED_LANGS.to_vec(),
                Some(config.language.clone()),
                Message::LanguageSelected,
            )
            .text_size(13.0)
            .padding(STANDARD_PADDING)
            .width(Fill),
        ),
        labeled::<Message>(
            t(lang, "settings-theme"),
            pick_list(
                vec![config::ThemeMode::Dark, config::ThemeMode::Light],
                Some(config.theme),
                Message::ThemeChanged,
            )
            .text_size(13.0)
            .padding(STANDARD_PADDING)
            .width(Fill),
        ),
        iced::widget::checkbox(config.streamer_mode).label(t(lang, "settings-streamer-mode"))
            .on_toggle(Message::ToggleStreamerMode)
            .text_size(13.0),
        labeled::<Message>(
            t(lang, "settings-data-path"),
            text(config.data_path.display().to_string()).size(TEXT_CAPTION),
        ),
    ]
    .spacing(14)
    .into()
}

fn settings_netplay<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
    column![
        labeled::<Message>(
            t(lang, "settings-matchmaking-endpoint"),
            text_input("", &config.matchmaking_endpoint)
                .on_input(Message::MatchmakingEndpointChanged)
                .size(13.0)
                .padding(STANDARD_PADDING),
        ),
        labeled::<Message>(
            t(lang, "settings-patch-repo"),
            text_input("", &config.patch_repo)
                .on_input(Message::PatchRepoChanged)
                .size(13.0)
                .padding(STANDARD_PADDING),
        ),
    ]
    .spacing(14)
    .into()
}

fn settings_input<'a>(
    lang: &'a LanguageIdentifier,
    config: &'a config::Config,
    state: &'a State,
) -> Element<'a, Message> {
    let mut col = column![].spacing(8);
    let slots = config.input_mapping.slots();
    for (k, bindings) in slots.iter() {
        let k = *k;
        let label = t(lang, k.label_key());
        let bindings: &Vec<input::PhysicalInput> = bindings;
        // Capture-mode hint for the currently-targeted slot;
        // matches show "press a key…" + a cancel chip instead
        // of the usual Add button.
        let action: Element<'a, Message> = if state.capture_target == Some(k) {
            row![
                text(t(lang, "settings-input-press-key"))
                    .size(13.0)
                    .style(save_view::muted_text_style),
                icons::icon_button(
                    icons::CANCEL,
                    t(lang, "save-action-cancel"),
                    Message::BindingCaptureCancel,
                    13.0,
                    STANDARD_PADDING,
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            icons::icon_button(
                icons::NEW,
                t(lang, "settings-input-add"),
                Message::BindingCaptureStart(k),
                13.0,
                STANDARD_PADDING,
            )
        };
        let mut chips = row![].spacing(6).align_y(iced::Alignment::Center);
        for (i, b) in bindings.iter().enumerate() {
            chips = chips.push(binding_chip(b, k, i));
        }
        let row = row![
            container(text(label).size(13.0)).width(Length::Fixed(120.0)),
            chips,
            horizontal_space(),
            action,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        col = col.push(row);
    }
    let reset = icons::icon_button(
        icons::RESCAN,
        t(lang, "settings-input-reset"),
        Message::BindingsReset,
        13.0,
        STANDARD_PADDING,
    );
    col = col
        .push(Space::new().height(8))
        .push(row![horizontal_space(), reset]);
    scrollable(col.padding(4)).into()
}

fn binding_chip<'a>(binding: &input::PhysicalInput, key: input::MappedKey, idx: usize) -> Element<'a, Message> {
    container(
        row![
            text(input::describe(binding)).size(TEXT_CAPTION),
            button(text("×").size(TEXT_CAPTION))
                .padding([2, 6])
                .style(button::danger)
                .on_press(Message::BindingRemove(key, idx)),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center),
    )
    .padding([2, 6])
    .style(iced::widget::container::bordered_box)
    .into()
}

fn settings_about<'a>(lang: &'a LanguageIdentifier) -> Element<'a, Message> {
    column![
        text("tango-ng").size(TEXT_DISPLAY),
        text(format!(
            "{}: {}",
            t(lang, "settings-version"),
            env!("CARGO_PKG_VERSION")
        ))
        .size(TEXT_CAPTION),
        Space::new().height(8),
        text(t(lang, "settings-about-blurb")).size(TEXT_CAPTION),
    ]
    .spacing(6)
    .into()
}

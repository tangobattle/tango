use crate::i18n::t;
use crate::{config, save_view, STANDARD_PADDING, STANDARD_TEXT_SIZE, SUPPORTED_LANGS};
use iced::widget::{button, column, container, pick_list, row, text, text_input, vertical_rule, Space};
use iced::{Element, Fill, Length};
use unic_langid::LanguageIdentifier;

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    #[default]
    General,
    Audio,
    Netplay,
    About,
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
    VolumeChanged(u8),
}

pub fn settings_panel<'a>(
    lang: &'a LanguageIdentifier,
    config: &'a config::Config,
    active: SettingsTab,
) -> Element<'a, Message> {
    // Vertical tab strip on the left; selected pane on the right.
    let side_btn = |key: &'static str, tab: SettingsTab| {
        let style = if tab == active { button::primary } else { button::text };
        button(text(t(lang, key)).size(STANDARD_TEXT_SIZE))
            .padding(STANDARD_PADDING)
            .width(Fill)
            .style(style)
            .on_press(Message::TabSelected(tab))
    };
    let sidebar = container(
        column![
            text(t(lang, "tab-settings")).size(18),
            Space::with_height(8),
            side_btn("settings-section-general", SettingsTab::General),
            side_btn("settings-section-audio", SettingsTab::Audio),
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
        SettingsTab::Audio => settings_audio(lang, config),
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
pub fn labeled<'a, M: Clone + 'a>(
    label: String,
    ctrl: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    column![
        text(label).size(11).style(save_view::muted_text_style),
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
                .padding(8),
        ),
        labeled::<Message>(
            t(lang, "settings-language"),
            pick_list(
                SUPPORTED_LANGS.to_vec(),
                Some(config.language.clone()),
                Message::LanguageSelected,
            )
            .width(Fill),
        ),
        labeled::<Message>(
            t(lang, "settings-theme"),
            pick_list(
                vec![config::ThemeMode::Dark, config::ThemeMode::Light],
                Some(config.theme),
                Message::ThemeChanged,
            )
            .width(Fill),
        ),
        iced::widget::checkbox(t(lang, "settings-streamer-mode"), config.streamer_mode)
            .on_toggle(Message::ToggleStreamerMode)
            .text_size(STANDARD_TEXT_SIZE),
        labeled::<Message>(
            t(lang, "settings-data-path"),
            text(config.data_path.display().to_string()).size(11),
        ),
    ]
    .spacing(14)
    .into()
}

fn settings_audio<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
    column![
        labeled::<Message>(
            format!("{}: {}%", t(lang, "settings-volume"), config.volume),
            iced::widget::slider(0..=100u8, config.volume, Message::VolumeChanged),
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
                .padding(8),
        ),
        labeled::<Message>(
            t(lang, "settings-patch-repo"),
            text_input("", &config.patch_repo)
                .on_input(Message::PatchRepoChanged)
                .padding(8),
        ),
    ]
    .spacing(14)
    .into()
}

fn settings_about<'a>(lang: &'a LanguageIdentifier) -> Element<'a, Message> {
    column![
        text("tango-ng").size(22),
        text(format!("{}: {}", t(lang, "settings-version"), env!("CARGO_PKG_VERSION"))).size(12),
        Space::with_height(8),
        text(t(lang, "settings-about-blurb")).size(12),
    ]
    .spacing(6)
    .into()
}

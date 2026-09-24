//! General pane: profile, interface, storage, updates, patches.

use super::*;
// Explicit: macros reached only through the glob above are ambiguous.
use sweeten::widget::{column, row};

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
            config::AccentColor::GutsManYellow => t!(lang, "settings-accent-gutsman-yellow"),
            config::AccentColor::BassPurple => t!(lang, "settings-accent-bass-purple"),
        },
    )
}

pub(super) fn settings_general<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
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
                        accent_choice(lang, config::AccentColor::GutsManYellow),
                        accent_choice(lang, config::AccentColor::BassPurple),
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

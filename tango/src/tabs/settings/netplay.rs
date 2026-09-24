//! Netplay pane: matchmaking endpoint, relay mode, opponent setup.

use super::*;
// Explicit: macros reached only through the glob above are ambiguous.
use sweeten::widget::column;

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

pub(super) fn settings_netplay<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
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

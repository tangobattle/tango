//! Audio pane.

use super::*;
// Explicit: macros reached only through the glob above are ambiguous.
use sweeten::widget::{column, row};

pub(super) fn settings_audio<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
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
                        .step(0.01_f32)
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

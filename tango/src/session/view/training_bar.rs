//! The training session's floating control bar, built around the drill
//! loop: reset / set drill point, the dummy's script (name readout,
//! reload, latched error), auto-reset, and the passive utilities (pause,
//! frame advance, speed, input display). Bottom-center over the emulator,
//! riding the shared auto-hide like the replay transport. Every control
//! fires `Message::Training(_)`; the hotkeys listed in the settings fire
//! the same actions through the input mapping.

use super::*;
// Explicit so these win over iced's prelude `column!`/`row!` macros (see mod.rs).
use sweeten::widget::row;

use crate::session::training::{Action, ScriptSource, TrainingSession, SPEED_STEPS};

/// One 32×32 icon chip in the bar. `lit` renders the "this is engaged"
/// treatment (primary glyph + tinted hairline — the replay bar's
/// input-toggle recipe); `msg = None` disables.
fn chip<'a>(icon: Icon, label: String, lit: bool, msg: Option<Message>) -> Element<'a, Message> {
    let enabled = msg.is_some();
    let style = move |theme: &iced::Theme, status: iced::widget::button::Status| {
        let mut st = telemetry_plate_button(theme, status);
        if lit {
            let primary = theme.palette().primary;
            st.text_color = primary;
            st.border.color = iced::Color { a: 0.35, ..primary };
        } else if !enabled {
            st.text_color = widgets::muted_color(theme);
        }
        st
    };
    let mut btn = button(
        container(icon.widget().size(16.0))
            .width(iced::Length::Fixed(18.0))
            .height(iced::Length::Fixed(18.0))
            .center(Fill),
    )
    .padding(0)
    .width(iced::Length::Fixed(32.0))
    .height(iced::Length::Fixed(32.0))
    .style(style);
    if let Some(msg) = msg {
        btn = btn.on_press(msg);
    }
    iced::widget::tooltip(
        btn,
        widgets::tooltip_bubble(label),
        iced::widget::tooltip::Position::Top,
    )
    .gap(4)
    .into()
}

/// A thin vertical hairline separating the bar's control groups.
fn divider<'a>() -> Element<'a, Message> {
    container(iced::widget::Space::new().width(1.0).height(20.0))
        .style(|theme: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(iced::Color {
                a: 0.15,
                ..theme.palette().text
            })),
            ..Default::default()
        })
        .into()
}

pub(super) fn training_bar<'a>(lang: &'a LanguageIdentifier, t: &'a TrainingSession) -> Element<'a, Message> {
    let paused = t.is_paused();
    let round_live = t.round_live();
    let train = |action: Action| Message::Training(action);

    // The drill loop: reset, set drill point, possess the dummy.
    let drill_controls = [
        chip(
            Icon::RotateCcw,
            t!(lang, "training-reset"),
            false,
            round_live.then_some(train(Action::Reset)),
        ),
        chip(
            Icon::Bookmark,
            t!(lang, "training-set-drill-point"),
            t.has_drill_point(),
            round_live.then_some(train(Action::SetDrillPoint)),
        ),
        chip(
            Icon::Bot,
            t!(lang, "training-possess"),
            t.is_possessing(),
            round_live.then_some(train(Action::TogglePossess)),
        ),
    ];

    // The dummy's script: a picker over the scripts dir (rescanned on
    // every drill action), "None" = the scriptless stand-still dummy.
    let script_options: Vec<widgets::Choice<Option<ScriptSource>>> =
        std::iter::once(widgets::Choice::new(None, t!(lang, "training-script-none")))
            .chain(
                t.available_scripts()
                    .into_iter()
                    .map(|s| widgets::Choice::new(Some(s.clone()), s.label())),
            )
            .collect();
    let current_script = t.script_source();
    let selected_script = script_options.iter().find(|c| c.value == current_script).cloned();
    let script_picker = sweeten::widget::pick_list(
        script_options,
        selected_script,
        |c: widgets::Choice<Option<ScriptSource>>| Message::Training(Action::SetScript(c.value)),
    )
    .padding([6.0, 10.0])
    .width(Length::Fixed(150.0))
    .style(flat_pick_list);
    // A fixed status slot next to the picker so the error appearing or
    // clearing never shifts the bar: the latched script error as a
    // danger glyph with the message in its tooltip, else blank.
    let script_status: Element<'a, Message> = match t.script_error() {
        Some(error) => iced::widget::tooltip(
            container(Icon::AlertTriangle.widget().size(16.0).style(
                |theme: &iced::Theme| iced::widget::text::Style {
                    color: Some(theme.palette().danger),
                },
            ))
            .width(iced::Length::Fixed(20.0))
            .height(iced::Length::Fixed(32.0))
            .center(Fill),
            widgets::tooltip_bubble(error),
            iced::widget::tooltip::Position::Top,
        )
        .gap(4)
        .into(),
        None => container(iced::widget::Space::new())
            .width(iced::Length::Fixed(20.0))
            .height(iced::Length::Fixed(32.0))
            .into(),
    };
    let reload_chip = chip(
        Icon::RefreshCw,
        t!(lang, "training-reload-script"),
        false,
        Some(train(Action::ReloadScript)),
    );

    // The round-end interception toggle.
    let auto_reset_chip = chip(
        Icon::Repeat,
        t!(lang, "training-auto-reset"),
        t.auto_reset(),
        Some(train(Action::ToggleAutoReset)),
    );

    // Passive utilities.
    let transport = [
        chip(
            if paused { Icon::Play } else { Icon::Pause },
            t!(lang, "training-pause"),
            paused,
            Some(train(Action::TogglePause)),
        ),
        chip(
            Icon::StepForward,
            t!(lang, "training-frame-advance"),
            false,
            Some(train(Action::FrameAdvance)),
        ),
    ];

    // Speed picker, mirroring the replay bar's.
    let current = t.speed();
    let speed_options: Vec<widgets::Choice<u32>> = SPEED_STEPS
        .iter()
        .map(|&v| {
            let label = if (v - v.trunc()).abs() < 1e-3 {
                format!("{}×", v as i32)
            } else {
                format!("{:.2}×", v).trim_end_matches('0').to_string()
            };
            widgets::Choice::new((v * 100.0) as u32, label)
        })
        .collect();
    let selected_speed = speed_options
        .iter()
        .find(|c| c.value == (current * 100.0) as u32)
        .cloned();
    let speed_picker = sweeten::widget::pick_list(speed_options, selected_speed, |c: widgets::Choice<u32>| {
        Message::Training(Action::SetSpeed(c.value as f32 / 100.0))
    })
    .padding([6.0, 10.0])
    .width(Length::Fixed(84.0))
    .style(flat_pick_list);

    let input_toggle = chip(
        Icon::Gamepad2,
        t!(lang, "playback-input-display"),
        t.show_inputs(),
        Some(train(Action::ToggleInputDisplay)),
    );

    // Dummy-screen PiP: the shadow's render, inset top-right — same
    // treatment as the replay bar's opponent-screen toggle.
    let pip_toggle = chip(
        Icon::PictureInPicture2,
        t!(lang, "playback-pip"),
        t.show_pip(),
        Some(train(Action::TogglePip)),
    );

    let mut bar = row![].spacing(8).align_y(Alignment::Center).padding([8, 10]);
    for el in drill_controls {
        bar = bar.push(el);
    }
    bar = bar.push(divider());
    bar = bar.push(script_picker);
    bar = bar.push(script_status);
    bar = bar.push(reload_chip);
    bar = bar.push(auto_reset_chip);
    bar = bar.push(divider());
    for el in transport {
        bar = bar.push(el);
    }
    bar = bar.push(speed_picker);
    bar = bar.push(input_toggle);
    bar = bar.push(pip_toggle);
    bar.into()
}

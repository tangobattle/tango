//! The training session's floating control bar: transport (pause /
//! frame advance / restart), the dummy-mode chip group with its loop
//! toggle and take readout, checkpoint slots with save/load, the speed
//! picker, and the input-display toggle. Bottom-center over the
//! emulator, riding the shared auto-hide like the replay transport.
//! Every control fires `Message::Training(_)`; the hotkeys listed in
//! the tooltips fire the same actions through the input mapping.

use super::*;
// Explicit so these win over iced's prelude `column!`/`row!` macros (see mod.rs).
use sweeten::widget::row;

use crate::session::training::{Action, DummyMode, TrainingSession, CHECKPOINT_SLOTS, SPEED_STEPS};

/// One 32×32 icon chip in the bar. `lit` renders the "this is engaged"
/// treatment (primary glyph + tinted hairline — the replay bar's
/// input-toggle recipe); `msg = None` disables.
fn chip<'a>(
    icon: Icon,
    label: String,
    lit: bool,
    msg: Option<Message>,
) -> Element<'a, Message> {
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
    let dummy = t.dummy_status();
    let round_live = t.round_live();
    let train = |action: Action| Message::Training(action);

    // Transport: pause/resume, frame advance, restart round.
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
        chip(
            Icon::RotateCcw,
            t!(lang, "training-restart"),
            false,
            round_live.then_some(train(Action::RestartRound)),
        ),
    ];

    // Dummy mode chips: exactly one is lit. The chips set modes
    // absolutely; the hotkeys toggle the same modes against Idle.
    let mode_chip = |icon: Icon, label: String, mode: DummyMode| {
        chip(icon, label, dummy.mode == mode, Some(train(Action::SetDummyMode(mode))))
    };
    let modes = [
        mode_chip(Icon::Ghost, t!(lang, "training-dummy-idle"), DummyMode::Idle),
        mode_chip(Icon::Joystick, t!(lang, "training-dummy-possess"), DummyMode::Possess),
        mode_chip(Icon::CircleDot, t!(lang, "training-dummy-record"), DummyMode::Record),
        mode_chip(Icon::Play, t!(lang, "training-dummy-playback"), DummyMode::Playback),
        chip(
            Icon::Repeat,
            t!(lang, "training-loop"),
            dummy.looping,
            Some(train(Action::ToggleLoop)),
        ),
    ];
    // Take readout: recorded length (and playback progress while it
    // plays). Fixed-width so ticking numbers don't wobble the bar.
    let take_label = match dummy.mode {
        DummyMode::Playback if dummy.script_len > 0 => {
            format!("{}/{}f", dummy.play_pos.min(dummy.script_len), dummy.script_len)
        }
        _ if dummy.script_len > 0 => format!("{}f", dummy.script_len),
        _ => String::new(),
    };
    let take_readout: Element<'a, Message> = container(
        text(take_label)
            .size(TEXT_CAPTION)
            .style(widgets::muted_text_style),
    )
    .width(iced::Length::Fixed(64.0))
    .align_x(iced::alignment::Horizontal::Center)
    .into();

    // Checkpoint slots: pick the active slot, then save/load it.
    let mut slots = row![].spacing(6).align_y(Alignment::Center);
    for slot in 0..CHECKPOINT_SLOTS {
        let selected = t.active_slot() == slot;
        let filled = t.slot_filled(slot);
        slots = slots.push(chip(
            if filled { Icon::BookmarkCheck } else { Icon::Bookmark },
            t!(lang, "training-slot", number = (slot + 1) as i64),
            selected,
            Some(train(Action::SelectSlot(slot))),
        ));
    }
    slots = slots.push(chip(
        Icon::Save,
        t!(lang, "training-save-state"),
        false,
        round_live.then_some(train(Action::SaveSlot)),
    ));
    slots = slots.push(chip(
        Icon::Download,
        t!(lang, "training-load-state"),
        false,
        t.slot_filled(t.active_slot()).then_some(train(Action::LoadSlot)),
    ));

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

    let mut bar = row![].spacing(8).align_y(Alignment::Center).padding([8, 10]);
    for el in transport {
        bar = bar.push(el);
    }
    bar = bar.push(divider());
    for el in modes {
        bar = bar.push(el);
    }
    bar = bar.push(take_readout);
    bar = bar.push(divider());
    bar = bar.push(slots);
    bar = bar.push(divider());
    bar = bar.push(speed_picker);
    bar = bar.push(input_toggle);
    bar.into()
}

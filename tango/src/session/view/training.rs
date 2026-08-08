//! Training-mode session view: the emulator pane plus the training
//! bar — opponent-screen picture-in-picture, a side-swap that hands
//! the player control of the other core, the dummy's custom-screen
//! policy, and the forced-hand chip picker — over the shared corner
//! commands.

use super::*;
use crate::session::training::{DummyPolicy, TrainingSession};
use crate::session::Message as SessionMessage;

/// Training-view messages. Wrapped in [`SessionMessage::Training`] on the
/// way out; inert unless a training session is active.
#[derive(Debug, Clone)]
pub enum Message {
    /// Toggle the opponent-screen picture-in-picture.
    TogglePip,
    /// Swap which side (core) the player controls.
    ToggleSwap,
    /// Step the dummy's custom-screen policy through its cycle.
    CyclePolicy,
    /// Open/close the forced-hand picker panel.
    ToggleChips,
    /// Switch which player's hand the picker edits.
    PickerSide(usize),
    /// The picker's chip-name filter changed.
    QueryChanged(String),
    /// Append a chip to the edited side's forced hand.
    AddChip(u16),
    /// Remove the forced chip at a hand slot.
    RemoveChip(usize),
    /// Clear the edited side's forced hand — the game's own picks
    /// stand again.
    ClearHand,
}

/// The edited side's forced hand, for the picker widgets.
fn picker_hand(state: &State) -> Vec<u16> {
    let Some(side) = state.training_panes.as_ref().map(|p| p.picker_side) else {
        return Vec::new();
    };
    state
        .active_as::<TrainingSession>()
        .and_then(|s| s.forced_hand(side))
        .unwrap_or_default()
}

/// Apply a training-view message.
pub(crate) fn update(state: &mut State, msg: Message) -> iced::Task<Message> {
    match msg {
        Message::TogglePip => {
            if let Some(s) = state.active_as::<TrainingSession>() {
                s.toggle_pip();
            }
        }
        Message::ToggleSwap => {
            if let Some(s) = state.active_as::<TrainingSession>() {
                s.toggle_swap();
            }
        }
        Message::CyclePolicy => {
            if let Some(s) = state.active_as::<TrainingSession>() {
                s.cycle_policy();
            }
        }
        Message::ToggleChips => {
            if let Some(p) = state.training_panes.as_mut() {
                p.picker_open = !p.picker_open;
            }
        }
        Message::PickerSide(side) => {
            if let Some(p) = state.training_panes.as_mut() {
                p.picker_side = side & 1;
            }
        }
        Message::QueryChanged(q) => {
            if let Some(p) = state.training_panes.as_mut() {
                p.query = q;
            }
        }
        Message::AddChip(id) => {
            let mut hand = picker_hand(state);
            if hand.len() < 6 {
                hand.push(id);
                if let (Some(side), Some(s)) = (
                    state.training_panes.as_ref().map(|p| p.picker_side),
                    state.active_as::<TrainingSession>(),
                ) {
                    s.set_forced_hand(side, Some(hand));
                }
            }
        }
        Message::RemoveChip(slot) => {
            let mut hand = picker_hand(state);
            if slot < hand.len() {
                hand.remove(slot);
                if let (Some(side), Some(s)) = (
                    state.training_panes.as_ref().map(|p| p.picker_side),
                    state.active_as::<TrainingSession>(),
                ) {
                    s.set_forced_hand(side, (!hand.is_empty()).then_some(hand));
                }
            }
        }
        Message::ClearHand => {
            if let (Some(side), Some(s)) = (
                state.training_panes.as_ref().map(|p| p.picker_side),
                state.active_as::<TrainingSession>(),
            ) {
                s.set_forced_hand(side, None);
            }
        }
    }
    iced::Task::none()
}

/// Training: emulator + PiP inset + the training bar (with the chip
/// picker stacked over it while open) + the shared corner commands.
pub(crate) fn view<'a>(s: &'a TrainingSession, ctx: Ctx<'a>) -> Element<'a, SessionMessage> {
    let Ctx { lang, state, .. } = ctx;
    let now = iced::time::Instant::now();
    let frame = framebuffer_view(ctx, None);
    let body = emulator_body(s.local_game(), frame, ctx.hide_emulator_border, [None, None]);
    let mut stacked = stack![body];
    // Opponent-screen PiP — outside the controls gate, so it doesn't tuck
    // away with the idle cursor (same treatment as replay).
    if let Some(o) = pip_overlay(ctx, None) {
        stacked = stacked.push(o);
    }
    if state.controls_anim.visible(now) {
        stacked = stacked.push(bottom_bar(lang, s, state));
        stacked = stacked.push(corner_commands_overlay(lang, state, SessionMessage::Close, false));
    }
    finish_session_stack(lang, state, stacked)
}

/// One 32×32 icon toggle, lit (primary text + hairline) while `active` —
/// the same chip treatment the replay transport uses for its display
/// toggles. `msg: None` renders it disabled.
fn toggle_button<'a>(icon: Icon, active: bool, label: String, msg: Option<Message>) -> Element<'a, Message> {
    let style = move |theme: &iced::Theme, status: iced::widget::button::Status| {
        let mut st = telemetry_plate_button(theme, status);
        if active {
            let primary = theme.palette().primary;
            st.text_color = primary;
            st.border.color = iced::Color { a: 0.35, ..primary };
        }
        st
    };
    iced::widget::tooltip(
        button(
            container(icon.widget().size(16.0))
                .width(Length::Fixed(18.0))
                .height(Length::Fixed(18.0))
                .center(Fill),
        )
        .padding(0)
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(32.0))
        .style(style)
        .on_press_maybe(msg),
        widgets::tooltip_bubble(label),
        iced::widget::tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

/// A 28×28 chip icon off the baked table, or a dim placeholder glyph
/// for a chip with no icon. Nearest-neighbour: the source art is 14×14
/// pixel art.
fn chip_icon<'a>(chips: &[tango_gamesupport::ChipDisplay], id: u16) -> Element<'a, Message> {
    match chips.get(id as usize).and_then(|c| c.icon.clone()) {
        Some(handle) => iced::widget::image(handle)
            .filter_method(iced::widget::image::FilterMethod::Nearest)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .into(),
        None => container(Icon::CircleHelp.widget().size(16.0))
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .center(Fill)
            .into(),
    }
}

/// A chip's display name, `"???"` for an id the table doesn't name.
fn chip_name(chips: &[tango_gamesupport::ChipDisplay], id: u16) -> String {
    chips
        .get(id as usize)
        .and_then(|c| c.name.clone())
        .unwrap_or_else(|| "???".to_string())
}

/// The forced-hand picker panel: side selector + the up-to-6 forced
/// slots (click to remove) + a name filter over the game's whole chip
/// table (click to add).
fn picker_panel<'a>(
    lang: &'a unic_langid::LanguageIdentifier,
    s: &'a TrainingSession,
    panes: &'a crate::session::TrainingPanes,
) -> Element<'a, Message> {
    let side = panes.picker_side & 1;
    let hand = s.forced_hand(side).unwrap_or_default();

    let side_chip = |label: &'static str, idx: usize| {
        let active = side == idx;
        let style = move |theme: &iced::Theme, status: iced::widget::button::Status| {
            let mut st = telemetry_plate_button(theme, status);
            if active {
                let primary = theme.palette().primary;
                st.text_color = primary;
                st.border.color = iced::Color { a: 0.35, ..primary };
            }
            st
        };
        button(text(label).size(12))
            .padding([3, 10])
            .style(style)
            .on_press(Message::PickerSide(idx))
    };
    let header = row![
        side_chip("P1", 0),
        side_chip("P2", 1),
        horizontal_space(),
        button(text(t!(lang, "training-chips-clear")).size(12))
            .padding([3, 10])
            .style(telemetry_plate_button)
            .on_press_maybe((!hand.is_empty()).then_some(Message::ClearHand)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // The forced slots, in fire order; empty slots draw as dim wells so
    // the row never reflows as chips come and go.
    let mut slots = row![].spacing(4).align_y(Alignment::Center);
    for slot in 0..6usize {
        let well = |theme: &iced::Theme, status: iced::widget::button::Status| {
            let mut st = telemetry_plate_button(theme, status);
            st.border.width = 1.0;
            st
        };
        slots = slots.push(match hand.get(slot) {
            Some(&id) => Element::from(
                iced::widget::tooltip(
                    button(chip_icon(&panes.chips, id))
                        .padding(3)
                        .style(well)
                        .on_press(Message::RemoveChip(slot)),
                    widgets::tooltip_bubble(chip_name(&panes.chips, id)),
                    iced::widget::tooltip::Position::Top,
                )
                .gap(4),
            ),
            None => container(
                iced::widget::Space::new()
                    .width(Length::Fixed(28.0))
                    .height(Length::Fixed(28.0)),
            )
            .padding(3)
                .style(|theme: &iced::Theme| {
                    let mut st = hud_chip_plate(theme);
                    st.background = None;
                    st
                })
                .into(),
        });
    }

    let search = iced::widget::text_input(&t!(lang, "training-chips-search"), &panes.query)
        .size(13)
        .padding([4, 8])
        .on_input(Message::QueryChanged);

    let query = panes.query.to_lowercase();
    let mut list = iced::widget::column![].spacing(2);
    for (id, chip) in panes.chips.iter().enumerate() {
        let Some(name) = chip.name.as_ref() else {
            continue;
        };
        if !query.is_empty() && !name.to_lowercase().contains(&query) {
            continue;
        }
        list = list.push(
            button(
                row![chip_icon(&panes.chips, id as u16), text(name.clone()).size(13)]
                    .spacing(8)
                    .align_y(Alignment::Center),
            )
            .padding([2, 6])
            .width(Fill)
            .style(telemetry_plate_button)
            .on_press_maybe((hand.len() < 6).then_some(Message::AddChip(id as u16))),
        );
    }

    container(
        iced::widget::column![
            header,
            slots,
            search,
            iced::widget::scrollable(list).height(Length::Fixed(220.0)).width(Fill),
        ]
        .spacing(8),
    )
    .padding(10)
    .width(Length::Fixed(300.0))
    .style(hud_chip_plate)
    .into()
}

/// The training toggles in a floating plate bar, bottom-centered over
/// the emulator and sliding past the bottom edge when the cursor idles —
/// the compact twin of the replay transport bar. The chip picker stacks
/// above it while open; a shared hover pin keeps both up while the
/// cursor rests on them.
fn bottom_bar<'a>(
    lang: &'a unic_langid::LanguageIdentifier,
    s: &'a TrainingSession,
    state: &'a State,
) -> Element<'a, SessionMessage> {
    let now = iced::time::Instant::now();
    let policy = s.policy();
    let (policy_icon, policy_label) = match policy {
        DummyPolicy::AutoConfirm => (Icon::Bot, t!(lang, "training-dummy-auto-confirm")),
        DummyPolicy::AutoPossess => (Icon::Ghost, t!(lang, "training-dummy-auto-possess")),
        DummyPolicy::Manual => (Icon::Hand, t!(lang, "training-dummy-manual")),
    };
    let chips_available = s.chip_forcing_available();
    let picker_open = state.training_panes.as_ref().is_some_and(|p| p.picker_open);
    let bar = row![
        toggle_button(
            Icon::PictureInPicture2,
            s.show_pip(),
            t!(lang, "training-pip"),
            Some(Message::TogglePip)
        ),
        toggle_button(
            Icon::ArrowLeftRight,
            s.is_swapped(),
            t!(lang, "training-swap"),
            Some(Message::ToggleSwap)
        ),
        toggle_button(
            policy_icon,
            policy != DummyPolicy::Manual,
            policy_label,
            Some(Message::CyclePolicy)
        ),
        toggle_button(
            Icon::Swords,
            picker_open,
            if chips_available {
                t!(lang, "training-chips")
            } else {
                t!(lang, "training-chips-unavailable")
            },
            chips_available.then_some(Message::ToggleChips),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let plate = container(bar).padding([8, 12]).style(hud_chip_plate);
    let mut cluster = iced::widget::column![].spacing(8).align_x(Alignment::Center);
    if picker_open && chips_available {
        if let Some(panes) = state.training_panes.as_ref() {
            cluster = cluster.push(picker_panel(lang, s, panes));
        }
    }
    cluster = cluster.push(plate);
    let mapped: Element<'a, SessionMessage> = Element::from(cluster).map(SessionMessage::Training);
    // Hover pin: on_press is a capture sink so a click on the plate
    // between the toggles re-asserts the pin instead of falling through.
    let hover_pin = iced::widget::mouse_area(mapped)
        .on_enter(SessionMessage::ControlsHovered(true))
        .on_exit(SessionMessage::ControlsHovered(false))
        .on_press(SessionMessage::ControlsHovered(true));
    let slid = anim::slide_in(
        hover_pin,
        state.controls_anim.progress(now),
        iced::Vector::new(0.0, CONTROLS_SLIDE),
    );
    container(slid)
        .width(Fill)
        .height(Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(12)
        .into()
}

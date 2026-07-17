//! Replay-playback session view: the transport bar (play/pause +
//! scrubber + speed and display toggles), the input-display overlay,
//! the opponent-screen PiP, and the scrub hover thumbnail — plus the
//! [`Message`]s those controls emit and their [`update`] handler.

use super::*;
use crate::session::replay::{scrubber, ReplaySession};
use crate::session::Message as SessionMessage;
// Explicit so these win over iced's prelude `column!`/`row!` macros (see mod.rs).
use sweeten::widget::{column, row};

/// Messages the replay view emits. Wrapped as
/// [`SessionMessage::Replay`] on the way out; inert unless a replay
/// session is active.
#[derive(Debug, Clone)]
pub enum Message {
    /// Toggle play/pause (the transport button, or clicking the
    /// screen itself — any video player's idiom).
    TogglePlay,
    /// Scrub-bar drag in progress — fires per tick change while the
    /// button is held. Pauses playback and blits the nearest prefetched
    /// snapshot's framebuffer as an instant preview; the exact seek
    /// waits for [`Message::ScrubCommit`].
    ScrubPreview(u32),
    /// Scrub-bar drag released. Fires the real (asynchronous) seek to
    /// the last previewed tick and resumes playback if it was running
    /// when the drag started.
    ScrubCommit(u32),
    /// Cursor moved onto / along the scrub bar (`Some`) or off it
    /// (`None`) without a button held. Drives the floating keyframe
    /// thumbnail above the bar.
    ScrubHover(Option<scrubber::HoverInfo>),
    /// Set the playback speed factor (1.0 = realtime) — the bar's
    /// speed menu.
    SetSpeed(f32),
    /// Toggle the input display overlay (the recorded pad state of
    /// both sides, drawn over playback). The flag lives in config —
    /// the App's wrapper flips + persists it; nothing to do here.
    ToggleInputDisplay,
    /// Toggle the opponent-screen picture-in-picture (the bar's PiP
    /// button). The App wrapper also persists the choice to config.
    TogglePip,
    /// Swap which perspective the main screen shows (the bar's swap
    /// button). Per-session, unlike the PiP — it isn't persisted.
    ToggleSwapPerspective,
    /// The bar's speed dropdown opened (`true`) or closed (`false`) —
    /// [`crate::ui::widgets::MenuButton::on_toggle`]. While any
    /// overlay pane is up, iced hides the cursor from the base tree
    /// (`Cursor::Unavailable`), so the bar's hover pin goes blind
    /// exactly when the chrome must not hide or collapse under the
    /// open pane — the dropdown reports its state instead.
    BarMenuToggled(bool),
}

/// Apply a replay-view message. Takes the whole session [`State`]:
/// the scrub bookkeeping lives there, beside the session slot.
pub(crate) fn update(state: &mut State, msg: Message) -> iced::Task<Message> {
    match msg {
        Message::TogglePlay => state.toggle_replay_play(),
        Message::ScrubPreview(target) => {
            // Field-level borrow (not `active_as`): `scrub` is
            // mutated while the session ref is live.
            if let Some(s) = state.active.as_deref().and_then(|s| s.downcast_ref::<ReplaySession>()) {
                state.scrub.drag(target, s);
            }
            // The drag blits its keyframes to the main screen —
            // the floating hover thumbnail is redundant under it.
            state.scrub.hover = None;
        }
        Message::ScrubCommit(target) => {
            if let Some(s) = state.active_as::<ReplaySession>() {
                s.seek_to(target, state.scrub.resume);
            }
            state.scrub.end_drag();
        }
        Message::ScrubHover(hover) => {
            state.scrub.hover = hover;
            // Field-level borrow (not `active_as`): `scrub` is
            // mutated while the session ref is live.
            if let Some(s) = state.active.as_deref().and_then(|s| s.downcast_ref::<ReplaySession>()) {
                state.scrub.refresh_thumb(s);
            }
        }
        Message::SetSpeed(factor) => {
            if let Some(s) = state.active.as_ref() {
                s.set_speed(factor);
            }
        }
        Message::ToggleInputDisplay => {
            // Config-owned flag; the App wrapper flips + persists it
            // before this dispatch. The view reads it from config.
        }
        Message::TogglePip => {
            if let Some(s) = state.active_as::<ReplaySession>() {
                s.toggle_pip();
            }
        }
        Message::ToggleSwapPerspective => {
            if let Some(s) = state.active_as::<ReplaySession>() {
                s.toggle_swap_perspective();
            }
        }
        Message::BarMenuToggled(open) => {
            state.bar_menu_open = open;
        }
    }
    iced::Task::none()
}

/// Vertical clearance that floats a bottom-anchored popover just
/// above the replay transport bar (bottom margin + strip padding
/// + control height + row spacing + scrub bar + row spacing to the
/// collapsed analysis-strip slot + plate border + gap).
const POPOVER_LIFT: f32 = 12.0 + 16.0 + 32.0 + 4.0 + 26.0 + 4.0 + 2.0 + 6.0;

/// Height of the hover analysis strip above the scrubber — what the
/// bar (and anything floating above it) grows by while the strip is
/// expanded (its slot and row spacing are always in the layout; see
/// `replay_bar` for why it collapses instead of unmounting).
const HOVER_CHART_LIFT: f32 = 40.0;

/// Extra clearance for bottom-anchored floats while the analysis strip
/// is expanded above the scrubber (see `replay_bar`): the strip shows
/// while the transport is engaged — cursor on the bar, or a drag in
/// flight — and anything sitting just above the bar has to ride up
/// with it or the taller plate slides underneath.
fn hover_chart_lift(state: &State) -> f32 {
    let engaged = state.controls_hovered || state.bar_menu_open || state.scrub.preview.is_some();
    if engaged && state.replay_chart.as_ref().is_some_and(|c| !c.rounds.is_empty()) {
        HOVER_CHART_LIFT
    } else {
        0.0
    }
}

/// Replay playback: emulator + click-to-play base, the transport bar,
/// input display, PiP inset, and the scrub hover thumbnail.
pub(crate) fn view<'a>(r: &'a ReplaySession, ctx: Ctx<'a>) -> Element<'a, SessionMessage> {
    let Ctx { lang, state, .. } = ctx;
    let now = iced::time::Instant::now();
    let frame = framebuffer_view(state, ctx.fractional_scaling, ctx.effect);
    let body = emulator_body(r.local_game(), frame, ctx.hide_emulator_border, [false, false]);
    // Clicking the screen itself plays/pauses, like any video player.
    // This is the stack's bottom layer, and iced dispatches presses
    // topmost-first with capture — so the transport bar's controls
    // (and its plate, via the hover pin's press sink) never leak a
    // click down here.
    let base: Element<'a, SessionMessage> = iced::widget::mouse_area(body)
        .on_press(SessionMessage::Replay(Message::TogglePlay))
        .into();
    let mut stacked = stack![base];
    // The controls live in a floating bar over the emulator (no
    // reserved bottom strip), sliding away after the cursor sits
    // still — see `replay_controls`. When fully hidden it isn't
    // in the tree at all, so no invisible buttons linger where it
    // used to be.
    if state.controls_anim.visible(now) {
        stacked = stacked.push(replay_controls(lang, r, state, ctx.show_replay_inputs));
        stacked = stacked.push(corner_commands_overlay(lang, state, SessionMessage::Close, false));
    }
    // Input display, above the transport bar's resting spot.
    // Deliberately outside the floating-controls gate — the whole
    // point is reading inputs during playback, when the cursor (and
    // the bar with it) has gone idle.
    if let Some(o) = input_display_overlay(r, state, ctx.show_replay_inputs) {
        stacked = stacked.push(o.map(SessionMessage::Replay));
    }
    // PiP: the opponent's screen while the bar toggle is on. Also
    // outside the controls gate — it's for watching, so it must not
    // tuck away with the idle cursor.
    if let Some(o) = pip_overlay(state) {
        stacked = stacked.push(o.map(SessionMessage::Replay));
    }
    if let Some(o) = scrub_thumbnail_overlay(state) {
        stacked = stacked.push(o.map(SessionMessage::Replay));
    }
    finish_session_stack(lang, state, stacked)
}

/// The floating replay transport: the transport / toggles strip in a
/// [`widgets::panel`] plate, bottom-anchored over the emulator and
/// spanning the window (the scrubber is Fill-width). Hiding slides it
/// past the window's bottom edge — iced has no subtree opacity to
/// fade with, but fully clearing the edge reads the same. The bar's
/// own hover pin keeps it up while the cursor rests on it.
fn replay_controls<'a>(
    lang: &'a LanguageIdentifier,
    r: &'a ReplaySession,
    state: &'a State,
    show_replay_inputs: bool,
) -> Element<'a, SessionMessage> {
    let now = iced::time::Instant::now();
    let hide_progress = state.controls_anim.progress(now);
    let panel = container(replay_bar(lang, r, state, show_replay_inputs))
        .width(Fill)
        .style(hud_chip_plate);
    // The bar's own messages are replay-local; lift them into the
    // session message space before the shared hover-pin wrapper.
    let panel = Element::from(panel).map(SessionMessage::Replay);
    // iced's mouse_area — sweeten's `on_exit` never fires (see the
    // note in `finish_session_stack`), which left the hover pin stuck
    // and the bar permanently visible. `on_press` is a capture sink: a click on
    // the bar's plate (between controls) re-asserts the pin instead
    // of falling through to the screen's play/pause toggle.
    let hover_pin = iced::widget::mouse_area(panel)
        .on_enter(SessionMessage::ControlsHovered(true))
        .on_exit(SessionMessage::ControlsHovered(false))
        .on_press(SessionMessage::ControlsHovered(true));
    let slid = anim::slide_in(hover_pin, hide_progress, iced::Vector::new(0.0, CONTROLS_SLIDE));
    container(slid)
        .width(Fill)
        .height(Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(12)
        .into()
}

/// The replay bar's strip: full transport (play/pause + scrubber +
/// tick readouts) plus the options trigger, at the chunky
/// BAR_CONTROL_HEIGHT sizing. SP/PvP don't use this — their few
/// controls live in compact corner chips ([`corner_chips`]).
fn replay_bar<'a>(
    lang: &'a LanguageIdentifier,
    r: &'a ReplaySession,
    state: &'a State,
    show_replay_inputs: bool,
) -> Element<'a, Message> {
    // No ellipsis popover for replays — the speed picker sits
    // directly in the bar, and Settings + Close float top-right
    // (see `corner_commands_overlay`).
    // Speed: a dropdown of the steps, triggered by a chip wearing the
    // same plate chrome as the toggles beside it. The current step
    // carries the menu's check and the tooltip names it; the plate
    // lights up while off realtime.
    const SPEED_STEPS: [f32; 4] = [0.5, 1.0, 2.0, 4.0];
    let current = r.speed();
    let speed_idx = SPEED_STEPS
        .iter()
        .position(|&v| (current - v).abs() < 0.05)
        .unwrap_or(1);
    let speed_step_label = |v: f32| {
        if (v - v.trunc()).abs() < 1e-3 {
            format!("{}×", v as i32)
        } else {
            format!("{:.1}×", v)
        }
    };
    let speed_engaged = speed_idx != 1;
    let speed_style = move |theme: &iced::Theme, status: iced::widget::button::Status| {
        let mut st = telemetry_plate_button(theme, status);
        if speed_engaged {
            let primary = theme.palette().primary;
            st.text_color = primary;
            st.border.color = iced::Color { a: 0.35, ..primary };
        }
        st
    };
    let speed_items: Vec<widgets::MenuItem<Message>> = SPEED_STEPS
        .iter()
        .enumerate()
        .map(|(i, &v)| widgets::MenuItem::toggle(speed_step_label(v), Message::SetSpeed(v), i == speed_idx))
        .collect();
    let speed_menu = iced::widget::tooltip(
        widgets::MenuButton::new(
            container(Icon::Gauge.widget().size(16.0))
                .width(iced::Length::Fixed(18.0))
                .height(iced::Length::Fixed(18.0))
                .center(Fill),
            speed_items,
            true,
            [7.0, 7.0],
            crate::ui::style::STANDARD_PADDING,
            speed_style,
        )
        // Short labels + a check: the default pane would be mostly air.
        .menu_width(88.0)
        // Pin the bar + keep the strip expanded while the pane is up:
        // iced hides the cursor from the base tree while any overlay
        // is open (Cursor::Unavailable), so the hover pin goes blind —
        // and gets actively cleared by its own on_exit — exactly when
        // the chrome must not hide or collapse under the open menu.
        .on_toggle(Message::BarMenuToggled),
        widgets::tooltip_bubble(format!(
            "{}: {}",
            t!(lang, "playback-speed"),
            speed_step_label(SPEED_STEPS[speed_idx])
        )),
        iced::widget::tooltip::Position::Top,
    )
    .gap(4);

    // Input display toggle: quiet plate at rest, lit glyph + tinted
    // hairline while the overlay is on — the setup handles'
    // "identity in the glyph" treatment, not a full CTA fill.
    let input_toggle_style = move |theme: &iced::Theme, status: iced::widget::button::Status| {
        let mut st = telemetry_plate_button(theme, status);
        if show_replay_inputs {
            let primary = theme.palette().primary;
            st.text_color = primary;
            st.border.color = iced::Color { a: 0.35, ..primary };
        }
        st
    };
    let input_toggle = iced::widget::tooltip(
        button(
            container(Icon::Gamepad2.widget().size(16.0))
                .width(iced::Length::Fixed(18.0))
                .height(iced::Length::Fixed(18.0))
                .center(Fill),
        )
        .padding(0)
        .width(iced::Length::Fixed(32.0))
        .height(iced::Length::Fixed(32.0))
        .style(input_toggle_style)
        .on_press(Message::ToggleInputDisplay),
        widgets::tooltip_bubble(t!(lang, "playback-input-display")),
        iced::widget::tooltip::Position::Top,
    )
    .gap(4);

    // Opponent-screen PiP toggle: same chip treatment as the input
    // display. The replay re-simulates the opponent's core anyway; this
    // just turns its renderer on and insets the result top-right.
    let pip_on = r.show_pip();
    let pip_toggle_style = move |theme: &iced::Theme, status: iced::widget::button::Status| {
        let mut st = telemetry_plate_button(theme, status);
        if pip_on {
            let primary = theme.palette().primary;
            st.text_color = primary;
            st.border.color = iced::Color { a: 0.35, ..primary };
        }
        st
    };
    let pip_toggle = iced::widget::tooltip(
        button(
            container(Icon::PictureInPicture2.widget().size(16.0))
                .width(iced::Length::Fixed(18.0))
                .height(iced::Length::Fixed(18.0))
                .center(Fill),
        )
        .padding(0)
        .width(iced::Length::Fixed(32.0))
        .height(iced::Length::Fixed(32.0))
        .style(pip_toggle_style)
        .on_press(Message::TogglePip),
        widgets::tooltip_bubble(t!(lang, "playback-pip")),
        iced::widget::tooltip::Position::Top,
    )
    .gap(4);

    // Perspective swap: the main screen shows the opponent's re-simulated
    // view; the PiP (if on) carries the local screen. Same chip recipe.
    let swapped = r.swap_perspective();
    let swap_toggle_style = move |theme: &iced::Theme, status: iced::widget::button::Status| {
        let mut st = telemetry_plate_button(theme, status);
        if swapped {
            let primary = theme.palette().primary;
            st.text_color = primary;
            st.border.color = iced::Color { a: 0.35, ..primary };
        }
        st
    };
    let swap_toggle = iced::widget::tooltip(
        button(
            container(Icon::ArrowLeftRight.widget().size(16.0))
                .width(iced::Length::Fixed(18.0))
                .height(iced::Length::Fixed(18.0))
                .center(Fill),
        )
        .padding(0)
        .width(iced::Length::Fixed(32.0))
        .height(iced::Length::Fixed(32.0))
        .style(swap_toggle_style)
        .on_press(Message::ToggleSwapPerspective),
        widgets::tooltip_bubble(t!(lang, "playback-swap-perspective")),
        iced::widget::tooltip::Position::Top,
    )
    .gap(4);

    // The analysis strip, YouTube-style: a minimal trace chart
    // ([`widgets::hp_hover_strip`]) that expands directly above the
    // scrubber while the cursor rests on the bar (the panel's hover
    // pin) or a scrub drag is in flight. Both rows span the bar and
    // map ticks linearly across it, so every trace point sits directly
    // over the scrubber position that seeks to it. Passive: the
    // scrubber below stays the seek surface and its handle is the
    // position indicator.
    //
    // The strip is ALWAYS in the tree, collapsing to zero height
    // rather than unmounting: iced diffs widget state by tree
    // position, so mounting it on hover would shift the scrubber +
    // controls subtree and reset their widget state mid-interaction —
    // the speed menu's open dropdown (which reaches above the panel,
    // where the cursor drops the hover pin) died exactly that way.
    // Empty rounds = no stats at all (and none building), so the slot
    // just never expands; an in-flight analysis has planned-width
    // rounds from its first progress message, so it draws (and fills
    // in live) from the start.
    let bar_engaged = state.controls_hovered || state.bar_menu_open || state.scrub.preview.is_some();
    let rounds = state
        .replay_chart
        .as_ref()
        .map(|c| c.rounds.as_slice())
        .unwrap_or_default();
    let strip_h = if bar_engaged && !rounds.is_empty() {
        HOVER_CHART_LIFT
    } else {
        0.0
    };
    let graph = crate::ui::widgets::hp_hover_strip::<Message>(rounds, strip_h);

    // YouTube-style rows: [strip] / [scrubber, full width] / [play +
    // readout + spacer + chips].
    let total = r.total_ticks().max(1);
    let scrub = scrubber::Scrubber::new(
        playhead_tick(r, state),
        total,
        r.prefetch_progress().min(total),
        Message::ScrubPreview,
        Message::ScrubCommit,
        Message::ScrubHover,
    )
    .round_boundaries(r.round_boundaries())
    .view();

    let controls = row![].spacing(10).align_y(Alignment::Center);
    let controls = replay_transport(lang, r, state, controls)
        .push(speed_menu)
        .push(input_toggle)
        .push(pip_toggle)
        .push(swap_toggle);

    column![]
        .spacing(4)
        .padding([8, 8])
        .width(Fill)
        .push(graph)
        .push(scrub)
        .push(controls)
        .into()
}

/// The playhead position everything user-facing reads: the tick under
/// an active drag, else the target of an in-flight seek (so readouts
/// don't snap back while the chase catches up), else the emulator's
/// actual position — clamped to the replay's length. Shared by the
/// transport's readout/scrubber and the input display's lookup so
/// they can never disagree.
fn playhead_tick(r: &ReplaySession, state: &State) -> u32 {
    state
        .scrub
        .preview
        .or_else(|| r.pending_seek_target())
        .unwrap_or_else(|| r.current_tick())
        .min(r.total_ticks().max(1))
}

/// The replay transport: circular play/pause, current tick, scrubber,
/// total tick — pushed onto the strip in that order.
fn replay_transport<'a>(
    lang: &'a LanguageIdentifier,
    r: &'a ReplaySession,
    state: &State,
    controls: sweeten::widget::Row<'a, Message>,
) -> sweeten::widget::Row<'a, Message> {
    let total = r.total_ticks().max(1);
    let cur = playhead_tick(r, state);
    // The mgba thread is paused for the duration of a scrub drag and
    // the seek chase that follows it, but when playback resumes on
    // landing the session is logically still *playing* — flipping the
    // button to "Play" mid-scrub reads as a stuck pause.
    let logically_playing = (state.scrub.preview.is_some() && state.scrub.resume) || r.seek_will_resume();
    let (play_pause_icon, play_pause_label, paused) = if r.is_paused() && !logically_playing {
        (Icon::Play, t!(lang, "playback-play"), true)
    } else {
        (Icon::Pause, t!(lang, "playback-pause"), false)
    };

    // Play/Pause is the transport's centerpiece — promote to
    // the primary-button style when paused (the affordance
    // the user is most likely looking for at rest) and keep
    // it neutral while playing. Either way it sits a notch
    // bigger than the other strip controls and is rendered
    // as a perfect circle (square padding + huge radius) so
    // it reads as a console transport button instead of a
    // generic pill.
    let base_style: fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style = if paused {
        // Paused keeps the one accent in the bar — Play is the
        // affordance the user is looking for at rest.
        widgets::primary_button
    } else {
        // Playing rides the same flat plate as the floating chips.
        telemetry_plate_button
    };
    let play_pause_style = move |theme: &iced::Theme, status: iced::widget::button::Status| {
        let mut style = base_style(theme, status);
        style.border.radius = 999.0.into();
        style
    };
    // Compact circle, a notch bigger than the chip buttons so it
    // still reads as the transport's centerpiece.
    let play_pause_btn = iced::widget::tooltip(
        button(
            iced::widget::container(play_pause_icon.widget().size(16.0))
                .width(iced::Length::Fixed(18.0))
                .height(iced::Length::Fixed(18.0))
                .center(Fill),
        )
        .padding(0)
        .width(iced::Length::Fixed(32.0))
        .height(iced::Length::Fixed(32.0))
        .style(play_pause_style)
        .on_press(Message::TogglePlay),
        widgets::tooltip_bubble(play_pause_label),
        iced::widget::tooltip::Position::Top,
    )
    .gap(4);

    // Tick readout, YouTube-style "cur / total" beside the play
    // button: monospaced + bumped one tier above caption so it reads
    // as digital-clock numerals, the current tick primary-tinted so
    // the eye picks it up as playback state.
    let tick_style = |theme: &iced::Theme| iced::widget::text::Style {
        color: Some(theme.palette().primary),
    };
    controls
        .push(play_pause_btn)
        .push(
            row![
                text(format_tick(cur))
                    .size(14)
                    .font(iced::Font::MONOSPACE)
                    .style(tick_style),
                text("/").size(14).style(widgets::muted_text_style),
                text(format_tick(total))
                    .size(14)
                    .font(iced::Font::MONOSPACE)
                    .style(widgets::muted_text_style),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        // The chips ride the bar's right edge; the transport cluster
        // stays left, split apart by this filler.
        .push(iced::widget::space::horizontal())
}

/// Floating keyframe thumbnail + timestamp, hovering above the scrub
/// bar while the cursor rests on it (replay-only). Centered on the
/// cursor and clamped to the window edges (with a small margin so it
/// never sits flush against the border), lifted to the same height as
/// the bottom-anchored popovers. `responsive` is how the clamp learns
/// the window width — the overlay layer spans the whole session view.
/// Pure presentation — no mouse handlers anywhere in the chain, so it
/// never steals events from the transport below.
fn scrub_thumbnail_overlay(state: &State) -> Option<Element<'_, Message>> {
    let h = state.scrub.hover?;
    let (_, handle) = state.scrub.thumb.as_ref()?;
    let handle = handle.clone();
    // Native 240×160 at 0.75 — big enough to read the scene, small
    // enough not to feel like a second screen.
    const THUMB_W: f32 = 180.0;
    const THUMB_H: f32 = 120.0;
    const CARD_PAD: f32 = 4.0;
    const EDGE_MARGIN: f32 = 8.0;
    // A visible hover means the bar is engaged, which is exactly when
    // the analysis strip expands above the scrubber — lift the card
    // over it so they never overlap.
    let lift = POPOVER_LIFT + hover_chart_lift(state);
    Some(
        iced::widget::responsive(move |size| {
            let img = iced::widget::image(handle.clone())
                .width(Length::Fixed(THUMB_W))
                .height(Length::Fixed(THUMB_H));
            // Same numeral treatment as the transport's tick readouts
            // so the hover timestamp reads as playback state.
            let stamp = text(format_tick(h.tick))
                .size(TEXT_CAPTION)
                .font(iced::Font::MONOSPACE)
                .style(|theme: &iced::Theme| iced::widget::text::Style {
                    color: Some(theme.palette().primary),
                });
            // Same flat scrim plate as the transport bar below it.
            let card = container(column![img, stamp].spacing(2).align_x(Alignment::Center))
                .padding(CARD_PAD)
                .style(hud_chip_plate);
            let card_w = THUMB_W + CARD_PAD * 2.0;
            let hi = (size.width - EDGE_MARGIN - card_w).max(EDGE_MARGIN);
            let left = (h.x - card_w / 2.0).clamp(EDGE_MARGIN.min(hi), hi);
            container(card)
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Left)
                .align_y(iced::alignment::Vertical::Bottom)
                .padding(iced::Padding {
                    top: 0.0,
                    right: 0.0,
                    bottom: lift,
                    left,
                })
                .into()
        })
        .into(),
    )
}

/// Width of one input-display pad: the D-pad cross (three 24px cells
/// + 2px seams) plus the B/A cluster, spread to the edges by the
/// shoulders' `horizontal_space`.
const PAD_W: f32 = 160.0;

/// One side's recorded pad state, drawn as the settings input pane's
/// console face ([`crate::tabs::settings`]) at ~0.7 scale, minus the
/// screen: chevron D-pad cross left with the Start/Select pills below
/// it, B/A round keys on the console's diagonal right, L/R shoulder
/// pills capping the top corners. Non-interactive twin of that pane's
/// `key_btn`/`gba_key`: every key is always drawn on the shared
/// molded plate, and a pressed key mixes toward palette primary —
/// the same lit chrome as the settings' live binding test — so the
/// chip never changes size or layout as inputs flip.
fn input_pad<'a>(joyflags: u16) -> Element<'a, Message> {
    use mgba::input::keys;
    let cell = 24.0;
    let key = move |content: Element<'a, Message>, bit: u32, w: f32, h: f32, radius: iced::border::Radius| {
        let lit = joyflags as u32 & bit != 0;
        container(container(content).center(Fill))
            .width(Length::Fixed(w))
            .height(Length::Fixed(h))
            .style(move |theme: &iced::Theme| {
                let plate = widgets::gba_key_plate(theme);
                iced::widget::container::Style {
                    background: Some(iced::Background::Color(if lit {
                        widgets::mix(plate, theme.palette().primary, 0.55)
                    } else {
                        plate
                    })),
                    text_color: Some(theme.palette().text),
                    border: iced::Border {
                        radius,
                        width: 1.0,
                        color: theme.extended_palette().background.strong.color,
                    },
                    ..Default::default()
                }
            })
    };

    let arm = |icon: Icon, bit: u32, corners: [f32; 4]| {
        key(
            icon.widget().size(11.0).into(),
            bit,
            cell,
            cell,
            iced::border::Radius {
                top_left: corners[0],
                top_right: corners[1],
                bottom_right: corners[2],
                bottom_left: corners[3],
            },
        )
    };
    let corner = || iced::widget::Space::new().width(cell).height(cell);
    // Inert hub: `bit` 0 is never held, which is exactly the
    // settings hub's always-plate look.
    let hub = key(iced::widget::Space::new().into(), 0, cell, cell, 3.0.into());
    let (ro, ri) = (7.0, 3.0);
    let dpad = column![
        row![corner(), arm(Icon::ChevronUp, keys::UP, [ro, ro, ri, ri]), corner()].spacing(2),
        row![
            arm(Icon::ChevronLeft, keys::LEFT, [ro, ri, ri, ro]),
            hub,
            arm(Icon::ChevronRight, keys::RIGHT, [ri, ro, ro, ri]),
        ]
        .spacing(2),
        row![corner(), arm(Icon::ChevronDown, keys::DOWN, [ri, ri, ro, ro]), corner()].spacing(2),
    ]
    .spacing(2);

    let pill = |label: &'static str, bit: u32| key(text(label).size(8.0).into(), bit, 44.0, 14.0, 999.0.into());
    let start_select = column![
        row![iced::widget::Space::new().width(8.0), pill("START", keys::START)],
        pill("SELECT", keys::SELECT),
    ]
    .spacing(4);
    let left_col = column![dpad, start_select].spacing(10);

    let ab_d = 32.0;
    let ab = row![
        column![
            iced::widget::Space::new().height(14.0),
            key(text("B").size(TEXT_BODY).into(), keys::B, ab_d, ab_d, 999.0.into()),
        ],
        column![
            key(text("A").size(TEXT_BODY).into(), keys::A, ab_d, ab_d, 999.0.into()),
            iced::widget::Space::new().height(14.0),
        ],
    ]
    .spacing(6);

    let shoulder = |label: &'static str, bit: u32| key(text(label).size(9.0).into(), bit, 56.0, 15.0, 999.0.into());
    let shoulders = row![shoulder("L", keys::L), horizontal_space(), shoulder("R", keys::R)];
    let face = row![left_col, horizontal_space(), ab].align_y(Alignment::Center);
    column![shoulders, face].spacing(8).width(Length::Fixed(PAD_W)).into()
}

/// Replay-only: the input display overlay — one pad chip per side,
/// the recorder bottom-left and their opponent bottom-right (matching
/// the battle screen, which renders the recording side's navi on the
/// left), each captioned with the side's nickname and lit with the
/// recorded buttons at the playhead. Sampled through [`playhead_tick`]
/// so scrubbing previews inputs along with the readout. Anchored at
/// the transport bar's popover lift so it never moves — the bar
/// auto-hides beneath it, the chips stay. Pure presentation: no mouse
/// handlers anywhere in the chain.
/// Picture-in-picture inset, top-right below the corner commands: the
/// opponent's screen during replay playback (their perspective is
/// re-simulated anyway; this just turns its renderer on). Drawn through
/// its own shader surface ([`PipProgram`]) because the main framebuffer's
/// pipeline owns a single resident texture.
///
/// [`PipProgram`]: crate::platform::video::framebuffer::PipProgram
fn pip_overlay(state: &State) -> Option<Element<'_, Message>> {
    let frame = state.pip_frame.clone()?;
    // 1.5x native: readable without dominating the main view.
    let (w, h) = (frame.width as f32 * 1.5, frame.height as f32 * 1.5);
    let fb = iced::widget::shader::Shader::new(crate::platform::video::framebuffer::PipProgram::new(frame))
        .width(Length::Fixed(w))
        .height(Length::Fixed(h));
    let plate = container(fb).padding(3).style(hud_chip_plate);
    Some(
        container(plate)
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Top)
            .padding(iced::Padding {
                // Clear the corner commands' resting spot.
                top: 56.0,
                right: 12.0,
                bottom: 0.0,
                left: 0.0,
            })
            .into(),
    )
}

fn input_display_overlay<'a>(
    r: &'a ReplaySession,
    state: &'a State,
    show_replay_inputs: bool,
) -> Option<Element<'a, Message>> {
    if !show_replay_inputs {
        return None;
    }
    let (mut local, mut remote) = r.input_at(playhead_tick(r, state));
    let (mut local_nick, mut remote_nick) = r.nicknames();
    // While the perspective is swapped, the main screen is the opponent's
    // — the pads follow it, so the left chip always belongs to whoever is
    // on the big screen.
    if r.swap_perspective() {
        std::mem::swap(&mut local, &mut remote);
        std::mem::swap(&mut local_nick, &mut remote_nick);
    }
    let chip = |joyflags: u16, nick: &str| -> Element<'a, Message> {
        // The caption renders even when the nickname is empty so the
        // two chips always match heights.
        let name = text(nick.to_string())
            .size(TEXT_CAPTION)
            .style(widgets::muted_text_style);
        container(column![input_pad(joyflags), name].spacing(4).align_x(Alignment::Center))
            .padding([8, 10])
            .style(hud_chip_plate)
            .into()
    };
    Some(
        container(row![
            chip(local, local_nick),
            horizontal_space(),
            chip(remote, remote_nick)
        ])
        .width(Fill)
        .height(Fill)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(iced::Padding {
            top: 0.0,
            right: 12.0,
            // Rides up with the analysis strip so the engaged bar's
            // taller plate never slides underneath the pads.
            bottom: POPOVER_LIFT + hover_chart_lift(state),
            left: 12.0,
        })
        .into(),
    )
}

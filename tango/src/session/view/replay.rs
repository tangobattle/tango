//! Replay-playback session view: the transport bar (play/pause +
//! scrubber + speed and display toggles), the input-display overlay,
//! the opponent-screen presentation, and the scrub hover thumbnail. The
//! [`Message`]s those controls emit are handled in
//! [`crate::session::update::replay`].

use super::*;
use crate::session::replay::ReplaySession;
use crate::session::scrubber;
use crate::session::update::replay::{playhead_tick, Message, SPEED_STEPS};
use crate::session::Message as SessionMessage;
// Explicit so these win over iced's prelude `column!`/`row!` macros (see mod.rs).
use sweeten::widget::{column, row};

mod clip;
mod inputs;
use clip::{clip_lift, clip_strip};
use inputs::input_display_overlay;

/// Vertical clearance that floats a bottom-anchored popover just
/// above the replay transport bar (bottom margin + strip padding
/// + control height + row spacing to the collapsed clip-strip slot
/// + row spacing + scrub bar + plate border + gap). The clip strip's
/// expanded height rides on top via [`clip_lift`].
const POPOVER_LIFT: f32 = 12.0 + 16.0 + 32.0 + 4.0 + 4.0 + 26.0 + 2.0 + 6.0;

/// Replay playback: emulator + click-to-play base, the transport bar,
/// input display, PiP inset, and the scrub hover thumbnail.
pub(crate) fn view<'a>(r: &'a ReplaySession, ctx: Ctx<'a>) -> Element<'a, SessionMessage> {
    let Ctx { lang, state, .. } = ctx;
    let now = iced::time::Instant::now();
    // While the input display is on, a recorded touch draws at its
    // spot on the touch screen — the displayed perspective's touch on
    // the main pane, the other side's on the PiP inset — following
    // the swap toggle like the pad chips do.
    let (touch_spot, pip_touch_spot) = if ctx.show_replay_inputs {
        let (mut local, mut remote) = r.touch_at(playhead_tick(r, state));
        if r.swap_perspective() {
            std::mem::swap(&mut local, &mut remote);
        }
        (local, remote)
    } else {
        (None, None)
    };
    let (main_horizontal, main_vertical) = main_frame_alignment(ctx.opponent_view);
    let frame = framebuffer_view(ctx, touch_spot, main_horizontal, main_vertical);
    let frame = stacked_framebuffers(ctx, frame, pip_touch_spot, ctx.opponent_view);
    let body = emulator_body(ctx, frame, [None, None]);
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
        if let Some(info) = replay_info_overlay(lang, r, state) {
            stacked = stacked.push(info);
        }
        stacked = stacked.push(replay_controls(
            lang,
            r,
            state,
            ctx.show_replay_inputs,
            ctx.opponent_view,
            ctx.clip_export_scale,
            ctx.clip_job,
            ctx.queued,
        ));
        stacked = stacked.push(corner_commands_overlay(lang, state, SessionMessage::Close, false));
    }
    // Input display, above the transport bar's resting spot.
    // Deliberately outside the floating-controls gate — the whole
    // point is reading inputs during playback, when the cursor (and
    // the bar with it) has gone idle.
    if let Some(o) = input_display_overlay(r, state, ctx.show_replay_inputs) {
        stacked = stacked.push(o.map(SessionMessage::Replay));
    }
    // PiP: the opponent's screen while that presentation is selected. Also
    // outside the controls gate — it's for watching, so it must not
    // tuck away with the idle cursor.
    if ctx.opponent_view == crate::config::OpponentView::PictureInPicture {
        if let Some(o) = pip_overlay(ctx, pip_touch_spot) {
            stacked = stacked.push(o);
        }
    }
    if let Some(o) = scrub_thumbnail_overlay(state) {
        stacked = stacked.push(o.map(SessionMessage::Replay));
    }
    finish_session_stack(lang, state, stacked)
}

/// The watched recording's identity, top-left opposite the session
/// commands. It rides the controls' auto-hide transition: visible while the
/// viewer is being operated, then clear of the game once the cursor rests.
/// The filename leads; the denser replay header is one muted line beneath it.
/// The plate may use all room left of the corner commands, wrapping only when
/// the window genuinely cannot fit the name on one line.
fn replay_info_overlay<'a>(
    lang: &'a LanguageIdentifier,
    r: &'a ReplaySession,
    state: &'a State,
) -> Option<Element<'a, SessionMessage>> {
    let path = state.replay_path.as_ref()?;
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());

    let metadata = r.metadata();
    let game = crate::library::game::short_name(lang, r.local_game());
    let family = r.local_game().family_and_variant().0;
    let match_type =
        crate::library::game::match_type_name(lang, family, metadata.match_type as u8, metadata.match_subtype as u8);
    let (local, remote) = r.nicknames();
    let players = match (local.is_empty(), remote.is_empty()) {
        (false, false) => Some(format!("{local} vs {remote}")),
        (false, true) => Some(local.to_owned()),
        (true, false) => Some(remote.to_owned()),
        (true, true) => None,
    };
    let recorded_at = std::time::UNIX_EPOCH
        .checked_add(std::time::Duration::from_millis(metadata.ts))
        .map(|time| {
            chrono::DateTime::<chrono::Local>::from(time)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        });
    let detail = std::iter::once(game)
        .chain(std::iter::once(match_type))
        .chain(players)
        .chain(recorded_at)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");

    let copy = column![
        text(filename)
            .size(TEXT_BODY)
            .wrapping(iced::advanced::text::Wrapping::WordOrGlyph),
        text(detail).size(TEXT_CAPTION).style(widgets::muted_text_style),
    ]
    .spacing(2);
    let plate = container(copy).padding([8, 12]).style(hud_chip_plate);
    let slid = anim::slide_in(
        plate,
        state.controls_anim.progress(iced::time::Instant::now()),
        iced::Vector::new(0.0, -CONTROLS_SLIDE),
    );
    Some(
        container(slid)
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Left)
            .align_y(iced::alignment::Vertical::Top)
            .padding(iced::Padding {
                top: 12.0,
                // Two 32 px command buttons, their 6 px gap, and breathing
                // room: the title can grow across the rest without colliding.
                right: 94.0,
                bottom: 12.0,
                left: 12.0,
            })
            .into(),
    )
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
    opponent_view: crate::config::OpponentView,
    clip_export_scale: u8,
    clip_job: Option<ClipJob<'a>>,
    queued: usize,
) -> Element<'a, SessionMessage> {
    let now = iced::time::Instant::now();
    let hide_progress = state.controls_anim.progress(now);
    let panel = container(replay_bar(
        lang,
        r,
        state,
        show_replay_inputs,
        opponent_view,
        clip_export_scale,
        clip_job,
        queued,
    ))
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
/// tick readouts), the clip strip when it's open, and the speed and
/// display toggles. Other session kinds have no transport; their few
/// controls live in the corner commands ([`corner_commands_overlay`]).
fn replay_bar<'a>(
    lang: &'a LanguageIdentifier,
    r: &'a ReplaySession,
    state: &'a State,
    show_replay_inputs: bool,
    opponent_view: crate::config::OpponentView,
    clip_export_scale: u8,
    clip_job: Option<ClipJob<'a>>,
    queued: usize,
) -> Element<'a, Message> {
    // No ellipsis popover for replays — the speed picker sits
    // directly in the bar, and Settings + Close float top-right
    // (see `corner_commands_overlay`).
    // Speed: a dropdown of the steps, triggered by a chip wearing the
    // same plate chrome as the toggles beside it. The current step
    // carries the menu's check and the tooltip names it; the plate
    // lights up while off realtime.
    let current = r.speed();
    let speed_idx = SPEED_STEPS
        .iter()
        .position(|&v| current == v)
        .expect("replay speed is always a transport preset");
    let speed_step_label = |v: f32| {
        if v.fract() == 0.0 {
            format!("{}×", v as i32)
        } else {
            format!("{:.1}×", v)
        }
    };
    let custom_screen_speedup = r.custom_screen_speedup();
    let speed_engaged = speed_idx != 1 || custom_screen_speedup;
    let speed_style = lit_plate_button(speed_engaged);
    let mut speed_items: Vec<widgets::MenuItem<Message>> = SPEED_STEPS
        .iter()
        .enumerate()
        .map(|(i, &v)| widgets::MenuItem::toggle(speed_step_label(v), Message::SetSpeed(v), i == speed_idx))
        .collect();
    let custom_screen_speedup_label = t!(lang, "playback-speed-custom-screen");
    speed_items.push(widgets::MenuItem::toggle(
        custom_screen_speedup_label.clone(),
        Message::ToggleCustomScreenSpeedup,
        custom_screen_speedup,
    ));
    let speed_tooltip = if custom_screen_speedup {
        format!(
            "{}: {} · {}",
            t!(lang, "playback-speed"),
            speed_step_label(SPEED_STEPS[speed_idx]),
            custom_screen_speedup_label,
        )
    } else {
        format!(
            "{}: {}",
            t!(lang, "playback-speed"),
            speed_step_label(SPEED_STEPS[speed_idx]),
        )
    };
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
        .menu_width(240.0)
        // Pin the bar + keep the strip expanded while the pane is up:
        // iced hides the cursor from the base tree while any overlay
        // is open (Cursor::Unavailable), so the hover pin goes blind —
        // and gets actively cleared by its own on_exit — exactly when
        // the chrome must not hide or collapse under the open menu.
        .on_toggle(Message::BarMenuToggled),
        widgets::tooltip_bubble(speed_tooltip),
        iced::widget::tooltip::Position::Top,
    )
    .gap(4);

    // Input display toggle: quiet plate at rest, lit glyph + tinted
    // hairline while the overlay is on — the setup handles'
    // "identity in the glyph" treatment, not a full CTA fill.
    let input_toggle_style = lit_plate_button(show_replay_inputs);
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

    // Opponent view: one menu replaces the old binary PiP toggle. The
    // selected row owns both whether the auxiliary renderer runs and how its
    // surface is laid out; non-Off choices light the trigger like the other
    // display controls.
    let opponent_view_style = lit_plate_button(opponent_view != crate::config::OpponentView::Off);
    let opponent_view_menu = iced::widget::tooltip(
        widgets::MenuButton::new(
            container(opponent_view_icon(opponent_view).widget().size(16.0))
                .width(iced::Length::Fixed(18.0))
                .height(iced::Length::Fixed(18.0))
                .center(Fill),
            opponent_view_items(lang, opponent_view, Message::SetOpponentView, true),
            true,
            [7.0, 7.0],
            crate::ui::style::STANDARD_PADDING,
            opponent_view_style,
        )
        .menu_width(320.0)
        .on_toggle(Message::BarMenuToggled),
        widgets::tooltip_bubble(format!(
            "{} ({}): {}",
            t!(lang, "playback-opponent-view"),
            opponent_view_shortcut(opponent_view),
            opponent_view_label(lang, opponent_view)
        )),
        iced::widget::tooltip::Position::Top,
    )
    .gap(4);

    // Perspective swap: the main screen shows the opponent's re-simulated
    // view; the PiP (if on) carries the local screen. Same chip recipe.
    let swapped = r.swap_perspective();
    let swap_toggle_style = lit_plate_button(swapped);
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

    // Clip tools live behind one scissors toggle so the resting bar
    // stays a transport, not an editor: toggling expands the clip
    // strip (see [`clip_strip`]) between the scrubber and this row.
    let tools_open = state.scrub.tools_open;
    let (mark_in, mark_out) = (state.scrub.mark_in, state.scrub.mark_out);
    let clip_toggle_style = lit_plate_button(tools_open);
    let clip_toggle = iced::widget::tooltip(
        button(
            container(Icon::Scissors.widget().size(16.0))
                .width(iced::Length::Fixed(18.0))
                .height(iced::Length::Fixed(18.0))
                .center(Fill),
        )
        .padding(0)
        .width(iced::Length::Fixed(32.0))
        .height(iced::Length::Fixed(32.0))
        .style(clip_toggle_style)
        .on_press(Message::ToggleClipTools),
        widgets::tooltip_bubble(t!(lang, "playback-clip-tools")),
        iced::widget::tooltip::Position::Top,
    )
    .gap(4);

    // YouTube-style rows: [scrubber, full width] / [play + readout +
    // spacer + chips].
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
    .clip_marks((mark_in, mark_out))
    .view();

    // The clip strip's slot is always in the tree, collapsed to a
    // sliver rather than unmounted: iced diffs widget state by tree
    // position, so mounting it on toggle would shift the controls
    // subtree and reset its widget state mid-interaction — the speed
    // menu's open dropdown died exactly that way.
    let clip_row: Element<'a, Message> = if tools_open {
        clip_strip(lang, state, clip_export_scale, clip_job)
    } else {
        iced::widget::Space::new().height(0.001).into()
    };

    let controls = row![].spacing(10).align_y(Alignment::Center);
    let controls = replay_transport(lang, r, state, controls, queued)
        .push(clip_toggle)
        .push(speed_menu)
        .push(input_toggle)
        .push(opponent_view_menu)
        .push(swap_toggle);

    column![]
        .spacing(4)
        .padding([8, 8])
        .width(Fill)
        .push(scrub)
        .push(clip_row)
        .push(controls)
        .into()
}

/// The replay transport: circular play/pause, current tick, scrubber,
/// total tick — pushed onto the strip in that order.
fn replay_transport<'a>(
    lang: &'a LanguageIdentifier,
    r: &'a ReplaySession,
    state: &State,
    controls: sweeten::widget::Row<'a, Message>,
    queued: usize,
) -> sweeten::widget::Row<'a, Message> {
    let total = r.total_ticks().max(1);
    let cur = playhead_tick(r, state);
    // The emu thread is paused for the duration of a scrub drag and
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
    // Skip-to-next, immediately right of Play — it's transport, not a
    // display toggle, so it belongs in this cluster rather than out with
    // the chips. Icon only: the count is a detail, so it rides the
    // tooltip instead of putting a live number in the bar. Absent
    // entirely when nothing is queued.
    let skip_btn: Option<Element<'a, Message>> = (queued > 0).then(|| {
        iced::widget::tooltip(
            button(
                iced::widget::container(Icon::SkipForward.widget().size(16.0))
                    .width(iced::Length::Fixed(18.0))
                    .height(iced::Length::Fixed(18.0))
                    .center(Fill),
            )
            .padding(0)
            .width(iced::Length::Fixed(32.0))
            .height(iced::Length::Fixed(32.0))
            .style(telemetry_plate_button)
            .on_press(Message::SkipToQueued),
            widgets::tooltip_bubble(t!(lang, "replays-queue-up-next", n = queued as i64)),
            iced::widget::tooltip::Position::Top,
        )
        .gap(4)
        .into()
    });
    let controls = controls.push(play_pause_btn);
    let controls = match skip_btn {
        Some(b) => controls.push(b),
        None => controls,
    };
    controls
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
    // Lift the card over the bar (and over the clip strip, when open)
    // so they never overlap.
    let lift = POPOVER_LIFT + clip_lift(state);
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

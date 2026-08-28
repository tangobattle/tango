//! Replay-playback session view: the transport bar (play/pause +
//! scrubber + speed and display toggles), the input-display overlay,
//! the opponent-screen presentation, and the scrub hover thumbnail — plus the
//! [`Message`]s those controls emit and their [`update`] handler.

use super::*;
use crate::session::replay::ReplaySession;
use crate::session::scrubber;
use crate::session::Message as SessionMessage;
// Explicit so these win over iced's prelude `column!`/`row!` macros (see mod.rs).
use sweeten::widget::{column, row};

/// The replay transport's discrete playback rates, shared by the speed
/// menu and the keyboard stepper so both controls always land on the same
/// values.
const SPEED_STEPS: [f32; 4] = [0.5, 1.0, 2.0, 4.0];

/// Arrow-key seek distance: five seconds of recorded 60 Hz input.
const SEEK_JUMP: i32 = 300;

/// Messages the replay view emits. Wrapped as
/// [`SessionMessage::Replay`] on the way out; inert unless a replay
/// session is active.
#[derive(Debug, Clone)]
pub enum Message {
    /// Toggle play/pause (the transport button, or clicking the
    /// screen itself — any video player's idiom).
    TogglePlay,
    /// Move the playhead by a signed number of recorded frames. Keyboard
    /// frame-step and skip shortcuts both use this transport command.
    SeekRelative(i32),
    /// Seek directly to the replay's first frame (`Home`, or `⌘←` on macOS).
    SeekToStart,
    /// Seek to the start of the next round (`Alt+Right`).
    SeekToNextRound,
    /// Seek to the start of the previous round (`Alt+Left`).
    SeekToPreviousRound,
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
    /// Temporarily raise playback to at least 2× while either player's
    /// custom screen is open. The preference is persisted by the App;
    /// the session owns the live speed change.
    ToggleCustomScreenSpeedup,
    /// Toggle the input display overlay (the recorded pad state of
    /// both sides, drawn over playback). The flag lives in config —
    /// the App's wrapper flips + persists it; nothing to do here.
    ToggleInputDisplay,
    /// Set the quality mode shared with the replay export form. The
    /// replays tab owns this setting; the App wrapper forwards it.
    SetClipExportScale(u8),
    /// Select how the opponent screen is presented. The App wrapper also
    /// persists the choice to config.
    SetOpponentView(crate::config::OpponentView),
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
    /// Expand / collapse the clip strip (the bar's scissors toggle).
    /// The strip carries all the mark/export controls so the resting
    /// bar stays a transport.
    ToggleClipTools,
    /// Stamp the clip-selection start at the playhead (the strip's
    /// mark-in chip). Stamping past the end mark drops the end mark —
    /// the new mark wins, so the pair can never invert.
    SetClipStart,
    /// Stamp the clip-selection end at the playhead — mirror of
    /// [`SetClipStart`](Self::SetClipStart).
    SetClipEnd,
    /// Drop both clip marks (the strip's clear chip).
    ClearClipMarks,
    /// Export the marked span. Handled by the App's wrapper (it needs
    /// the scanners, the save dialog, and the replays tab's export
    /// job machinery); the session handler is a no-op.
    ExportClip { start: u32, end: u32 },
    /// Cancel the running export shown in the clip strip. Also
    /// App-handled — the job's canceller lives in the replays tab.
    CancelClipExport,
    /// Abandon this replay and start the next queued one (the bar's
    /// up-next chip). App-handled: the queue lives in the replays tab and
    /// only the App can build a playback session. No-op here.
    SkipToQueued,
}

/// Map a raw keyboard press to a replay transport command. This stays
/// beside the replay view and its message handler so every replay-only
/// key—including modifier and repeat semantics—has one owner.
pub(crate) fn keyboard_shortcut(event: &iced::keyboard::Event, speed: f32) -> Option<Message> {
    use iced::keyboard::key::{Code, Physical};
    use iced::keyboard::{Event, Modifiers};

    let Event::KeyPressed {
        physical_key: Physical::Code(code),
        modifiers,
        repeat,
        ..
    } = event
    else {
        return None;
    };

    match (*code, *modifiers, *repeat) {
        #[cfg(target_os = "macos")]
        (Code::ArrowLeft, Modifiers::LOGO, false) => Some(Message::SeekToStart),
        (Code::ArrowLeft, Modifiers::ALT, false) => Some(Message::SeekToPreviousRound),
        (Code::ArrowRight, Modifiers::ALT, false) => Some(Message::SeekToNextRound),
        (Code::ArrowLeft, Modifiers::NONE, _) => Some(Message::SeekRelative(-SEEK_JUMP)),
        (Code::ArrowRight, Modifiers::NONE, _) => Some(Message::SeekRelative(SEEK_JUMP)),
        (Code::Home, Modifiers::NONE, false) => Some(Message::SeekToStart),
        (Code::Comma, Modifiers::NONE, _) => Some(Message::SeekRelative(-1)),
        (Code::Period, Modifiers::NONE, _) => Some(Message::SeekRelative(1)),
        (Code::Comma, Modifiers::SHIFT, _) => Some(Message::SetSpeed(
            SPEED_STEPS
                .iter()
                .rev()
                .copied()
                .find(|&candidate| candidate < speed)
                .unwrap_or(SPEED_STEPS[0]),
        )),
        (Code::Period, Modifiers::SHIFT, _) => Some(Message::SetSpeed(
            SPEED_STEPS
                .iter()
                .copied()
                .find(|&candidate| candidate > speed)
                .unwrap_or(SPEED_STEPS[SPEED_STEPS.len() - 1]),
        )),
        (Code::Space, Modifiers::NONE, false) => Some(Message::TogglePlay),
        (Code::KeyI, Modifiers::NONE, false) => Some(Message::ToggleInputDisplay),
        (Code::Tab, Modifiers::NONE, false) => Some(Message::ToggleSwapPerspective),
        // Option reports as Alt on macOS. Keep bare digits free for
        // conventional timeline seeking and make each view a direct preset.
        (Code::Digit1, Modifiers::ALT, false) => Some(Message::SetOpponentView(crate::config::OpponentView::Off)),
        (Code::Digit2, Modifiers::ALT, false) => {
            Some(Message::SetOpponentView(crate::config::OpponentView::PictureInPicture))
        }
        (Code::Digit3, Modifiers::ALT, false) => {
            Some(Message::SetOpponentView(crate::config::OpponentView::StackHorizontally))
        }
        (Code::Digit4, Modifiers::ALT, false) => {
            Some(Message::SetOpponentView(crate::config::OpponentView::StackVertically))
        }
        _ => None,
    }
}

/// Apply a replay-view message. Takes the whole session [`State`]:
/// the scrub bookkeeping lives there, beside the session slot.
pub(crate) fn update(state: &mut State, msg: Message) -> iced::Task<Message> {
    match msg {
        Message::TogglePlay => state.toggle_replay_play(),
        Message::SeekRelative(delta) => {
            if let Some(s) = state.active_as::<ReplaySession>() {
                // Chain off the in-flight seek's target so a burst of
                // presses accumulates instead of snapping to one base tick.
                let base = s.pending_seek_target().unwrap_or_else(|| s.current_tick());
                let target = base.saturating_add_signed(delta).min(s.total_ticks());
                // Preserve the logical play state across the asynchronous
                // seek (the playback thread pauses for the chase either way).
                let playing = !s.is_paused() || s.seek_will_resume();
                s.seek_to(target, playing);
            }
        }
        Message::SeekToStart => {
            if let Some(s) = state.active_as::<ReplaySession>() {
                // Seeking is a transport move, not a pause command: preserve
                // whether playback should resume after the asynchronous chase.
                let playing = !s.is_paused() || s.seek_will_resume();
                s.seek_to(0, playing);
            }
        }
        round_message @ (Message::SeekToNextRound | Message::SeekToPreviousRound) => {
            if let Some(s) = state.active_as::<ReplaySession>() {
                // Chain off an in-flight target so quick repeated presses
                // can traverse several rounds without waiting for each seek.
                let base = s.pending_seek_target().unwrap_or_else(|| s.current_tick());
                let boundaries = s.round_boundaries();
                let forward = matches!(round_message, Message::SeekToNextRound);
                if let Some(target) =
                    round_skip_target(base, &boundaries, forward, s.prefetch_progress())
                {
                    let playing = !s.is_paused() || s.seek_will_resume();
                    s.seek_to(target, playing);
                }
            }
        }
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
        Message::ToggleCustomScreenSpeedup => {
            if let Some(s) = state.active_as::<ReplaySession>() {
                s.set_custom_screen_speedup(!s.custom_screen_speedup());
            }
        }
        Message::ToggleInputDisplay => {
            // Config-owned flag; the App wrapper flips + persists it
            // before this dispatch. The view reads it from config.
        }
        Message::SetOpponentView(view) => {
            if let Some(s) = state.active_as::<ReplaySession>() {
                s.set_opponent_visible(view != crate::config::OpponentView::Off);
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
        Message::SetClipStart => {
            // Field-level borrow (not `active_as`): `scrub` is mutated
            // after the session ref computed the playhead.
            let t = state
                .active
                .as_deref()
                .and_then(|s| s.downcast_ref::<ReplaySession>())
                .map(|s| playhead_tick(s, state));
            if let Some(t) = t {
                state.scrub.mark_in = Some(t);
                if state.scrub.mark_out.is_some_and(|o| o <= t) {
                    state.scrub.mark_out = None;
                }
            }
        }
        Message::SetClipEnd => {
            let t = state
                .active
                .as_deref()
                .and_then(|s| s.downcast_ref::<ReplaySession>())
                .map(|s| playhead_tick(s, state));
            if let Some(t) = t {
                state.scrub.mark_out = Some(t);
                if state.scrub.mark_in.is_some_and(|i| i >= t) {
                    state.scrub.mark_in = None;
                }
            }
        }
        Message::ClearClipMarks => {
            state.scrub.mark_in = None;
            state.scrub.mark_out = None;
        }
        Message::ToggleClipTools => {
            state.scrub.tools_open = !state.scrub.tools_open;
        }
        Message::SetClipExportScale(_)
        | Message::ExportClip { .. }
        | Message::CancelClipExport
        | Message::SkipToQueued => {
            // App-side: see the wrappers in crate::app.
        }
    }
    iced::Task::none()
}

/// Find the round start in `direction` from the section containing `current`.
/// Tick zero is the implicit start of the replay's first section. Forward
/// skips are only safe through the prefetch frontier: later boundaries may be
/// known from a cached analysis even though no seek capture has reached them.
fn round_skip_target(
    current: u32,
    round_boundaries: &[u32],
    forward: bool,
    prefetched_through: u32,
) -> Option<u32> {
    if forward {
        round_boundaries
            .iter()
            .copied()
            .find(|&tick| tick > current && tick <= prefetched_through)
    } else {
        // Boundaries at or before the playhead identify the current
        // section's start. Skip over that one to reach the prior section.
        let passed = round_boundaries
            .iter()
            .take_while(|&&tick| tick <= current)
            .count();
        match passed {
            0 => None,
            1 => Some(0),
            n => Some(round_boundaries[n - 2]),
        }
    }
}

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
    let body = emulator_body(r.local_game(), frame, ctx.hide_emulator_border, [None, None]);
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
/// tick readouts) plus the options trigger, at the chunky
/// BAR_CONTROL_HEIGHT sizing. SP/PvP don't use this — their few
/// controls live in compact corner chips ([`corner_chips`]).
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
    let speed_style = move |theme: &iced::Theme, status: iced::widget::button::Status| {
        let mut st = telemetry_plate_button(theme, status);
        if speed_engaged {
            let primary = theme.palette().primary;
            st.text_color = primary;
            st.border.color = iced::Color { a: 0.35, ..primary };
        }
        st
    };
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

    // Opponent view: one menu replaces the old binary PiP toggle. The
    // selected row owns both whether the auxiliary renderer runs and how its
    // surface is laid out; non-Off choices light the trigger like the other
    // display controls.
    let opponent_view_style = move |theme: &iced::Theme, status: iced::widget::button::Status| {
        let mut st = telemetry_plate_button(theme, status);
        if opponent_view != crate::config::OpponentView::Off {
            let primary = theme.palette().primary;
            st.text_color = primary;
            st.border.color = iced::Color { a: 0.35, ..primary };
        }
        st
    };
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

    // Clip tools live behind one scissors toggle so the resting bar
    // stays a transport, not an editor: toggling expands the clip
    // strip (see [`clip_strip`]) between the scrubber and this row.
    let tools_open = state.scrub.tools_open;
    let (mark_in, mark_out) = (state.scrub.mark_in, state.scrub.mark_out);
    let clip_toggle_style = move |theme: &iced::Theme, status: iced::widget::button::Status| {
        let mut st = telemetry_plate_button(theme, status);
        if tools_open {
            let primary = theme.palette().primary;
            st.text_color = primary;
            st.border.color = iced::Color { a: 0.35, ..primary };
        }
        st
    };
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

/// Fixed height of the expanded clip strip (chips + the row spacing
/// above it come out of [`clip_lift`] too, so the floats above the
/// bar ride up in step).
const CLIP_ROW_H: f32 = 28.0;

/// How much the expanded clip strip grows the bar — added to the
/// bottom-anchored floats' resting lift ([`POPOVER_LIFT`]).
fn clip_lift(state: &State) -> f32 {
    if state.scrub.tools_open {
        CLIP_ROW_H
    } else {
        0.0
    }
}

/// The clip strip: mark-in/mark-out stamps, the marked span's
/// wallclock readout, export quality, clear, and the export CTA —
/// swapped wholesale for a progress line while an export job is
/// running. Lives between the scrubber and the transport row, only
/// while the bar's scissors toggle is on.
fn clip_strip<'a>(
    lang: &'a LanguageIdentifier,
    state: &'a State,
    export_scale: u8,
    job: Option<ClipJob<'a>>,
) -> Element<'a, Message> {
    let chip = |icon: Icon, lit: bool, tip: String, msg: Option<Message>| -> Element<'a, Message> {
        let style = move |theme: &iced::Theme, status: iced::widget::button::Status| {
            let mut st = telemetry_plate_button(theme, status);
            if lit {
                let primary = theme.palette().primary;
                st.text_color = primary;
                st.border.color = iced::Color { a: 0.35, ..primary };
            }
            st
        };
        iced::widget::tooltip(
            button(
                container(icon.widget().size(14.0))
                    .width(iced::Length::Fixed(16.0))
                    .height(iced::Length::Fixed(16.0))
                    .center(Fill),
            )
            .padding(0)
            .width(iced::Length::Fixed(26.0))
            .height(iced::Length::Fixed(26.0))
            .style(style)
            .on_press_maybe(msg),
            widgets::tooltip_bubble(tip),
            iced::widget::tooltip::Position::Top,
        )
        .gap(4)
        .into()
    };

    // A running export replaces the tools with its progress — the
    // strip is the player-side face of the same per-replay job the
    // replays tab shows, so there's exactly one of these at a time.
    if let Some(job) = job.filter(|j| j.result.is_none()) {
        let pct = if job.total > 0 {
            (job.completed as f32 / job.total as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let caption = if job.cancelling {
            t!(lang, "replays-export-cancelling")
        } else {
            format!(
                "{} {}%",
                t!(lang, "replays-export-progress"),
                (pct * 100.0).round() as u32
            )
        };
        let cancel = chip(
            Icon::X,
            false,
            t!(lang, "replays-export-cancel"),
            (!job.cancelling).then_some(Message::CancelClipExport),
        );
        return container(
            row![
                text(caption).size(TEXT_CAPTION).style(widgets::muted_text_style),
                iced::widget::progress_bar(0.0..=1.0, pct)
                    .girth(Length::Fixed(4.0))
                    .style(widgets::slim_progress_bar),
                cancel,
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .height(iced::Length::Fixed(CLIP_ROW_H))
        .align_y(iced::alignment::Vertical::Center)
        .into();
    }

    let (mark_in, mark_out) = (state.scrub.mark_in, state.scrub.mark_out);
    // Mark stamps ride beside their chips in the transport's numeral
    // treatment; an unset mark shows a muted placeholder so setting
    // one never reflows the row.
    let stamp = |mark: Option<u32>| {
        let (label, style): (String, fn(&iced::Theme) -> iced::widget::text::Style) = match mark {
            Some(m) => (crate::session::format_tick(m), |theme: &iced::Theme| {
                iced::widget::text::Style {
                    color: Some(theme.palette().primary),
                }
            }),
            None => ("–:––".to_string(), widgets::muted_text_style),
        };
        text(label).size(12).font(iced::Font::MONOSPACE).style(style)
    };
    let mut strip = row![
        chip(
            Icon::ArrowRightFromLine,
            mark_in.is_some(),
            t!(lang, "playback-clip-start"),
            Some(Message::SetClipStart),
        ),
        stamp(mark_in),
        chip(
            Icon::ArrowRightToLine,
            mark_out.is_some(),
            t!(lang, "playback-clip-end"),
            Some(Message::SetClipEnd),
        ),
        stamp(mark_out),
        chip(
            Icon::Eraser,
            false,
            t!(lang, "playback-clip-clear"),
            (mark_in.is_some() || mark_out.is_some()).then_some(Message::ClearClipMarks),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    // The marked span's length, once it exists.
    if let (Some(a), Some(b)) = (mark_in, mark_out) {
        strip = strip.push(
            text(format!("({})", crate::session::format_tick(b - a)))
                .size(12)
                .style(widgets::muted_text_style),
        );
    }
    strip = strip.push(iced::widget::space::horizontal());
    // The last job's outcome, quietly, with the full line in a
    // tooltip when it failed.
    if let Some(job) = job {
        match job.result {
            Some(Ok(())) => {
                strip = strip.push(
                    text(t!(lang, "replays-export-success"))
                        .size(TEXT_CAPTION)
                        .style(widgets::muted_text_style),
                );
            }
            Some(Err(e)) => {
                strip = strip.push(
                    iced::widget::tooltip(
                        text(t!(lang, "replays-export-error", error = "…"))
                            .size(TEXT_CAPTION)
                            .style(widgets::muted_text_style),
                        widgets::tooltip_bubble(e.to_string()),
                        iced::widget::tooltip::Position::Top,
                    )
                    .gap(4),
                );
            }
            None => {}
        }
    }
    // Full replay exports use this exact picker and state too.
    let quality_menu = widgets::replay_export_scale_picker(
        lang,
        export_scale,
        Message::SetClipExportScale,
        Some(Message::BarMenuToggled),
    );
    strip = strip.push(quality_menu);
    // The one CTA in the strip: primary once a valid span exists.
    let export_msg = match (mark_in, mark_out) {
        (Some(start), Some(end)) if start < end => Some(Message::ExportClip { start, end }),
        _ => None,
    };
    strip = strip.push(
        button(text(t!(lang, "playback-clip-export")).size(12))
            .padding([4, 10])
            .height(iced::Length::Fixed(26.0))
            .style(widgets::primary_button)
            .on_press_maybe(export_msg),
    );
    container(strip)
        .height(iced::Length::Fixed(CLIP_ROW_H))
        .align_y(iced::alignment::Vertical::Center)
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
fn input_pad<'a>(joyflags: u16, ds: bool) -> Element<'a, Message> {
    use tango_session::keys;
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
    // Start/Select below the face cluster, plainly stacked — the DS
    // face arrangement, same as the settings shell draws.
    let start_select = column![pill("START", keys::START), pill("SELECT", keys::SELECT)].spacing(4);

    let ab_d = 32.0;
    let face_key =
        |label: &'static str, bit: u32| key(text(label).size(TEXT_BODY).into(), bit, ab_d, ab_d, 999.0.into());
    // The face cluster shows the recorded console's keys: a DS pad
    // gets the full diamond (the settings shell's, at chip scale), a
    // GBA pad keeps its two-key diagonal.
    let cluster: Element<'a, Message> = if ds {
        use iced::alignment::{Horizontal as Ax, Vertical as Ay};
        let diamond_box = 90.0;
        let place = |el, ax, ay| {
            container(el)
                .width(Length::Fixed(diamond_box))
                .height(Length::Fixed(diamond_box))
                .align_x(ax)
                .align_y(ay)
        };
        iced::widget::stack![
            place(face_key("X", keys::X), Ax::Center, Ay::Top),
            place(face_key("Y", keys::Y), Ax::Left, Ay::Center),
            place(face_key("A", keys::A), Ax::Right, Ay::Center),
            place(face_key("B", keys::B), Ax::Center, Ay::Bottom),
        ]
        .into()
    } else {
        row![
            column![iced::widget::Space::new().height(14.0), face_key("B", keys::B)],
            column![face_key("A", keys::A), iced::widget::Space::new().height(14.0)],
        ]
        .spacing(6)
        .into()
    };
    let right_col = column![cluster, start_select].spacing(10).align_x(Alignment::Center);

    let shoulder = |label: &'static str, bit: u32| key(text(label).size(9.0).into(), bit, 56.0, 15.0, 999.0.into());
    // A DS recording carries the mic, so the chip shows when the
    // recorder was blowing into it, on the hinge between the shoulders
    // where the console's own hole is. It is drawn whether or not it
    // was held, like every other key here.
    let shoulders = if ds {
        row![
            shoulder("L", keys::L),
            horizontal_space(),
            key(text("BLOW").size(9.0).into(), keys::MIC, 44.0, 15.0, 999.0.into()),
            horizontal_space(),
            shoulder("R", keys::R),
        ]
    } else {
        row![shoulder("L", keys::L), horizontal_space(), shoulder("R", keys::R)]
    };
    let face = row![dpad, horizontal_space(), right_col].align_y(Alignment::Center);
    // The diamond needs more shell than the GBA diagonal.
    let pad_w = if ds { 176.0 } else { PAD_W };
    column![shoulders, face].spacing(8).width(Length::Fixed(pad_w)).into()
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
    // X and Y mean a DS, and the pads draw the full face diamond for
    // one. Asked of the console rather than counted off the screens:
    // a session composes only the screens its mode uses, so a DS can
    // present one.
    let ds = {
        use tango_session::keys;
        r.local_game().pvp.keys_mask() & (keys::X | keys::Y) != 0
    };
    let chip = |joyflags: u16, nick: &str| -> Element<'a, Message> {
        // The caption renders even when the nickname is empty so the
        // two chips always match heights.
        let name = text(nick.to_string())
            .size(TEXT_CAPTION)
            .style(widgets::muted_text_style);
        container(column![input_pad(joyflags, ds), name].spacing(4).align_x(Alignment::Center))
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
            // Rides up with the clip strip so the expanded bar's
            // taller plate never slides underneath the pads.
            bottom: POPOVER_LIFT + clip_lift(state),
            left: 12.0,
        })
        .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::key::{Code, Physical};
    use iced::keyboard::{Event, Key, Location, Modifiers};

    fn key_press(code: Code, modifiers: Modifiers, repeat: bool) -> Event {
        Event::KeyPressed {
            key: Key::Unidentified,
            modified_key: Key::Unidentified,
            physical_key: Physical::Code(code),
            location: Location::Standard,
            modifiers,
            text: None,
            repeat,
        }
    }

    #[test]
    fn keyboard_shortcuts_keep_transport_semantics_together() {
        assert!(matches!(
            keyboard_shortcut(&key_press(Code::Period, Modifiers::NONE, true), 1.0,),
            Some(Message::SeekRelative(1))
        ));
        assert!(matches!(
            keyboard_shortcut(&key_press(Code::Home, Modifiers::NONE, false), 1.0),
            Some(Message::SeekToStart)
        ));
        #[cfg(target_os = "macos")]
        assert!(matches!(
            keyboard_shortcut(&key_press(Code::ArrowLeft, Modifiers::LOGO, false), 1.0),
            Some(Message::SeekToStart)
        ));
        assert!(matches!(
            keyboard_shortcut(&key_press(Code::ArrowLeft, Modifiers::ALT, false), 1.0),
            Some(Message::SeekToPreviousRound)
        ));
        assert!(matches!(
            keyboard_shortcut(&key_press(Code::ArrowRight, Modifiers::ALT, false), 1.0),
            Some(Message::SeekToNextRound)
        ));
        assert!(matches!(
            keyboard_shortcut(&key_press(Code::Period, Modifiers::SHIFT, false), 1.0,),
            Some(Message::SetSpeed(2.0))
        ));
        assert!(matches!(
            keyboard_shortcut(&key_press(Code::Tab, Modifiers::NONE, false), 1.0,),
            Some(Message::ToggleSwapPerspective)
        ));
        assert!(matches!(
            keyboard_shortcut(&key_press(Code::KeyI, Modifiers::NONE, false), 1.0,),
            Some(Message::ToggleInputDisplay)
        ));
        assert!(matches!(
            keyboard_shortcut(&key_press(Code::Digit1, Modifiers::ALT, false), 1.0),
            Some(Message::SetOpponentView(crate::config::OpponentView::Off)),
        ));
        assert!(matches!(
            keyboard_shortcut(&key_press(Code::Digit2, Modifiers::ALT, false), 1.0),
            Some(Message::SetOpponentView(crate::config::OpponentView::PictureInPicture)),
        ));
        assert!(matches!(
            keyboard_shortcut(&key_press(Code::Digit3, Modifiers::ALT, false), 1.0),
            Some(Message::SetOpponentView(crate::config::OpponentView::StackHorizontally)),
        ));
        assert!(matches!(
            keyboard_shortcut(&key_press(Code::Digit4, Modifiers::ALT, false), 1.0),
            Some(Message::SetOpponentView(crate::config::OpponentView::StackVertically)),
        ));
        assert!(keyboard_shortcut(&key_press(Code::Tab, Modifiers::NONE, true), 1.0,).is_none());
        assert!(keyboard_shortcut(&key_press(Code::KeyI, Modifiers::NONE, true), 1.0,).is_none());
        assert!(keyboard_shortcut(&key_press(Code::KeyP, Modifiers::NONE, false), 1.0,).is_none());
        assert!(keyboard_shortcut(&key_press(Code::Digit1, Modifiers::NONE, false), 1.0,).is_none());
        assert!(keyboard_shortcut(&key_press(Code::Digit1, Modifiers::ALT, true), 1.0,).is_none());
        assert!(keyboard_shortcut(&key_press(Code::ArrowRight, Modifiers::ALT, true), 1.0).is_none());
        assert!(keyboard_shortcut(&key_press(Code::Home, Modifiers::NONE, true), 1.0).is_none());
    }

    #[test]
    fn round_shortcuts_move_between_rounds_within_the_prefetch_frontier() {
        let boundaries = [300, 600, 900];

        assert_eq!(round_skip_target(0, &boundaries, true, 299), None);
        assert_eq!(round_skip_target(0, &boundaries, true, 300), Some(300));
        assert_eq!(round_skip_target(450, &boundaries, true, 599), None);
        assert_eq!(round_skip_target(450, &boundaries, true, 600), Some(600));
        assert_eq!(round_skip_target(600, &boundaries, true, 899), None);
        assert_eq!(round_skip_target(600, &boundaries, true, 900), Some(900));
        assert_eq!(round_skip_target(900, &boundaries, true, u32::MAX), None);

        assert_eq!(round_skip_target(750, &boundaries, false, 0), Some(300));
        assert_eq!(round_skip_target(600, &boundaries, false, 0), Some(300));
        assert_eq!(round_skip_target(450, &boundaries, false, 0), Some(0));
        assert_eq!(round_skip_target(300, &boundaries, false, 0), Some(0));
        assert_eq!(round_skip_target(1, &boundaries, false, 0), None);
        assert_eq!(round_skip_target(0, &boundaries, false, 0), None);
    }
}

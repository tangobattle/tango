//! Replay-playback controls: the transport, scrub and clip messages the
//! replay view emits, what they do to the live session, the scrub bar's
//! interaction state, and the keyboard shortcuts that map onto them.

use super::Effect;
use crate::config;
use crate::session::replay::ReplaySession;
use crate::session::{scrubber, Session as _, State};

/// The replay transport's discrete playback rates, shared by the speed
/// menu and the keyboard stepper so both controls always land on the same
/// values.
pub(crate) const SPEED_STEPS: [f32; 4] = [0.5, 1.0, 2.0, 4.0];

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
    /// custom screen is open. Applied to the live session and persisted
    /// as a preference.
    ToggleCustomScreenSpeedup,
    /// Toggle the input display overlay (the recorded pad state of
    /// both sides, drawn over playback). The flag lives in config.
    ToggleInputDisplay,
    /// Set the quality mode shared with the replay export form. The
    /// replays tab owns this setting.
    SetClipExportScale(u8),
    /// Select how the opponent screen is presented, and persist the
    /// choice.
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
    /// Export the marked span: the session captures the clip, and the
    /// App runs the save dialog and the replays tab's export job.
    ExportClip { start: u32, end: u32 },
    /// Cancel the running export shown in the clip strip. The job's
    /// canceller lives in the replays tab.
    CancelClipExport,
    /// Abandon this replay and start the next queued one (the bar's
    /// up-next chip). The queue lives in the replays tab and only the App
    /// can build a playback session.
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

/// Scrub-bar interaction state for a replay session. Splits the
/// drag/hover bookkeeping out of the game-mode-agnostic parts of
/// [`State`]; the owning state holds one of these and the transport
/// widget reads it to draw the playhead + the floating keyframe
/// thumbnail, decoded only when the hovered keyframe changes.
#[derive(Default)]
pub struct Scrub {
    /// `Some(tick)` while the user is dragging — the previewed
    /// position. The transport draws the playhead here instead of at
    /// the emulator's actual tick, and the first event of a drag
    /// pauses playback.
    pub preview: Option<u32>,
    /// Whether playback was running when the drag started, so
    /// [`end_drag`](Self::end_drag)'s commit can resume it once the
    /// seek lands.
    pub resume: bool,
    /// Whether this drag has blitted a keyframe preview yet. Until it
    /// has, the live frame is still on screen and beats a farther
    /// keyframe; afterwards previews always blit (the live frame is
    /// gone from the buffer).
    pub blitted: bool,
    /// Where the cursor is resting on the scrub bar, driving the
    /// floating thumbnail card above it. `None` when the cursor is off
    /// the bar — and during a drag, when the full-screen blit preview
    /// supersedes it.
    pub hover: Option<scrubber::HoverInfo>,
    /// Image handle for the snapshot behind the hover thumbnail,
    /// keyed by the snapshot's absolute tick so cursor moves within
    /// the same keyframe reuse the handle instead of rebuilding it.
    pub thumb: Option<(u32, iced::widget::image::Handle)>,
    /// Whether the transport bar's clip strip is expanded (the
    /// scissors toggle). The strip owns the mark/export controls so
    /// the resting bar stays a transport.
    pub tools_open: bool,
    /// Clip-selection start mark (playhead tick), set by the clip
    /// strip's mark-in chip. Setting a mark that would invert the
    /// pair drops the other mark, so `mark_in < mark_out` always
    /// holds when both are set.
    pub mark_in: Option<u32>,
    /// Clip-selection end mark — see [`mark_in`](Self::mark_in).
    pub mark_out: Option<u32>,
}

impl Scrub {
    /// Begin or continue a drag at `target`. The first event of a drag
    /// freezes playback under the cursor (remembering whether to
    /// resume) and starts blitting previews from the snapshot buffers.
    fn drag(&mut self, target: u32, replay: &ReplaySession) {
        let press = self.preview.is_none();
        if press {
            self.resume = !replay.is_paused();
            replay.set_paused(true);
        }
        self.preview = Some(target);
        // The press itself only previews an exact frame: a click seeks
        // to the tick under the cursor, and blitting the *nearest*
        // keyframe there would flash a wrong frame until the chase
        // delivers the real one. Once the drag is actually moving,
        // nearest-keyframe previews are the scrubbing feedback.
        let blitted = if press {
            replay.scrub_preview_exact(target)
        } else {
            replay.scrub_preview(target, self.blitted)
        };
        if blitted {
            self.blitted = true;
        }
    }

    /// Reset the per-drag fields once a drag is released. The actual
    /// (asynchronous) seek is fired by the caller, which still owns the
    /// `&ReplaySession` — this just clears the drag bookkeeping.
    fn end_drag(&mut self) {
        self.preview = None;
        self.resume = false;
        self.blitted = false;
    }

    /// Refresh the floating hover thumbnail for the current
    /// [`hover`](Self::hover) position. Caches by the nearest
    /// snapshot's absolute tick, so cursor moves within one keyframe
    /// reuse the decoded handle.
    fn refresh_thumb(&mut self, replay: &ReplaySession) {
        let Some(h) = self.hover else { return };
        if let Some(snap) = replay.nearest_snapshot(h.tick) {
            let snap_tick = snap.frame_index();
            if self.thumb.as_ref().map(|(t, _)| *t) != Some(snap_tick) {
                let fb = snap.local_framebuffer();
                // Length-checked rather than just non-empty: a capture
                // that doesn't match the session's declared shape would
                // otherwise upload as a sheared texture.
                let (w, h) = replay.frame_size();
                if fb.len() == (w * h * 4) as usize {
                    self.thumb = Some((snap_tick, iced::widget::image::Handle::from_rgba(w, h, fb)));
                }
            }
        }
    }
}

/// Play/pause the active replay — the transport button's
/// [`Message::TogglePlay`] and the spacebar keybind.
fn toggle_play(state: &State) {
    let Some(s) = state.active_as::<ReplaySession>() else {
        return;
    };
    if s.seek_will_resume() {
        // An in-flight seek is about to resume playback, so the
        // button shows "Pause" — honor the press as one: land the
        // seek, stay paused.
        s.cancel_seek_resume();
    } else {
        // Play at end-of-replay: rewind to start and play through
        // again. Mirrors any media player — "play" on a finished
        // track restarts it. The seek is asynchronous, so resuming
        // is deferred to the chase landing — unpausing here would
        // run frames off the end before the rewind starts.
        let paused = s.is_paused();
        if paused && s.current_tick() >= s.total_ticks() {
            s.seek_to(0, true);
        } else {
            s.set_paused(!paused);
        }
    }
}

/// Apply a replay-view message. Takes the whole session [`State`]:
/// the scrub bookkeeping lives there, beside the session slot.
pub(crate) fn update(state: &mut State, msg: Message, config: &config::Config) -> Option<Effect> {
    match msg {
        Message::TogglePlay => toggle_play(state),
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
                if let Some(target) = round_skip_target(base, &boundaries, forward, s.prefetch_progress()) {
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
            let s = state.active_as::<ReplaySession>()?;
            let enabled = !s.custom_screen_speedup();
            s.set_custom_screen_speedup(enabled);
            return Some(Effect::PersistCustomScreenSpeedup(enabled));
        }
        Message::ToggleInputDisplay => {
            // Config-owned flag, which the view reads from config.
            return Some(Effect::PersistReplayInputs(!config.show_replay_inputs));
        }
        Message::SetOpponentView(view) => {
            if let Some(s) = state.active_as::<ReplaySession>() {
                s.set_opponent_visible(view != config::OpponentView::Off);
            }
            return Some(Effect::PersistOpponentView(view));
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
        Message::SetClipExportScale(scale) => return Some(Effect::SetClipExportScale(scale)),
        Message::ExportClip { start, end } => {
            // Capture the live seek snapshot and perspective now; the save
            // dialog in between is asynchronous.
            let replay = state.replay_path.clone()?;
            let (snapshot, round_marks, swap_sides) = state
                .active_as::<ReplaySession>()
                .map(|s| (s.clip_start_capture(start), s.round_boundaries(), s.swap_perspective()))
                .unwrap_or_default();
            return Some(Effect::ExportClip {
                replay,
                clip: crate::replay_render::Clip {
                    start,
                    end,
                    snapshot,
                    round_marks,
                },
                swap_sides,
            });
        }
        Message::CancelClipExport => return state.replay_path.clone().map(Effect::CancelClipExport),
        Message::SkipToQueued => return Some(Effect::SkipToQueued),
    }
    None
}

/// Find the round start in `direction` from the section containing `current`.
/// Tick zero is the implicit start of the replay's first section. Forward
/// skips are only safe through the prefetch frontier: later boundaries may be
/// known from a cached analysis even though no seek capture has reached them.
fn round_skip_target(current: u32, round_boundaries: &[u32], forward: bool, prefetched_through: u32) -> Option<u32> {
    if forward {
        round_boundaries
            .iter()
            .copied()
            .find(|&tick| tick > current && tick <= prefetched_through)
    } else {
        // Boundaries at or before the playhead identify the current
        // section's start. Skip over that one to reach the prior section.
        let passed = round_boundaries.iter().take_while(|&&tick| tick <= current).count();
        match passed {
            // There is no earlier round to select from round 1, so restart
            // it instead. At tick zero the shortcut is already satisfied.
            0 => (current > 0).then_some(0),
            1 => Some(0),
            n => Some(round_boundaries[n - 2]),
        }
    }
}

/// The playhead position everything user-facing reads: the tick under
/// an active drag, else the target of an in-flight seek (so readouts
/// don't snap back while the chase catches up), else the emulator's
/// actual position — clamped to the replay's length. Shared by the
/// transport's readout/scrubber and the input display's lookup so
/// they can never disagree.
pub(crate) fn playhead_tick(r: &ReplaySession, state: &State) -> u32 {
    state
        .scrub
        .preview
        .or_else(|| r.pending_seek_target())
        .unwrap_or_else(|| r.current_tick())
        .min(r.total_ticks().max(1))
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
        assert_eq!(round_skip_target(1, &boundaries, false, 0), Some(0));
        assert_eq!(round_skip_target(0, &boundaries, false, 0), None);
    }
}

//! The app-facing half of emulator sessions: per-session UI state,
//! Message + update + view + subscription. Owned by App as
//! `session: session::State` and routed via `Message::Session(_)`.
//!
//! The sessions themselves — the drive threads, the audio stream, the
//! netplay transport — live in the [`tango_session`] crate, which
//! knows nothing about iced; this module re-exports its surface (so
//! `crate::session::pvp::PvpSession` etc. keep resolving), owns the
//! iced-shaped state layered on top (the PvP setup panes, the replay
//! scrub bookkeeping, the post-match results cook), and dispatches
//! each session kind to its view.
//!
//! The Play / Replays tabs are responsible for STARTING sessions
//! (they construct an Session via [`build_playback`] /
//! [`spawn_singleplayer`] and stuff it into `state.active`); this
//! module handles everything that happens after.

pub mod scrubber;
pub mod view;

pub use tango_session::{pvp, replay, singleplayer, Session};

use crate::app::Scanners;
use crate::config;
use crate::i18n::t;
use crate::library::game;
use crate::library::patch;
use crate::platform::audio;
use crate::platform::video::framebuffer::Effect;
use crate::save_view;
use crate::selection;
use crate::ui::anim;
use crate::ui::style::{self, TEXT_BODY, TEXT_CAPTION};
use crate::ui::widgets;
use iced::widget::canvas::{self, Canvas, Frame, LineCap, Path, Stroke};
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{button, container, stack, text};
use iced::{mouse, Alignment, Color, Element, Fill, Length, Point, Rectangle, Renderer, Theme};
use lucide_icons::Icon;
use pvp::{suggest_frame_delay, MAX_FRAME_DELAY, MIN_FRAME_DELAY};
use unic_langid::LanguageIdentifier;

/// One per-frame snapshot of the live PvP telemetry, retained in a short ring
/// buffer ([`State::metric_history`]) so the match-settings popover can draw a
/// sparkline per metric. `round` is `None` between rounds, when no
/// skew/lead/depth reading exists; when present it is `(skew, depth, lead)`.
#[derive(Clone, Copy)]
pub struct MetricSample {
    pub tps: f32,
    pub fps_target: f32,
    pub ping_ms: u128,
    pub round: Option<(i32, u32, i32)>,
}

impl MetricSample {
    /// Read the current telemetry off a live PvP session. Called once per
    /// emulator frame by the [`Message::UpdateFramebuffer`] handler.
    fn capture(pvp: &pvp::PvpSession) -> Self {
        Self {
            tps: pvp.tps(),
            fps_target: pvp.fps_target(),
            // Raw latest ping (not the median) — the sparkline is a live
            // display, so it should track the true per-frame reading and
            // show spikes. The median feeds only the frame-delay suggestion.
            ping_ms: pvp.latency_raw().map_or(0, |d| d.as_millis()),
            round: pvp.round_stats().map(|s| (s.skew, s.depth, s.lead)),
        }
    }
}

/// How many frames of telemetry the sparklines retain (~3 s at 60 fps).
const METRIC_HISTORY_LEN: usize = 180;

/// The watched replay's cooked analysis rounds, drawn as the minimal
/// hover strip above the playback transport's scrubber
/// ([`crate::ui::widgets::hp_hover_strip`]). Cooked by the App when the
/// playback session starts (from the Replays tab's already-cooked
/// chart when available, else from the stats sidecar) and re-cooked
/// live while a background analysis is still building this replay's
/// stats — the App watches the tab's progress messages for `path`.
/// Empty `rounds` (no stats at all) draw no strip.
pub struct ReplayChart {
    pub path: std::path::PathBuf,
    pub rounds: Vec<crate::ui::widgets::CookedHpRound>,
}

/// PvP-only presentation state riding alongside the session engine:
/// both sides' fully-loaded selections (rom + parsed save + derived
/// assets) for the in-match setup drawers, plus each drawer's
/// save-view tab/grouping state. Built by [`spawn_pvp`] and installed
/// into [`State::pvp_panes`] with the session; the loadeds also feed
/// the post-match results cook ([`MatchResults::capture`]).
pub struct PvpPanes {
    /// Local side's loaded selection — the "my setup" drawer.
    pub local_loaded: Option<selection::Loaded>,
    /// Opponent's loaded selection, unless they blinded their setup.
    pub opponent_loaded: Option<selection::Loaded>,
    /// Active-tab / grouping state for the self save-view panel.
    pub local_save_view: save_view::State,
    /// Active-tab / grouping state for the opponent save-view panel.
    pub opponent_save_view: save_view::State,
}

// A Loaded is a whole parsed rom + save; a placeholder keeps the
// enclosing app Message (which carries a `Slot<(PvpSession, PvpPanes)>`)
// derivable, same as PreMatchData.
impl std::fmt::Debug for PvpPanes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PvpPanes { .. }")
    }
}

/// Scrub-bar interaction state for a replay session. Splits the
/// drag/hover bookkeeping out of the game-mode-agnostic parts of
/// [`State`]; the owning state holds one of these and the transport
/// widget reads it to draw the playhead + the floating keyframe
/// thumbnail.
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
    /// RGBA conversion of the snapshot behind the hover thumbnail,
    /// keyed by the snapshot's absolute tick so cursor moves within
    /// the same keyframe reuse the handle instead of re-converting.
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
    pub fn drag(&mut self, target: u32, replay: &replay::ReplaySession) {
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
    pub fn end_drag(&mut self) {
        self.preview = None;
        self.resume = false;
        self.blitted = false;
    }

    /// Refresh the floating hover thumbnail for the current
    /// [`hover`](Self::hover) position. Caches by the nearest
    /// snapshot's absolute tick, so cursor moves within one keyframe
    /// reuse the decoded handle.
    pub fn refresh_thumb(&mut self, replay: &replay::ReplaySession) {
        let Some(h) = self.hover else { return };
        if let Some(snap) = replay.nearest_snapshot(h.tick) {
            let snap_tick = snap.key_tick();
            let fb = snap.local_framebuffer();
            if !fb.is_empty() && self.thumb.as_ref().map(|(t, _)| *t) != Some(snap_tick) {
                self.thumb = Some((snap_tick, thumbnail_handle(fb)));
            }
        }
    }

    /// Drop all scrub state, drag and hover alike — used when the
    /// session closes.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// How the match on the results screen came to its end. The disconnect
/// variant renders the same card at rest — no reveal choreography, and a
/// "connection lost" headline instead of a verdict (the match never
/// finished, so declaring victory or defeat would be a lie).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MatchEnd {
    /// Natural end: the deciding round finished and the runout elapsed.
    Completed,
    /// The remote vanished mid-match: their channel EOF'd or the
    /// reconnect window expired.
    Disconnected,
}

/// Snapshot of a finished PvP match, taken at the session teardown
/// (`is_ended`) and shown as the post-match results screen until dismissed:
/// on a natural end, and on a remote disconnect (the match state as it
/// stood — see [`MatchEnd`]). Owned data only — the session (and everything
/// network-side) is already gone while this is on screen. User-initiated
/// quits (Esc hold, disconnect confirm) skip the capture: the player chose
/// to leave, so they go straight back to the menu.
pub struct MatchResults {
    pub remote_nickname: String,
    /// How the match ended — picks the card's dress (verdict reveal vs
    /// the quiet disconnect layout).
    pub end: MatchEnd,
    /// Per-round outcome + presentation-ready HP trace, in play order. Empty
    /// when the match tore down before any round finished (e.g. a comm error
    /// mid-round-1) — the screen shows a neutral headline then.
    pub rounds: Vec<RoundCard>,
    /// Session start to local completion.
    pub duration: std::time::Duration,
    /// The replay recorded for this match, for the Watch button. `None` if
    /// the writer failed to open at match start.
    pub replay_path: Option<std::path::PathBuf>,
    /// The match-wide HP scale the round traces were normalized against —
    /// the chart's hover readout multiplies back through it.
    pub max_hp: f32,
    /// When the results screen was put up — the zero point of its reveal
    /// choreography (per-round HP sweeps, then the verdict stamp). One-shot:
    /// returning from a replay watch finds it long elapsed, so the card sits
    /// at rest instead of replaying its entrance.
    pub revealed_at: iced::time::Instant,
}

/// One round on the results card: the outcome plus the cooked series for
/// the round graph. `trace` points are `(x, you, opponent)`, all normalized —
/// x over the round's sampled ticks, HP against the match-wide maximum so
/// every round shares one vertical scale; `custom` is the normalized
/// `[start, end)` x spans where the custom screen stood open. Empty when the
/// round produced no HP samples (torn down mid-intro).
pub struct RoundCard {
    pub outcome: tango_match::analysis::BattleOutcome,
    pub trace: Vec<(f32, f32, f32)>,
    pub custom: Vec<(f32, f32)>,
    /// Chip-use events per side (`[you, opponent]`), cooked for the
    /// graph's event lanes. Names/icons are resolved at capture time —
    /// the session (and both sides' Loadeds) is gone while the card is
    /// on screen — each side through its own Loaded, the opponent
    /// falling back to the local game's table when they blinded their
    /// setup. Empty on games whose traps don't report chips (bn1).
    pub chip_uses: [Vec<crate::ui::widgets::ChipUseMark>; 2],
    /// Tick span of the round — its share of the continuous timeline.
    pub weight: f32,
}

impl MatchResults {
    fn capture(pvp: &pvp::PvpSession, panes: Option<&PvpPanes>, end: MatchEnd) -> Self {
        // The same aggregation the replay sidecar gets: the match folded
        // each round into its MatchStatsBuilder as it ended, so this snapshot
        // can never disagree with what the Replays tab later shows for
        // the same match.
        let stats = pvp.stats_snapshot();
        let local_loaded = panes.and_then(|p| p.local_loaded.as_ref());
        let loadeds = [
            local_loaded,
            panes.and_then(|p| p.opponent_loaded.as_ref()).or(local_loaded),
        ];
        // No plan: the results cards are per-round, so each round's
        // trace anchors at its own first sample.
        let (cooked, max_hp) = crate::ui::widgets::cook_hp_rounds(&stats, loadeds, None);
        let rounds = cooked
            .into_iter()
            .filter_map(|c| {
                // Live reports always carry an outcome — the match only
                // pushes them when a round actually ends.
                Some(RoundCard {
                    outcome: c.outcome?,
                    trace: c.trace,
                    custom: c.custom,
                    chip_uses: c.chip_uses,
                    weight: c.weight,
                })
            })
            .collect::<Vec<_>>();
        let results = Self {
            remote_nickname: pvp.remote_nickname.clone(),
            end,
            rounds,
            duration: pvp.match_duration(),
            replay_path: pvp.replay_path.clone(),
            max_hp,
            revealed_at: iced::time::Instant::now(),
        };
        anim::kick(view::results::reveal_duration(&results));
        results
    }
}

/// Post-match results for the results screen, snapshotted at teardown
/// (right before the `is_ended` close drops the session) — `None` for
/// everything but a PvP match that ran to completion or lost its
/// remote: on a natural end the results card comes up with its reveal
/// choreography, on a remote disconnect (their channel EOF'd or the
/// reconnect window expired) in its disconnect dress with the match as
/// it stood. Our own quit paths (Esc hold, disconnect confirm) set
/// neither flag and go straight back to the menu: the player chose to
/// leave.
fn capture_results(session: &dyn Session, panes: Option<&PvpPanes>) -> Option<MatchResults> {
    let pvp = session.downcast_ref::<pvp::PvpSession>()?;
    if pvp.is_completed() {
        Some(MatchResults::capture(pvp, panes, MatchEnd::Completed))
    } else if pvp.remote_disconnected() {
        Some(MatchResults::capture(pvp, panes, MatchEnd::Disconnected))
    } else {
        None
    }
}

/// Per-session UI state. App holds `session: State`; the Play and
/// Replays tabs swap an `Session` into `active` to start a
/// session, then [`State::update`] handles the rest until [`Close`]
/// clears it.
pub struct State {
    pub active: Option<Box<dyn Session>>,
    /// Count of sessions ever installed — bumped by
    /// [`session_installed`](Self::session_installed), and the frame
    /// [`subscription`]'s identity for the active session. Keying the
    /// wake stream by anything address-based (the `Notify` Arc's
    /// pointer) would be ABA-prone: a new session can allocate at a
    /// dropped one's address, and iced would keep the old stream —
    /// parked on the old Notify — instead of spinning up the new one.
    session_seq: u64,
    /// Keeps the active session's audio stream routed into the host
    /// output for exactly the session's lifetime — dropping it returns
    /// the [`audio::LateBinder`] to silence. Set beside `active` at
    /// install (the spawn helpers hand it back alongside the session),
    /// cleared first in [`close_session`](State::close_session) so the
    /// stream stops pulling before the session's cores wind down.
    pub audio_binding: Option<audio::Binding>,
    /// PvP-only: the two sides' loaded assets + save-view panel state
    /// for the in-match setup drawers — the presentation state the
    /// session engine deliberately doesn't carry. Set alongside
    /// `active` when a PvP session installs, cleared on close.
    pub pvp_panes: Option<PvpPanes>,
    /// Analysis chart for the active replay-playback session — see
    /// [`ReplayChart`]. Set alongside `active` on watch, cleared on
    /// close.
    pub replay_chart: Option<ReplayChart>,
    /// Post-match results, `Some` from a PvP session's natural end until the
    /// user dismisses the results screen. Deliberately not cleared by
    /// [`close_session`](State::close_session): watching the recorded replay
    /// from the results screen runs a whole replay session, and closing that
    /// should land back on the results. The App's view routes here whenever
    /// no session is active.
    pub results: Option<MatchResults>,
    /// PvP-only: the opponent's save-view side panel, shown when
    /// they haven't blinded their setup. Defaults to hidden; user
    /// opens it via the edge handle. The drawer slides in from the
    /// screen edge and the edge handle rides its moving inner edge.
    pub opponent_panel: anim::Overlay,
    /// PvP-only: the local player's save-view side panel. Defaults
    /// to hidden; user toggles it via the red toolbar button. Slides
    /// the same way as [`opponent_panel`](Self::opponent_panel).
    pub self_panel: anim::Overlay,
    /// Combined keyboard + gamepad held state. Updated from
    /// the input event stream; the user's Mapping resolves it
    /// into mgba joyflags each event.
    pub input_held: crate::platform::input::HeldState,
    /// Last value of `mapping.speed_up_held(...)` so we can
    /// detect the falling/rising edge and only call set_speed
    /// when it actually flips.
    pub speed_up_engaged: bool,
    /// In-session Settings overlay. Toggled by the Settings
    /// icon in the status bar (`Message::OpenSettings`) and the
    /// "back to session" button on the overlay itself
    /// (`Message::CloseSettings`). The emulator keeps running
    /// underneath; we just swap what `App::view` renders.
    pub settings: anim::Overlay,
    /// PvP-only: the "are you sure?" modal that gates the
    /// Disconnect item in the options menu. Disconnect tears the
    /// session down mid-match (same as Close), so the confirm
    /// keeps a stray click from costing the user a real game.
    pub disconnect: anim::Overlay,
    /// PvP-only: the match-settings popover, anchored above the
    /// telemetry plate (instrument panel) and toggled by clicking it.
    /// Holds the live frame-delay control (moved here from the footer).
    /// Mutually exclusive with the options menu.
    pub match_settings: anim::Overlay,
    /// Latest GBA framebuffer (post upscale filter), presented by the
    /// [`crate::platform::video::framebuffer`] shader widget. Refreshed in
    /// [`Message::UpdateFramebuffer`] (which the session subscription
    /// fires once per emulator vblank). `None` between sessions and
    /// before the first frame lands.
    pub current_frame: Option<crate::platform::video::framebuffer::Frame>,
    /// Monotonic counter stamped into each [`current_frame`] so the
    /// framebuffer pipeline can skip re-uploading when the same frame
    /// is presented twice (a UI redraw with no new emu frame).
    pub frame_revision: u64,
    /// Replay-only: the opponent's screen while the PiP toggle is on,
    /// drawn as a picture-in-picture inset by the session view. `None`
    /// whenever the PiP isn't live. Rebuilt alongside
    /// [`current_frame`](Self::current_frame) each emu frame.
    pub pip_frame: Option<crate::platform::video::framebuffer::Frame>,
    /// [`frame_revision`](Self::frame_revision)'s twin for the PiP
    /// surface (a separate GPU texture with its own upload dedupe).
    pub pip_revision: u64,
    /// Rolling window of PvP telemetry snapshots (newest at the back),
    /// sampled once per frame from the [`Message::UpdateFramebuffer`] handler
    /// and drawn as sparklines in the match-settings popover. Capped at
    /// [`METRIC_HISTORY_LEN`]; cleared whenever the active session is not a
    /// live PvP match.
    pub metric_history: std::collections::VecDeque<MetricSample>,
    /// Replay-only: scrub-bar interaction state (drag preview, the
    /// floating hover thumbnail, and the bookkeeping that ties them to
    /// the running playback session). Inert outside a replay session.
    pub scrub: Scrub,
    /// Wall-clock of the last cursor movement over the session
    /// view — drives the floating controls' auto-hide. Bumped by
    /// [`Message::MouseMoved`] and on session start
    /// ([`State::session_installed`]).
    pub last_mouse_move: std::time::Instant,
    /// Cursor is currently over the floating controls bar — pins
    /// it visible regardless of the idle timer.
    pub controls_hovered: bool,
    /// A transport-bar dropdown is open — pins the bar (and its hover
    /// strip) like `controls_hovered` does, which can't cover this
    /// case itself: see [`Message::BarMenuToggled`].
    pub bar_menu_open: bool,
    /// Instant the current Esc hold started, `None` while Esc is up.
    /// Armed on the first [`Message::EscPressed`] of a physical hold
    /// (key repeat re-fires the message but not the arm), cleared on
    /// [`Message::EscReleased`]. Drives hold-to-quit: the view draws
    /// the exit overlay for the whole hold, and at [`ESC_QUIT_HOLD`]
    /// the [`update`](State::update) wrapper tears the session down.
    pub esc_hold: Option<std::time::Instant>,
    /// Show/hide transition for the floating controls bar. Synced
    /// after every update: shown while the mouse moved recently,
    /// the cursor rests on the bar, any overlay is open, a scrub
    /// is in flight, or a replay is paused. Unlike the [`anim::Overlay`]
    /// fields above it has no companion bool — its target is recomputed
    /// from those inputs each update rather than toggled by a handler.
    pub controls_anim: anim::Transition,
}

impl Default for State {
    fn default() -> Self {
        Self {
            active: None,
            session_seq: 0,
            audio_binding: None,
            pvp_panes: None,
            replay_chart: None,
            results: None,
            opponent_panel: anim::Overlay::new(false),
            self_panel: anim::Overlay::new(false),
            input_held: crate::platform::input::HeldState::default(),
            speed_up_engaged: false,
            settings: anim::Overlay::new(false),
            disconnect: anim::Overlay::new(false),
            match_settings: anim::Overlay::new(false),
            current_frame: None,
            frame_revision: 0,
            pip_frame: None,
            pip_revision: 0,
            metric_history: std::collections::VecDeque::new(),
            scrub: Scrub::default(),
            last_mouse_move: std::time::Instant::now(),
            controls_hovered: false,
            bar_menu_open: false,
            esc_hold: None,
            controls_anim: anim::Transition::new(true),
        }
    }
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// True iff a session is running. Drives main.rs's view routing.
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// The active session as concrete kind `T` — `None` while idle or
    /// when a different kind is running.
    pub fn active_as<T: Session>(&self) -> Option<&T> {
        self.active.as_deref().and_then(|s| s.downcast_ref())
    }
}

/// Messages the session pane emits + handles. All variants are
/// inert when `state.active` is `None`.
#[derive(Debug, Clone)]
pub enum Message {
    /// Close the session and return to the previous tab.
    Close,
    /// Cursor moved anywhere over the session view. Resets the
    /// floating controls' idle timer.
    MouseMoved,
    /// Cursor entered (`true`) / left (`false`) the floating
    /// controls bar. While inside, the bar never auto-hides.
    ControlsHovered(bool),
    /// Raw input event from the keyboard or a gamepad. The
    /// handler updates the held-state set, resolves the user's
    /// Mapping into joyflags, and pushes them to the active
    /// session. Speed-up uses the same mechanism (edge-
    /// detected).
    Input(InputEvent),
    /// Replay-view messages (transport, scrubber, display toggles) —
    /// defined + handled in [`view::replay`].
    Replay(view::replay::Message),
    /// PvP-view messages (frame delay, setup panels, save views,
    /// disconnect confirm) — defined + handled in [`view::pvp`].
    Pvp(view::pvp::Message),
    /// Post-match results screen messages — defined in
    /// [`view::results`]. Dismiss is handled here; WatchReplay by the
    /// App wrapper (building a playback session needs the scanners +
    /// config).
    Results(view::results::Message),
    /// User pressed Esc inside a session. Dismisses whichever overlay
    /// is on top (settings modal, disconnect confirm, match-settings
    /// popover) if any, and arms the hold-to-quit timer — a tap never
    /// tears the session down, but holding Esc for [`ESC_QUIT_HOLD`]
    /// does (with the exit overlay counting down the hold). Routed
    /// here rather than from the InputCapture so the decision sees
    /// the current overlay state.
    EscPressed,
    /// Esc came back up — disarms hold-to-quit.
    EscReleased,
    /// Redraw/quit-check heartbeat while Esc is held, from the
    /// [`subscription`] timer branch. No handler work of its own:
    /// the elapsed-hold check lives in the [`update`](State::update)
    /// wrapper, and a paused replay (or a mid-reconnect PvP pause)
    /// produces no frame wakes to run it — this keeps the overlay
    /// filling and the quit firing anyway.
    EscHoldTick,
    /// Show the in-session Settings overlay. The emulator keeps
    /// running; only the visible body swaps. Replaces the
    /// legacy in-game pause menu.
    OpenSettings,
    /// Hide the in-session Settings overlay (the "back to
    /// session" button on the overlay's header).
    CloseSettings,
    /// One emulator frame has landed, or `is_ended` could have
    /// flipped (PvP peer-end / disconnect / grace-timeout). The
    /// handler rebuilds the framebuffer from the active session's
    /// [`frame`](Session::frame) into [`State::current_frame`]
    /// and tears the session down if it's now ended. Fired by the
    /// session subscription, which parks on the active session's
    /// [`wake`](Session::wake) — signalled by both the frame
    /// callback and the PvP end-detection wires.
    UpdateFramebuffer,
    /// Click-swallower for modal panel chrome — keeps presses
    /// on the panel's inert regions from falling through to the
    /// dismiss-on-press backdrop layer beneath it.
    NoOp,
}

/// Atomic input event we feed to the mapping resolver. Lives in
/// [`crate::platform::input`] (as [`Event`](crate::platform::input::Event)) because the
/// settings input pane's live binding highlight consumes the same
/// normalized stream.
pub use crate::platform::input::Event as InputEvent;

/// Per-keypress playhead delta for the replay seek keybinds, in recorded
/// frames. Arrow keys jump ±5 seconds (300 frames at 60fps); comma/period
/// frame-step by ±1. `None` for any other key.
fn replay_seek_delta(physical: iced::keyboard::key::Physical) -> Option<i32> {
    use iced::keyboard::key::{Code, Physical};
    // 5 seconds at the GBA's 60fps.
    const JUMP: i32 = 300;
    match physical {
        Physical::Code(Code::ArrowLeft) => Some(-JUMP),
        Physical::Code(Code::ArrowRight) => Some(JUMP),
        Physical::Code(Code::Comma) => Some(-1),
        Physical::Code(Code::Period) => Some(1),
        _ => None,
    }
}

impl State {
    /// Apply a session message to the state. Returns the iced Task
    /// that should be scheduled (always Task::none today — kept for
    /// API parity with the other tabs).
    pub fn update(&mut self, msg: Message, mapping: &crate::platform::input::Mapping) -> iced::Task<Message> {
        let task = self.update_inner(msg, mapping);
        // Hold-to-quit: Esc held to the threshold tears the session
        // down, same as the Close button. Checked here on every
        // message (the 60 Hz frame wakes, plus the dedicated
        // EscHoldTick stream when the emulator is paused) instead of
        // in a handler, so it doesn't care which message crossed the
        // finish line.
        if self.esc_hold.is_some_and(|t| t.elapsed() >= ESC_QUIT_HOLD) {
            if self.active.is_some() {
                self.close_session();
            } else {
                // The session went away mid-hold with the release
                // swallowed by the view unmount — disarm so the tick
                // subscription doesn't run forever.
                self.esc_hold = None;
            }
        }
        // Mirror each overlay's bool into its transition in one
        // place — handlers above flip them freely and the
        // animations follow, including the multi-flip paths (Esc
        // peeling, mutual-exclusion closes).
        let now = iced::time::Instant::now();
        self.settings.sync(now);
        self.disconnect.sync(now);
        self.match_settings.sync(now);
        self.self_panel.sync(now);
        self.opponent_panel.sync(now);
        // Floating controls auto-hide. The per-frame
        // UpdateFramebuffer messages re-run this, so the idle
        // timer expires without needing its own timer source; a
        // paused replay (no frames) pins the bar visible anyway.
        let replay_paused = self.active_as::<replay::ReplaySession>().is_some_and(|r| r.is_paused());
        // The telemetry panel (match_settings) deliberately
        // doesn't count: it lives in the permanently-visible
        // top-right indicator, independent of the HUD controls,
        // so leaving the graph open shouldn't pin the chips up.
        let overlay_open = self.settings.shown() || self.disconnect.shown();
        let show_controls = self.controls_hovered
            || self.bar_menu_open
            || overlay_open
            || replay_paused
            || self.scrub.preview.is_some()
            // Clip tools stay pinned while expanded — marking in/out
            // is a multi-step interaction with idle stretches between.
            || self.scrub.tools_open
            || self.last_mouse_move.elapsed() < CONTROLS_HIDE_AFTER;
        self.controls_anim.set(show_controls, now);
        task
    }

    /// Post-install hook — the App calls this right after stuffing a
    /// new session into [`active`](Self::active). Stamps the session's
    /// identity ([`session_seq`](Self::session_seq), which keys the
    /// frame subscription) and resets the floating controls' idle
    /// timer so the bar greets the user visible even if the mouse
    /// hasn't moved in a while. Also clears the hover pin: closing a
    /// session removes its widgets without any `on_exit` firing (the
    /// cursor is usually ON the close button), and a latched
    /// `controls_hovered` would pin the next session's chrome on
    /// screen permanently.
    pub fn session_installed(&mut self) {
        self.session_seq += 1;
        self.last_mouse_move = std::time::Instant::now();
        self.controls_hovered = false;
        // Same reasoning as the hover pin — a menu whose widget went
        // away with the old session never publishes its close.
        self.bar_menu_open = false;
        // Belt-and-braces: a hold left over from a previous session
        // (its release swallowed with the session view) must not
        // count against the new one.
        self.esc_hold = None;
    }

    /// Tear down the active session: PvP pre-drop close request, then
    /// drop-by-clearing plus the reset of every piece of per-session
    /// UI state. Shared by [`Message::Close`] (the Close button /
    /// disconnect confirm) and the Esc hold-to-quit expiry in
    /// [`update`](State::update).
    fn close_session(&mut self) {
        if let Some(s) = self.active.as_ref() {
            s.request_close();
        }
        // Unbind audio before dropping the session so the output stream
        // stops pulling from cores that are about to wind down.
        self.audio_binding = None;
        self.active = None;
        self.pvp_panes = None;
        self.replay_chart = None;
        self.current_frame = None;
        self.pip_frame = None;
        self.controls_hovered = false;
        self.bar_menu_open = false;
        self.disconnect.close();
        self.match_settings.close();
        self.scrub.clear();
        self.esc_hold = None;
    }

    /// Play/pause the active replay (no-op for other session kinds).
    /// Shared by the transport button's [`Message::TogglePlay`] and the
    /// spacebar keybind.
    fn toggle_replay_play(&self) {
        if let Some(s) = self.active_as::<replay::ReplaySession>() {
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
    }

    fn update_inner(&mut self, msg: Message, mapping: &crate::platform::input::Mapping) -> iced::Task<Message> {
        match msg {
            Message::Close => {
                self.close_session();
            }
            Message::Input(ev) => {
                // Replay transport keybinds: arrow keys jump ±5s, comma/period
                // step ±1 frame. A replay plays back recorded input, so these
                // keys are free to drive the seek bar; live sessions fall
                // through to the joyflag pipeline below as normal. Fires on
                // every press, key-repeat included, so holding scrubs.
                if let InputEvent::Key {
                    physical,
                    pressed: true,
                } = &ev
                {
                    // Edge: compare against the held set *before* the match
                    // below records this press, so OS key-repeat (which the
                    // seek keys want but the pause toggle doesn't) is filtered.
                    let fresh_press = !self.input_held.is_key_held(physical);
                    if let Some(s) = self.active_as::<replay::ReplaySession>() {
                        if let Some(delta) = replay_seek_delta(*physical) {
                            // Chain off the in-flight seek's target so a burst
                            // of presses accumulates instead of all snapping to
                            // the same spot.
                            let base = s.pending_seek_target().unwrap_or_else(|| s.current_tick());
                            let target = base.saturating_add_signed(delta).min(s.total_ticks());
                            // Preserve the logical play state across the seek
                            // (the thread is paused for the chase either way).
                            let playing = !s.is_paused() || s.seek_will_resume();
                            s.seek_to(target, playing);
                        } else if fresh_press
                            && matches!(
                                physical,
                                iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Space)
                            )
                        {
                            self.toggle_replay_play();
                        }
                    }
                }
                self.input_held.apply(&ev);
                let joyflags = mapping.to_mgba_keys(&self.input_held);
                if let Some(s) = self.active.as_ref() {
                    s.set_joyflags(joyflags);
                }
                // Speed-up: only fire set_speed on the rising or
                // falling edge so we don't spam mgba's audio
                // sync target with no-op writes.
                let now_engaged = mapping.speed_up_held(&self.input_held);
                if now_engaged != self.speed_up_engaged {
                    self.speed_up_engaged = now_engaged;
                    let factor = if now_engaged { 4.0 } else { 1.0 };
                    if let Some(s) = self.active.as_ref() {
                        s.set_speed(factor);
                    }
                }
            }
            // Kind-specific view messages — defined + handled beside
            // the views that emit them.
            Message::Replay(m) => return view::replay::update(self, m).map(Message::Replay),
            Message::Pvp(m) => return view::pvp::update(self, m).map(Message::Pvp),
            Message::Results(m) => match m {
                view::results::Message::Dismiss => self.results = None,
                // App-level: the wrapper intercepts this and builds the
                // playback session (needs scanners + config).
                view::results::Message::WatchReplay => {}
            },
            Message::EscPressed => {
                // Arm hold-to-quit on the first press of a physical
                // hold only — OS key repeat re-fires EscPressed, and
                // re-arming would push the deadline out forever.
                if self.esc_hold.is_none() {
                    self.esc_hold = Some(std::time::Instant::now());
                }
                // Peel overlays off top-down: the settings modal, then
                // the disconnect confirm, then the match-settings
                // popover. A tap stops there — tearing the session
                // down takes an explicit button action or the full
                // [`ESC_QUIT_HOLD`] hold.
                if self.settings.shown() {
                    self.settings.close();
                } else if self.disconnect.shown() {
                    self.disconnect.close();
                } else if self.match_settings.shown() {
                    self.match_settings.close();
                }
            }
            Message::EscReleased => {
                self.esc_hold = None;
            }
            Message::EscHoldTick => {
                // Nothing here — the hold check lives in `update`'s
                // wrapper so every message runs it; this variant only
                // exists to generate message traffic while held.
            }
            Message::MouseMoved => {
                self.last_mouse_move = std::time::Instant::now();
            }
            Message::ControlsHovered(h) => {
                self.controls_hovered = h;
            }
            Message::NoOp => {}
            Message::OpenSettings => {
                self.settings.open();
            }
            Message::CloseSettings => {
                self.settings.close();
            }
            Message::UpdateFramebuffer => {
                // Telemetry snapshot for the popover sparklines, captured while
                // the session is borrowed below and pushed afterward. `None`
                // (no live PvP match) clears the history so a fresh match — or
                // a return to SP/replay — starts the charts clean.
                let mut sample = None;
                if let Some(session) = self.active.as_ref() {
                    // PvP self-closes when the per-game match-end
                    // hook + peer-end handshake (or grace timeout)
                    // are both satisfied. The end-detection paths
                    // each call `notify_one()` so this branch fires
                    // even after the emu thread has paused.
                    if session.is_ended() {
                        // Snapshot the finished match for the results
                        // screen before the teardown drops the session —
                        // see [`capture_results`].
                        let results = capture_results(session.as_ref(), self.pvp_panes.as_ref());
                        self.close_session();
                        self.results = results;
                    } else {
                        // Upload the native frame as-is; the selected effect
                        // magnifies it on the GPU at draw time.
                        let pixels = session.frame();
                        self.frame_revision = self.frame_revision.wrapping_add(1);
                        self.current_frame = Some(crate::platform::video::framebuffer::Frame {
                            pixels: std::sync::Arc::new(pixels),
                            width: replay::SCREEN_WIDTH,
                            height: replay::SCREEN_HEIGHT,
                            revision: self.frame_revision,
                            // Neutral placeholder — the view picks the live
                            // effect from config at draw time (see
                            // `framebuffer_view`), so the producer doesn't need
                            // to know the current filter.
                            effect: &crate::platform::video::effects::PASSTHROUGH,
                        });
                        sample = session.downcast_ref::<pvp::PvpSession>().map(MetricSample::capture);
                        // Replay PiP: the opponent's screen while the bar
                        // toggle is on.
                        self.pip_frame = session.pip_frame().map(|pixels| {
                            self.pip_revision = self.pip_revision.wrapping_add(1);
                            crate::platform::video::framebuffer::Frame {
                                pixels: std::sync::Arc::new(pixels),
                                width: replay::SCREEN_WIDTH,
                                height: replay::SCREEN_HEIGHT,
                                revision: self.pip_revision,
                                // The PiP draws at a small fixed size; no
                                // upscale filter, just the plain surface.
                                effect: &crate::platform::video::effects::PASSTHROUGH,
                            }
                        });
                    }
                }
                match sample {
                    Some(s) => {
                        self.metric_history.push_back(s);
                        while self.metric_history.len() > METRIC_HISTORY_LEN {
                            self.metric_history.pop_front();
                        }
                    }
                    None => self.metric_history.clear(),
                }
            }
        }
        iced::Task::none()
    }
}

/// Per-emulator-frame wake stream. Yields
/// [`Message::UpdateFramebuffer`] each time someone signals the active
/// session's [`wake`](Session::wake) — the
/// per-frame callback for new screen contents, and the PvP
/// end-detection wires (peer-end packet, peer disconnect, grace
/// timeout) for state-transition checks. Lives only while a session is
/// active, keyed by [`State::session_seq`] so each new session swaps in
/// a stream on its own fresh wake (a signal fired before the stream
/// spins up isn't lost — Notify stores the permit). Keyboard input still
/// flows through [`crate::platform::input_capture`] — see that
/// module's docs for why the subscription path is too laggy for
/// joypad state.
pub fn subscription(state: &State) -> iced::Subscription<Message> {
    let mut subs = Vec::new();
    if let Some(wake) = state.active.as_deref().map(|s| s.wake()) {
        subs.push(iced::Subscription::run_with(
            FrameTag {
                id: state.session_seq,
                wake,
            },
            build_frame_stream,
        ));
    }
    // The scrub bar's prefetch-progress fill is only repainted on redraw,
    // and a paused (or mid-seek) replay fires no `frame_notify` — so the bar
    // would sit frozen while the background prefetcher races ahead. Tick a
    // redraw at ~20 Hz for the duration of the prefetch so it fills live.
    // Playback already redraws at 60 Hz from the frame callback, hence the
    // `is_paused` gate, and the whole thing switches off once prefetch lands.
    let prefetching = state
        .active_as::<replay::ReplaySession>()
        .is_some_and(|r| r.is_paused() && r.prefetch_progress() < r.total_ticks());
    if prefetching {
        subs.push(iced::time::every(std::time::Duration::from_millis(50)).map(|_| Message::UpdateFramebuffer));
    }
    // While Esc is held, tick ~30 Hz so the exit overlay's progress
    // bar fills (and the quit fires) even when the emulator isn't
    // producing frame wakes — a paused replay or a mid-reconnect
    // PvP pause. Live sessions redraw at 60 Hz regardless; the tick
    // is only ever load-bearing on the paused paths, and it stops
    // the moment the key comes back up.
    if state.esc_hold.is_some() {
        subs.push(iced::time::every(std::time::Duration::from_millis(33)).map(|_| Message::EscHoldTick));
    }
    iced::Subscription::batch(subs)
}

/// Frame-stream subscription identity. Hashes the install counter
/// ([`State::session_seq`]), so iced keeps one stream alive per
/// session (stable across view rebuilds) and rebuilds it when a new
/// session — with a fresh wake — comes up. The `wake` payload carries
/// the actual handle through to [`build_frame_stream`].
struct FrameTag {
    id: u64,
    wake: std::sync::Arc<tokio::sync::Notify>,
}

impl std::hash::Hash for FrameTag {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        ("session-frame", self.id).hash(h);
    }
}

fn build_frame_stream(tag: &FrameTag) -> impl futures::Stream<Item = Message> {
    futures::stream::unfold(tag.wake.clone(), |wake| async move {
        wake.notified().await;
        Some((Message::UpdateFramebuffer, wake))
    })
}

/// Optional iced texture handle for a Game's background art. Pulls
/// the TGA out of the appropriate BNLC volume's shared `exe.dat` and
/// caches the decoded iced `Handle` per game. `None` whenever Steam
/// / BNLC / the target entry can't be read — caller drops the
/// background widget instead of degrading to a placeholder.
fn background_handle(game: &'static crate::library::game::Game) -> Option<iced::widget::image::Handle> {
    use std::collections::HashMap;
    use std::sync::LazyLock;
    static CACHE: LazyLock<std::sync::Mutex<HashMap<usize, Option<iced::widget::image::Handle>>>> =
        LazyLock::new(Default::default);
    let key = game as *const _ as usize;
    if let Some(cached) = CACHE.lock().unwrap().get(&key).cloned() {
        return cached;
    }
    let bg = game.background;
    let path = format!("exe/data/bg/{}", bg.tga);
    let handle = crate::library::bnlc::get(bg.volume)
        .and_then(|b| b.read_shared_file(&path))
        .and_then(|bytes| {
            // TGA has no magic prefix, so the image crate's
            // auto-detect refuses to guess it. Pass the format
            // explicitly — every shared-archive background is TGA.
            image::load_from_memory_with_format(&bytes, image::ImageFormat::Tga)
                .inspect_err(|e| log::warn!("bnlc bg {:?}/{}: decode: {e}", bg.volume, bg.tga))
                .ok()
        })
        .map(|img| {
            let rgba = img.into_rgba8();
            let (w, h) = rgba.dimensions();
            iced::widget::image::Handle::from_rgba(w, h, rgba.into_raw())
        });
    CACHE.lock().unwrap().insert(key, handle.clone());
    handle
}

/// How long the cursor has to sit still before the floating
/// controls slide away.
const CONTROLS_HIDE_AFTER: std::time::Duration = std::time::Duration::from_millis(2500);

/// How long Esc must be held down to quit the active session. The
/// countdown chip appears the moment the hold arms — no grace
/// period; it's a compact HUD chip, not a dim, so an Esc tap just
/// flashes it as feedback that the key registered.
const ESC_QUIT_HOLD: std::time::Duration = std::time::Duration::from_secs(3);

/// Expand an mgba-native BGR555 framebuffer (one little-endian `u16`
/// per pixel — what [`Session::frame`] hands back) to an RGBA8 image handle for
/// the hover thumbnail, via dataview's shared conversion — the same
/// table that renders ROM sprites/palettes and replay video exports,
/// and the CPU twin of the GPU decode in `video/effects/common.wgsl`.
/// At 240×160 it's cheap, and it only runs when the hovered keyframe
/// changes.
fn thumbnail_handle(framebuffer: &[u8]) -> iced::widget::image::Handle {
    let mut rgba = vec![0u8; framebuffer.len() * 2];
    tango_dataview::rom::bgr555_to_rgba8(framebuffer, &mut rgba);
    iced::widget::image::Handle::from_rgba(replay::SCREEN_WIDTH, replay::SCREEN_HEIGHT, rgba)
}

/// Route a freshly-built session's audio stream into the host output,
/// returning the RAII binding the caller stores beside the session
/// ([`State::audio_binding`]). A failed bind is logged and downgraded
/// to silence rather than aborting the session — no session kind
/// depends on the audio device.
fn bind_session_audio(
    audio_binder: &audio::LateBinder,
    stream: tango_session::core_stream::CoreStream,
) -> Option<audio::Binding> {
    match audio_binder.bind(Some(Box::new(stream))) {
        Ok(b) => Some(b),
        Err(e) => {
            log::warn!("session audio bind failed: {e:?}");
            None
        }
    }
}

/// Decode a `.tangoreplay`, resolve both sides' ROM (+ optional
/// patch) from the scanners, and spin up a playback session with its
/// audio routed into the shared output. Ready to drop straight into
/// the app's `session` slot (the binding goes in
/// [`State::audio_binding`]).
pub fn build_playback(
    scanners: &Scanners,
    config: &config::Config,
    audio_binder: &audio::LateBinder,
    path: &std::path::Path,
    // Have the prefetch pass double as the match-stats analysis — see
    // [`replay::PrefetchStatsJob`] and `App::replay_stats_takeover`.
    stats_job: Option<replay::PrefetchStatsJob>,
) -> anyhow::Result<(replay::ReplaySession, Option<audio::Binding>)> {
    let f = std::fs::File::open(path)?;
    let replay = std::sync::Arc::new(tango_replay::Replay::decode(f)?);
    let patches_path = config.patches_path();
    let resolve_rom = |side: Option<&tango_replay::metadata::Side>| -> anyhow::Result<(
        &'static game::Game,
        std::sync::Arc<Vec<u8>>,
    )> {
        let gi = side
            .and_then(|s| s.game_info.as_ref())
            .ok_or_else(|| anyhow::anyhow!("replay side has no game info"))?;
        let variant = u8::try_from(gi.rom_variant)
            .map_err(|_| anyhow::anyhow!("variant {} out of range", gi.rom_variant))?;
        let entry = crate::library::game::find_by_family_and_variant(&gi.rom_family, variant)
            .ok_or_else(|| anyhow::anyhow!("unknown rom {}/{}", gi.rom_family, gi.rom_variant))?;
        let g = game::from_gamedb_entry(entry).ok_or_else(|| {
            anyhow::anyhow!("no impl for {}/{}", gi.rom_family, gi.rom_variant)
        })?;
        let rom = scanners
            .roms
            .read()
            .get(&entry)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("rom for {}/{} not scanned", gi.rom_family, gi.rom_variant))?;
        let rom = if let Some(patch_info) = gi.patch.as_ref() {
            let v = semver::Version::parse(&patch_info.version)?;
            patch::apply_patch_from_disk(&rom, entry, &patches_path, &patch_info.name, &v)?
        } else {
            rom
        };
        Ok((g, std::sync::Arc::new(rom)))
    };

    let (p1_game, p1_rom) = resolve_rom(replay.metadata.side(0))?;
    let (p2_game, p2_rom) = resolve_rom(replay.metadata.side(1))?;
    let (session, audio) = replay::ReplaySession::new(
        [p1_game, p2_game],
        [p1_rom, p2_rom],
        replay,
        audio_binder.sample_rate(),
        config.show_opponent_pip,
        stats_job,
    )?;
    Ok((session, bind_session_audio(audio_binder, audio)))
}

/// Build the live PvP session from the netplay handoff data
/// plus the local selection + scanners, along with the [`PvpPanes`]
/// presentation state (both sides' Loadeds + save-view panels) the
/// App installs beside it. Async because PvpSession::new awaits the
/// lobby loop's receiver handoff, and because remote-side rom
/// resolution might apply a patch.
pub async fn spawn_pvp(
    scanners: Scanners,
    config: config::Config,
    audio_binder: audio::LateBinder,
    local_game: crate::library::rom::GameRef,
    local_patch: Option<(String, semver::Version)>,
    pre_match: crate::netplay::PreMatchData,
) -> anyhow::Result<(pvp::PvpSession, PvpPanes, Option<audio::Binding>)> {
    let local_game_impl =
        game::from_gamedb_entry(local_game).ok_or_else(|| anyhow::anyhow!("no impl for local game"))?;
    let local_rom_raw = scanners
        .roms
        .read()
        .get(&local_game)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("local rom not scanned"))?;
    let local_rom_bytes = if let Some((name, version)) = local_patch.as_ref() {
        patch::apply_patch_from_disk(&local_rom_raw, local_game, &config.patches_path(), name, version)?
    } else {
        local_rom_raw
    };

    // Remote-side game + rom. Falls back to the local game if
    // the remote's GameInfo is missing, but a Compatible verdict
    // would have caught that.
    let remote_gi = pre_match
        .remote_settings
        .game_info
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("remote settings missing game info"))?;
    let remote_game = crate::library::game::find_by_family_and_variant(
        &remote_gi.family_and_variant.0,
        remote_gi.family_and_variant.1,
    )
    .ok_or_else(|| anyhow::anyhow!("unknown remote rom"))?;
    let remote_game_impl =
        game::from_gamedb_entry(remote_game).ok_or_else(|| anyhow::anyhow!("no impl for remote game"))?;
    let remote_rom_raw = scanners
        .roms
        .read()
        .get(&remote_game)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("remote rom not scanned"))?;
    let remote_rom_bytes = if let Some(p) = remote_gi.patch.as_ref() {
        patch::apply_patch_from_disk(
            &remote_rom_raw,
            remote_game,
            &config.patches_path(),
            &p.name,
            &p.version,
        )?
    } else {
        remote_rom_raw
    };

    // Build the opponent's Loaded only if they didn't blind their
    // setup — otherwise we don't have visibility into their save.
    // Loaded parses chip/navi/navicust assets from the rom + wram,
    // so the session pane can render them with the same widgets we
    // use for the local side.
    let opponent_loaded = if !pre_match.remote_settings.blind_setup {
        let remote_save = remote_game
            .parse_save(&pre_match.remote_save_data)
            .map_err(|e| anyhow::anyhow!("parse remote save: {e:?}"))?;
        // `remote_rom_bytes` is already the patched image we run in the
        // session, so resolve the matching `rom_overrides` + charset and
        // hand both straight to `from_patched_rom` — no second BPS apply.
        let applied_patch = remote_gi.patch.as_ref().and_then(|p| {
            let patches = scanners.patches.read();
            let version_meta = patches.version(&p.name, &p.version).cloned()?;
            Some(crate::selection::AppliedPatch {
                name: p.name.clone(),
                version: p.version.clone(),
                version_meta,
            })
        });
        Some(crate::selection::Loaded::from_patched_rom(
            remote_game,
            remote_rom_bytes.clone(),
            std::path::PathBuf::new(),
            remote_save,
            applied_patch,
        ))
    } else {
        None
    };

    // Build the local-side Loaded so the in-session "my setup"
    // toggle can render the same save-view we use for the
    // opponent panel.
    let local_loaded = {
        let local_save = local_game
            .parse_save(&pre_match.local_save_data)
            .map_err(|e| anyhow::anyhow!("parse local save: {e:?}"))?;
        // Same as the opponent side: `local_rom_bytes` is already
        // patched, so layer the overrides on via `from_patched_rom`
        // instead of re-applying the BPS patch.
        let applied_patch = local_patch.as_ref().and_then(|(name, version)| {
            let patches = scanners.patches.read();
            let version_meta = patches.version(name, version).cloned()?;
            Some(crate::selection::AppliedPatch {
                name: name.clone(),
                version: version.clone(),
                version_meta,
            })
        });
        Some(crate::selection::Loaded::from_patched_rom(
            local_game,
            local_rom_bytes.clone(),
            std::path::PathBuf::new(),
            local_save,
            applied_patch,
        ))
    };

    let (session, audio) = pvp::PvpSession::new(pvp::PvpSessionArgs {
        local_game: local_game_impl,
        local_rom: std::sync::Arc::new(local_rom_bytes),
        remote_game: remote_game_impl,
        remote_rom: std::sync::Arc::new(remote_rom_bytes),
        pre_match,
        // Presentation delay is purely local — read straight from config (clamped
        // to the supported range), not negotiated with the peer.
        frame_delay: config.frame_delay.clamp(MIN_FRAME_DELAY, MAX_FRAME_DELAY),
        disable_bgm: config.disable_bgm_in_pvp,
        replays_path: &config.replays_path(),
        cache_path: &config.cache_path(),
        sample_rate: audio_binder.sample_rate(),
    })
    .await?;
    Ok((
        session,
        PvpPanes {
            local_loaded,
            opponent_loaded,
            local_save_view: save_view::State::new(),
            opponent_save_view: save_view::State::new(),
        },
        bind_session_audio(&audio_binder, audio),
    ))
}

/// Boot the supplied selection in single-player mode. Caller must
/// already have a complete (game + rom + save) Loaded — there's no
/// fallback for missing pieces, so the Play button is responsible for
/// gating.
pub fn spawn_singleplayer(
    scanners: &Scanners,
    config: &config::Config,
    audio_binder: &audio::LateBinder,
    loaded: &selection::Loaded,
) -> anyhow::Result<(singleplayer::SinglePlayerSession, Option<audio::Binding>)> {
    let game = game::from_gamedb_entry(loaded.game)
        .ok_or_else(|| anyhow::anyhow!("no game impl for {:?}", loaded.game.family_and_variant()))?;
    // Loaded stashes the *parsed* ROM (assets), not the raw bytes —
    // grab them back from the scanner and re-apply the patch if any so
    // the emulator sees the same image it would in the legacy app.
    let raw = scanners
        .roms
        .read()
        .get(&loaded.game)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("rom not in scanner cache"))?;
    let rom_bytes = if let Some(p) = loaded.patch.as_ref() {
        patch::apply_patch_from_disk(&raw, loaded.game, &config.patches_path(), &p.name, &p.version)?
    } else {
        raw
    };
    let (session, audio) = singleplayer::SinglePlayerSession::new(
        game,
        std::sync::Arc::new(rom_bytes),
        &loaded.save_path,
        audio_binder.sample_rate(),
    )?;
    Ok((session, bind_session_audio(audio_binder, audio)))
}

/// Convert a tick count (60 Hz GBA frames) into `m:ss` for the scrub
/// bar's wallclock labels.
pub fn format_tick(tick: u32) -> String {
    let total_s = tick / 60;
    let m = total_s / 60;
    let s = total_s % 60;
    format!("{m}:{s:02}")
}

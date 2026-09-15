//! The app-facing half of emulator sessions: per-session UI state,
//! Message + update + view + subscription. Owned by App as
//! `session: session::State` and routed via `Message::Session(_)`.
//!
//! Portable drivers and controls live in [`tango_session`]. `launch` resolves
//! library inputs and starts desktop workers; `runtime` owns their cleanup
//! and save persistence. Hosts install the resulting [`Launch`] through
//! [`State::install`], which also resets the presentation for the new session.

mod launch;
mod runtime;
pub use launch::{build_playback, spawn_pvp, spawn_singleplayer, spawn_training, Launch};

pub mod scrubber;
pub mod view;

pub use tango_session::{pvp, replay, singleplayer, training, Session};

use crate::config;
use crate::i18n::t;
use crate::platform::audio;
use crate::platform::video::framebuffer::Effect;
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
/// buffer ([`State::metric_history`]) so the persistent PvP panel can draw a
/// sparkline per metric. `round` is `None` between rounds, when no
/// skew/lead/depth reading exists; when present it is `(skew, depth, lead)`.
#[derive(Clone, Copy)]
pub struct MetricSample {
    pub tps: f32,
    pub fps_target: f32,
    /// Latest raw RTT, absent while the link is still coming up or is
    /// temporarily down. Keeping absence distinct from `0 ms` lets the
    /// persistent chart show an honest gap instead of a perfect-looking sample
    /// before the first pong.
    pub ping_ms: Option<u128>,
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
            ping_ms: pvp.latency_raw().map(|d| d.as_millis()),
            round: pvp.round_stats().map(|s| (s.skew, s.depth, s.lead)),
        }
    }
}

/// How many frames of telemetry the sparklines retain (~3 s at 60 fps).
const METRIC_HISTORY_LEN: usize = 180;

/// Session-redraw cadence while a priming walk holds the session up
/// (~30 fps), so the notice's pulse and its clock keep moving with no
/// frames coming off the pair. Purely cosmetic, and only ever alive for
/// the length of a walk.
const PRIME_WAIT_UI_TICK: std::time::Duration = std::time::Duration::from_millis(33);

/// PvP-only presentation state riding alongside the session engine:
/// both sides' fully-loaded selections (rom + parsed save + derived
/// assets) for the in-match setup drawers, plus each drawer's
/// save-view tab/grouping state. Built by [`spawn_pvp`] and installed
/// into [`State::pvp_panes`] with the session; the loadeds also feed
/// the post-match results cook ([`MatchResults::capture`]).
pub struct PvpPanes {
    /// Local side's loaded selection — the "my setup" drawer.
    pub local_loaded: Option<selection::LoadedSave>,
    /// Opponent's loaded selection, unless they blinded their setup.
    pub opponent_loaded: Option<selection::LoadedSave>,
    /// Local validation warnings for the opponent's committed save. `None`
    /// means legal. This opaque report is computed from the bytes the match
    /// actually runs, never trusted from a peer flag.
    pub opponent_build_warnings: Option<tango_gamesupport::OpaqueBuildWarnings>,
    /// A build warning remains visible until explicitly dismissed. Replacing
    /// `PvpPanes` for the next match naturally resets it.
    pub build_warning_dismissed: bool,
    /// Whether the warning's exact violation list has been explicitly opened.
    /// Starts collapsed so opponent build details are never shown implicitly.
    pub build_warning_violations_expanded: bool,
    /// Current width of each setup drawer (`[self, opponent]`), seeded
    /// from `config.pvp_setup_pane_widths` at match start and moved by
    /// dragging a drawer's inner edge. The App mirrors it back into
    /// config when a drag ends.
    pub pane_widths: [f32; 2],
    /// The drawer edge currently being dragged, `None` at rest.
    pub pane_drag: Option<PaneDrag>,
}

/// A setup drawer being sized by its inner edge. `anchor_x` is latched
/// on the drag's FIRST move rather than at the press: iced's
/// `mouse_area` press carries no cursor position, so the first move
/// establishes the origin and every move after it is a delta off
/// `start_width` — which keeps the pane from jumping to sit centered
/// under the cursor when the grab lands off the edge's exact pixel.
#[derive(Clone, Copy)]
pub struct PaneDrag {
    /// Which drawer, indexing [`PvpPanes::pane_widths`]: 0 = self
    /// (left edge), 1 = opponent (right).
    pub side: usize,
    /// The drawer's width when the grab started.
    pub start_width: f32,
    /// Window x the drag measures from, `None` until the first move.
    pub anchor_x: Option<f32>,
}

// A LoadedSave is a whole parsed rom + save; a placeholder keeps the
// enclosing app Message (which carries a `Slot<(PvpSession, PvpPanes)>`)
// derivable, same as PreMatchData.
impl std::fmt::Debug for PvpPanes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PvpPanes { .. }")
    }
}

/// What the framebuffer widget's mouse area reports for a console with
/// a touch screen. Positions arrive already mapped into the touch
/// screen's own pixels (clamped to its edges), plus whether the raw
/// cursor was actually over that screen — a press only counts there,
/// but a drag in progress follows the clamped position off the edge the
/// way a real stylus scrapes along the bezel.
#[derive(Debug, Clone, Copy)]
pub enum StylusEvent {
    Moved { pos: (u16, u16), inside: bool },
    Pressed,
    Released,
}

/// Stylus interaction state: where the cursor last was over the
/// emulator surface, and whether a touch is in progress. Inert unless
/// the running console has a touch screen.
#[derive(Default)]
pub struct Stylus {
    /// Last reported position (clamped into the touch screen) and
    /// whether the cursor was truly inside it.
    hover: Option<((u16, u16), bool)>,
    /// A press landed inside the touch screen and hasn't lifted.
    down: bool,
}

impl Stylus {
    /// The touch the session should see right now.
    fn touch(&self) -> Option<(u16, u16)> {
        match (self.down, self.hover) {
            (true, Some((pos, _))) => Some(pos),
            _ => None,
        }
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
            if self.thumb.as_ref().map(|(t, _)| *t) != Some(snap_tick) {
                let fb = snap.local_framebuffer();
                // Length-checked rather than just non-empty: a capture
                // that doesn't match the session's declared shape would
                // otherwise upload as a sheared texture.
                let (w, h) = replay.frame_size();
                if fb.len() == (w * h * 4) as usize {
                    self.thumb = Some((snap_tick, thumbnail_handle(w, h, fb)));
                }
            }
        }
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
    /// Per-round outcome + presentation-ready HP trace, in play order —
    /// including a round the match never decided, which carries its trace
    /// with no outcome. Empty only when the match tore down before any
    /// round was sampled at all (e.g. a comm error in the intro) — the
    /// screen shows a neutral headline then.
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
    /// `None` for a round the match never decided — a mid-round
    /// disconnect keeps the round and its trace, it just has no verdict
    /// to report.
    pub outcome: Option<crate::ui::widgets::RoundOutcome>,
    pub trace: Vec<(f32, f32, f32)>,
    pub custom: Vec<(f32, f32)>,
    /// Chip-use events per side (`[you, opponent]`), cooked for the
    /// graph's event lanes. Names/icons are resolved at capture time —
    /// the session (and both sides' loadeds) is gone while the card is
    /// on screen — each side through its own LoadedSave, the opponent
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
        // No recording length to pin the timeline to — the match just
        // ended and its replay is still flushing — so the cards run to
        // the last reading.
        let (cooked, max_hp) = crate::ui::widgets::cook_hp_rounds(&stats, loadeds, None);
        let rounds = cooked
            .into_iter()
            // Every round the match simulated is on the card, decided or
            // not: the last one of a mid-round disconnect comes through
            // with its trace and no outcome.
            .map(|c| RoundCard {
                outcome: c.outcome,
                trace: c.trace,
                custom: c.custom,
                chip_uses: c.chip_uses,
                weight: c.weight,
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

/// Session presentation and one owned desktop runtime.
pub struct State {
    active: Option<runtime::RunningSession>,
    /// Count of sessions ever installed — bumped by
    /// [`install`](Self::install), and the frame
    /// [`subscription`]'s identity for the active session. Keying the
    /// wake stream by anything address-based (the `Notify` Arc's
    /// pointer) would be ABA-prone: a new session can allocate at a
    /// dropped one's address, and iced would keep the old stream —
    /// parked on the old Notify — instead of spinning up the new one.
    session_seq: u64,
    /// PvP-only: the two sides' loaded assets + save-view panel state
    /// for the in-match setup drawers — the presentation state the
    /// session engine deliberately doesn't carry. Set alongside
    /// `active` when a PvP session installs, cleared on close.
    pub pvp_panes: Option<PvpPanes>,
    /// Path of the replay the active playback session is watching —
    /// what the transport bar's clip export addresses (the export job
    /// itself lives in the Replays tab, keyed by path). Set alongside
    /// `active` on watch, cleared on close.
    pub replay_path: Option<std::path::PathBuf>,
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
    /// into GBA joyflags each event.
    pub input_held: crate::platform::input::HeldState,
    /// Pointer-as-stylus state over the emulator surface, for the DS's
    /// touch screen. Fed by the framebuffer widget's mouse area; folded
    /// into the session input alongside the joyflags.
    pub stylus: Stylus,
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
    /// PvP-only: compact frame-delay slider popover above the persistent
    /// telemetry bar. The telemetry itself has no collapsed state.
    pub frame_delay_control: anim::Overlay,
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
    /// and drawn as sparklines in the persistent PvP status panel. Capped at
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
    /// ([`State::install`]).
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
    /// The priming wait the session is in and when it started, `None`
    /// whenever it isn't in one. Stamped by the frame handler (which
    /// the wait's own redraw tick keeps firing, since a pair that isn't
    /// running publishes no frames) so the notice can count the wait
    /// up. Re-stamped when the wait *changes* — our own walk giving way
    /// to the wait on the peer's is a new wait, and carrying the first
    /// one's clock into it would overstate how long the opponent has
    /// been holding things up.
    pub prime_wait_since: Option<(PrimeWait, std::time::Instant)>,
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
            pvp_panes: None,
            replay_path: None,
            results: None,
            opponent_panel: anim::Overlay::new(false),
            self_panel: anim::Overlay::new(false),
            input_held: crate::platform::input::HeldState::default(),
            stylus: Stylus::default(),
            speed_up_engaged: false,
            settings: anim::Overlay::new(false),
            disconnect: anim::Overlay::new(false),
            frame_delay_control: anim::Overlay::new(false),
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
            prime_wait_since: None,
            controls_anim: anim::Transition::new(true),
        }
    }
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active(&self) -> Option<&(dyn Session + 'static)> {
        self.active.as_deref()
    }

    /// Whether a session is running.
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Start an orderly application shutdown. PvP returns a completion token
    /// because its supervisor must get a chance to send `Goodbye` before the
    /// process runtime disappears; other session kinds have no asynchronous
    /// network teardown to await.
    pub(crate) fn request_app_close(&self) -> Option<tokio_util::sync::CancellationToken> {
        let done = self
            .active_as::<pvp::PvpSession>()
            .map(pvp::PvpSession::supervisor_done);
        if let Some(session) = self.active.as_ref() {
            session.request_close();
        }
        done
    }

    /// The active session as concrete kind `T` — `None` while idle or
    /// when a different kind is running.
    pub fn active_as<T: Session>(&self) -> Option<&T> {
        self.active.as_deref().and_then(|s| s.downcast_ref())
    }

    /// Where the active session stands on its priming walk, or `None`
    /// once it is simply running. The one place the cases are
    /// recognized: the [`subscription`] reads it to keep redraws coming
    /// while nothing is producing frames, the frame handler to stamp
    /// [`prime_wait_since`](Self::prime_wait_since), and the views to
    /// draw the notice over the session's own screen.
    pub fn prime_wait(&self) -> Option<PrimeWait> {
        if let Some(pvp) = self.active_as::<pvp::PvpSession>() {
            if let Some(error) = pvp.prime_error() {
                return Some(PrimeWait::Failed(error));
            }
            if pvp.is_booting() {
                return Some(PrimeWait::Match);
            }
            return pvp.waiting_for_peer().then_some(PrimeWait::Peer);
        }
        if let Some(replay) = self.active_as::<replay::ReplaySession>() {
            if let Some(error) = replay.prime_error() {
                return Some(PrimeWait::Failed(error));
            }
            return replay.is_booting().then_some(PrimeWait::Playback);
        }
        None
    }
}

/// Where a session is in its priming walk — the seconds of emulation
/// that get a game from power-on to its link battle. Both kinds that
/// walk come up before it runs and show nothing at all while it does,
/// which on a DS-class game is long enough to read as a hang.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimeWait {
    /// A match's own pair is being booted and primed
    /// ([`pvp::PvpSession::is_booting`]).
    Match,
    /// Ours is primed and the drive loop is idling at the ready gate
    /// for the peer's to get there
    /// ([`pvp::PvpSession::waiting_for_peer`]).
    Peer,
    /// A replay's pair is being booted and primed
    /// ([`replay::ReplaySession::is_booting`]).
    Playback,
    /// The walk failed, carrying its reason. Terminal, and the only
    /// state the user has to act on: the session stays up with nothing
    /// to run until they dismiss it.
    Failed(String),
}

impl PrimeWait {
    /// Whether this is a wait that is still going somewhere — which is
    /// also what decides whether the host keeps redrawing for it (see
    /// [`subscription`]). A failure is over; it just hasn't been read
    /// yet.
    pub fn in_progress(&self) -> bool {
        !matches!(self, PrimeWait::Failed(_))
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
    /// session. Live-session speed-up uses the same mechanism
    /// (edge-detected); replay transport keys are decoded before it.
    Input(InputEvent),
    /// Pointer event over the emulator surface of a console with a
    /// touch screen, already mapped into that screen's pixels by the
    /// framebuffer widget. Folded into the same session input push as
    /// [`Input`](Self::Input).
    Stylus(StylusEvent),
    /// Replay-view messages (transport, scrubber, display toggles) —
    /// defined + handled in [`view::replay`].
    Replay(view::replay::Message),
    /// PvP-view messages (frame delay, setup panels, save views,
    /// disconnect confirm) — defined + handled in [`view::pvp`].
    Pvp(view::pvp::Message),
    /// Training-view messages (PiP + side-swap toggles) — defined +
    /// handled in [`view::training`].
    Training(view::training::Message),
    /// Post-match results screen messages — defined in
    /// [`view::results`]. Dismiss is handled here; WatchReplay by the
    /// App wrapper (building a playback session needs the scanners +
    /// config).
    Results(view::results::Message),
    /// User pressed Esc inside a session. Dismisses whichever overlay is on top
    /// (settings, disconnect confirm, or frame-delay control), if any, and arms
    /// the hold-to-quit timer — a tap never
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

impl State {
    /// Apply a session message to the state. Returns the iced Task
    /// that should be scheduled (always Task::none today — kept for
    /// API parity with the other tabs).
    pub fn update(
        &mut self,
        msg: Message,
        mapping: &crate::platform::input::Mapping,
        lang: &LanguageIdentifier,
    ) -> iced::Task<Message> {
        let task = self.update_inner(msg, mapping, lang);
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
        self.frame_delay_control.sync(now);
        self.self_panel.sync(now);
        self.opponent_panel.sync(now);
        // Floating controls auto-hide. The per-frame
        // UpdateFramebuffer messages re-run this, so the idle
        // timer expires without needing its own timer source; a
        // paused replay (no frames) pins the bar visible anyway.
        let replay_paused = self.active_as::<replay::ReplaySession>().is_some_and(|r| r.is_paused());
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

    /// Replace the current runtime and install its presentation in one step.
    /// Old audio and workers are released before the new stream is bound.
    pub fn install(&mut self, mut launch: Launch, binder: &audio::LateBinder, config: &config::Config) {
        self.close_session();
        if let Some(pvp) = launch.runtime.downcast_ref::<pvp::PvpSession>() {
            // The local slider remains live while an async launch is building.
            pvp.set_frame_delay(config.frame_delay.clamp(MIN_FRAME_DELAY, MAX_FRAME_DELAY));
        }
        launch.runtime.bind_audio(binder);
        self.active = Some(launch.runtime);
        self.pvp_panes = launch.pvp_panes;
        self.replay_path = launch.replay_path;
        if config.show_opponent_setup && self.pvp_panes.as_ref().is_some_and(|p| p.opponent_loaded.is_some()) {
            self.opponent_panel.open();
        }
        self.session_seq = self.session_seq.wrapping_add(1);
        self.last_mouse_move = std::time::Instant::now();
        self.controls_anim = anim::Transition::new(true);
    }

    /// Tear down the active session: PvP pre-drop close request, then
    /// drop-by-clearing plus the reset of every piece of per-session
    /// UI state. Shared by [`Message::Close`] (the Close button /
    /// disconnect confirm), the Esc hold-to-quit expiry in
    /// [`update`](State::update), and the App's replay-queue advance —
    /// which has to wind the finished session down before installing the
    /// next one over the slot.
    pub(crate) fn close_session(&mut self) {
        self.active = None;
        self.pvp_panes = None;
        self.replay_path = None;
        self.current_frame = None;
        self.pip_frame = None;
        self.input_held = Default::default();
        self.speed_up_engaged = false;
        self.controls_hovered = false;
        self.bar_menu_open = false;
        self.settings = anim::Overlay::new(false);
        self.disconnect = anim::Overlay::new(false);
        self.frame_delay_control = anim::Overlay::new(false);
        self.self_panel = anim::Overlay::new(false);
        self.opponent_panel = anim::Overlay::new(false);
        self.metric_history.clear();
        self.scrub = Scrub::default();
        self.esc_hold = None;
        self.prime_wait_since = None;
        self.stylus = Stylus::default();
    }

    fn autosave_singleplayer(&mut self) {
        if let Some(active) = self.active.as_mut() {
            active.autosave();
        }
    }

    /// Push the local player's whole current input — the mapping's
    /// resolution of everything held, plus the stylus — to the active
    /// session. Every input-shaped event ends here, so the session
    /// always holds the latest complete picture.
    fn push_input(&self, mapping: &crate::platform::input::Mapping) {
        if let Some(s) = self.active.as_ref() {
            s.set_input(tango_session::HostInput {
                keys: mapping.to_joyflags(&self.input_held),
                touch: self.stylus.touch(),
            });
        }
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

    fn update_inner(
        &mut self,
        msg: Message,
        mapping: &crate::platform::input::Mapping,
        lang: &LanguageIdentifier,
    ) -> iced::Task<Message> {
        match msg {
            Message::Close => {
                self.close_session();
            }
            Message::Input(ev) => {
                self.input_held.apply(&ev);
                self.push_input(mapping);
                // Live-session speed-up: only fire set_speed on the
                // rising or falling edge so we don't spam the audio sync
                // target with no-op writes. Replays use preset controls.
                let now_engaged =
                    self.active_as::<replay::ReplaySession>().is_none() && mapping.speed_up_held(&self.input_held);
                if now_engaged != self.speed_up_engaged {
                    self.speed_up_engaged = now_engaged;
                    let factor = if now_engaged { 4.0 } else { 1.0 };
                    if let Some(s) = self.active.as_ref() {
                        s.set_speed(factor);
                    }
                }
            }
            Message::Stylus(ev) => {
                match ev {
                    StylusEvent::Moved { pos, inside } => {
                        self.stylus.hover = Some((pos, inside));
                    }
                    StylusEvent::Pressed => {
                        // A touch starts only on the touch screen itself;
                        // a press over the top screen is just a click.
                        if let Some((_, true)) = self.stylus.hover {
                            self.stylus.down = true;
                        }
                    }
                    StylusEvent::Released => {
                        self.stylus.down = false;
                    }
                }
                self.push_input(mapping);
            }
            // Kind-specific view messages — defined + handled beside
            // the views that emit them.
            Message::Replay(m) => return view::replay::update(self, m).map(Message::Replay),
            Message::Pvp(m) => return view::pvp::update(self, m, lang).map(Message::Pvp),
            Message::Training(m) => return view::training::update(self, m).map(Message::Training),
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
                // Peel overlays off top-down: settings, disconnect confirm,
                // then the frame-delay control. A tap stops there — tearing
                // the session down takes an explicit button action or the
                // full [`ESC_QUIT_HOLD`] hold.
                if self.settings.shown() {
                    self.settings.close();
                } else if self.disconnect.shown() {
                    self.disconnect.close();
                } else if self.frame_delay_control.shown() {
                    self.frame_delay_control.close();
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
                // Keep the user's .sav current while they play — see
                // [`SaveBackup`]. No-op for every other session kind.
                self.autosave_singleplayer();
                // Telemetry snapshot for the persistent sparklines, captured while
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
                        // Frames arrive already expanded to RGBA8; the
                        // selected effect magnifies them on the GPU at
                        // draw time.
                        let pixels = session.frame();
                        // Sized by the session: a DS lays out two screens
                        // where a GBA has one.
                        let (width, height) = session.frame_size();
                        self.frame_revision = self.frame_revision.wrapping_add(1);
                        self.current_frame = Some(crate::platform::video::framebuffer::Frame {
                            pixels: std::sync::Arc::new(pixels),
                            width,
                            height,
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
                                // The PiP surface shares the main
                                // screen's layout.
                                width,
                                height,
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
                // Run the priming-wait clock off the same frame message:
                // stamped when a wait begins, kept while it's the same
                // wait, dropped the moment the pair starts producing
                // frames again.
                self.prime_wait_since = match (self.prime_wait(), self.prime_wait_since.take()) {
                    (None, _) => None,
                    (Some(wait), Some((started_on, at))) if started_on == wait => Some((wait, at)),
                    (Some(wait), _) => Some((wait, std::time::Instant::now())),
                };
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
    // A session mid-priming publishes no frames at all — the pair is
    // either not up yet or idling at the ready gate — so its wake never
    // fires and the view would freeze on the frame that installed it.
    // Tick ~30 Hz for the duration so the notice animates and its clock
    // runs; it stops the moment the pair starts producing frames. A
    // failed walk needs no tick: its notice is static, and the boot
    // fired the wake that put it up.
    if state.prime_wait().is_some_and(|w| w.in_progress()) {
        subs.push(iced::time::every(PRIME_WAIT_UI_TICK).map(|_| Message::UpdateFramebuffer));
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
    // No BNLC release to borrow art from — the pane falls back to no
    // background, which it already does when BNLC is not installed.
    let bg = game.background?;
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

/// Wrap a snapshot's RGBA8 pixels into an image handle for the hover
/// thumbnail; it only runs when the hovered keyframe changes.
fn thumbnail_handle(width: u32, height: u32, pixels: Vec<u8>) -> iced::widget::image::Handle {
    iced::widget::image::Handle::from_rgba(width, height, pixels)
}

/// Convert a tick count (60 Hz GBA frames) into `m:ss` for the scrub
/// bar's wallclock labels.
pub fn format_tick(tick: u32) -> String {
    let total_s = tick / 60;
    let m = total_s / 60;
    let s = total_s % 60;
    format!("{m}:{s:02}")
}

//! Live PvP emulator session — peer-paired netplay sibling of
//! [`crate::singleplayer::SinglePlayerSession`].
//!
//! Both consoles run locally as one [`tango_match::Match`], with the pair
//! as the rollback unit. The host paces [`PvpBoot`] and [`PvpDriver`]; this
//! module exposes the controls and readouts it uses between ticks.
//!
//! - `setup` assembles the transport, shared state, driver, and recording.
//! - `driver` primes the pair, advances frames, and records confirmed inputs.
//! - `supervisor` pumps network input and handles reconnect and shutdown.
//! - `recording` owns replay storage adapters and metadata construction.
//!
//! Construction awaits the lobby's transport handoff. Emulation starts on
//! the driver's first tick, after the host can display the session's status.

mod driver;
mod recording;
mod setup;
mod supervisor;

pub use driver::{PvpBoot, PvpDriver};
pub use recording::{Recording, ReplayStore};

// The pure codec crate carries its own copy of the key mask so it
// doesn't drag in the emulator stack; this crate sees both, so a drift
// becomes a build failure here.
const _: () = assert!(
    tango_net_protocol::data::KEYS_MASK as u32 == tango_match::keys::MASK,
    "tango-net-protocol's KEYS_MASK drifted from tango-match's"
);

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use web_time::Instant;

/// Inclusive bounds for a side's `frame_delay`, which is realized purely as
/// local frame delay (how far the display trails the netcode frontier).
/// Each side picks its own; there's no negotiation. The lobby slider and config
/// clamp to this range. 0 presents the frontier itself — pure rollback, every
/// misprediction visible immediately; the default (`default_frame_delay`, 2)
/// stays above it, and the ping-based suggestion never lands below 1.
pub const MIN_FRAME_DELAY: u32 = 0;
pub const MAX_FRAME_DELAY: u32 = 10;

pub fn suggest_frame_delay(rtt: std::time::Duration) -> u32 {
    let one_way_frames = (rtt.as_millis() * 60 / 2 / std::time::Duration::from_secs(1).as_millis()) as i32;
    (one_way_frames + 1).clamp(MIN_FRAME_DELAY as i32, MAX_FRAME_DELAY as i32) as u32
}

/// A stored or user-entered frame delay, clamped into the supported range.
pub fn clamp_frame_delay(frame_delay: u32) -> u32 {
    frame_delay.clamp(MIN_FRAME_DELAY, MAX_FRAME_DELAY)
}

/// The frame delay to start a match at when the user hasn't picked one:
/// suggested from the lobby's median RTT, or `fallback` while no Pong
/// has come back yet. A zero median is "we don't know", not a 0 ms link.
pub fn initial_frame_delay(median_rtt: std::time::Duration, fallback: u32) -> u32 {
    if median_rtt.is_zero() {
        clamp_frame_delay(fallback)
    } else {
        suggest_frame_delay(median_rtt)
    }
}

/// Upper bound on how long `is_ended` waits for the peer's
/// `EndOfMatch` packet after local completion. Wide enough to
/// cover slow networks + the typical match-end animation, tight
/// enough that a crashed peer doesn't pin the UI for long.
const PEER_END_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// The latching end-of-match signals, grouped so the teardown policy
/// lives in one place instead of four loose atomics on the session.
///
/// Each field starts cleared and flips exactly once as the match winds
/// down; [`PvpSession::is_ended`] combines them with the completion +
/// cancellation tokens. Every field is an `Arc`, so [`Clone`] hands out a
/// shared handle: the net receive task, the supervisor, and the drive
/// thread each keep one and raise their own signal.
#[derive(Clone, Default)]
struct EndState {
    /// Remote's in-game match-end handshake (the in-band `data::wire`
    /// `EndOfMatch` marker) arrived — raised by the net receive task
    /// ([`tango_net::PvpReceiver`]).
    /// `is_ended` honors it so the lagging side gets time to write its
    /// replay tail before we drop the data channel.
    remote_ended: Arc<AtomicBool>,
    /// Remote's channel closed (clean RTC `on_closed` or receiver `Err`)
    /// or it announced a deliberate quit (the control channel's `Goodbye`)
    /// — raised by the supervisor. No more packets are coming, so
    /// `is_ended` skips straight past the grace window.
    remote_disconnected: Arc<AtomicBool>,
    /// Wall-clock instant we first observed local completion, or `None`
    /// until then. Pulls double duty: the drive thread fires our
    /// `EndOfMatch` exactly once on the `None → Some` edge, and `is_ended`
    /// reads the stamp as the fallback grace deadline so a silent peer
    /// can't pin us forever.
    local_ended_at: Arc<Mutex<Option<web_time::Instant>>>,
    /// The players abandoned the match in-game before any battle (the
    /// telemetry store's abort latch — bn6 random battle's rank-select
    /// cancel), raised by the drive thread. The match ends uncleanly:
    /// no results card, no reconnect, no disconnect dress. Both peers
    /// simulate the same exit on the same tick, so each raises this for
    /// itself — the supervisor reads it to keep the peer's near-
    /// simultaneous teardown from being mistaken for a disconnect.
    aborted: Arc<AtomicBool>,
}

/// Live per-frame readouts the drive thread publishes for the UI —
/// the instrument panel and sparklines read these between frames.
#[derive(Default)]
struct Metrics {
    /// Clock-sync skew the throttler reacts to (positive = we lead).
    skew: std::sync::atomic::AtomicI32,
    /// Local inputs not yet matched by a remote input.
    queue_len: AtomicU32,
    /// Speculative ticks the last advance rolled back.
    depth: AtomicU32,
    /// What the pacing loop currently targets, f32 bits (base rate minus
    /// the throttler's shave).
    fps_target: AtomicU32,
}

pub struct PvpSession {
    local_game: &'static tango_gamesupport::Game,
    /// This side's player index (P1 = 0, P2 = 1), picked once at match start and
    /// stable for the whole match. The pair is symmetric — core 0 always runs
    /// player 0's game on both peers — so this is also which core is "ours".
    local_player_index: u8,
    local_input: Arc<crate::InputCell>,
    /// Flipped once the games' own match-end path is confirmed — the
    /// direct successor of the trap engine's per-game completion hook.
    completed: Arc<AtomicBool>,
    /// Which of the console's screens the host is actually showing,
    /// shared with the drive loop so the setting can move mid-match.
    displayed_screens: Arc<std::sync::atomic::AtomicU8>,
    /// Latching end-of-match signals (remote-ended / remote-disconnected /
    /// local-ended). Grouped in [`EndState`]; `is_ended` reads them
    /// alongside `completed` and `cancellation_token`.
    end: EndState,
    /// Sliding-window timestamp counter marked once per drive-loop frame —
    /// yields the true simulation TPS regardless of how often the UI polls.
    tps_counter: Arc<Mutex<TpsCounter>>,
    /// Drops fire-cancellation through the drive thread and the network
    /// tasks. On Close we cancel + drop the session, which tears the
    /// network loop down cleanly.
    cancellation_token: tokio_util::sync::CancellationToken,
    /// Cancelled when the receive supervisor has finished teardown. The
    /// desktop exit path waits on this so its bounded best-effort `Goodbye`
    /// can leave before iced destroys the async runtime.
    supervisor_done: tokio_util::sync::CancellationToken,
    /// The console's screens, as the local game's engine presents them
    /// — what `screen` is sized for and what the host sizes its
    /// texture from.
    layout: tango_match::ScreenLayout,
    /// The peer link: owns the peer connection, both channels' halves, the
    /// latency readout, and the transparent mid-match reconnect (see
    /// [`tango_net::link`]). The supervisor task holds its own `Arc`; the
    /// session's keeps the transport alive for the match's lifetime, and its
    /// eventual drop closes the connection gracefully (DTLS close_notify → the
    /// peer's prompt EOF).
    link: Arc<tango_net::link::Link>,
    /// The two halves of the ready gate, as the session's own readouts:
    /// `local_primed` is set when our pair reaches its link battle
    /// ([`is_booting`](Self::is_booting) until then), `peer_primed`
    /// when the peer says the same
    /// ([`waiting_for_peer`](Self::waiting_for_peer) in between). The
    /// session is on screen for both waits, which is why it publishes
    /// them at all.
    local_primed: Arc<AtomicBool>,
    peer_primed: Arc<AtomicBool>,
    /// Why the boot failed, if it did — the priming walk timing out on
    /// a game that never reached its link battle, or the engine
    /// refusing to start at all. Filled in by the drive loop
    /// ([`PvpBoot::tick`](crate::Drive::tick)); the session stays up
    /// with nothing to run so the host can say what happened.
    prime_error: Arc<Mutex<Option<crate::Error>>>,
    /// Aborts a priming walk still in flight when the session closes,
    /// so the host's drive-thread join doesn't wait out the seconds of
    /// emulation the user just walked away from.
    boot_cancel: Arc<AtomicBool>,
    /// Live UI readouts published by the drive thread.
    metrics: Arc<Metrics>,
    pub link_code: String,
    pub remote_nickname: String,
    /// Live local frame delay — realized as the engine's present delay
    /// (how far the displayed tick trails the local input frontier). The
    /// footer slider writes it; the drive loop applies changes each frame.
    /// Purely local — never negotiated or sent to the peer.
    frame_delay: Arc<AtomicU32>,
    /// Incremental local-perspective match stats, fed by the drive thread
    /// from confirmed telemetry as rounds close. Our own `Arc` (the drive
    /// thread holds a clone), so the post-match results snapshot and the
    /// sidecar write can read it during teardown regardless of how far the
    /// background tasks have already wound down.
    stats: Arc<Mutex<tango_match::analysis::StatsBuilder>>,
    /// Where this match's replay is being recorded, or `None` if the writer
    /// failed to open. The post-match results screen offers to play it back.
    pub replay_path: Option<std::path::PathBuf>,
    /// This session's display, written by the drive thread once per
    /// simulated frame.
    screen: Arc<crate::Framebuffer>,
    /// Repaint/re-check wake — fired per frame by the drive thread and
    /// by the end-detection wires that produce no frame at all (the
    /// receive pump, the reconnect supervisor, the peer-end grace).
    wake: Arc<tokio::sync::Notify>,
    /// When the session was built, for the results screen's match duration.
    started_at: web_time::Instant,
}

pub use tango_net::handoff::PreMatchData;

/// One seat's prepared launch inputs, in local/remote terms: its game,
/// its ROM with any patch already applied, and the SRAM image its
/// console boots. The host builds these from its library's preparation
/// of the committed saves; the session never parses a save itself.
pub struct Seat {
    pub game: &'static tango_gamesupport::Game,
    pub rom: Arc<Vec<u8>>,
    pub sram: Vec<u8>,
}

/// Everything [`PvpSession::new`] needs, as named fields. Assembled by
/// the app's `spawn_pvp` glue.
pub struct PvpSessionArgs<'a> {
    pub local: Seat,
    pub remote: Seat,
    /// The netplay handoff: negotiated terms + the transport bundle.
    pub pre_match: crate::pvp::PreMatchData,
    /// This side's frame delay — realized purely as local display lag (the
    /// engine's present delay). Comes straight from local config; never
    /// negotiated with or sent to the peer.
    pub frame_delay: u32,
    /// Silence the battle BGM (the primers skip the games' battle-start
    /// music call). Comes straight from local config; never negotiated
    /// with or sent to the peer — sound-driver state never feeds battle
    /// logic.
    pub disable_bgm: bool,
    /// Where the match is recorded, or `None` not to record it.
    pub replays: Option<&'a dyn ReplayStore>,
    /// Optional host-owned destination for the finished recording's statistics.
    pub stats_sink: Option<Arc<dyn crate::stats::StatsSink>>,
    /// The host output rate the session's audio stream resamples to.
    pub sample_rate: u32,
}

impl PvpSession {
    /// This side's player index (P1 = 0, P2 = 1) for the match. Stable across
    /// rounds, so the instrument panel's P1/P2 tag reads it directly rather than
    /// pulling it from the per-round [`RoundStats`].
    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    /// Current local frame delay — drives the footer slider's
    /// displayed value.
    pub fn frame_delay(&self) -> u32 {
        self.frame_delay.load(Ordering::Relaxed)
    }

    /// Live-set the local frame delay. Purely local: the drive loop applies
    /// it as the engine's present delay on the next frame, no peer
    /// coordination. Clamped to the supported range as a guard against an
    /// out-of-range caller.
    pub fn set_frame_delay(&self, frame_delay: u32) {
        self.frame_delay
            .store(clamp_frame_delay(frame_delay), Ordering::Relaxed);
    }

    /// Completion signal for an orderly process exit. Request the session's
    /// close first, then keep the async runtime alive until this is cancelled.
    pub fn supervisor_done(&self) -> tokio_util::sync::CancellationToken {
        self.supervisor_done.clone()
    }

    /// `true` while our own pair is still being booted and primed on
    /// the drive loop — the session comes up the moment the lobby
    /// exchange is done, and the walk into the games' link battle
    /// happens under it. Seconds on a DS-class game, an instant on a
    /// GBA one; no frame and no sound until it lands.
    pub fn is_booting(&self) -> bool {
        !self.local_primed.load(Ordering::Acquire)
    }

    /// `true` while the drive loop is idling at the ready gate for the
    /// peer to finish priming their own pair (see [`PvpDriver::tick`]).
    /// Our pair is already at its link battle; theirs isn't, so nothing
    /// advances and no frame is published — the match's screen would
    /// otherwise sit black with nothing said about why. A slower
    /// machine on the other end can make this wait quite long.
    pub fn waiting_for_peer(&self) -> bool {
        !self.is_booting() && !self.peer_primed.load(Ordering::Acquire)
    }

    /// Why the boot failed, ready to show, or `None` while it is still
    /// running or has succeeded. A failed boot leaves the session up
    /// with nothing to run: there is no frame coming, and this is the
    /// only thing left to tell the user.
    pub fn prime_error(&self) -> Option<String> {
        self.prime_error.lock().unwrap().as_ref().map(|e| e.to_string())
    }

    /// `true` while the link has dropped and the session is transparently
    /// rebuilding it (direct or matchmaking) — the drive loop is paused and the
    /// PvP view shows a "Reconnecting…" overlay.
    pub fn is_reconnecting(&self) -> bool {
        matches!(self.link.health(), tango_net::link::LinkHealth::Reconnecting { .. })
    }

    /// Fraction of the reconnect give-up window still remaining — `1.0` when a
    /// reconnect just started, falling to `0.0` at the give-up deadline, or
    /// `None` when not reconnecting. Drives the overlay's depleting progress bar.
    pub fn reconnect_progress(&self) -> Option<f32> {
        match self.link.health() {
            tango_net::link::LinkHealth::Reconnecting { started, give_up_at } => {
                let total = give_up_at
                    .saturating_duration_since(started)
                    .as_secs_f32()
                    .max(f32::EPSILON);
                let remaining = give_up_at
                    .saturating_duration_since(web_time::Instant::now())
                    .as_secs_f32();
                Some((remaining / total).clamp(0.0, 1.0))
            }
            _ => None,
        }
    }

    /// Whether the match ran to its natural end (a final round ended and
    /// the runout elapsed) — as opposed to ending by disconnect or quit.
    /// Gates the post-match results screen.
    pub fn is_completed(&self) -> bool {
        self.completed.load(Ordering::Acquire)
    }

    /// Whether the match ended because the remote vanished mid-match —
    /// the peer announced a quit, its channel EOF'd (crash, its own
    /// give-up) or the reconnect window expired (see the link
    /// supervisor). Never set by our own quit paths. Gates the
    /// disconnect dress of the results screen.
    pub fn remote_disconnected(&self) -> bool {
        self.end.remote_disconnected.load(Ordering::Acquire)
    }

    /// Median ping over the last few seconds — drives the frame-delay
    /// suggestion, where smoothing out a transient spike is what we want.
    /// `Some(ZERO)` until the first sample arrives, then `Some(median)`
    /// while the link is up; `None` once the remote drops (the supervisor
    /// retires the link's counter). The UI keys the instrument panel off this:
    /// `None` means "no live link".
    pub fn latency(&self) -> Option<std::time::Duration> {
        self.link.latency()
    }

    /// Raw latest ping — the most recent single measurement, unsmoothed.
    /// Drives the live telemetry plate + sparkline, where the median's lag
    /// would mask a real spike. Same `Some`/`None` link-up semantics as
    /// [`latency`](Self::latency) (both read the same counter), so it gates
    /// the instrument panel identically; only the reported value differs.
    pub fn latency_raw(&self) -> Option<std::time::Duration> {
        self.link.latency_raw()
    }

    /// Smoothed simulation ticks-per-second from the drive loop's
    /// per-frame marks. Independent of UI refresh rate. ZERO until the
    /// second sample lands.
    pub fn tps(&self) -> f32 {
        let mean = self.tps_counter.lock().unwrap().mean_duration();
        if mean.is_zero() {
            0.0
        } else {
            1.0 / mean.as_secs_f32()
        }
    }

    /// What the pacing loop is currently targeting. Pairs with `tps()` —
    /// gap between the two tells you whether the throttler is the cause of
    /// a slow tps or just observing one.
    pub fn fps_target(&self) -> f32 {
        f32::from_bits(self.metrics.fps_target.load(Ordering::Relaxed))
    }

    /// The match stats aggregated so far, in play order. Read at teardown
    /// for the post-match results screen; a round the match never
    /// finished (mid-round disconnect) is in it with its telemetry
    /// intact and no outcome — only the verdict is missing.
    pub fn stats_snapshot(&self) -> tango_match::analysis::MatchStats {
        self.stats.lock().unwrap().snapshot()
    }

    /// How long the match ran, start of session to local completion — or to
    /// now, if completion hasn't been observed yet (it is stamped a frame
    /// after the completion flag flips). For the results screen.
    pub fn match_duration(&self) -> std::time::Duration {
        match *self.end.local_ended_at.lock().unwrap() {
            Some(ended_at) => ended_at.duration_since(self.started_at),
            None => self.started_at.elapsed(),
        }
    }

    /// Snapshot of the live netcode metrics for the status bar
    /// (skew, lead, rollback depth). Always available while the
    /// session runs — the SIO engine's simulation never stops
    /// between rounds.
    pub fn round_stats(&self) -> Option<RoundStats> {
        Some(RoundStats {
            skew: self.metrics.skew.load(Ordering::Relaxed),
            lead: self.metrics.queue_len.load(Ordering::Relaxed) as i32,
            depth: self.metrics.depth.load(Ordering::Relaxed),
        })
    }
}

impl crate::Session for PvpSession {
    fn set_displayed_screens(&self, screens: u8) {
        self.displayed_screens.store(screens, Ordering::Relaxed);
    }

    fn local_game(&self) -> &'static tango_gamesupport::Game {
        self.local_game
    }

    fn frame(&self) -> Vec<u8> {
        self.screen.read()
    }

    fn screen_layout(&self) -> tango_match::ScreenLayout {
        self.layout.clone()
    }

    fn wake(&self) -> Arc<tokio::sync::Notify> {
        self.wake.clone()
    }

    fn set_input(&self, input: crate::HostInput) {
        self.local_input.store(input);
    }

    fn request_close(&self) {
        // Cancelling the token is the whole close signal: it stops the
        // drive thread and the supervisor and flips `is_ended`'s
        // cancellation check. The supervisor announces the quit to the
        // peer (best-effort `Goodbye`) on its way out, so the peer ends
        // at once instead of trying to reconnect to us.
        self.cancellation_token.cancel();
        // Its plain-flag twin, which is what a priming walk deep in a
        // backend can read (see `driver::DriveContext::boot_cancel`). A close
        // during the walk is the one case the token can't reach.
        self.boot_cancel.store(true, Ordering::Release);
    }

    /// True once it's safe to tear the session down. Requires
    /// local completion (the deciding round's end confirmed + runout)
    /// PLUS one of:
    ///   * the peer also sent us `EndOfMatch`, or
    ///   * `PEER_END_GRACE` has elapsed since local completion
    ///     (peer crashed / disconnected — give up waiting).
    ///
    /// The handshake keeps the data channel alive long enough
    /// for the lagging side to also confirm its end and write
    /// its replay tail before we drop the connection. Without it,
    /// whichever side finishes first kills the connection out
    /// from under the other and the other side's replay ends up
    /// truncated.
    fn is_ended(&self) -> bool {
        // A failed boot outranks every check below: the session has
        // nothing left to run, but its reason is on screen and the host
        // tears down on the user's dismissal, not from under it. (The
        // peer dropping while it is up is exactly the case that would
        // otherwise swallow the message.)
        if self.prime_error.lock().unwrap().is_some() {
            return false;
        }
        // The dead-link checks come before the completion gate: a match
        // that ends by disconnect (the peer quit, the reconnect window
        // expired, our own netcode tore down) is over whether or not it
        // ever completed — leaving these behind `completed` stranded
        // mid-match disconnects on a frozen session forever.
        //
        // Remote's data channel closed (RTCPeerConnection drop or
        // SCTP-level disconnect): no EndOfMatch is ever coming, so skip
        // straight to teardown without burning the grace window.
        if self.end.remote_disconnected.load(Ordering::Acquire) {
            return true;
        }
        // The players abandoned the match in-game before any battle:
        // over, uncleanly — nothing to hand the results screen and no
        // EndOfMatch handshake to wait for (the peer's simulation ends
        // its session the same way, on the same pair tick).
        if self.end.aborted.load(Ordering::Acquire) {
            return true;
        }
        // We tore our own netcode down. Same rationale, from our side.
        if self.cancellation_token.is_cancelled() {
            return true;
        }
        if !self.completed.load(Ordering::Acquire) {
            return false;
        }
        if self.end.remote_ended.load(Ordering::Acquire) {
            return true;
        }
        match *self.end.local_ended_at.lock().unwrap() {
            Some(t) => t.elapsed() >= PEER_END_GRACE,
            // The completion flag can flip before the drive loop
            // observes it and stamps the deadline. Hold off
            // teardown for one extra tick rather than firing the
            // grace timer from t=0.
            None => false,
        }
    }
}

/// Subset of the engine's per-frame metrics surfaced in the status bar.
#[derive(Clone, Copy, Debug)]
pub struct RoundStats {
    /// Real-time clock skew the throttler reacts to (see
    /// [`tango_match::Throttler`]). The symmetric network term cancels
    /// in the difference, so this reads ~0 at clock sync, positive when
    /// we're leading (and being slowed), and negative when the peer is
    /// leading.
    pub skew: i32,
    /// Local tick lead: how many local inputs are still unmatched by a
    /// confirmed remote input. Steady around the wire latency at clock
    /// sync; ramps up when the remote falls behind or a delivery stall
    /// holds its confirmed frontier still.
    pub lead: i32,
    /// Misprediction depth: how many speculative frames the last advance
    /// discarded and re-simulated because a confirmed remote input
    /// contradicted the prediction. 0 on a clean frame; spikes mark the
    /// size of each rollback.
    pub depth: u32,
}

impl std::fmt::Debug for PvpSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PvpSession")
            .field("link_code", &self.link_code)
            .field("remote_nickname", &self.remote_nickname)
            .finish_non_exhaustive()
    }
}

impl Drop for PvpSession {
    fn drop(&mut self) {
        // Belt-and-suspenders: even if request_close wasn't
        // called, cancelling the token signals the drive thread and
        // network tasks to wind down.
        self.cancellation_token.cancel();
    }
}

/// Rolling-window tick counter behind the status bar's TPS readout.
/// Marked once per emulated frame, so the reading follows the match
/// rather than the host's refresh rate.
pub struct TpsCounter {
    marks: VecDeque<Instant>,
    window_size: usize,
}

impl TpsCounter {
    pub fn new(window_size: usize) -> Self {
        Self {
            marks: VecDeque::with_capacity(window_size),
            window_size,
        }
    }

    pub fn mark(&mut self) {
        while self.marks.len() >= self.window_size {
            self.marks.pop_front();
        }
        self.marks.push_back(Instant::now());
    }

    /// Average interval between consecutive marks. ZERO if the
    /// counter has fewer than two marks.
    pub fn mean_duration(&self) -> Duration {
        if self.marks.len() < 2 {
            return Duration::ZERO;
        }
        let mut total = Duration::ZERO;
        let mut count = 0u32;
        for (a, b) in self.marks.iter().zip(self.marks.iter().skip(1)) {
            total += *b - *a;
            count += 1;
        }
        total / count
    }
}

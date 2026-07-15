//! Live PvP emulator session on the SIO-lockstep engine — peer-paired
//! netplay sibling of
//! [`crate::session::singleplayer::SinglePlayerSession`].
//!
//! Both games run locally as an [`mgba_siolink::Link`] pair linked through
//! mgba's lockstep SIO driver (see [`tango_pvp::sio`]): the games speak
//! their *real* link protocol over the emulated cable, and the pair is
//! the rollback unit. There is no mgba thread, no traps, and no shadow —
//! a dedicated drive thread paces the [`Match`] at the GBA frame
//! rate, feeding the local joypad in and shipping each tick's input to
//! the peer. HP/custom/chip telemetry is RAM-polled out of the
//! simulation by the engine's per-tick observer; round starts and the
//! match end are trap-driven off the games' own code paths.
//!
//! Construction is async because it has to wait for the lobby
//! background loop to release the data-channel `Receiver` (it holds it
//! through the cancel-exit path), and then for the drive thread to boot
//! and prime the pair to a live link battle. Once up, this is the same
//! kind of session the UI tick loop already knows how to draw.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tango_pvp::engine::{Match, MatchConfig};
use tango_pvp::telemetry;

pub use tango_pvp::battle::EXPECTED_FPS;

/// Upper bound on how long `is_ended` waits for the peer's
/// `EndOfMatch` packet after local completion. Wide enough to
/// cover slow networks + the typical match-end animation, tight
/// enough that a crashed peer doesn't pin the UI for long.
const PEER_END_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Retransmit-heartbeat cadence for the in-match channel — one emulator frame
/// at [`EXPECTED_FPS`]. Keeps the unacked redundancy window flowing while the
/// local sim is throttled or stalled, so loss recovery isn't coupled to the
/// frame rate (see [`crate::net::InMatchTx`]).
const IN_MATCH_HEARTBEAT: std::time::Duration =
    std::time::Duration::from_nanos((1_000_000_000.0 / EXPECTED_FPS as f64) as u64);

/// Session-redraw cadence while reconnecting (~30 fps), so the give-up progress
/// bar drains smoothly even though the paused drive loop emits no frames. Purely
/// cosmetic.
const RECONNECT_UI_TICK: std::time::Duration = std::time::Duration::from_millis(33);

/// How long the drive loop sleeps per iteration while paused (reconnect) or
/// stalled (peer far behind) — just short enough to react promptly when the
/// condition clears.
const PAUSED_TICK: std::time::Duration = std::time::Duration::from_millis(10);

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
    /// ([`crate::net::PvpReceiver`]).
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
    local_ended_at: Arc<Mutex<Option<std::time::Instant>>>,
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
    local_game: &'static crate::library::game::Game,
    /// This side's player index (P1 = 0, P2 = 1), picked once at match start and
    /// stable for the whole match. The pair is symmetric — core 0 always runs
    /// player 0's game on both peers — so this is also which core is "ours".
    local_player_index: u8,
    joyflags: Arc<AtomicU32>,
    /// Flipped once the games' own match-end path is confirmed — the
    /// direct successor of the trap engine's per-game completion hook.
    completed: Arc<AtomicBool>,
    /// Latching end-of-match signals (remote-ended / remote-disconnected /
    /// local-ended). Grouped in [`EndState`]; `is_ended` reads them
    /// alongside `completed` and `cancellation_token`.
    end: EndState,
    /// Sliding-window timestamp counter marked once per drive-loop frame —
    /// yields the true simulation TPS regardless of how often the UI polls.
    tps_counter: Arc<Mutex<crate::session::stats::Counter>>,
    _audio_binding: Option<crate::platform::audio::Binding>,
    /// Drops fire-cancellation through the drive thread and the network
    /// tasks. On Close we cancel + drop the session, which tears the
    /// network loop down cleanly.
    cancellation_token: tokio_util::sync::CancellationToken,
    /// The peer link: owns the peer connection, both channels' halves, the
    /// latency readout, and the transparent mid-match reconnect (see
    /// [`crate::net::link`]). The supervisor task holds its own `Arc`; the
    /// session's keeps the transport alive for the match's lifetime, and its
    /// eventual drop closes the connection gracefully (DTLS close_notify → the
    /// peer's prompt EOF).
    link: Arc<crate::net::link::Link>,
    /// Live UI readouts published by the drive thread.
    metrics: Arc<Metrics>,
    pub link_code: String,
    pub remote_nickname: String,
    /// Opponent's fully-loaded selection (rom + parsed save +
    /// derived assets) unless they blinded their setup. The session
    /// pane uses it to embed the same save-view we render for
    /// our own side — folder, navi, navicust, the whole thing.
    pub opponent_loaded: Option<crate::selection::Loaded>,
    /// Active-tab / grouping state for the in-match opponent
    /// save-view panel. Mutated via
    /// `session::Message::OpponentSaveViewAction`.
    pub opponent_save_view: crate::save_view::State,
    /// Local side's fully-loaded selection — mirror of
    /// [`opponent_loaded`] for the in-session "my setup" toggle.
    pub local_loaded: Option<crate::selection::Loaded>,
    /// Active-tab / grouping state for the in-match self
    /// save-view panel. Mirror of [`opponent_save_view`].
    pub local_save_view: crate::save_view::State,
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
    stats: Arc<Mutex<tango_pvp::analysis::StatsBuilder>>,
    /// Where this match's replay is being recorded, or `None` if the writer
    /// failed to open. The post-match results screen offers to play it back.
    pub replay_path: Option<std::path::PathBuf>,
    /// When the session was built, for the results screen's match duration.
    started_at: std::time::Instant,
}

/// Everything [`PvpSession::new`] needs, as named fields. Assembled by
/// [`spawn_pvp`](crate::session::spawn_pvp).
pub struct PvpSessionArgs<'a> {
    /// Local/remote game impls; the roms must already have any patch applied.
    pub local_game: &'static crate::library::game::Game,
    pub local_rom: Arc<Vec<u8>>,
    pub remote_game: &'static crate::library::game::Game,
    pub remote_rom: Arc<Vec<u8>>,
    /// The netplay handoff: negotiated terms + the transport bundle.
    pub pre_match: crate::netplay::PreMatchData,
    /// This side's frame delay — realized purely as local display lag (the
    /// engine's present delay). Comes straight from local config; never
    /// negotiated with or sent to the peer.
    pub frame_delay: u32,
    /// Silence the battle BGM (the primers skip the games' battle-start
    /// music call). Comes straight from local config; never negotiated
    /// with or sent to the peer — sound-driver state never feeds battle
    /// logic.
    pub disable_bgm: bool,
    pub replays_path: &'a Path,
    pub cache_path: &'a Path,
    pub audio_binder: &'a crate::platform::audio::LateBinder,
    pub opponent_loaded: Option<crate::selection::Loaded>,
    pub local_loaded: Option<crate::selection::Loaded>,
    pub frame_notify: Arc<tokio::sync::Notify>,
    pub vbuf: Arc<Mutex<Vec<u8>>>,
}

impl PvpSession {
    /// Build the live match from [`PvpSessionArgs`].
    ///
    /// Async because the lobby loop holds the data-channel `Receiver`
    /// until it observes its cancellation and exits (`Link::bring_up`
    /// awaits its handback), and because the drive thread then boots and
    /// primes the pair — a couple of seconds of emulation — before the
    /// session is live.
    pub async fn new(args: PvpSessionArgs<'_>) -> anyhow::Result<Self> {
        let PvpSessionArgs {
            local_game,
            local_rom,
            remote_game,
            remote_rom,
            pre_match,
            frame_delay,
            disable_bgm,
            replays_path,
            cache_path,
            audio_binder,
            opponent_loaded,
            local_loaded,
            frame_notify,
            vbuf,
        } = args;
        let cancellation_token = tokio_util::sync::CancellationToken::new();

        // Parse both sides' committed SRAM dumps. PvP runs entirely off
        // these in-memory images — writes don't persist back to anyone's
        // .sav file.
        let remote_save = remote_game
            .parse_save(&pre_match.remote_save_data)
            .map_err(|e| anyhow::anyhow!("parse remote save: {e:?}"))?;
        let local_save = local_game
            .parse_save(&pre_match.local_save_data)
            .map_err(|e| anyhow::anyhow!("parse local save: {e:?}"))?;

        let local_sio = local_game.pvp;
        let remote_sio = remote_game.pvp;

        // Player index off the shared RNG seed, same negotiation as ever:
        // both peers derive the same assignment, mirrored.
        use rand::SeedableRng;
        let mut rng = rand_pcg::Mcg128Xsl64::from_seed(pre_match.rng_seed);
        let local_player_index = tango_pvp::battle::pick_local_player_index(&mut rng, pre_match.is_offerer);

        // The match clock, pinned into both carts' RTC and recorded in the
        // replay metadata so playback re-primes to the identical state.
        let rtc_time = std::time::UNIX_EPOCH + std::time::Duration::from_millis(pre_match.match_ts);

        // Replay writer. Failing to open it shouldn't kill the
        // match — log and continue without recording.
        let (replay_writer, replay_path) = match build_replay_writer(
            replays_path,
            &pre_match,
            local_player_index,
            local_save.as_ref(),
            remote_save.as_ref(),
        ) {
            Ok((writer, path)) => (Some(writer), Some(path)),
            Err(e) => {
                log::warn!("pvp: replay writer open failed: {e}");
                (None, None)
            }
        };

        // Assemble the peer link from the lobby handoff — this awaits the
        // lobby loop releasing the reliable receiver (typically a few ms after
        // take_pre_match flipped the cancel) and starts the in-match
        // retransmit heartbeat.
        let link = Arc::new(
            crate::net::link::Link::bring_up(pre_match.link_parts, IN_MATCH_HEARTBEAT, cancellation_token.clone())
                .await?,
        );
        let in_match = link.in_match().clone();

        let end = EndState::default();
        let joyflags = Arc::new(AtomicU32::new(0));
        let completed = Arc::new(AtomicBool::new(false));
        let frame_delay = Arc::new(AtomicU32::new(frame_delay));
        let metrics = Arc::new(Metrics::default());
        let drive_paused = Arc::new(AtomicBool::new(false));
        // ~1 s window at 60 Hz, matching the legacy emu_tps_counter.
        let tps_counter = Arc::new(Mutex::new(crate::session::stats::Counter::new(60)));
        vbuf.lock().unwrap().fill(0);

        // Usage semantics can depend on the applied patch (exe45's PvP
        // patch), so they're probed off the patched ROM.
        let stats = Arc::new(Mutex::new(tango_pvp::analysis::StatsBuilder::new(
            local_game.pvp.chip_semantics(local_rom.as_ref()),
            local_game.pvp.counts_buster(local_rom.as_ref()),
        )));

        // Remote input events flow receive-task → drive thread over this
        // queue; the rennet reassembly in PvpReceiver already ordered and
        // deduplicated them (one Input per remote tick, in tick order).
        let (event_tx, event_rx) = std::sync::mpsc::channel::<tango_pvp::net::Event>();

        // The sender pump: the drive thread pushes one Input per advance;
        // the pump ships each as a rennet frame over the unreliable channel.
        let sender = crate::net::PvpSender::new(in_match.clone());

        // Pair-order arrays: core 0 always runs player 0's game, on both
        // peers, so priming and simulation are bit-identical across the pair.
        let (roms, saves, supports) = if local_player_index == 0 {
            (
                [local_rom.as_ref().clone(), remote_rom.as_ref().clone()],
                [local_save.to_sram_dump(), remote_save.to_sram_dump()],
                [local_sio, remote_sio],
            )
        } else {
            (
                [remote_rom.as_ref().clone(), local_rom.as_ref().clone()],
                [remote_save.to_sram_dump(), local_save.to_sram_dump()],
                [remote_sio, local_sio],
            )
        };

        // Boot + prime + run on a dedicated drive thread (the pair is
        // single-threaded by design). Construction happens on the thread —
        // priming is a couple seconds of emulation — and readiness comes
        // back over a oneshot so `new` can fail cleanly.
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<anyhow::Result<tango_pvp::LinkHandle>>();
        {
            let boot = BootPieces {
                roms,
                saves,
                supports,
                match_type: pre_match.match_type,
                rng_seed: pre_match.rng_seed,
                rtc: rtc_time,
                local_player: local_player_index as usize,
                present_delay: frame_delay.load(Ordering::Relaxed),
                disable_bgm,
            };
            let drive = DriveContext {
                joyflags: joyflags.clone(),
                frame_delay: frame_delay.clone(),
                metrics: metrics.clone(),
                drive_paused: drive_paused.clone(),
                cancel: cancellation_token.clone(),
                completed: completed.clone(),
                end: end.clone(),
                event_rx,
                sender,
                in_match: in_match.clone(),
                replay_writer,
                stats: stats.clone(),
                stats_path: replay_path
                    .as_ref()
                    .map(|p| crate::library::replays::stats_path(cache_path, replays_path, p)),
                tps_counter: tps_counter.clone(),
                vbuf: vbuf.clone(),
                frame_notify: frame_notify.clone(),
                local_player: local_player_index as usize,
                rt: tokio::runtime::Handle::current(),
            };
            std::thread::Builder::new()
                .name("tango-sio-drive".to_owned())
                .spawn(move || drive.run(boot, ready_tx))?;
        }

        // Wait for boot + priming before declaring the session live. Ready
        // hands back the handle to the live pair for the audio stream.
        let pair_handle = ready_rx
            .await
            .map_err(|_| anyhow::anyhow!("sio drive thread died during boot"))??;

        // Audio: the host output stream plays the local core directly,
        // pulling and resampling its samples straight off the pair (rate
        // control follows the drive loop's published fps target) — see
        // [`crate::session::pair_stream::PairStream`].
        let audio_binding = match audio_binder.bind(Some(Box::new(crate::session::pair_stream::PairStream::new(
            pair_handle,
            {
                let local_player = local_player_index as usize;
                move || local_player
            },
            {
                let metrics = metrics.clone();
                move || f32::from_bits(metrics.fps_target.load(Ordering::Relaxed))
            },
            audio_binder.sample_rate(),
        )))) {
            Ok(b) => Some(b),
            Err(e) => {
                log::warn!("pvp: audio bind failed: {e:?}");
                None
            }
        };

        // Receive pump + link supervisor: reads peer frames into the event
        // queue, watches for stalls, and runs the transparent reconnect.
        spawn_supervisor(SupervisorContext {
            link: link.clone(),
            in_match,
            event_tx,
            end: end.clone(),
            completed: completed.clone(),
            cancel: cancellation_token.clone(),
            metrics: metrics.clone(),
            drive_paused: drive_paused.clone(),
            frame_notify: frame_notify.clone(),
        });

        Ok(Self {
            local_game,
            local_player_index,
            joyflags,
            completed,
            end,
            tps_counter,
            _audio_binding: audio_binding,
            cancellation_token,
            link,
            metrics,
            link_code: pre_match.link_code,
            remote_nickname: pre_match.remote_settings.nickname,
            opponent_loaded,
            opponent_save_view: crate::save_view::State::new(),
            local_loaded,
            local_save_view: crate::save_view::State::new(),
            frame_delay,
            stats,
            replay_path,
            started_at: std::time::Instant::now(),
        })
    }

    pub fn game(&self) -> &'static crate::library::game::Game {
        self.local_game
    }

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
        self.frame_delay.store(
            frame_delay.clamp(tango_pvp::battle::MIN_FRAME_DELAY, tango_pvp::battle::MAX_FRAME_DELAY),
            Ordering::Relaxed,
        );
    }

    /// Overwrite the joyflag bitmap (same shape as singleplayer's
    /// — see [`crate::session::singleplayer::SinglePlayerSession::set_joyflags`]).
    pub fn set_joyflags(&self, mgba_keys: u32) {
        self.joyflags.store(mgba_keys, Ordering::Relaxed);
    }

    pub fn request_close(&self) {
        // Cancelling the token is the whole close signal: it stops the
        // drive thread and the supervisor and flips `is_ended`'s
        // cancellation check. The supervisor announces the quit to the
        // peer (best-effort `Goodbye`) on its way out, so the peer ends
        // at once instead of trying to reconnect to us.
        self.cancellation_token.cancel();
    }

    /// `true` while the link has dropped and the session is transparently
    /// rebuilding it (direct or matchmaking) — the drive loop is paused and the
    /// PvP view shows a "Reconnecting…" overlay.
    pub fn is_reconnecting(&self) -> bool {
        matches!(self.link.health(), crate::net::link::LinkHealth::Reconnecting { .. })
    }

    /// Fraction of the reconnect give-up window still remaining — `1.0` when a
    /// reconnect just started, falling to `0.0` at the give-up deadline, or
    /// `None` when not reconnecting. Drives the overlay's depleting progress bar.
    pub fn reconnect_progress(&self) -> Option<f32> {
        match self.link.health() {
            crate::net::link::LinkHealth::Reconnecting { started, give_up_at } => {
                let total = give_up_at
                    .saturating_duration_since(started)
                    .as_secs_f32()
                    .max(f32::EPSILON);
                let remaining = give_up_at
                    .saturating_duration_since(std::time::Instant::now())
                    .as_secs_f32();
                Some((remaining / total).clamp(0.0, 1.0))
            }
            _ => None,
        }
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
    pub fn is_ended(&self) -> bool {
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
    /// for the post-match results screen; rounds the match never finished
    /// (mid-round disconnect) simply aren't in it.
    pub fn stats_snapshot(&self) -> tango_pvp::analysis::MatchStats {
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

/// Subset of the engine's per-frame metrics surfaced in the status bar.
#[derive(Clone, Copy, Debug)]
pub struct RoundStats {
    /// Real-time clock skew the throttler reacts to (see
    /// [`tango_pvp::Throttler`]). The symmetric network term cancels
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

// ---------------------------------------------------------------------------
// The drive thread: boots + primes the pair, then paces the session.

/// What the drive thread needs to boot the [`Match`].
struct BootPieces {
    roms: [Vec<u8>; 2],
    saves: [Vec<u8>; 2],
    supports: [&'static (dyn tango_pvp::GameSupport + Send + Sync); 2],
    match_type: (u8, u8),
    rng_seed: [u8; 16],
    rtc: std::time::SystemTime,
    local_player: usize,
    present_delay: u32,
    disable_bgm: bool,
}

struct DriveContext {
    joyflags: Arc<AtomicU32>,
    frame_delay: Arc<AtomicU32>,
    metrics: Arc<Metrics>,
    drive_paused: Arc<AtomicBool>,
    cancel: tokio_util::sync::CancellationToken,
    completed: Arc<AtomicBool>,
    end: EndState,
    event_rx: std::sync::mpsc::Receiver<tango_pvp::net::Event>,
    sender: crate::net::PvpSender,
    in_match: crate::net::InMatchTx,
    replay_writer: Option<tango_pvp::replay::Writer>,
    stats: Arc<Mutex<tango_pvp::analysis::StatsBuilder>>,
    stats_path: Option<std::path::PathBuf>,
    tps_counter: Arc<Mutex<crate::session::stats::Counter>>,
    vbuf: Arc<Mutex<Vec<u8>>>,
    frame_notify: Arc<tokio::sync::Notify>,
    local_player: usize,
    rt: tokio::runtime::Handle,
}

impl DriveContext {
    fn run(
        mut self,
        pieces: BootPieces,
        ready_tx: tokio::sync::oneshot::Sender<anyhow::Result<tango_pvp::LinkHandle>>,
    ) {
        let mut match_ = match Match::new(MatchConfig {
            roms: pieces.roms,
            saves: pieces.saves,
            support: [pieces.supports[0], pieces.supports[1]],
            match_type: pieces.match_type,
            rng_seed: pieces.rng_seed,
            rtc: pieces.rtc,
            local_player: pieces.local_player,
            present_delay: pieces
                .present_delay
                .clamp(tango_pvp::battle::MIN_FRAME_DELAY, tango_pvp::battle::MAX_FRAME_DELAY),
            disable_bgm: pieces.disable_bgm,
        }) {
            Ok(m) => m,
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
        let _ = ready_tx.send(Ok(match_.pair_handle()));

        if let Some(w) = self.replay_writer.as_mut() {
            // The SIO stream is one continuous run of pair ticks; the
            // container wants at least one open round.
            let _ = w.start_round();
        }

        let mut throttler = tango_pvp::Throttler::new();
        let mut next_tick = std::time::Instant::now();
        // (tick, [p0, p1]) confirmed input pairs not yet folded into
        // stats (the telemetry for those ticks may confirm later).
        let mut pending_buttons: std::collections::VecDeque<(u32, [u32; 2])> = std::collections::VecDeque::new();
        // Whether the first round's Started has been seen. Later Started
        // events stamp round markers into the replay stream.
        let mut first_round_started = false;
        // Confirmed ticks whose replay input record must carry a
        // round-start marker (see the write loop below).
        let mut pending_round_marks: std::collections::VecDeque<u32> = std::collections::VecDeque::new();
        let mut fired_end_of_match = false;

        loop {
            if self.cancel.is_cancelled() {
                break;
            }
            if self.drive_paused.load(Ordering::Relaxed) {
                std::thread::sleep(PAUSED_TICK);
                next_tick = std::time::Instant::now();
                continue;
            }

            // Live present-delay adjustment from the footer slider.
            let pd = self.frame_delay.load(Ordering::Relaxed);
            if pd != match_.present_delay() {
                match_.set_present_delay(pd);
            }

            // Drain the network before advancing: every confirmed tick we
            // ingest now is a rollback we don't take deeper.
            for event in self.event_rx.try_iter() {
                match event {
                    tango_pvp::net::Event::Input(input) => {
                        match_.add_remote_input(input.joyflags as u32, input.tick_advantage);
                    }
                }
            }

            // Stall guard: the peer is too far behind (or gone) — advancing
            // further would run the input stream past the rennet horizon.
            // The in-match heartbeat keeps the redundancy window + acks
            // flowing while we wait; the supervisor watches queue_len and
            // decides whether this is a reconnect.
            let queue_len = match_.local_queue_length() as u32;
            self.metrics.queue_len.store(queue_len, Ordering::Relaxed);
            if queue_len as usize >= tango_pvp::battle::RECONNECT_QUEUE_LENGTH {
                std::thread::sleep(PAUSED_TICK);
                next_tick = std::time::Instant::now();
                continue;
            }

            // Sample the skew before `advance` enqueues this tick's local
            // input, so our half matches the advantage we ship the peer.
            let skew = match_.skew();
            self.metrics.skew.store(skew, Ordering::Relaxed);

            let keys = self.joyflags.load(Ordering::Relaxed) & tango_pvp::input::JOYFLAGS_MASK as u32;
            let (outgoing, report) = match match_.advance(keys) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("pvp: sio advance failed: {e}");
                    self.cancel.cancel();
                    break;
                }
            };
            self.metrics.depth.store(report.rolled_back, Ordering::Relaxed);

            // Ship this tick's local input. Push-before-send semantics live
            // in the pump; a transport error is non-terminal (the heartbeat
            // retransmits once the reconnect swaps a live channel back in).
            if tango_pvp::net::Sender::send(
                &mut self.sender,
                &tango_pvp::net::Event::Input(tango_pvp::net::Input {
                    joyflags: outgoing.keys as u16,
                    tick_advantage: outgoing.tick_advantage,
                }),
            )
            .is_err()
            {
                log::warn!("pvp: send pump terminated; ending match");
                self.end.remote_disconnected.store(true, Ordering::Release);
                self.cancel.cancel();
                break;
            }

            // Confirmed telemetry, drained before the replay write so this
            // batch's round-start events can stamp markers onto this
            // batch's input records. Everything at or below the confirmed
            // boundary is final — no revocation bookkeeping needed on this
            // side of the engine.
            let (samples, events) = match_.telemetry().lock().unwrap().drain_confirmed(report.confirmed);

            // Round lifecycle, trap-driven off the games' own code paths:
            // a round start (after the first) stamps a marker into the
            // replay; the match-end anchor firing means the players left
            // the battle loop for good — the direct successor of the trap
            // engine's match-end hook.
            let mut match_ended = false;
            for (tick, event) in &events {
                match event {
                    telemetry::RoundEvent::Started => {
                        if first_round_started {
                            pending_round_marks.push_back(*tick);
                        }
                        first_round_started = true;
                    }
                    telemetry::RoundEvent::Ended { .. } => {}
                    telemetry::RoundEvent::MatchEnded => {
                        match_ended = true;
                    }
                }
            }

            // Confirmed inputs: replay sink + the buttons half of the
            // stats merge below.
            let confirmed_inputs = match_.drain_confirmed();
            if let Some(w) = self.replay_writer.as_mut() {
                for (tick, keys) in &confirmed_inputs {
                    if pending_round_marks.front().is_some_and(|t| t <= tick) {
                        pending_round_marks.pop_front();
                        let _ = w.start_round();
                    }
                    let (p0, p1) = (keys[0] as u16, keys[1] as u16);
                    let (local, remote) = if self.local_player == 0 { (p0, p1) } else { (p1, p0) };
                    if let Err(e) = w.write_input(
                        self.local_player as u8,
                        &(
                            tango_pvp::input::PartialInput { joyflags: local },
                            tango_pvp::input::PartialInput { joyflags: remote },
                        ),
                    ) {
                        log::warn!("pvp: replay write failed (recording stops): {e}");
                        self.replay_writer = None;
                        break;
                    }
                }
            }
            pending_buttons.extend(confirmed_inputs);

            if !samples.is_empty() || !events.is_empty() {
                self.fold_confirmed_telemetry(samples, events, &mut pending_buttons);
            }

            // Completion: the games' own match-end path ran (confirmed —
            // both peers see it at the same pair tick). No runout needed:
            // the anchor fires after the result screens have played.
            if match_ended {
                self.completed.store(true, Ordering::Release);
            }
            if self.completed.load(Ordering::Acquire) && !fired_end_of_match {
                fired_end_of_match = true;
                let first_completion = {
                    let mut completed_at = self.end.local_ended_at.lock().unwrap();
                    if completed_at.is_some() {
                        false
                    } else {
                        *completed_at = Some(std::time::Instant::now());
                        true
                    }
                };
                if first_completion {
                    // In-band EndOfMatch: rides the same ordered seq stream
                    // as inputs, so the peer sees it exactly once and only
                    // after every preceding input.
                    let in_match = self.in_match.clone();
                    self.rt.spawn(async move {
                        if let Err(e) = in_match.send_end_of_match().await {
                            log::warn!("pvp: send EndOfMatch failed: {e}");
                        }
                    });
                    // Wall-clock fallback wake so `is_ended` is rechecked
                    // even if the peer never sends EndOfMatch.
                    let notify = self.frame_notify.clone();
                    self.rt.spawn(async move {
                        tokio::time::sleep(PEER_END_GRACE).await;
                        notify.notify_one();
                    });
                }
            }

            // Present the local screen to the UI. (Audio needs no push —
            // the output stream pulls it straight off the pair.)
            if let Some(buf) = match_.local_video_buffer() {
                let mut vbuf = self.vbuf.lock().unwrap();
                if vbuf.len() == buf.len() {
                    vbuf.copy_from_slice(&buf);
                }
            }
            self.tps_counter.lock().unwrap().mark();
            self.frame_notify.notify_one();

            // Clock sync: only the leading peer shaves tick rate, and only
            // once the presented frame actually speculates past the present
            // delay.
            let slowdown = throttler.step(skew, match_.speculation_balance());
            let target = EXPECTED_FPS - slowdown;
            self.metrics.fps_target.store(target.to_bits(), Ordering::Relaxed);

            // Pace at the base rate minus whatever the throttler shaved.
            next_tick += std::time::Duration::from_secs_f64(1.0 / target as f64);
            let now = std::time::Instant::now();
            if next_tick > now {
                std::thread::sleep(next_tick - now);
            } else if now - next_tick > std::time::Duration::from_millis(250) {
                // Fell way behind (debugger, laptop lid, ...): don't sprint
                // to catch up, just resynchronize the cadence.
                next_tick = now;
            }
        }

        // Teardown: flush the replay tail. Finalize (write the EOR
        // sentinel) only if the match completed — same policy as the trap
        // engine, so an aborted match leaves a truncated-but-parseable
        // recording.
        if let Some(mut w) = self.replay_writer.take() {
            for (_, keys) in match_.drain_confirmed() {
                let (p0, p1) = (keys[0] as u16, keys[1] as u16);
                let (local, remote) = if self.local_player == 0 { (p0, p1) } else { (p1, p0) };
                let _ = w.write_input(
                    self.local_player as u8,
                    &(
                        tango_pvp::input::PartialInput { joyflags: local },
                        tango_pvp::input::PartialInput { joyflags: remote },
                    ),
                );
            }
            if self.completed.load(Ordering::Acquire) {
                if let Err(e) = w.finish() {
                    log::error!("finish replay failed: {e}");
                }
                // Cache the finished match's stats — each round already
                // folded as it ended, so the Replays tab never has to
                // re-simulate this one.
                if let Some(stats_path) = self.stats_path.as_ref() {
                    let snapshot = self.stats.lock().unwrap().snapshot();
                    if let Err(e) = crate::library::replays::write_match_stats(stats_path, &snapshot) {
                        log::warn!("failed to write replay stats cache entry: {e}");
                    }
                }
            }
        }
    }

    /// Fold a batch of confirmed telemetry into the stats builder (the
    /// shared [`tango_pvp::analysis::fold_confirmed`], so live stats
    /// and offline re-analysis stay byte-equivalent) and drive the round
    /// lifecycle off the events.
    fn fold_confirmed_telemetry(
        &mut self,
        samples: Vec<(u32, telemetry::BattleObs)>,
        events: Vec<(u32, telemetry::RoundEvent)>,
        pending_buttons: &mut std::collections::VecDeque<(u32, [u32; 2])>,
    ) {
        let mut stats = self.stats.lock().unwrap();
        tango_pvp::analysis::fold_confirmed(&mut stats, self.local_player, samples, events, &mut |tick| {
            // Discard input pairs older than this sample; the front pair
            // at the sample's tick carries its buttons. Samples arrive
            // tick-ascending, so this never skips a later sample's pair.
            while pending_buttons.front().is_some_and(|(t, _)| *t < tick) {
                pending_buttons.pop_front();
            }
            match pending_buttons.front() {
                Some(&(t, keys)) if t == tick => Some(keys),
                _ => None,
            }
        });
    }
}

// ---------------------------------------------------------------------------
// The receive pump + link supervisor.

struct SupervisorContext {
    link: Arc<crate::net::link::Link>,
    in_match: crate::net::InMatchTx,
    event_tx: std::sync::mpsc::Sender<tango_pvp::net::Event>,
    end: EndState,
    completed: Arc<AtomicBool>,
    cancel: tokio_util::sync::CancellationToken,
    metrics: Arc<Metrics>,
    drive_paused: Arc<AtomicBool>,
    frame_notify: Arc<tokio::sync::Notify>,
}

/// Pump one receiver until error/EOF, forwarding events to the drive
/// thread. Returns when the channel dies (the reconnect decision is the
/// supervisor's).
async fn run_receive_pump(
    mut receiver: crate::net::PvpReceiver,
    event_tx: std::sync::mpsc::Sender<tango_pvp::net::Event>,
    frame_notify: Arc<tokio::sync::Notify>,
) -> std::io::Error {
    loop {
        match tango_pvp::net::Receiver::receive(&mut receiver).await {
            Ok(event) => {
                if event_tx.send(event).is_err() {
                    return std::io::Error::new(std::io::ErrorKind::BrokenPipe, "drive thread gone");
                }
                // Remote inputs settle ticks; make sure a paused/idle UI
                // still observes progress.
                frame_notify.notify_one();
            }
            Err(e) => return e,
        }
    }
}

/// Receive loop + link supervisor. Reads peer frames into the drive
/// thread's queue until the match ends (completion / cancel) or, when the
/// link drops, until reconnection gives up. Policy lives here — deciding
/// when a trip is worth reconnecting and freezing/unfreezing the drive
/// loop around the attempt. The transport surgery (silent teardown,
/// rebuild, hot-swap under the persistent rennet streams) is
/// [`crate::net::link::Link::reconnect`]'s; the lockstep sim treats the
/// whole gap as a pause, so no state resync is needed.
fn spawn_supervisor(ctx: SupervisorContext) {
    let SupervisorContext {
        link,
        in_match,
        event_tx,
        end,
        completed,
        cancel,
        metrics,
        drive_paused,
        frame_notify,
    } = ctx;

    let make_receiver = {
        let link = link.clone();
        let in_match = in_match.clone();
        let end = end.clone();
        let frame_notify = frame_notify.clone();
        move || -> Option<crate::net::PvpReceiver> {
            Some(crate::net::PvpReceiver::new(
                link.take_match_receiver()?,
                in_match.clone(),
                link.latency_handle(),
                end.remote_ended.clone(),
                frame_notify.clone(),
            ))
        }
    };

    tokio::task::spawn(async move {
        // Why the receive loop ended this iteration.
        enum Trip {
            /// Clean local teardown (user closed / cancelled). Announces the
            /// quit to the peer (best-effort `Goodbye`), never reconnects.
            Cancelled,
            /// The peer announced a deliberate quit (the control channel's
            /// `Goodbye`): it is leaving and will never be at a rendezvous,
            /// so the match ends at once — no reconnect window at all.
            PeerQuit,
            /// A channel hit EOF without a goodbye — the peer's reconnect
            /// dropping its old transport (libdatachannel closes gracefully;
            /// there is no silent teardown), its transport declaring the
            /// link dead, or a quit whose goodbye was lost. We can't tell
            /// those apart, so this reconnects on a *short* window: a real
            /// drop's peer is already waiting at the rendezvous and rejoins
            /// in a second or two, while a lost-goodbye quit finds no one
            /// there and ends quickly.
            Closed,
            /// The local input queue climbed to `RECONNECT_QUEUE_LENGTH`:
            /// the peer stopped matching our inputs, i.e. a quiet/dead link.
            /// Reconnects on the full per-transport window.
            Stalled,
        }

        let mut receiver = make_receiver().expect("bring_up parks the in-match receiver");
        loop {
            // The stall watch: poll the drive thread's published queue
            // length. Coarse (10 Hz) is fine — the queue takes seconds to
            // climb to the trip point.
            let stall_watch = async {
                loop {
                    if metrics.queue_len.load(Ordering::Relaxed) as usize >= tango_pvp::battle::RECONNECT_QUEUE_LENGTH {
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            };
            let trip = tokio::select! {
                biased;
                _ = cancel.cancelled() => Trip::Cancelled,
                end = link.watch_control() => match end {
                    crate::net::link::ControlEnd::Goodbye => {
                        log::info!("pvp: peer announced a quit");
                        Trip::PeerQuit
                    }
                    crate::net::link::ControlEnd::Eof => Trip::Closed,
                },
                e = run_receive_pump(receiver, event_tx.clone(), frame_notify.clone()) => {
                    log::info!("pvp in-match channel closed: {e:?}");
                    Trip::Closed
                }
                _ = stall_watch => Trip::Stalled,
            };

            // Our own deliberate close: announce it, then stop. The
            // session's teardown drops the peer connection gracefully, and
            // its DTLS close_notify hands the peer a prompt EOF — but that
            // EOF alone is ambiguous over there (our reconnect's transport
            // drop looks identical), so send the goodbye first to let the
            // peer end at once instead of burning its clean-close reconnect
            // window on us. Best-effort: if it's lost, that window is the
            // fallback.
            if matches!(trip, Trip::Cancelled) {
                link.send_goodbye().await;
                break;
            }

            // Reconnect on any mid-match link loss — a stalled input queue
            // *or* a bare channel close — as long as the transport can
            // rebuild and the match isn't ending (our completion or the
            // peer's EndOfMatch). A close uses the short give-up window, so
            // a real drop reconnects fast while a lost-goodbye quit still
            // ends quickly. An announced quit (`PeerQuit`) never reconnects
            // — the peer told us it isn't coming back.
            let reconnectable = matches!(trip, Trip::Stalled | Trip::Closed)
                && link.can_reconnect()
                && !completed.load(Ordering::Acquire)
                && !end.remote_ended.load(Ordering::Acquire);
            if !reconnectable {
                end.remote_disconnected.store(true, Ordering::Release);
                cancel.cancel();
                break;
            }

            // Freeze the drive loop so its speculative lead can't run past
            // the rollback horizon while the link is down. Both peers
            // converge on the rebuild: whoever trips first goes silent,
            // which stall-trips the other within RECONNECT_QUEUE_LENGTH
            // frames.
            drive_paused.store(true, Ordering::Relaxed);
            frame_notify.notify_one();
            log::info!("pvp link dropped — pausing to reconnect");

            // Rebuild + hot-swap (the link's job), ticking the UI at
            // ~30 fps so the give-up bar drains smoothly while the paused
            // drive loop produces no frames.
            let restored = {
                let ui_tick = async {
                    let mut iv = tokio::time::interval(RECONNECT_UI_TICK);
                    loop {
                        iv.tick().await;
                        frame_notify.notify_one();
                    }
                };
                let cause = if matches!(trip, Trip::Closed) {
                    crate::net::link::ReconnectCause::CleanClose
                } else {
                    crate::net::link::ReconnectCause::Stall
                };
                tokio::select! {
                    restored = link.reconnect(cause) => restored,
                    _ = ui_tick => unreachable!(),
                }
            };

            if !restored {
                // Timed out or cancelled — give up and end the match.
                end.remote_disconnected.store(true, Ordering::Release);
                cancel.cancel();
                drive_paused.store(false, Ordering::Relaxed);
                break;
            }

            // Fresh receiver over the swapped channel, same `in_match` —
            // the rennet in-stream (seq/ack) carries across the swap, so
            // the peer's resent window fills our gap contiguously. The
            // drive loop resumes; its stall guard holds it below the
            // horizon until the resends drain the queue.
            receiver = make_receiver().expect("reconnect parks a fresh receiver");
            drive_paused.store(false, Ordering::Relaxed);
            frame_notify.notify_one();
            log::info!("pvp transparently reconnected the link");
        }

        // Teardown: retire latency so `latency()` reads `None` and the
        // telemetry panel retires, and wake the session to re-check
        // `is_ended` (the drive loop may already be gone, so no frame is
        // coming).
        link.retire_latency();
        drive_paused.store(false, Ordering::Relaxed);
        frame_notify.notify_one();
    });
}

/// Open the replay file + write its metadata frame, returning the writer
/// along with the path it records to (surfaced on the session so the
/// post-match results screen can offer playback). Everything the metadata
/// needs lives on `pre_match` (settings, seed, match clock, link code).
/// Filename format mirrors the legacy app:
/// `YYYYMMDDhhmmss-<link_code>-<compat>-vs-<opponent>-p<idx>.tangoreplay`.
fn build_replay_writer(
    replays_path: &Path,
    pre_match: &crate::netplay::PreMatchData,
    local_player_index: u8,
    local_save: &(dyn tango_dataview::save::Save + Send + Sync),
    remote_save: &(dyn tango_dataview::save::Save + Send + Sync),
) -> anyhow::Result<(tango_pvp::replay::Writer, std::path::PathBuf)> {
    let link_code = &pre_match.link_code;
    let local_settings = &pre_match.local_settings;
    let remote_settings = &pre_match.remote_settings;
    std::fs::create_dir_all(replays_path)?;
    let local_gi = local_settings
        .game_info
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("local settings missing game info"))?;
    let remote_gi = remote_settings
        .game_info
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("remote settings missing game info"))?;
    let netplay_compat = local_gi
        .patch
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| local_gi.family_and_variant.0.clone());
    let ts = chrono::Local::now().format("%Y%m%d%H%M%S");
    // Direct sessions have no link code in their metadata —
    // substitute a stable placeholder here so the filename
    // doesn't end up with a double-dash where the slot would be.
    let filename_link_code = if link_code.is_empty() { "direct" } else { link_code };
    let raw_name = format!(
        "{ts}-{filename_link_code}-{netplay_compat}-vs-{}-p{}",
        remote_settings.nickname,
        local_player_index + 1
    );
    let safe_name: String = raw_name.chars().filter(|c| !"/\\?%*:|\"<>. ".contains(*c)).collect();
    let replay_filename = replays_path.join(format!("{safe_name}.tangoreplay"));
    log::info!("pvp: opening replay file {}", replay_filename.display());

    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&replay_filename)?;
    let local_sram = local_save.to_sram_dump();
    let remote_sram = remote_save.to_sram_dump();
    let writer = tango_pvp::replay::Writer::new(
        // Buffered: write_input runs on the drive thread once per
        // confirmed tick, and unbuffered it costs a few small write
        // syscalls each time. The format already recovers truncated tails,
        // so a hard crash losing the buffered tail of an (already
        // incomplete) replay changes nothing; finish() flushes.
        std::io::BufWriter::new(file),
        // SIO-engine stream: one continuous run of pair ticks.
        tango_pvp::replay::VERSION,
        tango_pvp::replay::Metadata {
            // The negotiated match clock, not the local wall clock: both
            // cores' cart RTC is pinned to this instant, and playback
            // re-primes pinned to `metadata.ts`, so recording the same
            // value is what makes playback reproduce the live match. Both
            // peers' replays of one match carry the identical ts.
            ts: pre_match.match_ts,
            link_code: link_code.clone(),
            local_side: Some(tango_pvp::replay::metadata::Side {
                nickname: local_settings.nickname.clone(),
                game_info: Some(tango_pvp::replay::metadata::GameInfo {
                    rom_family: local_gi.family_and_variant.0.clone(),
                    rom_variant: local_gi.family_and_variant.1 as u32,
                    patch: local_gi
                        .patch
                        .as_ref()
                        .map(|p| tango_pvp::replay::metadata::game_info::Patch {
                            name: p.name.clone(),
                            version: p.version.to_string(),
                        }),
                }),
                // The replay metadata proto (replay11) predates the
                // blind-setup inversion and still stores the
                // positive "reveal" sense.
                reveal_setup: !local_settings.blind_setup,
                client_cert_fingerprint_sha256: pre_match.local_client_cert_fingerprint.clone(),
            }),
            remote_side: Some(tango_pvp::replay::metadata::Side {
                nickname: remote_settings.nickname.clone(),
                game_info: Some(tango_pvp::replay::metadata::GameInfo {
                    rom_family: remote_gi.family_and_variant.0.clone(),
                    rom_variant: remote_gi.family_and_variant.1 as u32,
                    patch: remote_gi
                        .patch
                        .as_ref()
                        .map(|p| tango_pvp::replay::metadata::game_info::Patch {
                            name: p.name.clone(),
                            version: p.version.to_string(),
                        }),
                }),
                reveal_setup: !remote_settings.blind_setup,
                client_cert_fingerprint_sha256: pre_match.peer_client_cert_fingerprint.clone(),
            }),
            match_type: pre_match.match_type.0 as u32,
            match_subtype: pre_match.match_type.1 as u32,
        },
        pre_match.is_offerer,
        local_player_index,
        pre_match.rng_seed,
        &local_sram,
        &remote_sram,
    )?;
    Ok((writer, replay_filename))
}

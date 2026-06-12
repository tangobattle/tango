//! Live PvP emulator session — peer-paired netplay sibling of
//! [`crate::singleplayer_session::SinglePlayerSession`]. Owns the
//! mgba thread driven by the local ROM + save, hooks the primary
//! traps that talk to the shared `tango_pvp::battle::Match`, and
//! spawns the background match-run task that pumps remote inputs
//! into the in-progress round.
//!
//! Construction is async because it has to wait for the lobby
//! background loop to release the data-channel `Receiver` (it
//! holds it through the cancel-exit path). Once the receiver
//! arrives, this is the same kind of session the UI tick loop
//! already knows how to draw.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub use tango_pvp::battle::EXPECTED_FPS;

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
/// shared handle: the net receive task, the match-run task, and the
/// frame_callback each keep one and raise their own signal.
#[derive(Clone, Default)]
struct EndState {
    /// Remote's in-game match-end handshake (`Packet::EndOfMatch`) arrived
    /// — raised by the net receive task ([`crate::net::PvpReceiver`]).
    /// `is_ended` honors it so the lagging side gets time to write its
    /// replay tail before we drop the data channel.
    remote_ended: Arc<AtomicBool>,
    /// Remote's data channel closed (clean RTC `on_closed` or receiver
    /// `Err`) — raised by the match-run task. No more packets are coming,
    /// so `is_ended` skips straight past the grace window.
    remote_disconnected: Arc<AtomicBool>,
    /// Wall-clock instant we first observed local completion, or `None`
    /// until then. Pulls double duty: the frame_callback fires our
    /// `EndOfMatch` exactly once on the `None → Some` edge, and `is_ended`
    /// reads the stamp as the fallback grace deadline so a silent peer
    /// can't pin us forever.
    local_ended_at: Arc<Mutex<Option<std::time::Instant>>>,
}

pub struct PvpSession {
    local_game: &'static crate::game::Game,
    /// This side's player index (P1 = 0, P2 = 1), picked once at match start and
    /// stable for the whole match. Match-level, not round-level — the instrument
    /// panel's P1/P2 tag reads it directly so it shows even between rounds, when
    /// there's no live [`RoundStats`].
    local_player_index: u8,
    joyflags: Arc<AtomicU32>,
    /// Per-game in-match hook fires `completion_token.complete()`
    /// once the match has actually reached its end-game screen.
    /// We hold a handle to poll it from the UI tick so the
    /// session can self-close — same trigger as the legacy
    /// `Session::completed()` check (see `tango/src/gui.rs`'s
    /// `should_close` block).
    completion_token: tango_pvp::hooks::CompletionToken,
    /// Latching end-of-match signals (remote-ended / remote-disconnected /
    /// local-ended). Grouped in [`EndState`]; `is_ended` reads them
    /// alongside `completion_token` and `cancellation_token`.
    end: EndState,
    /// Sliding-window timestamp counter marked once per emulator
    /// frame_callback — yields the true emulator TPS regardless
    /// of how often the UI polls. Equivalent to legacy
    /// `tango::stats::Counter` driven by the same callback site.
    tps_counter: Arc<std::sync::Mutex<crate::stats::Counter>>,
    _audio_binding: Option<crate::audio::Binding>,
    _thread: mgba::thread::Thread,
    /// Drops fire-cancellation through the match background tasks
    /// (`Match::run`, `Match::cancel`). On Close we cancel + drop
    /// the session, which tears the network loop down cleanly.
    cancellation_token: tokio_util::sync::CancellationToken,
    /// Ping tracker shared with the net receive task. `Some` while the link is
    /// up; the match-run task swaps it to `None` the moment the remote drops,
    /// which is how the UI retires the instrument panel (see [`Self::latency`]).
    latency_counter: Arc<tokio::sync::Mutex<Option<crate::net::LatencyCounter>>>,
    /// `None` for the direct-TCP local transport (the TCP stream
    /// halves live inside the Sender/Receiver). `Some` for WebRTC,
    /// where the peer connection must outlive the data channel.
    _peer_conn: Option<datachannel_wrapper::PeerConnection>,
    /// Kept alive so the background `match_.run(receiver)` task
    /// has a referent. Cleared by that task when it exits. The UI
    /// also reads this each tick to scrape the current round's
    /// player-index / queue-lengths for the status bar.
    match_handle: tango_pvp::hooks::MatchHandle,
    pub link_code: String,
    pub remote_nickname: String,
    /// Opponent's fully-loaded selection (rom + parsed save +
    /// derived assets) if they enabled reveal-setup. The session
    /// pane uses it to embed the same save-view we render for
    /// our own side — folder, navi, navicust, the whole thing.
    pub opponent_loaded: Option<crate::selection::Loaded>,
    /// Active-tab / grouping state for the in-match opponent
    /// save-view panel. Mutated via
    /// `session::Message::OpponentSaveViewAction`.
    pub opponent_save_view: crate::save_view::State,
    /// Local side's fully-loaded selection — mirror of
    /// [`opponent_loaded`] for the in-session "my setup" toggle.
    /// Always present in PvP (the user always has access to
    /// their own save); kept Optional only to match the shape
    /// of the opponent field for the shared rendering path.
    pub local_loaded: Option<crate::selection::Loaded>,
    /// Active-tab / grouping state for the in-match self
    /// save-view panel. Mirror of [`opponent_save_view`].
    pub local_save_view: crate::save_view::State,
    /// Live local frame delay, shared with the running `Match`.
    /// The footer slider writes it via [`set_frame_delay`]; the netcode reads it
    /// each rendered frame. Purely local — never negotiated or sent to the peer.
    frame_delay: Arc<AtomicU32>,
}

impl PvpSession {
    /// Build the live match. `local_rom` must already have any
    /// patch applied; `pre_match` carries every other piece of
    /// state we negotiated with the peer.
    ///
    /// Async because the lobby loop holds the data-channel
    /// `Receiver` until it observes its cancellation and exits;
    /// we poll the handoff slot until it appears (worst case a
    /// few ms after `take_pre_match` flips the cancel flag).
    pub async fn new(
        local_game: &'static crate::game::Game,
        local_rom: Arc<Vec<u8>>,
        remote_game: &'static crate::game::Game,
        remote_rom: Arc<Vec<u8>>,
        pre_match: crate::netplay::PreMatchData,
        // This side's frame delay — realized purely as local display lag (how
        // far the display core trails the netcode frontier). Comes straight from
        // local config; never negotiated with or sent to the peer.
        frame_delay: u32,
        // Whether to skip the game's battle BGM for this match. Local-only
        // like the volume — the peer is unaffected, and the recorded replay
        // keeps its music (export has its own mute toggle). Sampled from
        // config at match start.
        disable_bgm: bool,
        replays_path: &Path,
        audio_binder: &crate::audio::LateBinder,
        opponent_loaded: Option<crate::selection::Loaded>,
        local_loaded: Option<crate::selection::Loaded>,
        frame_notify: Arc<tokio::sync::Notify>,
        vbuf: Arc<Mutex<Vec<u8>>>,
    ) -> anyhow::Result<Self> {
        // Wait for the lobby loop to drop the data-channel
        // receiver into the handoff slot (it does this on
        // cancel-exit, which take_pre_match has already
        // triggered). Polling is fine — the loop typically
        // returns within a few ms; cap at 5 s of safety.
        let receiver = drain_receiver(&pre_match.receiver_slot).await?;

        // Parse the peer's raw SRAM into a Save object. Needed
        // by the Shadow constructor (its primary trap needs
        // remote_save.as_raw_wram()).
        let remote_save = remote_game
            .gamedb_entry
            .parse_save(&pre_match.remote_save_data)
            .map_err(|e| anyhow::anyhow!("parse remote save: {e:?}"))?;
        // Local save is whatever we committed; same path.
        let local_save = local_game
            .gamedb_entry
            .parse_save(&pre_match.local_save_data)
            .map_err(|e| anyhow::anyhow!("parse local save: {e:?}"))?;

        let mut core = mgba::core::Core::new_gba(
            "tango",
            &mgba::core::Options {
                audio_sync: true,
                ..Default::default()
            },
        )?;
        core.enable_video_buffer();
        core.as_mut()
            .load_rom(mgba::vfile::VFile::from_vec(local_rom.as_ref().clone()))?;
        // PvP runs entirely off the in-memory SRAM dump from the
        // commitment — writes don't persist back to the user's
        // .sav file (matches legacy behavior; the only PvP-side
        // mutations are stat/zenny stuff which the user shouldn't
        // be carrying over from netplay anyway).
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(local_save.to_sram_dump()))?;

        let joyflags = Arc::new(AtomicU32::new(0));
        let local_hooks = local_game.hooks;
        local_hooks.patch(core.as_mut());

        let match_handle = tango_pvp::hooks::MatchHandle::new();
        let completion_token = tango_pvp::hooks::CompletionToken::new();

        // Hooks talk to the live Match via these traps —
        // common_traps + primary_traps wired with joyflags +
        // match_handle + completion_token. Each trap closure is
        // called from the mgba CPU thread, so we wrap it in a
        // tokio Handle::enter so the trap can spawn / await
        // (start_round / record_first_commit / end_round all
        // need an async runtime to do their work).
        let mut traps = local_hooks.common_traps();
        traps.extend(local_hooks.primary_traps(
            joyflags.clone(),
            match_handle.clone(),
            completion_token.clone(),
            disable_bgm,
        ));
        let rt_handle = tokio::runtime::Handle::current();
        core.set_traps(
            traps
                .into_iter()
                .map(|(addr, f)| {
                    let rt = rt_handle.clone();
                    (
                        addr,
                        Box::new(move |core: mgba::core::CoreMutRef<'_>| {
                            let _guard = rt.enter();
                            f(core)
                        }) as Box<dyn Fn(mgba::core::CoreMutRef<'_>)>,
                    )
                })
                .collect(),
        );

        let thread = mgba::thread::Thread::new(core);

        // RNG seeded from the XOR'd nonces. Match::new clones the
        // Mcg into its own state; we also need a clone for the
        // Shadow side so both sides have the same prefix.
        use rand::SeedableRng;
        let mut rng = rand_pcg::Mcg128Xsl64::from_seed(pre_match.rng_seed);
        let local_player_index = tango_pvp::battle::Match::pick_local_player_index(&mut rng, pre_match.is_offerer);

        // Replay writer. Failing to open it shouldn't kill the
        // match — log and continue without recording.
        let replay_writer = build_replay_writer(
            replays_path,
            &pre_match.link_code,
            &pre_match.local_settings,
            &pre_match.remote_settings,
            pre_match.match_type,
            pre_match.is_offerer,
            local_player_index,
            pre_match.rng_seed,
            local_save.as_ref(),
            remote_save.as_ref(),
        )
        .map_err(|e| {
            log::warn!("pvp: replay writer open failed: {e}");
            e
        })
        .ok();

        let remote_hooks = remote_game.hooks;
        let identity = tango_pvp::battle::MatchIdentity {
            match_type: pre_match.match_type,
            is_offerer: pre_match.is_offerer,
            local_player_index,
        };
        let shadow = tango_pvp::shadow::Shadow::new(
            remote_rom.as_ref(),
            remote_save.as_ref(),
            remote_hooks,
            pre_match.match_type,
            pre_match.is_offerer,
            local_player_index,
            rng.clone(),
        )?;

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        let latency_counter = Arc::new(tokio::sync::Mutex::new(Some(crate::net::LatencyCounter::new(5))));
        let end = EndState::default();
        // `frame_delay` (this side's frame delay) is realized entirely
        // locally by the display core trailing the netcode frontier; it's not
        // part of the deterministic simulation and never crosses the wire. Shared
        // as an atomic so the footer slider can live-adjust it mid-match.
        let frame_delay = Arc::new(AtomicU32::new(frame_delay));
        let inner_match = tango_pvp::battle::Match::new(
            local_rom.as_ref().clone(),
            local_hooks,
            thread.handle(),
            Box::new(crate::net::PvpSender::new(pre_match.sender.clone())),
            cancellation_token.clone(),
            rng,
            shadow,
            identity,
            tango_pvp::battle::ReplayConfig { writer: replay_writer },
            frame_delay.clone(),
            disable_bgm,
        );
        match_handle.set(inner_match.clone());

        // Spawn the network receive loop. Holds inner_match alive
        // until the receiver errors (peer disconnected) or the
        // cancellation_token fires. On exit, drop the handle out
        // of the shared slot so the session knows the match's gone.
        {
            let match_handle = match_handle.clone();
            let inner_match = inner_match.clone();
            let completion_token = completion_token.clone();
            let end = end.clone();
            let frame_notify_for_disc = frame_notify.clone();
            let latency_counter_for_disc = latency_counter.clone();
            let receiver = Box::new(crate::net::PvpReceiver::new(
                receiver,
                pre_match.sender.clone(),
                latency_counter.clone(),
                end.remote_ended.clone(),
                frame_notify.clone(),
            ));
            tokio::task::spawn(async move {
                tokio::select! {
                    r = inner_match.run(receiver) => {
                        // Network loop exited without our cancel
                        // firing → peer is gone (clean
                        // RTCPeerConnection close or transport
                        // error). Either way no more packets will
                        // arrive; raise the flag so `is_ended`
                        // can short-circuit past PEER_END_GRACE.
                        log::info!("pvp match thread ending: {:?}", r);
                        end.remote_disconnected.store(true, Ordering::Release);
                    }
                    _ = inner_match.cancelled() => {
                        // Local teardown — e.g. a comm error surfaced on the
                        // *send* side first (`add_local_input_and_fastforward`
                        // fails → the primary hook calls `Match::cancel`), or
                        // the user closed the session. `remote_disconnected`
                        // stays as-is; the cancellation token already gates
                        // `is_ended`.
                        log::info!("pvp match thread cancelled");
                    }
                }
                // However the match task ended, no more ping/pong will flow —
                // retire the latency tracker so `latency()` reads `None` and the
                // live telemetry panel retires, then wake the session so it
                // re-checks `is_ended` / re-renders without waiting on the next
                // vblank (which may never come — the emu thread can be paused).
                *latency_counter_for_disc.lock().await = None;
                frame_notify_for_disc.notify_one();
                // Only stamp END_OF_REPLAY when the in-game match
                // hooks fired `completion_token.complete()` — i.e.
                // the match was actually played to its end-game
                // screen. Disconnects, cancels, errors, and any
                // other premature exit drop the writer instead,
                // leaving the replay marked incomplete so the
                // export loop's "stop at pairs_left == 0" can
                // catch it cleanly.
                if completion_token.is_complete() {
                    if let Err(e) = inner_match.finish_replay() {
                        log::error!("finish replay failed: {e}");
                    }
                }
                match_handle.clear();
            });
        }

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        // ~1 s window at 60 Hz, matching the legacy emu_tps_counter.
        let tps_counter = Arc::new(std::sync::Mutex::new(crate::stats::Counter::new(60)));
        vbuf.lock().unwrap().fill(0);

        // Single-core PvP: the live mgba thread is the only core — it runs the
        // netcode and renders straight to the UI. The `Round` loads the FF's
        // computed `present_state` into it each frame, so the live core's game
        // tick lags `current_tick` by `frame_delay`.
        let audio_stream: Box<dyn crate::audio::Stream + Send> = Box::new(crate::audio::MGBAStream::new(
            thread.handle(),
            audio_binder.sample_rate(),
        ));
        let audio_binding = match audio_binder.bind(Some(audio_stream)) {
            Ok(b) => Some(b),
            Err(e) => {
                log::warn!("pvp: audio bind failed: {e:?}");
                None
            }
        };

        // Completion / EndOfMatch handling, shared by both core layouts. Runs
        // on the live core's frame_callback: returns whether the match has
        // completed (caller pauses the live emulator if so) and fires the
        // EndOfMatch packet + grace-window wake exactly once on the edge.
        let handle_completion = {
            let completion_token = completion_token.clone();
            let frame_notify = frame_notify.clone();
            let end = end.clone();
            let sender_for_eom = pre_match.sender.clone();
            let rt_handle = tokio::runtime::Handle::current();
            move || -> bool {
                if !completion_token.is_complete() {
                    return false;
                }
                // First frame on which local completion is visible: stamp the
                // grace deadline and fire our EndOfMatch + fallback wake exactly
                // once. The `None → Some` transition under the lock is the guard
                // (the frame_callback is the only writer, so check-then-set is
                // sound).
                let first_completion = {
                    let mut completed_at = end.local_ended_at.lock().unwrap();
                    if completed_at.is_some() {
                        false
                    } else {
                        *completed_at = Some(std::time::Instant::now());
                        true
                    }
                };
                if first_completion {
                    let sender = sender_for_eom.clone();
                    rt_handle.spawn(async move {
                        if let Err(e) = sender.lock().await.send_end_of_match().await {
                            log::warn!("pvp: send EndOfMatch failed: {e}");
                        }
                    });
                    // Wall-clock fallback wake so `is_ended` is rechecked even
                    // if the peer never sends EndOfMatch / the channel errors.
                    let notify = frame_notify.clone();
                    rt_handle.spawn(async move {
                        tokio::time::sleep(PEER_END_GRACE).await;
                        notify.notify_one();
                    });
                }
                true
            }
        };

        // Single core: feeds local input from the user's atomic, marks TPS,
        // pushes its rendered frame straight to the UI, drives completion. The
        // display now follows the network frontier (no frame_delay
        // mitigation yet — that comes back in Stage 1c when the Round loads the
        // FF's at-present_tick state instead of the at-frontier one).
        thread.set_frame_callback({
            let joyflags = joyflags.clone();
            let tps_counter = tps_counter.clone();
            let handle_completion = handle_completion.clone();
            let vbuf = vbuf.clone();
            let frame_notify = frame_notify.clone();
            move |mut core, video_buffer, mut thread_handle| {
                core.set_keys(joyflags.load(Ordering::Relaxed));
                tps_counter.lock().unwrap().mark();
                {
                    // Copy mgba's native BGR555 straight through; the
                    // framebuffer shader expands it to RGB on the GPU.
                    vbuf.lock().unwrap().copy_from_slice(video_buffer);
                }
                frame_notify.notify_one();
                if handle_completion() {
                    thread_handle.pause();
                }
            }
        });

        Ok(Self {
            local_game,
            local_player_index,
            joyflags,
            completion_token,
            end,
            tps_counter,
            _audio_binding: audio_binding,
            _thread: thread,
            cancellation_token,
            latency_counter,
            _peer_conn: pre_match.peer_conn,
            match_handle,
            link_code: pre_match.link_code,
            remote_nickname: pre_match.remote_settings.nickname,
            opponent_loaded,
            opponent_save_view: crate::save_view::State::new(),
            local_loaded,
            local_save_view: crate::save_view::State::new(),
            frame_delay,
        })
    }

    pub fn game(&self) -> &'static crate::game::Game {
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

    /// Live-set the local frame delay. Purely local: takes effect
    /// on the next rendered frame, no peer coordination. Clamped to the supported
    /// range as a guard against an out-of-range caller.
    pub fn set_frame_delay(&self, frame_delay: u32) {
        self.frame_delay.store(
            frame_delay.clamp(tango_pvp::battle::MIN_FRAME_DELAY, tango_pvp::battle::MAX_FRAME_DELAY),
            Ordering::Relaxed,
        );
    }

    /// Overwrite the joyflag bitmap (same shape as singleplayer's
    /// — see [`crate::singleplayer_session::SinglePlayerSession::set_joyflags`]).
    pub fn set_joyflags(&self, mgba_keys: u32) {
        self.joyflags.store(mgba_keys, Ordering::Relaxed);
    }

    pub fn request_close(&self) {
        // Cancelling the token is the whole close signal: it unblocks the
        // match-run task's `cancelled()` arm and flips `is_ended`'s
        // cancellation check. (The grace gate there still holds, so the
        // match-end screen plays out before the view tears down.)
        self.cancellation_token.cancel();
    }

    /// True once it's safe to tear the session down. Requires
    /// local completion (per-game `match_end_ret` hook fired)
    /// PLUS one of:
    ///   * the peer also sent us `EndOfMatch`, or
    ///   * `PEER_END_GRACE` has elapsed since local completion
    ///     (peer crashed / disconnected — give up waiting).
    /// The handshake keeps the data channel alive long enough
    /// for the lagging side to also reach its hook and write
    /// `END_OF_REPLAY` before we drop `_peer_conn`. Without it,
    /// whichever side finishes first kills the connection out
    /// from under the other and the other side's replay ends up
    /// truncated.
    pub fn is_ended(&self) -> bool {
        if !self.completion_token.is_complete() {
            return false;
        }
        if self.end.remote_ended.load(Ordering::Acquire) {
            return true;
        }
        // Remote's data channel closed (RTCPeerConnection drop or
        // SCTP-level disconnect). No EndOfMatch is ever coming
        // so skip straight to teardown without burning the
        // grace window.
        if self.end.remote_disconnected.load(Ordering::Acquire) {
            return true;
        }
        // We tore our own netcode down (local input-buffer overflow cancels the
        // match via the primary `main_read_joyflags` hook). Same rationale as
        // `remote_disconnected`, from our side: no useful EndOfMatch is coming, so
        // skip the grace window. The completion gate above still holds, so the
        // comm-error / match-end screen runs to completion as normal first.
        if self.cancellation_token.is_cancelled() {
            return true;
        }
        match *self.end.local_ended_at.lock().unwrap() {
            Some(t) => t.elapsed() >= PEER_END_GRACE,
            // Completion-token can flip before the frame_callback
            // observes it and stamps the deadline. Hold off
            // teardown for one extra tick rather than firing the
            // grace timer from t=0.
            None => false,
        }
    }

    /// Median ping over the last few seconds — drives the frame-delay
    /// suggestion, where smoothing out a transient spike is what we want.
    /// `Some(ZERO)` until the first Pong arrives, then `Some(median)`
    /// while the link is up; `None` once the remote drops (the match-run task
    /// clears the counter). The UI keys the instrument panel off this: `None`
    /// means "no live link", which `remote_disconnected` (still used internally
    /// by [`is_ended`](Self::is_ended)) can't distinguish from a legitimate 0 ms
    /// LAN ping that sticks at its last reading after a drop.
    pub fn latency(&self) -> Option<std::time::Duration> {
        self.latency_counter.blocking_lock().as_ref().map(|c| c.median())
    }

    /// Raw latest ping — the most recent single measurement, unsmoothed.
    /// Drives the live telemetry plate + sparkline, where the median's lag
    /// would mask a real spike. Same `Some`/`None` link-up semantics as
    /// [`latency`](Self::latency) (both read the same counter), so it gates
    /// the instrument panel identically; only the reported value differs.
    pub fn latency_raw(&self) -> Option<std::time::Duration> {
        self.latency_counter
            .blocking_lock()
            .as_ref()
            .map(|c| c.latest().unwrap_or(std::time::Duration::ZERO))
    }

    /// Smoothed emulator ticks-per-second from the per-frame
    /// callback's interval samples. Independent of UI refresh
    /// rate. ZERO until the second sample lands.
    pub fn tps(&self) -> f32 {
        let mean = self.tps_counter.lock().unwrap().mean_duration();
        if mean.is_zero() {
            0.0
        } else {
            1.0 / mean.as_secs_f32()
        }
    }

    /// What the throttler is currently asking mgba to run at. Pairs with
    /// `tps()` — gap between the two tells you whether the throttler is
    /// the cause of a slow tps or just observing one.
    pub fn fps_target(&self) -> f32 {
        self._thread.handle().lock_audio().sync().fps_target()
    }

    /// Snapshot of the current round's metrics for the status bar
    /// (P1/P2, frame advantage). `None` between rounds or before
    /// the first round starts.
    pub fn round_stats(&self) -> Option<RoundStats> {
        let metrics = self.match_handle.round_metrics()?;
        Some(RoundStats {
            skew: metrics.local_frame_advantage as i32 - metrics.remote_frame_advantage as i32,
            depth: metrics.misprediction_depth,
        })
    }
}

/// Subset of `tango_pvp::battle::Round` metrics surfaced in the
/// status bar. Per-round and `None` between rounds; the match-level
/// player index lives on [`PvpSession::local_player_index`] instead.
#[derive(Clone, Copy, Debug)]
pub struct RoundStats {
    /// Real-time clock skew the throttler reacts to: `local_advantage −
    /// remote_advantage` (see `Round::update_fps_target`). The symmetric
    /// network term cancels in the difference, so this reads ~0 at clock
    /// sync, positive when we're leading (and being slowed), and negative
    /// when the peer is leading.
    pub skew: i32,
    /// Misprediction depth: how many speculative frames this frame discarded and
    /// re-simulated because a confirmed remote input contradicted the prediction.
    /// 0 on a clean frame; spikes mark the size of each rollback.
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
        // called, cancelling the token signals the
        // network/match tasks to wind down before the mgba
        // thread joins.
        self.cancellation_token.cancel();
    }
}

/// Poll the receiver-handoff slot until the lobby loop drops
/// the live Receiver into it. Bounded to avoid hanging the
/// PvP setup forever if something went off the rails.
async fn drain_receiver(
    slot: &Arc<std::sync::Mutex<Option<crate::net::Receiver>>>,
) -> anyhow::Result<crate::net::Receiver> {
    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(5);
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        if let Some(r) = slot.lock().unwrap().take() {
            return Ok(r);
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for lobby loop to release receiver");
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Open the replay file + write its metadata frame. Filename
/// format mirrors the legacy app:
/// `YYYYMMDDhhmmss-<link_code>-<compat>-vs-<opponent>-p<idx>.tangoreplay`.
#[allow(clippy::too_many_arguments)]
fn build_replay_writer(
    replays_path: &Path,
    link_code: &str,
    local_settings: &crate::net::protocol::Settings,
    remote_settings: &crate::net::protocol::Settings,
    match_type: (u8, u8),
    is_offerer: bool,
    local_player_index: u8,
    rng_seed: [u8; 16],
    local_save: &(dyn tango_dataview::save::Save + Send + Sync),
    remote_save: &(dyn tango_dataview::save::Save + Send + Sync),
) -> anyhow::Result<tango_pvp::replay::Writer> {
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
    // Direct-TCP sessions have no link code in their metadata —
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
    Ok(tango_pvp::replay::Writer::new(
        // Buffered: write_input runs on the emulator thread once per
        // confirmed frame, and unbuffered it costs a few small write
        // syscalls each time. The format already recovers truncated tails,
        // so a hard crash losing the buffered tail of an (already
        // incomplete) replay changes nothing; finish() flushes.
        std::io::BufWriter::new(file),
        tango_pvp::replay::Metadata {
            ts: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            link_code: link_code.to_string(),
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
                reveal_setup: local_settings.reveal_setup,
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
                reveal_setup: remote_settings.reveal_setup,
            }),
            match_type: match_type.0 as u32,
            match_subtype: match_type.1 as u32,
        },
        is_offerer,
        local_player_index,
        rng_seed,
        &local_sram,
        &remote_sram,
    )?)
}

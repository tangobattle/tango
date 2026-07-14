//! Live PvP emulator session — peer-paired netplay sibling of
//! [`crate::session::singleplayer::SinglePlayerSession`]. Owns the
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

/// Retransmit-heartbeat cadence for the in-match channel — one emulator frame
/// at [`EXPECTED_FPS`]. Keeps the unacked redundancy window flowing while the
/// local sim is throttled or stalled, so loss recovery isn't coupled to the
/// frame rate (see [`crate::net::InMatchTx`]).
const IN_MATCH_HEARTBEAT: std::time::Duration =
    std::time::Duration::from_nanos((1_000_000_000.0 / EXPECTED_FPS as f64) as u64);

/// Session-redraw cadence while reconnecting (~30 fps), so the give-up progress
/// bar drains smoothly even though the paused emulator emits no frames. Purely
/// cosmetic.
const RECONNECT_UI_TICK: std::time::Duration = std::time::Duration::from_millis(33);

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
    /// Remote's in-game match-end handshake (the in-band `data::wire`
    /// `EndOfMatch` marker) arrived — raised by the net receive task
    /// ([`crate::net::PvpReceiver`]).
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
    local_game: &'static crate::library::game::Game,
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
    tps_counter: Arc<std::sync::Mutex<crate::session::stats::Counter>>,
    _audio_binding: Option<crate::platform::audio::Binding>,
    _thread: mgba::thread::Thread,
    /// Drops fire-cancellation through the match background tasks
    /// (`Match::run`, `Match::cancel`). On Close we cancel + drop
    /// the session, which tears the network loop down cleanly.
    cancellation_token: tokio_util::sync::CancellationToken,
    /// The peer link: owns the peer connection, both channels' halves, the
    /// latency readout, and the transparent mid-match reconnect (see
    /// [`crate::net::link`]). The supervisor task holds its own `Arc`; the
    /// session's keeps the transport alive for the match's lifetime, and its
    /// eventual drop closes the connection gracefully (DTLS close_notify → the
    /// peer's prompt EOF).
    link: Arc<crate::net::link::Link>,
    /// Kept alive so the background `match_.run(receiver)` task
    /// has a referent. Cleared by that task when it exits. The UI
    /// also reads this each tick to scrape the current round's
    /// player-index / queue-lengths for the status bar.
    match_handle: tango_pvp::hooks::MatchHandle,
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
    /// Incremental local-perspective match stats, fed by the match as
    /// rounds close. Our own `Arc` (the match holds a clone), so the
    /// post-match results snapshot and the sidecar write can read it
    /// during teardown regardless of how far the match's background tasks
    /// have already wound down.
    stats: Arc<Mutex<tango_pvp::analysis::MatchStatsBuilder>>,
    /// Where this match's replay is being recorded, or `None` if the writer
    /// failed to open. The post-match results screen offers to play it back.
    pub replay_path: Option<std::path::PathBuf>,
    /// When the session was built, for the results screen's match duration.
    started_at: std::time::Instant,
}

/// Everything [`PvpSession::new`] needs, as named fields (the positional
/// form had grown past a dozen arguments). Assembled by
/// [`spawn_pvp`](crate::session::spawn_pvp).
pub struct PvpSessionArgs<'a> {
    /// Local/remote game impls; the roms must already have any patch applied.
    pub local_game: &'static crate::library::game::Game,
    pub local_rom: Arc<Vec<u8>>,
    pub remote_game: &'static crate::library::game::Game,
    pub remote_rom: Arc<Vec<u8>>,
    /// The netplay handoff: negotiated terms + the transport bundle.
    pub pre_match: crate::netplay::PreMatchData,
    /// This side's frame delay — realized purely as local display lag (how
    /// far the display core trails the netcode frontier). Comes straight from
    /// local config; never negotiated with or sent to the peer.
    pub frame_delay: u32,
    /// Whether to skip the game's battle BGM for this match. Local-only
    /// like the volume — the peer is unaffected, and the recorded replay
    /// keeps its music (export has its own mute toggle). Sampled from
    /// config at match start.
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
    /// Async because the lobby loop holds the data-channel
    /// `Receiver` until it observes its cancellation and exits;
    /// `Link::bring_up` awaits its handback (worst case a few ms
    /// after `take_pre_match` flips the cancel flag).
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
        // Created up front: the link keys the in-match heartbeat's lifetime to
        // this token so it survives transport errors during a reconnect
        // (ending only at teardown). Shared with the Match and the supervisor.
        let cancellation_token = tokio_util::sync::CancellationToken::new();

        // Parse the peer's raw SRAM into a Save object. Needed
        // by the Shadow constructor (its primary trap needs
        // remote_save.as_raw_wram()).
        let remote_save = remote_game
            .parse_save(&pre_match.remote_save_data)
            .map_err(|e| anyhow::anyhow!("parse remote save: {e:?}"))?;
        // Local save is whatever we committed; same path.
        let local_save = local_game
            .parse_save(&pre_match.local_save_data)
            .map_err(|e| anyhow::anyhow!("parse local save: {e:?}"))?;

        let mut core = crate::session::new_gba_core(local_rom.as_ref())?;
        // PvP runs entirely off the in-memory SRAM dump from the
        // commitment — writes don't persist back to the user's
        // .sav file (matches legacy behavior; the only PvP-side
        // mutations are stat/zenny stuff which the user shouldn't
        // be carrying over from netplay anyway).
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(local_save.to_sram_dump()))?;
        // Pin the cart RTC to the negotiated match clock. Both peers pin all
        // their cores (primary here; shadow + re-sim stepper below via
        // Shadow::new / MatchIdentity) to this same instant, so RTC-reading
        // games (exe45) can't desync on it; the replay metadata records it
        // (build_replay_writer) so playback pins to the identical value.
        let rtc_time = std::time::UNIX_EPOCH + std::time::Duration::from_millis(pre_match.match_ts);
        core.set_rtc_fixed(rtc_time);

        let joyflags = Arc::new(AtomicU32::new(0));
        let local_hooks = local_game.hooks;
        // Usage semantics can depend on the applied patch (exe45's PvP
        // patch), so they're probed off the patched ROM.
        let chip_semantics = local_hooks.chip_semantics(local_rom.as_ref());
        let counts_buster = local_hooks.counts_buster(local_rom.as_ref());

        let match_handle = tango_pvp::hooks::MatchHandle::new();
        let completion_token = tango_pvp::hooks::CompletionToken::new();

        // Install the live-primary trap set (common + primary), wired to the
        // running Match through the shared joyflags / handle / completion token.
        local_hooks.install_on_primary(
            &mut core,
            tango_pvp::hooks::PrimaryState {
                joyflags: joyflags.clone(),
                match_: match_handle.clone(),
                completion_token: completion_token.clone(),
                disable_bgm,
            },
        );

        let thread = mgba::thread::Thread::new(core);

        // RNG seeded from the XOR'd nonces. Match::new clones the
        // Mcg into its own state; we also need a clone for the
        // Shadow side so both sides have the same prefix.
        use rand::SeedableRng;
        let mut rng = rand_pcg::Mcg128Xsl64::from_seed(pre_match.rng_seed);
        let local_player_index = tango_pvp::battle::Match::pick_local_player_index(&mut rng, pre_match.is_offerer);
        let identity = tango_pvp::battle::MatchIdentity {
            match_type: pre_match.match_type,
            is_offerer: pre_match.is_offerer,
            local_player_index,
            rtc_time,
        };

        // Replay writer. Failing to open it shouldn't kill the
        // match — log and continue without recording.
        let (replay_writer, replay_path) = match build_replay_writer(
            replays_path,
            &pre_match,
            &identity,
            local_save.as_ref(),
            remote_save.as_ref(),
        ) {
            Ok((writer, path)) => (Some(writer), Some(path)),
            Err(e) => {
                log::warn!("pvp: replay writer open failed: {e}");
                (None, None)
            }
        };

        let remote_hooks = remote_game.hooks;
        let shadow = tango_pvp::shadow::Shadow::new(
            remote_rom.as_ref(),
            remote_save.as_ref(),
            remote_hooks,
            identity,
            rng.clone(),
        )?;

        // Assemble the peer link from the lobby handoff — this awaits the
        // lobby loop releasing the reliable receiver (typically a few ms after
        // take_pre_match flipped the cancel) and starts the in-match
        // retransmit heartbeat. Consumes `link_parts` off `pre_match`, so it
        // runs after everything that borrows the handoff whole (the replay
        // writer's metadata).
        let link = Arc::new(
            crate::net::link::Link::bring_up(pre_match.link_parts, IN_MATCH_HEARTBEAT, cancellation_token.clone())
                .await?,
        );
        let in_match = link.in_match().clone();

        let end = EndState::default();
        // `frame_delay` (this side's frame delay) is realized entirely
        // locally by the display core trailing the netcode frontier; it's not
        // part of the deterministic simulation and never crosses the wire. Shared
        // as an atomic so the footer slider can live-adjust it mid-match.
        let frame_delay = Arc::new(AtomicU32::new(frame_delay));
        // Incremental match stats, shared with the match (which folds each
        // round in as it closes). We keep our own handle so the post-match
        // results snapshot survives the match teardown.
        let stats = Arc::new(Mutex::new(tango_pvp::analysis::MatchStatsBuilder::new(
            chip_semantics,
            counts_buster,
        )));
        let inner_match = tango_pvp::battle::Match::new(tango_pvp::battle::MatchConfig {
            rom: local_rom.as_ref().clone(),
            local_hooks,
            primary_thread_handle: thread.handle(),
            sender: Box::new(crate::net::PvpSender::new(in_match.clone())),
            cancellation_token: cancellation_token.clone(),
            rng,
            shadow,
            identity,
            replay: tango_pvp::battle::ReplayConfig { writer: replay_writer },
            frame_delay: frame_delay.clone(),
            disable_bgm,
            stats: stats.clone(),
        });
        match_handle.set(inner_match.clone());

        // Match receive loop + link supervisor. Holds inner_match alive until
        // the match ends (completion / cancel) or, when the link drops, until
        // reconnection gives up. Policy lives here — deciding when a trip is
        // worth reconnecting (that needs match-level knowledge: completion,
        // the peer's EndOfMatch) and freezing/unfreezing the emulator around
        // the attempt. The transport surgery (silent teardown, rebuild,
        // hot-swap under the persistent rennet streams) is [`Link::reconnect`]'s;
        // the lockstep sim treats the whole gap as a pause, so no state resync
        // is needed. A link without a rebuild recipe ends the match on the
        // first drop exactly as before.
        {
            let inner_match = inner_match.clone();
            let match_handle = match_handle.clone();
            let completion_token = completion_token.clone();
            let end = end.clone();
            let frame_notify = frame_notify.clone();
            let in_match = in_match.clone();
            let link = link.clone();
            let handle = thread.handle();
            let stats = stats.clone();
            let stats_path = replay_path
                .as_ref()
                .map(|p| crate::library::replays::stats_path(cache_path, replays_path, p));
            let cancel = cancellation_token.clone();
            let mut receiver: Box<dyn tango_pvp::net::Receiver + Send + Sync> = Box::new(crate::net::PvpReceiver::new(
                link.take_match_receiver()
                    .expect("bring_up parks the in-match receiver"),
                in_match.clone(),
                link.latency_handle(),
                end.remote_ended.clone(),
                frame_notify.clone(),
            ));
            tokio::task::spawn(async move {
                // Why the receive loop ended this iteration.
                enum Trip {
                    /// Clean local teardown (user closed / cancelled). Never reconnects.
                    Cancelled,
                    /// A channel hit EOF — the peer *told* us something: a deliberate
                    /// quit (its peer connection's graceful drop sends DTLS
                    /// close_notify), the peer's reconnect giving up, or its
                    /// transport declaring the link dead. All of those mean the
                    /// match is over; a mere link outage never produces an EOF (our
                    /// own reconnect teardown is silent — `Link::reconnect` abandons
                    /// rather than closes — and a dead network delivers nothing).
                    Closed,
                    /// The local input queue climbed to `RECONNECT_QUEUE_LENGTH`:
                    /// the peer stopped matching our inputs, i.e. a quiet/dead link.
                    /// The one trip that reconnects.
                    Stalled,
                }
                loop {
                    // `run` takes `receiver` by move, which is fine — every
                    // looping path rebuilds it.
                    let trip = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => Trip::Cancelled,
                        _ = link.watch_control_eof() => Trip::Closed,
                        r = inner_match.run(receiver) => {
                            log::info!("pvp in-match channel closed: {r:?}");
                            Trip::Closed
                        }
                        _ = inner_match.stalled() => Trip::Stalled,
                    };

                    // Our own deliberate close: just stop. The session's teardown
                    // drops the peer connection gracefully, and its DTLS
                    // close_notify hands the peer a prompt EOF (`Trip::Closed`
                    // over there — it ends instead of reconnecting).
                    if matches!(trip, Trip::Cancelled) {
                        break;
                    }

                    // Reconnect only on the stall watchdog — the one signal that
                    // means "the link went quiet under a live match" — and only if
                    // the transport can rebuild and the match isn't ending (our
                    // completion or the peer's EndOfMatch).
                    let reconnectable = matches!(trip, Trip::Stalled)
                        && link.can_reconnect()
                        && !completion_token.is_complete()
                        && !end.remote_ended.load(Ordering::Acquire);
                    if !reconnectable {
                        end.remote_disconnected.store(true, Ordering::Release);
                        cancel.cancel();
                        break;
                    }

                    // Freeze the sim so its speculative lead can't run past the
                    // rollback horizon (and overflow-bail) while the link is down.
                    // Both peers converge on the rebuild: whoever trips first goes
                    // silent, which stall-trips the other within
                    // `RECONNECT_QUEUE_LENGTH` frames.
                    handle.pause();
                    frame_notify.notify_one();
                    log::info!("pvp link dropped — pausing to reconnect");

                    // Rebuild + hot-swap (the link's job), ticking the UI at
                    // ~30 fps so the give-up bar drains smoothly while the paused
                    // emulator produces no frames. The ticker never completes; it
                    // just keeps redraws coming while `reconnect` runs.
                    let restored = {
                        let ui_tick = async {
                            let mut iv = tokio::time::interval(RECONNECT_UI_TICK);
                            loop {
                                iv.tick().await;
                                frame_notify.notify_one();
                            }
                        };
                        tokio::select! {
                            restored = link.reconnect() => restored,
                            _ = ui_tick => unreachable!(),
                        }
                    };

                    if !restored {
                        // Timed out or cancelled — give up and end the match.
                        end.remote_disconnected.store(true, Ordering::Release);
                        cancel.cancel();
                        handle.unpause();
                        break;
                    }

                    // Fresh receiver over the swapped channel, same `in_match` —
                    // the rennet in-stream (seq/ack) carries across the swap, so
                    // the peer's resent window fills our gap contiguously.
                    receiver = Box::new(crate::net::PvpReceiver::new(
                        link.take_match_receiver().expect("reconnect parks a fresh receiver"),
                        in_match.clone(),
                        link.latency_handle(),
                        end.remote_ended.clone(),
                        frame_notify.clone(),
                    ));
                    // The queue watchdog re-arms itself on the next loop turn (it
                    // builds fresh each `select!`): it waits for the rebuilt link's
                    // resent inputs to drain the standing queue back below the
                    // threshold before it can trip again, so it re-trips and
                    // reconnects once more (self-healing) if the link falls silent
                    // anew, without instantly re-firing on the not-yet-drained queue.
                    handle.unpause();
                    frame_notify.notify_one();
                    log::info!("pvp transparently reconnected the link");
                }

                // Teardown: retire latency so `latency()` reads `None` and the
                // telemetry panel retires, wake the session to re-check
                // `is_ended` (the emu thread may be paused, so no vblank is
                // coming), finalize the replay iff the match reached its end
                // screen, and release the match handle.
                link.retire_latency();
                frame_notify.notify_one();
                if completion_token.is_complete() {
                    if let Err(e) = inner_match.finish_replay() {
                        log::error!("finish replay failed: {e}");
                    }
                    // Cache the finished match's stats — the match already
                    // folded each round as it ended, so the Replays tab
                    // never has to re-simulate this one.
                    if let Some(stats_path) = stats_path.as_ref() {
                        let snapshot = stats.lock().unwrap().snapshot();
                        if let Err(e) = crate::library::replays::write_match_stats(stats_path, &snapshot) {
                            log::warn!("failed to write replay stats cache entry: {e}");
                        }
                    }
                }
                match_handle.clear();
            });
        }

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        // ~1 s window at 60 Hz, matching the legacy emu_tps_counter.
        let tps_counter = Arc::new(std::sync::Mutex::new(crate::session::stats::Counter::new(60)));
        vbuf.lock().unwrap().fill(0);

        // Single-core PvP: the live mgba thread is the only core — it runs the
        // netcode and renders straight to the UI. The `Round` loads the FF's
        // computed `present_state` into it each frame, so the live core's game
        // tick lags `current_tick` by `frame_delay`.
        let audio_binding = audio_binder.bind_mgba(thread.handle(), "pvp");

        // Completion / EndOfMatch handling, shared by both core layouts. Runs
        // on the live core's frame_callback: returns whether the match has
        // completed (caller pauses the live emulator if so) and fires the
        // EndOfMatch packet + grace-window wake exactly once on the edge.
        let handle_completion = {
            let completion_token = completion_token.clone();
            let frame_notify = frame_notify.clone();
            let end = end.clone();
            let in_match_for_eom = in_match.clone();
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
                    // In-band EndOfMatch: rides the same ordered seq stream as
                    // inputs over the unreliable channel, so the peer sees it
                    // exactly once and only after every preceding input.
                    let in_match = in_match_for_eom.clone();
                    rt_handle.spawn(async move {
                        if let Err(e) = in_match.send_end_of_match().await {
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
        // pushes its rendered frame straight to the UI, drives completion.
        // The display trails the network frontier by `frame_delay`: the Round
        // loads the engine's at-present_tick state into this core each frame.
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
            link,
            match_handle,
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
    /// — see [`crate::session::singleplayer::SinglePlayerSession::set_joyflags`]).
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

    /// `true` while the link has dropped and the session is transparently
    /// rebuilding it (direct or matchmaking) — the emulator is paused and the
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
    /// local completion (per-game `match_end_ret` hook fired)
    /// PLUS one of:
    ///   * the peer also sent us `EndOfMatch`, or
    ///   * `PEER_END_GRACE` has elapsed since local completion
    ///     (peer crashed / disconnected — give up waiting).
    ///
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
    /// `Some(ZERO)` until the first sample arrives, then `Some(median)`
    /// while the link is up; `None` once the remote drops (the supervisor
    /// retires the link's counter). The UI keys the instrument panel off this:
    /// `None` means "no live link", which `remote_disconnected` (still used
    /// internally by [`is_ended`](Self::is_ended)) can't distinguish from a
    /// legitimate 0 ms LAN ping that sticks at its last reading after a drop.
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

    /// The match stats aggregated so far, in play order. Read at teardown
    /// for the post-match results screen; rounds the match never finished
    /// (mid-round disconnect) simply aren't in it.
    pub fn stats_snapshot(&self) -> tango_pvp::analysis::MatchStats {
        self.stats.lock().unwrap().snapshot()
    }

    /// How long the match ran, start of session to local completion — or to
    /// now, if completion hasn't been observed yet (it is stamped a frame
    /// after the completion token flips). For the results screen.
    pub fn match_duration(&self) -> std::time::Duration {
        match *self.end.local_ended_at.lock().unwrap() {
            Some(ended_at) => ended_at.duration_since(self.started_at),
            None => self.started_at.elapsed(),
        }
    }

    /// Snapshot of the current round's metrics for the status bar
    /// (P1/P2, frame advantage). `None` between rounds or before
    /// the first round starts.
    pub fn round_stats(&self) -> Option<RoundStats> {
        let metrics = self.match_handle.round_metrics()?;
        Some(RoundStats {
            skew: metrics.local_tick_advantage as i32 - metrics.remote_tick_advantage as i32,
            lead: metrics.local_tick_advantage as i32,
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
    /// remote_advantage` (see `tango_pvp::battle::Throttler`). The symmetric
    /// network term cancels in the difference, so this reads ~0 at clock
    /// sync, positive when we're leading (and being slowed), and negative
    /// when the peer is leading.
    pub skew: i32,
    /// Local tick lead: how far the local frontier runs ahead of the confirmed
    /// remote input (the raw `local_tick_advantage`, one side of the skew pair).
    /// Steady around `present_delay` at clock sync; ramps up when the remote falls
    /// behind or a delivery stall holds its confirmed frontier still.
    pub lead: i32,
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

/// Open the replay file + write its metadata frame, returning the writer
/// along with the path it records to (surfaced on the session so the
/// post-match results screen can offer playback). Everything the metadata
/// needs lives on `pre_match` (settings, seed, match clock, link code) and
/// `identity` (roles + player index). Filename format mirrors the legacy
/// app: `YYYYMMDDhhmmss-<link_code>-<compat>-vs-<opponent>-p<idx>.tangoreplay`.
fn build_replay_writer(
    replays_path: &Path,
    pre_match: &crate::netplay::PreMatchData,
    identity: &tango_pvp::battle::MatchIdentity,
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
    // Direct-TCP sessions have no link code in their metadata —
    // substitute a stable placeholder here so the filename
    // doesn't end up with a double-dash where the slot would be.
    let filename_link_code = if link_code.is_empty() { "direct" } else { link_code };
    let raw_name = format!(
        "{ts}-{filename_link_code}-{netplay_compat}-vs-{}-p{}",
        remote_settings.nickname,
        identity.local_player_index + 1
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
        // Buffered: write_input runs on the emulator thread once per
        // confirmed frame, and unbuffered it costs a few small write
        // syscalls each time. The format already recovers truncated tails,
        // so a hard crash losing the buffered tail of an (already
        // incomplete) replay changes nothing; finish() flushes.
        std::io::BufWriter::new(file),
        tango_pvp::replay::Metadata {
            // The negotiated match clock, not the local wall clock: every live
            // core's cart RTC is pinned to this instant, and playback pins to
            // `metadata.ts` (see `Replay::rtc_time`), so recording the same
            // value is what makes exe45 replays reproduce the live match. Both
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
            match_type: identity.match_type.0 as u32,
            match_subtype: identity.match_type.1 as u32,
        },
        identity.is_offerer,
        identity.local_player_index,
        pre_match.rng_seed,
        &local_sram,
        &remote_sram,
    )?;
    Ok((writer, replay_filename))
}

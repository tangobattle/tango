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

/// How long the coordinator keeps trying to rebuild a dropped direct link
/// before giving up and ending the match. Generous: the sim is paused
/// throughout, so a long outage costs nothing but the wait.
const RECONNECT_DIRECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Per-attempt cap on a single `host`/`connect` + `negotiate` rebuild — the
/// dialer's `connect` will hang on ICE until the host is listening again, so
/// bound it and retry rather than blocking the whole budget on one attempt.
const RECONNECT_DIRECT_ATTEMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
/// Give-up window for the matchmaking path — longer than direct's, since each
/// attempt re-rendezvouses on the signaling server then re-gathers ICE (and
/// possibly TURN), which is much slower than re-binding a known local port.
const RECONNECT_MATCHMAKING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// Per-attempt cap for a matchmaking rebuild (signaling rendezvous + ICE/TURN
/// gathering + negotiate).
const RECONNECT_MATCHMAKING_ATTEMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// Pause between failed rebuild attempts (e.g. dialer racing ahead of the host
/// re-binding its port).
const RECONNECT_BACKOFF: std::time::Duration = std::time::Duration::from_millis(250);
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
    /// A std Mutex — every guard scope is a plain read or swap, never held
    /// across an await, and the UI reads it from the render thread.
    latency_counter: Arc<std::sync::Mutex<Option<crate::net::LatencyCounter>>>,
    /// The peer connection, kept alive so it outlives the data channels. Held
    /// behind a shared slot because the reconnect coordinator swaps it (drops
    /// the old, installs the rebuilt) on a transparent direct-link reconnect;
    /// the session just keeps the slot alive for the match's lifetime.
    _peer_conn: Arc<Mutex<Option<tango_rtc::PeerConnection>>>,
    /// `(started_at, give_up_at)` for the in-progress reconnect, or `None` when
    /// not reconnecting (the steady state). Drives the "Reconnecting…" overlay
    /// and its depleting give-up bar; the pair (rather than just the deadline)
    /// lets the bar's fraction work across the direct/matchmaking window sizes.
    /// The emulator is paused while this is `Some`.
    reconnect_window: Arc<Mutex<Option<(std::time::Instant, std::time::Instant)>>>,
    /// Reliable lobby channel's sender, parked for the match's lifetime. Idle
    /// in-match (all traffic is on the unreliable channel), but held open so
    /// its close doesn't surface as a spurious disconnect on the peer's
    /// reliable-channel watch.
    _lobby_sender: Arc<tokio::sync::Mutex<crate::net::Sender>>,
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
        // The unreliable in-match channel (parked at negotiate time) carries
        // the live match's `data::wire` datagrams. The reliable channel's
        // receiver arrives via the lobby-loop cancel-exit slot; the session
        // only watches it for the disconnect signal (a datagram channel has no
        // clean close event). Polling the slots is fine — the lobby loop
        // typically returns within a few ms; cap at 5 s of safety.
        let in_match_receiver = drain_receiver(&pre_match.in_match_receiver_slot).await?;
        let reliable_receiver = drain_receiver(&pre_match.reliable_receiver_slot).await?;
        // Created up front: the in-match heartbeat keys its lifetime to this
        // token so it survives transport errors during a reconnect (ending only
        // at teardown). Shared with the Match and the reconnect coordinator.
        let cancellation_token = tokio_util::sync::CancellationToken::new();
        let in_match = crate::net::InMatchTx::new(
            pre_match.in_match_sender.clone(),
            IN_MATCH_HEARTBEAT,
            cancellation_token.clone(),
        );

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
            pre_match.match_ts,
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
            rtc_time,
        };
        let shadow = tango_pvp::shadow::Shadow::new(
            remote_rom.as_ref(),
            remote_save.as_ref(),
            remote_hooks,
            pre_match.match_type,
            pre_match.is_offerer,
            local_player_index,
            rng.clone(),
            rtc_time,
        )?;

        let latency_counter = Arc::new(std::sync::Mutex::new(Some(crate::net::LatencyCounter::new(5))));
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
            Box::new(crate::net::PvpSender::new(in_match.clone())),
            cancellation_token.clone(),
            rng,
            shadow,
            identity,
            tango_pvp::battle::ReplayConfig { writer: replay_writer },
            frame_delay.clone(),
            disable_bgm,
        );
        match_handle.set(inner_match.clone());

        // Shared peer-connection ownership: the session keeps it alive for its
        // whole life, while the reconnect coordinator can drop the old one (to
        // free the host's pinned UDP port) and slot the rebuilt one back in.
        let peer_conn = Arc::new(Mutex::new(Some(pre_match.peer_conn)));
        // `(started_at, give_up_at)` of the in-progress reconnect, or `None` when
        // not reconnecting. The UI reads it to draw the "Reconnecting…" overlay
        // and its give-up bar; the emulator is paused for the duration.
        let reconnect_window = Arc::new(Mutex::new(None::<(std::time::Instant, std::time::Instant)>));

        // Match receive loop + transparent-reconnect coordinator. Holds
        // inner_match alive until the match ends (completion / cancel) or, for a
        // direct link that drops, until reconnection gives up. On a drop it pauses
        // the emulator, rebuilds the direct connection, hot-swaps the live
        // channels under the persistent rennet streams, and resumes — the lockstep
        // sim treats the gap as a pause, so no state resync is needed. The
        // matchmaking transport carries no rebuild recipe, so there the first drop
        // ends the match exactly as before.
        {
            let inner_match = inner_match.clone();
            let match_handle = match_handle.clone();
            let completion_token = completion_token.clone();
            let end = end.clone();
            let frame_notify = frame_notify.clone();
            let latency_counter = latency_counter.clone();
            let in_match = in_match.clone();
            let in_match_sender = pre_match.in_match_sender.clone();
            let lobby_sender = pre_match.lobby_sender.clone();
            // Mutable so a successful matchmaking reconnect can refresh the
            // rendezvous `session_id` for the next drop (see below). `rng_seed`
            // is the unchanging half of that derivation.
            let mut reconnect = pre_match.reconnect.clone();
            let rng_seed = pre_match.rng_seed;
            let handle = thread.handle();
            let peer_conn = peer_conn.clone();
            let reconnect_window = reconnect_window.clone();
            let cancel = cancellation_token.clone();
            let mut reliable_receiver = reliable_receiver;
            let mut receiver: Box<dyn tango_pvp::net::Receiver + Send + Sync> = Box::new(crate::net::PvpReceiver::new(
                in_match_receiver,
                in_match.clone(),
                latency_counter.clone(),
                end.remote_ended.clone(),
                frame_notify.clone(),
            ));
            tokio::task::spawn(async move {
                // Why the receive loop ended this iteration.
                enum Trip {
                    /// Clean local teardown (user closed / cancelled). Never reconnects.
                    Cancelled,
                    /// A channel hit EOF — the peer *told* us something: a deliberate
                    /// quit (usually preceded by its `Closing` marker), the peer's
                    /// reconnect coordinator giving up, or its transport declaring the
                    /// link dead. All of those mean the match is over; a mere link
                    /// outage never produces an EOF (our own reconnect teardown is
                    /// silent — see below — and a dead network delivers nothing).
                    Closed,
                    /// The local input queue climbed to `RECONNECT_QUEUE_LENGTH`:
                    /// the peer stopped matching our inputs, i.e. a quiet/dead link.
                    /// The one trip that reconnects.
                    Stalled,
                }
                // Latched by `watch_reliable` when the peer sends a `Closing`
                // marker ahead of its disconnect: it's leaving on purpose, so we
                // end rather than spend the reconnect window on it.
                let peer_closing = AtomicBool::new(false);
                loop {
                    // `run` takes `receiver` by move, which is fine — every
                    // looping path rebuilds it.
                    let trip = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => Trip::Cancelled,
                        // Ahead of the in-match EOF: a `Closing` marker rides the
                        // reliable channel just before the close it announces, so
                        // reading it first latches `peer_closing` before the close
                        // trips.
                        _ = watch_reliable(&mut reliable_receiver, &peer_closing) => Trip::Closed,
                        r = inner_match.run(receiver) => {
                            log::info!("pvp in-match channel closed: {r:?}");
                            Trip::Closed
                        }
                        _ = inner_match.stalled() => Trip::Stalled,
                    };

                    // Our own deliberate close: tell the peer (a `Closing` marker)
                    // so it ends immediately instead of waiting out its reconnect
                    // window, then stop. (`cancel` is already tripped.)
                    if matches!(trip, Trip::Cancelled) {
                        let _ = lobby_sender.lock().await.send_closing().await;
                        break;
                    }

                    // Reconnect only on the stall watchdog — the one signal that
                    // means "the link went quiet under a live match" — and only if
                    // the transport can rebuild, the match isn't ending (our
                    // completion or the peer's EndOfMatch), and the peer didn't
                    // announce a deliberate close.
                    let reconnectable = matches!(trip, Trip::Stalled)
                        && reconnect.is_some()
                        && !completion_token.is_complete()
                        && !end.remote_ended.load(Ordering::Acquire)
                        && !peer_closing.load(Ordering::Acquire);
                    if !reconnectable {
                        end.remote_disconnected.store(true, Ordering::Release);
                        cancel.cancel();
                        break;
                    }
                    let recipe = reconnect.clone().unwrap();

                    // Freeze the sim so its speculative lead can't run past the
                    // rollback horizon (and overflow-bail) while the link is down;
                    // retire the latency readout; arm the give-up window the UI bar
                    // drains over. One window, sized per transport (the sim is
                    // paused throughout, so a long wait costs nothing but the
                    // wait). Both peers converge on it: whoever trips first goes
                    // silent, which stall-trips the other within
                    // `RECONNECT_QUEUE_LENGTH` frames.
                    let start = std::time::Instant::now();
                    let timeout = match recipe {
                        crate::netplay::ReconnectRecipe::Direct(_) => RECONNECT_DIRECT_TIMEOUT,
                        crate::netplay::ReconnectRecipe::Matchmaking { .. } => RECONNECT_MATCHMAKING_TIMEOUT,
                    };
                    let deadline = start + timeout;
                    *reconnect_window.lock().unwrap() = Some((start, deadline));
                    *latency_counter.lock().unwrap() = None;
                    handle.pause();
                    frame_notify.notify_one();
                    log::info!("pvp link dropped — pausing to reconnect");

                    // Tear the old peer connection down *before* rebuilding so
                    // the host's pinned UDP port frees up for the re-bind — and
                    // tear it down *silently* (`abandon`: no DTLS close_notify).
                    // A clean EOF now means "the peer left" (Trip::Closed above),
                    // so handing the peer one mid-reconnect would end its match;
                    // silence instead trips its stall watchdog into the same
                    // rendezvous. The socket is released asynchronously when the
                    // old driver task tears down, so a rebuild attempt can race it
                    // and see AddrInUse — `rebuild_connection` retries, absorbing
                    // that.
                    if let Some(old) = peer_conn.lock().unwrap().take() {
                        old.abandon();
                    }

                    // Rebuild, ticking the UI at ~30 fps so the give-up bar drains
                    // smoothly while the paused emulator produces no frames.
                    let rebuilt = {
                        let ui_tick = async {
                            let mut iv = tokio::time::interval(RECONNECT_UI_TICK);
                            loop {
                                iv.tick().await;
                                frame_notify.notify_one();
                            }
                        };
                        tokio::select! {
                            r = rebuild_connection(&recipe, deadline, &cancel) => r,
                            _ = ui_tick => None,
                        }
                    };

                    let Some(channels) = rebuilt else {
                        // Timed out or cancelled — give up and end the match.
                        *reconnect_window.lock().unwrap() = None;
                        end.remote_disconnected.store(true, Ordering::Release);
                        cancel.cancel();
                        handle.unpause();
                        break;
                    };

                    // Hot-swap the rebuilt channels under the persistent streams.
                    let crate::net::channel::Channels {
                        control: (new_control_sender, new_control_receiver),
                        in_match: (new_in_match_sender, new_in_match_receiver),
                        peer_conn: new_peer_conn,
                        local_dtls_fingerprint,
                        peer_dtls_fingerprint,
                    } = channels;

                    // Refresh the matchmaking rendezvous so the *next* drop re-dials
                    // a fresh, unguessable `session_id` derived from this new
                    // connection's DTLS fingerprints (both peers just handshook, so
                    // they derive the same one) instead of reusing the original —
                    // which the signaling server has already seen. The direct path's
                    // recipe (re-run host/connect) needs no fingerprints, so its
                    // empty pair leaves the seed-only fallback in place harmlessly.
                    if let Some(crate::netplay::ReconnectRecipe::Matchmaking { session_id, .. }) = reconnect.as_mut() {
                        *session_id = crate::netplay::derive_reconnect_session_id(
                            &rng_seed,
                            &local_dtls_fingerprint,
                            &peer_dtls_fingerprint,
                        );
                    }
                    *peer_conn.lock().unwrap() = Some(new_peer_conn);
                    // Retarget the out-stream sink (pump + heartbeat both send
                    // through this shared handle); keep the new reliable sender
                    // alive so its channel doesn't half-close under the peer.
                    *in_match_sender.lock().await = new_in_match_sender;
                    *lobby_sender.lock().await = new_control_sender;
                    reliable_receiver = new_control_receiver;
                    // Fresh receiver over the new channel, same `in_match` — the
                    // rennet in-stream (seq/ack) carries across the swap, so the
                    // peer's resent window fills our gap contiguously.
                    receiver = Box::new(crate::net::PvpReceiver::new(
                        new_in_match_receiver,
                        in_match.clone(),
                        latency_counter.clone(),
                        end.remote_ended.clone(),
                        frame_notify.clone(),
                    ));
                    // The queue watchdog re-arms itself on the next loop turn (it
                    // builds fresh each `select!`): it waits for the rebuilt link's
                    // resent inputs to drain the standing queue back below the
                    // threshold before it can trip again, so it re-trips and
                    // reconnects once more (self-healing) if the link falls silent
                    // anew, without instantly re-firing on the not-yet-drained queue.
                    *latency_counter.lock().unwrap() = Some(crate::net::LatencyCounter::new(5));
                    *reconnect_window.lock().unwrap() = None;
                    handle.unpause();
                    frame_notify.notify_one();
                    log::info!("pvp transparently reconnected the direct link");
                }

                // Teardown: retire latency so `latency()` reads `None` and the
                // telemetry panel retires, wake the session to re-check
                // `is_ended` (the emu thread may be paused, so no vblank is
                // coming), finalize the replay iff the match reached its end
                // screen, and release the match handle.
                *latency_counter.lock().unwrap() = None;
                frame_notify.notify_one();
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
            _peer_conn: peer_conn,
            reconnect_window,
            _lobby_sender: pre_match.lobby_sender,
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
        self.reconnect_window.lock().unwrap().is_some()
    }

    /// Fraction of the reconnect give-up window still remaining — `1.0` when a
    /// reconnect just started, falling to `0.0` at the give-up deadline, or
    /// `None` when not reconnecting. Drives the overlay's depleting progress bar.
    pub fn reconnect_progress(&self) -> Option<f32> {
        self.reconnect_window.lock().unwrap().map(|(start, deadline)| {
            let total = deadline
                .saturating_duration_since(start)
                .as_secs_f32()
                .max(f32::EPSILON);
            let remaining = deadline
                .saturating_duration_since(std::time::Instant::now())
                .as_secs_f32();
            (remaining / total).clamp(0.0, 1.0)
        })
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
    /// `Some(ZERO)` until the first Pong arrives, then `Some(median)`
    /// while the link is up; `None` once the remote drops (the match-run task
    /// clears the counter). The UI keys the instrument panel off this: `None`
    /// means "no live link", which `remote_disconnected` (still used internally
    /// by [`is_ended`](Self::is_ended)) can't distinguish from a legitimate 0 ms
    /// LAN ping that sticks at its last reading after a drop.
    pub fn latency(&self) -> Option<std::time::Duration> {
        self.latency_counter.lock().unwrap().as_ref().map(|c| c.median())
    }

    /// Raw latest ping — the most recent single measurement, unsmoothed.
    /// Drives the live telemetry plate + sparkline, where the median's lag
    /// would mask a real spike. Same `Some`/`None` link-up semantics as
    /// [`latency`](Self::latency) (both read the same counter), so it gates
    /// the instrument panel identically; only the reported value differs.
    pub fn latency_raw(&self) -> Option<std::time::Duration> {
        self.latency_counter
            .lock()
            .unwrap()
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
    /// remote_advantage` (see `Round::update_fps_target`). The symmetric
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

/// Poll the receiver-handoff slot until the lobby loop drops the live receiver
/// into it. Generic over the receiver type so it serves both the reliable
/// control receiver and the unreliable in-match one. Bounded to avoid hanging
/// the PvP setup forever if something went off the rails.
async fn drain_receiver<R>(slot: &Arc<std::sync::Mutex<Option<R>>>) -> anyhow::Result<R> {
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

/// Watch the reliable channel mid-match for the peer's deliberate-close marker.
/// On a [`Closing`] packet it latches `peer_closing` — the peer is leaving on
/// purpose, so the disconnect it's about to send is clean and we should end
/// rather than reconnect — and keeps watching. It only returns when the channel
/// actually closes (a recv error). Nothing else legitimately flows here
/// mid-match, so stray traffic / undecodable bytes are ignored.
///
/// [`Closing`]: crate::net::protocol::Packet::Closing
async fn watch_reliable(receiver: &mut crate::net::Receiver, peer_closing: &AtomicBool) {
    use crate::net::protocol::Packet;
    loop {
        match receiver.receive().await {
            Ok(Packet::Closing(_)) => peer_closing.store(true, Ordering::Release),
            // Some other packet — nothing else legitimately flows here mid-match,
            // but ignore it and keep watching.
            Ok(_) => {}
            // Undecodable bytes (`InvalidData`) are stray traffic, not a close —
            // ignore and keep watching. Any other error (notably the channel's
            // `UnexpectedEof`) means it actually closed, so stop.
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {}
            Err(_) => return,
        }
    }
}

/// Rebuild a dropped connection from its recipe, then run the version
/// `negotiate` handshake on the rebuilt reliable channel. The bring-up doubles
/// as a rendezvous barrier — the direct `host`'s first send blocks until the
/// dialer is back, and matchmaking's `connect` blocks at the signaling server
/// until the peer rejoins — so both peers only return (and unpause) once the
/// link is genuinely carrying traffic again. Retries failed attempts (the
/// peers race each other to re-rendezvous) until `deadline`, returning `None`
/// on timeout or cancellation.
///
/// Returns the rebuilt [`crate::net::channel::Channels`] bundle regardless of
/// transport — the matchmaking path funnels the signaling client's `Connected`
/// through the same [`Channels::from_signaling`] the initial connect uses, so a
/// rebuild and a fresh build produce the identical shape (fingerprints and all).
async fn rebuild_connection(
    recipe: &crate::netplay::ReconnectRecipe,
    deadline: std::time::Instant,
    cancel: &tokio_util::sync::CancellationToken,
) -> Option<crate::net::channel::Channels> {
    use crate::netplay::{DirectRole, ReconnectRecipe};
    let attempt_timeout = match recipe {
        ReconnectRecipe::Direct(_) => RECONNECT_DIRECT_ATTEMPT_TIMEOUT,
        ReconnectRecipe::Matchmaking { .. } => RECONNECT_MATCHMAKING_ATTEMPT_TIMEOUT,
    };
    loop {
        let now = std::time::Instant::now();
        if cancel.is_cancelled() || now >= deadline {
            return None;
        }
        // Cap the attempt by whichever is sooner — the per-attempt limit or the
        // remaining give-up budget — so a short give-up window (a channel-close
        // reconnect) fires on time instead of overrunning by a whole attempt.
        let this_timeout = attempt_timeout.min(deadline.saturating_duration_since(now));
        let attempt = async {
            let mut channels = match recipe {
                ReconnectRecipe::Direct(DirectRole::Host { port }) => crate::net::direct_rtc::host(*port).await?,
                ReconnectRecipe::Direct(DirectRole::Connect { addr }) => crate::net::direct_rtc::connect(addr).await?,
                ReconnectRecipe::Matchmaking {
                    endpoint,
                    session_id,
                    use_relay,
                    identity,
                } => {
                    let connecting = tango_signaling::connect(
                        endpoint,
                        session_id,
                        *use_relay,
                        crate::netplay::PROTOCOL_VERSION,
                        vec![
                            crate::net::channel::control_channel(),
                            crate::net::channel::in_match_channel(),
                        ],
                        identity.clone(),
                    )
                    .await
                    .map_err(|e| std::io::Error::other(format!("signaling: {e}")))?;
                    // Blocks at the server until the peer rejoins the session, then
                    // completes the WebRTC handshake — the matchmaking rendezvous.
                    // The bundle carries this handshake's fingerprints so the
                    // coordinator can re-derive the session_id for the next drop;
                    // they don't affect *this* rendezvous (its id is already fixed).
                    let connected = connecting
                        .await
                        .map_err(|e| std::io::Error::other(format!("webrtc: {e}")))?;
                    crate::net::channel::Channels::from_signaling(connected)?
                }
            };
            crate::net::negotiate(&mut channels.control.0, &mut channels.control.1)
                .await
                .map_err(|e| std::io::Error::other(format!("negotiate: {e:?}")))?;
            Ok::<_, std::io::Error>(channels)
        };
        let outcome = tokio::select! {
            biased;
            _ = cancel.cancelled() => return None,
            r = tokio::time::timeout(this_timeout, attempt) => r,
        };
        match outcome {
            Ok(Ok(channels)) => return Some(channels),
            Ok(Err(e)) => log::debug!("pvp reconnect attempt failed: {e}"),
            Err(_) => log::debug!("pvp reconnect attempt timed out"),
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return None,
            _ = tokio::time::sleep(RECONNECT_BACKOFF) => {}
        }
    }
}

/// Open the replay file + write its metadata frame. Filename
/// format mirrors the legacy app:
/// `YYYYMMDDhhmmss-<link_code>-<compat>-vs-<opponent>-p<idx>.tangoreplay`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_replay_writer(
    replays_path: &Path,
    link_code: &str,
    local_settings: &crate::net::protocol::Settings,
    remote_settings: &crate::net::protocol::Settings,
    match_type: (u8, u8),
    is_offerer: bool,
    local_player_index: u8,
    rng_seed: [u8; 16],
    match_ts: u64,
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
            // The negotiated match clock, not the local wall clock: every live
            // core's cart RTC is pinned to this instant, and playback pins to
            // `metadata.ts` (see `Replay::rtc_time`), so recording the same
            // value is what makes exe45 replays reproduce the live match. Both
            // peers' replays of one match carry the identical ts.
            ts: match_ts,
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
                // The replay metadata proto (replay11) predates the
                // blind-setup inversion and still stores the
                // positive "reveal" sense.
                reveal_setup: !local_settings.blind_setup,
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

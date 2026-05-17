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

use parking_lot::Mutex;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

pub use tango_pvp::battle::EXPECTED_FPS;

pub struct PvpSession {
    vbuf: Arc<Mutex<Vec<u8>>>,
    joyflags: Arc<AtomicU32>,
    close_requested: Arc<AtomicBool>,
    /// Flipped true by the network/match background task when it
    /// exits — clean finish, peer disconnect, comm error, or
    /// user-cancel all converge to setting this. Polled by the
    /// session-view Tick so the UI can self-close instead of
    /// freezing on the last received frame.
    session_ended: Arc<AtomicBool>,
    _audio_binding: Option<crate::audio::Binding>,
    _thread: mgba::thread::Thread,
    /// Drops fire-cancellation through the match background tasks
    /// (`Match::run`, `Match::cancel`). On Close we cancel + drop
    /// the session, which tears the network loop down cleanly.
    cancellation_token: tokio_util::sync::CancellationToken,
    latency_counter: Arc<tokio::sync::Mutex<crate::net::LatencyCounter>>,
    _peer_conn: datachannel_wrapper::PeerConnection,
    /// Kept alive so the background `match_.run(receiver)` task
    /// has a referent. Cleared by that task when it exits.
    _match_handle: Arc<tokio::sync::Mutex<Option<Arc<tango_pvp::battle::Match>>>>,
    /// Bumped once per emulator frame in the frame callback —
    /// see `frame_id()` for usage.
    frame_id: Arc<std::sync::atomic::AtomicU64>,
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
        local_game: &'static (dyn crate::game::Game + Send + Sync),
        local_rom: Arc<Vec<u8>>,
        remote_game: &'static (dyn crate::game::Game + Send + Sync),
        remote_rom: Arc<Vec<u8>>,
        pre_match: crate::netplay::PreMatchData,
        replays_path: &Path,
        audio_binder: &crate::audio::LateBinder,
        opponent_loaded: Option<crate::selection::Loaded>,
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
            .gamedb_entry()
            .parse_save(&pre_match.remote_save_data)
            .map_err(|e| anyhow::anyhow!("parse remote save: {e:?}"))?;
        // Local save is whatever we committed; same path.
        let local_save = local_game
            .gamedb_entry()
            .parse_save(&pre_match.local_save_data)
            .map_err(|e| anyhow::anyhow!("parse local save: {e:?}"))?;

        let mut core = mgba::core::Core::new_gba("tango-ng")?;
        core.enable_video_buffer();
        core.as_mut()
            .load_rom(mgba::vfile::VFile::from_vec(local_rom.as_ref().clone()))?;
        // PvP runs entirely off the in-memory SRAM dump from the
        // commitment — writes don't persist back to the user's
        // .sav file (matches legacy behavior; the only PvP-side
        // mutations are stat/zenny stuff which the user shouldn't
        // be carrying over from netplay anyway).
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(local_save.as_sram_dump()))?;

        let joyflags = Arc::new(AtomicU32::new(0));
        let local_hooks = local_game.hooks();
        local_hooks.patch(core.as_mut());

        let match_handle: Arc<tokio::sync::Mutex<Option<Arc<tango_pvp::battle::Match>>>> =
            Arc::new(tokio::sync::Mutex::new(None));
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
        let local_player_index =
            tango_pvp::battle::Match::pick_local_player_index(&mut rng, pre_match.is_offerer);

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

        let remote_hooks = remote_game.hooks();
        let identity = tango_pvp::battle::MatchIdentity {
            match_type: pre_match.match_type,
            is_offerer: pre_match.is_offerer,
            local_player_index,
            input_delay: pre_match.input_delay as u32,
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
        let latency_counter = Arc::new(tokio::sync::Mutex::new(crate::net::LatencyCounter::new(5)));
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
        );
        *match_handle.try_lock().unwrap() = Some(inner_match.clone());

        // Spawn the network receive loop. Holds inner_match alive
        // until the receiver errors (peer disconnected) or the
        // cancellation_token fires. On exit, drop the handle out
        // of the shared slot so the session knows the match's gone.
        let session_ended = Arc::new(AtomicBool::new(false));
        {
            let match_handle = match_handle.clone();
            let inner_match = inner_match.clone();
            let completion_token = completion_token.clone();
            let session_ended = session_ended.clone();
            let receiver = Box::new(crate::net::PvpReceiver::new(
                receiver,
                pre_match.sender.clone(),
                latency_counter.clone(),
            ));
            tokio::task::spawn(async move {
                tokio::select! {
                    r = inner_match.run(receiver) => {
                        log::info!("pvp match thread ending: {:?}", r);
                    }
                    _ = inner_match.cancelled() => {
                        log::info!("pvp match thread cancelled");
                    }
                }
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
                *match_handle.lock().await = None;
                // Signal the UI: the match is done (clean win,
                // peer disconnect, comm error, user-cancel — all
                // converge here). The session-view tick handler
                // polls this and tears the session down so the
                // user is never stuck on a frozen frame after a
                // remote drop.
                session_ended.store(true, Ordering::SeqCst);
            });
        }

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        let audio_binding = match audio_binder.bind(Some(Box::new(crate::audio::MGBAStream::new(
            thread.handle(),
            audio_binder.sample_rate(),
        )))) {
            Ok(b) => Some(b),
            Err(e) => {
                log::warn!("pvp: audio bind failed: {e:?}");
                None
            }
        };

        let frame_id = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4) as usize
        ]));
        thread.set_frame_callback({
            let vbuf = vbuf.clone();
            let joyflags = joyflags.clone();
            let completion_token = completion_token.clone();
            let frame_id = frame_id.clone();
            move |mut core, video_buffer, mut thread_handle| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                fix_vbuf_alpha(&mut vbuf);
                core.set_keys(joyflags.load(Ordering::Relaxed));
                frame_id.fetch_add(1, Ordering::Release);
                if completion_token.is_complete() {
                    thread_handle.pause();
                }
            }
        });

        Ok(Self {
            vbuf,
            joyflags,
            close_requested: Arc::new(AtomicBool::new(false)),
            session_ended,
            _audio_binding: audio_binding,
            _thread: thread,
            cancellation_token,
            latency_counter,
            _peer_conn: pre_match.peer_conn,
            _match_handle: match_handle,
            frame_id,
            link_code: pre_match.link_code,
            remote_nickname: pre_match.remote_settings.nickname,
            opponent_loaded,
            opponent_save_view: crate::save_view::State::new(),
        })
    }

    /// See `singleplayer_session::SinglePlayerSession::frame_id`.
    pub fn frame_id(&self) -> u64 {
        self.frame_id.load(Ordering::Acquire)
    }

    pub fn snapshot_vbuf(&self) -> Vec<u8> {
        self.vbuf.lock().clone()
    }

    /// Overwrite the joyflag bitmap (same shape as singleplayer's
    /// — see [`crate::singleplayer_session::SinglePlayerSession::set_joyflags`]).
    pub fn set_joyflags(&self, mgba_keys: u32) {
        self.joyflags.store(mgba_keys, Ordering::Relaxed);
    }

    pub fn request_close(&self) {
        self.close_requested.store(true, Ordering::SeqCst);
        self.cancellation_token.cancel();
    }

    /// True once the match background task has wound down — the
    /// session view polls this each tick and emits `Message::Close`
    /// so the UI doesn't get stuck on a frozen final frame after
    /// a peer drop or comm error.
    pub fn is_ended(&self) -> bool {
        self.session_ended.load(Ordering::Acquire)
    }

    /// Median ping over the last few seconds — drives the in-
    /// match latency indicator. Returns ZERO until the first
    /// Pong arrives.
    pub fn latency_blocking(&self) -> std::time::Duration {
        self.latency_counter.blocking_lock().median()
    }
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

fn fix_vbuf_alpha(vbuf: &mut [u8]) {
    for px in vbuf.chunks_exact_mut(4) {
        px[3] = 0xFF;
    }
}

/// Poll the receiver-handoff slot until the lobby loop drops
/// the live Receiver into it. Bounded to avoid hanging the
/// PvP setup forever if something went off the rails.
async fn drain_receiver(
    slot: &Arc<parking_lot::Mutex<Option<crate::net::Receiver>>>,
) -> anyhow::Result<crate::net::Receiver> {
    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(5);
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        if let Some(r) = slot.lock().take() {
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
    let raw_name = format!(
        "{ts}-{link_code}-{netplay_compat}-vs-{}-p{}",
        remote_settings.nickname,
        local_player_index + 1
    );
    let safe_name: String = raw_name
        .chars()
        .filter(|c| !"/\\?%*:|\"<>. ".contains(*c))
        .collect();
    let replay_filename = replays_path.join(format!("{safe_name}.tangoreplay"));
    log::info!("pvp: opening replay file {}", replay_filename.display());

    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&replay_filename)?;
    let local_wram = local_save.as_raw_wram().into_owned();
    let remote_wram = remote_save.as_raw_wram().into_owned();
    Ok(tango_pvp::replay::Writer::new(
        file,
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
        &local_wram,
        &remote_wram,
    )?)
}

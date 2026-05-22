//! Replay playback session.
//!
//! Owns an mgba playback thread, installs the per-game stepper traps,
//! pushes captured snapshots into a [`SnapshotStore`] each frame, and
//! runs a background [`Prefetcher`] thread that races ahead of the
//! playhead to keep that store populated for seeks. Audio is bound via
//! the shared [`crate::audio::LateBinder`].

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use tango_pvp::replay::playback::SnapshotStore;
use tango_pvp::shadow::Shadow;

pub const SCREEN_WIDTH: u32 = mgba::gba::SCREEN_WIDTH;
pub const SCREEN_HEIGHT: u32 = mgba::gba::SCREEN_HEIGHT;
const EXPECTED_FPS: f32 = 60.0;

pub struct ReplaySession {
    vbuf: Arc<Mutex<Vec<u8>>>,
    completion_token: tango_pvp::hooks::CompletionToken,
    close_requested: Arc<AtomicBool>,
    replay: Arc<tango_pvp::replay::Replay>,
    stepper_state: tango_pvp::stepper::State,
    shadow: Arc<Mutex<Shadow>>,
    snapshots: SnapshotStore,
    prefetch_progress: Arc<AtomicU32>,
    total_ticks: u32,
    /// See `singleplayer_session::SinglePlayerSession::frame_id`.
    frame_id: Arc<std::sync::atomic::AtomicU64>,
    /// Held so the audio binding survives for the session's lifetime;
    /// the LateBinder swaps back to silence when this Drops.
    _audio_binding: Option<crate::audio::Binding>,
    /// Field order matters — `_prefetcher`'s Drop signals cancel and
    /// joins the background thread before `_thread` is torn down. Both
    /// come last so the frame callback (running on `_thread`) is dead
    /// by the time the earlier fields are freed.
    _prefetcher: Prefetcher,
    thread: mgba::thread::Thread,
}

impl ReplaySession {
    pub fn new(
        game: &'static (dyn crate::game::Game + Send + Sync),
        rom: Arc<Vec<u8>>,
        remote_game: &'static (dyn crate::game::Game + Send + Sync),
        remote_rom: Arc<Vec<u8>>,
        replay: Arc<tango_pvp::replay::Replay>,
        audio_binder: &crate::audio::LateBinder,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();
        core.as_mut()
            .load_rom(mgba::vfile::VFile::from_vec(rom.as_ref().clone()))?;
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(replay.local_sram_dump()))?;

        let hooks = game.hooks();
        hooks.patch(core.as_mut());

        let completion_token = tango_pvp::hooks::CompletionToken::new();
        if replay.rounds.is_empty() {
            anyhow::bail!("replay has no rounds");
        }
        let replay_is_complete = replay.is_complete;
        let total_ticks = replay.rounds.iter().map(|r| r.len() as u32).sum::<u32>();
        let match_type = (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8);

        let remote_hooks = remote_game.hooks();
        let shadow = Shadow::new_for_replay(remote_rom.as_ref(), &replay, remote_hooks)?;
        let shadow = Arc::new(Mutex::new(shadow));

        let stepper_state = tango_pvp::stepper::State::new(
            match_type,
            replay.local_player_index,
            replay.rounds.clone(),
            0,
            replay.rng_seed,
            replay.is_offerer,
            total_ticks,
            shadow.clone(),
            Box::new({
                let completion_token = completion_token.clone();
                move || completion_token.complete()
            }),
        );

        let mut traps = hooks.common_traps();
        traps.extend(hooks.stepper_traps(stepper_state.clone()));
        core.set_traps(traps);

        let thread = mgba::thread::Thread::new(core);
        let vbuf = Arc::new(Mutex::new(vec![0u8; (SCREEN_WIDTH * SCREEN_HEIGHT * 4) as usize]));

        let snapshots = SnapshotStore::new();
        let prefetch_progress = Arc::new(AtomicU32::new(0));
        let prefetcher = Prefetcher::spawn(
            rom.clone(),
            remote_rom.clone(),
            replay.clone(),
            game,
            remote_game,
            snapshots.clone(),
            prefetch_progress.clone(),
        );

        let frame_id = Arc::new(std::sync::atomic::AtomicU64::new(0));
        thread.set_frame_callback({
            let vbuf = vbuf.clone();
            let completion_token = completion_token.clone();
            let stepper_state = stepper_state.clone();
            let snapshots = snapshots.clone();
            let shadow = shadow.clone();
            let frame_id = frame_id.clone();
            move |mut core, video_buffer, mut thread_handle| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                fix_vbuf_alpha(&mut vbuf);
                frame_id.fetch_add(1, Ordering::Release);

                if let Some(err) = stepper_state.lock_inner().take_error() {
                    log::error!("replay stepper error: {err:?}");
                }

                // Capture round-start + every MID_ROUND_SNAPSHOT_INTERVAL
                // ticks so backward seeks have a nearby jumping-off point
                // even if the prefetcher hasn't reached them yet.
                if let Some(cp) = stepper_state.capture_replay_checkpoint() {
                    snapshots.capture_if_needed(cp, &mut core, &shadow);
                }

                let (total_left, is_round_ended) = {
                    let inner = stepper_state.lock_inner();
                    (inner.total_input_pairs_left(), inner.is_round_ended())
                };
                // Mirrors the legacy guard: clean replays wait for the
                // post-round end-of-round routine to flip is_round_ended;
                // incomplete replays just fall through on input exhaustion.
                if total_left == 0 && (is_round_ended || !replay_is_complete) {
                    completion_token.complete();
                }

                if completion_token.is_complete() {
                    thread_handle.pause();
                }
            }
        });

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        let audio_binding = match audio_binder.bind(Some(Box::new(crate::audio::MGBAStream::new(
            thread.handle(),
            audio_binder.sample_rate(),
        )))) {
            Ok(b) => Some(b),
            Err(e) => {
                log::warn!("replay: audio bind failed: {e:?}");
                None
            }
        };

        Ok(Self {
            vbuf,
            completion_token,
            close_requested: Arc::new(AtomicBool::new(false)),
            replay,
            stepper_state,
            shadow,
            snapshots,
            prefetch_progress,
            total_ticks,
            frame_id,
            _audio_binding: audio_binding,
            _prefetcher: prefetcher,
            thread,
        })
    }

    /// Clone of the latest framebuffer (RGBA8, 240x160).
    pub fn snapshot_vbuf(&self) -> Vec<u8> {
        self.vbuf.lock().clone()
    }

    /// See `singleplayer_session::SinglePlayerSession::frame_id`.
    pub fn frame_id(&self) -> u64 {
        self.frame_id.load(Ordering::Acquire)
    }

    pub fn is_paused(&self) -> bool {
        self.thread.handle().is_paused()
    }

    /// Drive the mgba thread at `factor * 60` fps. 1.0 = realtime,
    /// 0.5 = slow-mo, 2.0 / 4.0 = fast-forward. Audio paces frames, so
    /// values above ~4 start dropping samples.
    pub fn set_speed(&self, factor: f32) {
        let fps = (EXPECTED_FPS * factor).max(1.0);
        self.thread.handle().lock_audio().sync_mut().set_fps_target(fps);
    }

    /// Current factor (current fps / 60).
    pub fn speed(&self) -> f32 {
        self.thread.handle().lock_audio().sync().fps_target() / EXPECTED_FPS
    }

    /// Toggle the mgba thread between paused and running. Returns the
    /// new paused state.
    pub fn set_paused(&self, paused: bool) {
        let h = self.thread.handle();
        if paused {
            h.pause();
        } else {
            // The frame_callback re-pauses when completion_token is set,
            // so unpausing past the end of a replay only buys one frame
            // — that's fine and matches the legacy behavior.
            h.unpause();
        }
    }

    pub fn request_close(&self) {
        self.close_requested.store(true, Ordering::SeqCst);
    }

    /// Absolute playhead tick: how many emulated ticks the stepper has
    /// consumed since the start of the replay. Saturates at
    /// `total_ticks` once the replay finishes.
    pub fn current_tick(&self) -> u32 {
        self.stepper_state.lock_inner().absolute_tick()
    }

    pub fn total_ticks(&self) -> u32 {
        self.total_ticks
    }

    /// Highest tick the background prefetcher has reached, for the
    /// progress overlay on the scrub bar. Hits `total_ticks` when the
    /// prefetcher has run to completion.
    pub fn prefetch_progress(&self) -> u32 {
        self.prefetch_progress.load(Ordering::Relaxed)
    }

    /// Cumulative tick at the end of each round *except* the last —
    /// the inter-round boundaries the scrubber draws marks at. Empty
    /// for a single-round replay.
    pub fn round_boundaries(&self) -> Vec<u32> {
        let n = self.replay.rounds.len();
        if n <= 1 {
            return Vec::new();
        }
        let mut acc = 0u32;
        let mut out = Vec::with_capacity(n - 1);
        for r in self.replay.rounds.iter().take(n - 1) {
            acc += r.len() as u32;
            out.push(acc);
        }
        out
    }

    /// Jump the playhead to `target`. Submits the snapshot load + frame
    /// catch-up onto the mgba thread so the seek runs synchronously
    /// against the live core, but returns immediately. The UI's next
    /// tick will pick up the post-seek framebuffer.
    pub fn seek_to(&self, target: u32) {
        let target = target.min(self.total_ticks);
        let current = self.stepper_state.lock_inner().absolute_tick();
        if target == current {
            return;
        }

        let start_snap = if target < current {
            self.snapshots.best_at_or_before(target)
        } else {
            self.snapshots.best_in_range(current, target)
        };

        if target < current && start_snap.is_none() {
            // Pre-first-round boot window — no snapshot to land on.
            // Silently drop instead of bubbling up an error; the user
            // can drag the scrubber further right.
            log::debug!("seek: no snapshot at or before tick {target}");
            return;
        }

        // Clear completion so the frame_callback's pause-on-complete
        // check doesn't immediately re-pause after we seek backwards
        // from the end of the replay.
        self.completion_token.reset();

        let stepper_state = self.stepper_state.clone();
        let replay = self.replay.clone();
        let snapshots = self.snapshots.clone();
        let shadow = self.shadow.clone();

        self.thread.handle().run_on_core(move |core| {
            if let Err(e) = tango_pvp::replay::playback::seek_on_core(
                core,
                target,
                &stepper_state,
                &shadow,
                &replay,
                &snapshots,
                start_snap.as_ref(),
            ) {
                log::error!("seek to {target} failed: {e:?}");
            }
        });
    }
}

fn fix_vbuf_alpha(vbuf: &mut [u8]) {
    for px in vbuf.chunks_exact_mut(4) {
        px[3] = 0xFF;
    }
}

/// Background snapshot-prefetch worker. Spawns a fresh mgba core +
/// shadow on a std::thread and races ahead of the playhead, pushing
/// captured snapshots into the shared [`SnapshotStore`] so backward
/// (and long-forward) seeks have a nearby jumping-off point.
///
/// Drop cancels the worker and joins.
pub struct Prefetcher {
    cancel: Arc<AtomicBool>,
    join_handle: Option<std::thread::JoinHandle<()>>,
}

impl Prefetcher {
    pub fn spawn(
        rom: Arc<Vec<u8>>,
        remote_rom: Arc<Vec<u8>>,
        replay: Arc<tango_pvp::replay::Replay>,
        game: &'static (dyn crate::game::Game + Send + Sync),
        remote_game: &'static (dyn crate::game::Game + Send + Sync),
        snapshots: SnapshotStore,
        progress: Arc<AtomicU32>,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = cancel.clone();
        let hooks = game.hooks();
        let remote_hooks = remote_game.hooks();
        let join_handle = std::thread::spawn(move || {
            if let Err(e) = tango_pvp::replay::playback::run_prefetch(
                rom.as_ref(),
                remote_rom.as_ref(),
                &replay,
                hooks,
                remote_hooks,
                snapshots,
                cancel_for_thread,
                progress,
            ) {
                log::error!("replay prefetch worker exited with error: {e:?}");
            }
        });
        Self {
            cancel,
            join_handle: Some(join_handle),
        }
    }
}

impl Drop for Prefetcher {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(h) = self.join_handle.take() {
            let _ = h.join();
        }
    }
}

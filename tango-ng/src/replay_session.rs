//! Minimal replay playback session for tango-ng.
//!
//! Spawns an mgba thread, installs the per-game stepper traps, and
//! drives playback to completion. Audio is not bound (no `audio_binder`
//! integration yet); video frames land in `vbuf` and the UI tick loop
//! is responsible for uploading them to the iced texture.
//!
//! This is a pruned mirror of `tango/src/session.rs::build_replayer_from`
//! — no Prefetcher, no SnapshotStore, no audio. Those come later when we
//! need scrubbing and synchronized audio output.

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const SCREEN_WIDTH: u32 = mgba::gba::SCREEN_WIDTH;
pub const SCREEN_HEIGHT: u32 = mgba::gba::SCREEN_HEIGHT;
const EXPECTED_FPS: f32 = 60.0;

pub struct ReplaySession {
    // NB: thread must outlive vbuf — Drop order is field-order, so keep
    // thread last so the frame callback can't fire after vbuf is freed.
    vbuf: Arc<Mutex<Vec<u8>>>,
    completion_token: tango_pvp::hooks::CompletionToken,
    pause_on_next_frame: Arc<AtomicBool>,
    close_requested: Arc<AtomicBool>,
    replay: Arc<tango_pvp::replay::Replay>,
    stepper_state: tango_pvp::stepper::State,
    total_ticks: u32,
    _thread: mgba::thread::Thread,
}

impl ReplaySession {
    pub fn new(
        game: &'static (dyn crate::game::Game + Send + Sync),
        rom: Arc<Vec<u8>>,
        remote_game: &'static (dyn crate::game::Game + Send + Sync),
        remote_rom: Arc<Vec<u8>>,
        replay: Arc<tango_pvp::replay::Replay>,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango-ng")?;
        core.enable_video_buffer();
        core.as_mut()
            .load_rom(mgba::vfile::VFile::from_vec(rom.as_ref().clone()))?;
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(replay.local_sram_dump()?))?;

        let hooks = game.hooks();
        hooks.patch(core.as_mut());

        let completion_token = tango_pvp::hooks::CompletionToken::new();
        if replay.rounds.is_empty() {
            anyhow::bail!("replay has no rounds");
        }
        let replay_is_complete = replay.is_complete;
        let total_ticks = replay.rounds.iter().map(|r| r.len() as u32).sum::<u32>();
        let match_type = (
            replay.metadata.match_type as u8,
            replay.metadata.match_subtype as u8,
        );

        let remote_hooks = remote_game.hooks();
        use rand::SeedableRng;
        let mut shadow_rng = rand_pcg::Mcg128Xsl64::from_seed(replay.rng_seed);
        // Burn one RNG draw — mirrors the legacy session, which uses the
        // post-bool RNG state for the shadow to match the recorded run.
        let _ = rand::Rng::gen::<bool>(&mut shadow_rng);
        let shadow = tango_pvp::shadow::Shadow::new_from_sram(
            remote_rom.as_ref(),
            &replay.remote_sram_dump()?,
            remote_hooks,
            match_type,
            replay.is_offerer,
            replay.local_player_index,
            shadow_rng,
        )?;
        let shadow = Arc::new(parking_lot::Mutex::new(shadow));

        let stepper_state = tango_pvp::stepper::State::new(
            match_type,
            replay.local_player_index,
            replay.rounds.clone(),
            0,
            replay.rng_seed,
            replay.is_offerer,
            total_ticks,
            shadow,
            Box::new({
                let completion_token = completion_token.clone();
                move || completion_token.complete()
            }),
        );

        let mut traps = hooks.common_traps();
        traps.extend(hooks.stepper_traps(stepper_state.clone()));
        core.set_traps(traps);

        let thread = mgba::thread::Thread::new(core);
        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (SCREEN_WIDTH * SCREEN_HEIGHT * 4) as usize
        ]));
        let pause_on_next_frame = Arc::new(AtomicBool::new(false));

        thread.set_frame_callback({
            let vbuf = vbuf.clone();
            let completion_token = completion_token.clone();
            let stepper_state = stepper_state.clone();
            let pause_on_next_frame = pause_on_next_frame.clone();
            move |_core, video_buffer, mut thread_handle| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                fix_vbuf_alpha(&mut vbuf);

                if let Some(err) = stepper_state.lock_inner().take_error() {
                    log::error!("replay stepper error: {err:?}");
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

                if pause_on_next_frame.swap(false, Ordering::SeqCst) || completion_token.is_complete() {
                    thread_handle.pause();
                }
            }
        });

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        Ok(Self {
            vbuf,
            completion_token,
            pause_on_next_frame,
            close_requested: Arc::new(AtomicBool::new(false)),
            replay,
            stepper_state,
            total_ticks,
            _thread: thread,
        })
    }

    /// Clone of the latest framebuffer. RGBA8, 240x160. Cheap-ish — it's
    /// a 153 600 byte memcpy under a lock — and called at most once per
    /// UI frame.
    pub fn snapshot_vbuf(&self) -> Vec<u8> {
        self.vbuf.lock().clone()
    }

    pub fn is_complete(&self) -> bool {
        self.completion_token.is_complete()
    }

    pub fn request_pause(&self) {
        self.pause_on_next_frame.store(true, Ordering::SeqCst);
    }

    pub fn request_close(&self) {
        self.close_requested.store(true, Ordering::SeqCst);
    }

    pub fn close_requested(&self) -> bool {
        self.close_requested.load(Ordering::SeqCst)
    }

    /// 0..=total_ticks. Derived from the stepper's remaining input
    /// pairs, which is the only progress signal we get from the legacy
    /// stepper without prefetcher snapshots.
    pub fn current_tick(&self) -> u32 {
        let left = self.stepper_state.lock_inner().total_input_pairs_left() as u32;
        self.total_ticks.saturating_sub(left)
    }

    pub fn total_ticks(&self) -> u32 {
        self.total_ticks
    }

    pub fn replay(&self) -> &Arc<tango_pvp::replay::Replay> {
        &self.replay
    }
}

fn fix_vbuf_alpha(vbuf: &mut [u8]) {
    for px in vbuf.chunks_exact_mut(4) {
        px[3] = 0xFF;
    }
}

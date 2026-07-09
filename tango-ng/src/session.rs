//! Emulator sessions — ports of `tango/src/session/{singleplayer,replay}.rs`
//! with the iced/tokio frame plumbing replaced by a polled dirty flag: the
//! mgba frame callback marks new frames and the Slint frame timer collects
//! them via `frame_dirty` + `read_frame`.
//!
//! Singleplayer installs no hooks (vanilla emulator ride); replay playback
//! installs the local game's stepper traps and feeds the recorded input
//! pairs from inside tango-pvp — the frontend never injects input there.
//! Replay seek/prefetch is fully ported: SnapshotStore/RewindBuffer feed
//! the scrubber, a SeekController + worker chase seek targets, and a
//! background prefetcher keeps keyframes ahead of the playhead.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub const SCREEN_WIDTH: u32 = mgba::gba::SCREEN_WIDTH;
pub const SCREEN_HEIGHT: u32 = mgba::gba::SCREEN_HEIGHT;

const EXPECTED_FPS: f32 = 60.0;

/// Create the mgba core every session boots from: a GBA core with audio-sync
/// on, its video buffer enabled, and `rom` loaded. (Same shape as the tango
/// crate's `new_gba_core`; the replay session here and the PvP session in
/// [`crate::pvp`] layer their traps on top.)
pub(crate) fn new_gba_core(rom: &[u8]) -> anyhow::Result<mgba::core::Core> {
    let mut core = mgba::core::Core::new_gba(
        "tango",
        &mgba::core::Options {
            audio_sync: true,
            ..Default::default()
        },
    )?;
    core.enable_video_buffer();
    core.as_mut().load_rom(mgba::vfile::VFile::from_vec(rom.to_vec()))?;
    Ok(core)
}

pub struct SinglePlayerSession {
    joyflags: Arc<AtomicU32>,
    vbuf: Arc<Mutex<Vec<u8>>>,
    new_frame: Arc<AtomicBool>,
    _audio_binding: Option<crate::audio::Binding>,
    _thread: mgba::thread::Thread,
}

impl SinglePlayerSession {
    pub fn new(
        rom: &[u8],
        save_path: &std::path::Path,
        audio_binder: &crate::audio::LateBinder,
    ) -> anyhow::Result<Self> {
        let mut core = new_gba_core(rom)?;
        // Open RW so the game's own save writes persist back to disk —
        // mgba memory-maps the file and treats it as the cartridge SRAM.
        let save_file = std::fs::OpenOptions::new().read(true).write(true).open(save_path)?;
        core.as_mut().load_save(mgba::vfile::VFile::from_file(save_file))?;

        let joyflags = Arc::new(AtomicU32::new(0));
        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            SCREEN_WIDTH as usize * SCREEN_HEIGHT as usize * 2
        ]));
        let new_frame = Arc::new(AtomicBool::new(false));
        let thread = mgba::thread::Thread::new(core);

        thread.set_frame_callback({
            let vbuf = vbuf.clone();
            let joyflags = joyflags.clone();
            let new_frame = new_frame.clone();
            move |mut core, video_buffer, _thread_handle| {
                // Copy mgba's native BGR555 straight through; the UI side
                // expands it to RGBA8 when it collects the frame.
                vbuf.lock().unwrap().copy_from_slice(video_buffer);
                core.set_keys(joyflags.load(Ordering::Relaxed));
                new_frame.store(true, Ordering::Release);
            }
        });

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        let audio_binding = audio_binder.bind_mgba(thread.handle(), "singleplayer");

        Ok(Self {
            joyflags,
            vbuf,
            new_frame,
            _audio_binding: audio_binding,
            _thread: thread,
        })
    }

    /// Overwrite the entire mgba joyflag bitmap — the input mapping
    /// resolves multiple held bindings into one flag word and pushes
    /// the result here every event.
    pub fn set_joyflags(&self, mgba_keys: u32) {
        self.joyflags.store(mgba_keys, Ordering::Relaxed);
    }

    /// Drive the emulator at `factor * EXPECTED_FPS` fps. 1.0 = realtime,
    /// anything higher = fast-forward. Audio paces frames, so values
    /// above ~4x start dropping samples; clamp accordingly to keep audio
    /// coherent.
    pub fn set_speed(&self, factor: f32) {
        let fps = (EXPECTED_FPS * factor).clamp(1.0, EXPECTED_FPS * 4.0);
        self._thread.handle().lock_audio().sync_mut().set_fps_target(fps);
    }

    /// True (and self-clearing) when a frame arrived since the last call.
    pub fn frame_dirty(&self) -> bool {
        self.new_frame.swap(false, Ordering::AcqRel)
    }

    /// Convert the most recent frame to RGBA8 into `dst_rgba`
    /// (`SCREEN_WIDTH * SCREEN_HEIGHT * 4` bytes).
    pub fn read_frame(&self, dst_rgba: &mut [u8]) {
        let vbuf = self.vbuf.lock().unwrap();
        tango_dataview::rom::bgr555_to_rgba8(&vbuf, dst_rgba);
    }
}

pub struct ReplaySession {
    stepper_state: tango_pvp::stepper::State,
    snapshots: tango_pvp::replay::playback::SnapshotStore,
    rewind: tango_pvp::replay::playback::RewindBuffer,
    prefetch_progress: Arc<AtomicU32>,
    round_boundaries: Vec<u32>,
    total_ticks: u32,
    completion: tango_pvp::hooks::CompletionToken,
    vbuf: Arc<Mutex<Vec<u8>>>,
    new_frame: Arc<AtomicBool>,
    seek: Arc<tango_pvp::replay::playback::SeekController>,
    // Drop order (declaration order): audio binding reverts the mux to
    // silence, prefetcher and seek worker cancel+join their threads —
    // all before the mgba thread (which their closures drive) tears down.
    _audio_binding: Option<crate::audio::Binding>,
    _prefetcher: Prefetcher,
    _seek_worker: SeekWorker,
    _thread: mgba::thread::Thread,
}

impl ReplaySession {
    pub fn new(
        replay: tango_pvp::replay::Replay,
        local_game: tango_gamesupport::GameRef,
        local_rom: Vec<u8>,
        remote_game: tango_gamesupport::GameRef,
        remote_rom: Vec<u8>,
        audio_binder: &crate::audio::LateBinder,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(!replay.rounds.is_empty(), "replay has no rounds");

        let mut core = new_gba_core(&local_rom)?;
        // In-memory SRAM clone — playback must never touch a real save.
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(replay.local_sram.clone()))?;
        // Pin cart RTC to the recorded match clock (before the thread
        // starts/resets); RTC-reading games desync without it.
        core.set_rtc_fixed(replay.rtc_time());

        let completion = tango_pvp::hooks::CompletionToken::new();
        let replay_is_complete = replay.is_complete;
        let total_ticks: u32 = replay.rounds.iter().map(|r| r.len() as u32).sum();
        // Inter-round marks on the seek bar's coordinate: running sum of
        // round lengths, all but the last round.
        let round_boundaries = replay
            .rounds
            .iter()
            .take(replay.rounds.len().saturating_sub(1))
            .scan(0u32, |acc, r| {
                *acc += r.len() as u32;
                Some(*acc)
            })
            .collect::<Vec<_>>();

        let (stepper_state, shadow) = tango_pvp::stepper::State::new_for_replay(
            &replay,
            &remote_rom,
            remote_game.hooks,
            Box::new({
                let completion = completion.clone();
                move || completion.complete()
            }),
        )?;
        local_game.hooks.install_on_stepper(&mut core, stepper_state.clone());

        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            SCREEN_WIDTH as usize * SCREEN_HEIGHT as usize * 2
        ]));
        let new_frame = Arc::new(AtomicBool::new(false));
        let thread = mgba::thread::Thread::new(core);

        let replay = Arc::new(replay);
        let snapshots = tango_pvp::replay::playback::SnapshotStore::new();
        let rewind = tango_pvp::replay::playback::RewindBuffer::new();
        let prefetch_progress = Arc::new(AtomicU32::new(0));
        let prefetcher = Prefetcher::spawn(
            Arc::new(local_rom),
            Arc::new(remote_rom),
            replay.clone(),
            local_game,
            remote_game,
            snapshots.clone(),
            prefetch_progress.clone(),
        );
        let seek = Arc::new(tango_pvp::replay::playback::SeekController::new());

        thread.set_frame_callback({
            let vbuf = vbuf.clone();
            let new_frame = new_frame.clone();
            let stepper_state = stepper_state.clone();
            let completion = completion.clone();
            let snapshots = snapshots.clone();
            let rewind = rewind.clone();
            let shadow = shadow.clone();
            let seek = seek.clone();
            move |mut core, video_buffer, mut thread_handle| {
                let (inputs_consumed, total_left, is_round_ended) = {
                    let mut inner = stepper_state.lock_inner();
                    if let Some(err) = inner.take_error() {
                        log::error!("replay stepper error: {err:?}");
                    }
                    (
                        inner.inputs_consumed(),
                        inner.total_input_pairs_left(),
                        inner.is_round_ended(),
                    )
                };

                // During a seek chase only the landing frame reaches the
                // display; publishing every catch-up frame would strobe.
                if seek.should_publish_frame(inputs_consumed) {
                    vbuf.lock().unwrap().copy_from_slice(video_buffer);
                    new_frame.store(true, Ordering::Release);
                }

                // Capture every frame into the rewind window; lift the
                // sparse keyframes out of the same capture.
                if let Some(cp) = stepper_state.capture_replay_checkpoint() {
                    let keyframe_needed = snapshots.snapshot_needed(&cp);
                    if let Some(snap) = rewind.capture(cp, &mut core, &shadow, video_buffer) {
                        if keyframe_needed {
                            snapshots.push_arc(snap);
                        }
                    }
                }

                // Clean replays wait for the round-end animation; incomplete
                // ones fall through on input exhaustion.
                if total_left == 0 && (is_round_ended || !replay_is_complete) {
                    completion.complete();
                }
                if completion.is_complete() {
                    thread_handle.pause();
                }
            }
        });

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        let seek_worker = SeekWorker::spawn(
            thread.handle(),
            seek.clone(),
            stepper_state.clone(),
            shadow,
            replay,
            snapshots.clone(),
            rewind.clone(),
            completion.clone(),
            {
                // Zero-frame seek landings (exact snapshot hits) never run
                // a frame, so the frame callback can't publish them — blit
                // the snapshot's stored framebuffer instead.
                let vbuf = vbuf.clone();
                let new_frame = new_frame.clone();
                move |snap: &tango_pvp::stepper::ReplaySnapshot| {
                    {
                        let mut vbuf = vbuf.lock().unwrap();
                        if vbuf.len() != snap.framebuffer.len() {
                            return;
                        }
                        vbuf.copy_from_slice(&snap.framebuffer);
                    }
                    new_frame.store(true, Ordering::Release);
                }
            },
        );

        let audio_binding = audio_binder.bind_mgba(thread.handle(), "replay");

        Ok(Self {
            stepper_state,
            snapshots,
            rewind,
            prefetch_progress,
            round_boundaries,
            total_ticks,
            completion,
            vbuf,
            new_frame,
            seek,
            _audio_binding: audio_binding,
            _prefetcher: prefetcher,
            _seek_worker: seek_worker,
            _thread: thread,
        })
    }

    pub fn is_paused(&self) -> bool {
        self._thread.handle().is_paused()
    }

    pub fn set_paused(&self, paused: bool) {
        let handle = self._thread.handle();
        if paused {
            handle.pause();
        } else {
            handle.unpause();
        }
    }

    pub fn set_speed(&self, factor: f32) {
        let fps = (EXPECTED_FPS * factor).clamp(1.0, EXPECTED_FPS * 4.0);
        self._thread.handle().lock_audio().sync_mut().set_fps_target(fps);
    }

    /// Recorded-frame index of the playhead (freezes during the
    /// input-less inter-round animation).
    pub fn current_tick(&self) -> u32 {
        self.stepper_state.lock_inner().inputs_consumed()
    }

    pub fn total_ticks(&self) -> u32 {
        self.total_ticks
    }

    pub fn is_complete(&self) -> bool {
        self.completion.is_complete()
    }

    pub fn frame_dirty(&self) -> bool {
        self.new_frame.swap(false, Ordering::AcqRel)
    }

    pub fn read_frame(&self, dst_rgba: &mut [u8]) {
        let vbuf = self.vbuf.lock().unwrap();
        tango_dataview::rom::bgr555_to_rgba8(&vbuf, dst_rgba);
    }

    /// Highest tick the background prefetcher has buffered (scrub-bar
    /// fill overlay); reaches `total_ticks` when done.
    pub fn prefetch_progress(&self) -> u32 {
        self.prefetch_progress.load(Ordering::Relaxed)
    }

    /// Recorded-frame index of each inter-round transition — the marks
    /// the scrubber draws. Empty for a single-round replay.
    pub fn round_boundaries(&self) -> &[u32] {
        &self.round_boundaries
    }

    /// Jump the playhead to `target`, asynchronously. Newer requests
    /// supersede in-flight ones mid-chase. With `resume_after`, playback
    /// unpauses once the chase lands.
    pub fn seek_to(&self, target: u32, resume_after: bool) {
        self.seek.request(target.min(self.total_ticks), resume_after);
    }

    /// Target of the in-flight seek, if any — lets the UI draw the
    /// playhead where it's headed.
    pub fn pending_seek_target(&self) -> Option<u32> {
        self.seek.pending_target()
    }

    /// True while an in-flight seek will unpause playback on landing.
    pub fn seek_will_resume(&self) -> bool {
        self.seek.resume_pending()
    }

    /// Blit the captured framebuffer of the snapshot nearest `target`
    /// into the display buffer — instant scrub-drag feedback. Unless
    /// `force_keyframe`, skipped while the playhead's own frame is at
    /// least as close. Returns whether a blit happened.
    pub fn scrub_preview(&self, target: u32, force_keyframe: bool) -> bool {
        let Some(snap) = self.nearest_snapshot(target) else {
            return false;
        };
        if !force_keyframe {
            let cur = self.stepper_state.lock_inner().inputs_consumed();
            if cur.abs_diff(target) <= snap.checkpoint.frame_index.abs_diff(target) {
                return false;
            }
        }
        self.blit_snapshot(&snap)
    }

    /// The nearest captured frame to `target` as RGBA, for the
    /// scrubber's hover thumbnail. `None` before anything is captured.
    pub fn snapshot_rgba(&self, target: u32) -> Option<(u32, u32, Vec<u8>)> {
        let snap = self.nearest_snapshot(target)?;
        let expected = SCREEN_WIDTH as usize * SCREEN_HEIGHT as usize * 2;
        if snap.framebuffer.len() != expected {
            return None;
        }
        let mut rgba = vec![0u8; SCREEN_WIDTH as usize * SCREEN_HEIGHT as usize * 4];
        tango_dataview::rom::bgr555_to_rgba8(&snap.framebuffer, &mut rgba);
        Some((SCREEN_WIDTH, SCREEN_HEIGHT, rgba))
    }

    fn nearest_snapshot(&self, target: u32) -> Option<Arc<tango_pvp::stepper::ReplaySnapshot>> {
        [self.snapshots.nearest(target), self.rewind.nearest(target)]
            .into_iter()
            .flatten()
            .min_by_key(|s| s.checkpoint.frame_index.abs_diff(target))
    }

    fn blit_snapshot(&self, snap: &tango_pvp::stepper::ReplaySnapshot) -> bool {
        {
            let mut vbuf = self.vbuf.lock().unwrap();
            if vbuf.len() != snap.framebuffer.len() {
                return false;
            }
            vbuf.copy_from_slice(&snap.framebuffer);
        }
        self.new_frame.store(true, Ordering::Release);
        true
    }
}

/// Owns the seek worker thread driving `SeekController` requests against
/// the playback core. Drop cancels the controller (aborting any in-flight
/// chase at its next frame boundary) and joins.
struct SeekWorker {
    ctrl: Arc<tango_pvp::replay::playback::SeekController>,
    join_handle: Option<std::thread::JoinHandle<()>>,
}

impl SeekWorker {
    #[allow(clippy::too_many_arguments)]
    fn spawn(
        handle: mgba::thread::Handle,
        ctrl: Arc<tango_pvp::replay::playback::SeekController>,
        stepper_state: tango_pvp::stepper::State,
        shadow: tango_pvp::stepper::SharedShadow,
        replay: Arc<tango_pvp::replay::Replay>,
        snapshots: tango_pvp::replay::playback::SnapshotStore,
        rewind: tango_pvp::replay::playback::RewindBuffer,
        completion_token: tango_pvp::hooks::CompletionToken,
        publish_landing: impl Fn(&tango_pvp::stepper::ReplaySnapshot) + Send + Sync + 'static,
    ) -> Self {
        let join_handle = std::thread::spawn({
            let ctrl = ctrl.clone();
            move || {
                tango_pvp::replay::playback::run_seek_worker(
                    handle,
                    ctrl,
                    stepper_state,
                    shadow,
                    replay,
                    snapshots,
                    rewind,
                    completion_token,
                    publish_landing,
                );
            }
        });
        Self {
            ctrl,
            join_handle: Some(join_handle),
        }
    }
}

impl Drop for SeekWorker {
    fn drop(&mut self) {
        self.ctrl.shutdown();
        if let Some(h) = self.join_handle.take() {
            let _ = h.join();
        }
    }
}

/// Background snapshot-prefetch worker: a fresh core + shadow racing
/// ahead of the playhead, pushing keyframes into the shared store so
/// backward (and long-forward) seeks have a nearby jumping-off point.
/// Drop cancels the worker and joins.
struct Prefetcher {
    cancel: Arc<AtomicBool>,
    join_handle: Option<std::thread::JoinHandle<()>>,
}

impl Prefetcher {
    fn spawn(
        rom: Arc<Vec<u8>>,
        remote_rom: Arc<Vec<u8>>,
        replay: Arc<tango_pvp::replay::Replay>,
        game: tango_gamesupport::GameRef,
        remote_game: tango_gamesupport::GameRef,
        snapshots: tango_pvp::replay::playback::SnapshotStore,
        progress: Arc<AtomicU32>,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = cancel.clone();
        let hooks = game.hooks;
        let remote_hooks = remote_game.hooks;
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

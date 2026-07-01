//! Replay playback session.
//!
//! Owns an mgba playback thread, installs the per-game stepper traps,
//! pushes captured snapshots into a [`SnapshotStore`] each frame, and
//! runs a background [`Prefetcher`] thread that races ahead of the
//! playhead to keep that store populated for seeks. A [`RewindBuffer`]
//! additionally keeps every frame of the last ~1.5s behind the
//! playhead, so single-frame backward steps land on exact snapshots
//! with no catch-up emulation. Seeks are asynchronous: requests land
//! on a [`SeekController`] and a dedicated [`SeekWorker`] thread
//! chases the newest target on the mgba thread, so the UI never
//! blocks on catch-up emulation. Audio is bound via the shared
//! [`crate::audio::LateBinder`].

pub mod scrubber;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use tango_pvp::replay::playback::{RewindBuffer, SeekController, SnapshotStore};
use tango_pvp::shadow::Shadow;

pub const SCREEN_WIDTH: u32 = mgba::gba::SCREEN_WIDTH;
pub const SCREEN_HEIGHT: u32 = mgba::gba::SCREEN_HEIGHT;
const EXPECTED_FPS: f32 = 60.0;

/// Scrub-bar interaction state for a replay session. Splits the
/// drag/hover bookkeeping out of [`crate::session::State`] (which is
/// otherwise game-mode-agnostic) and keeps it next to the
/// [`ReplaySession`] it drives. The owning state holds one of these
/// and the transport widget reads it to draw the playhead + the
/// floating keyframe thumbnail.
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
    /// RGBA conversion of the snapshot behind the hover thumbnail,
    /// keyed by the snapshot's absolute tick so cursor moves within
    /// the same keyframe reuse the handle instead of re-converting.
    pub thumb: Option<(u32, iced::widget::image::Handle)>,
}

impl Scrub {
    /// Begin or continue a drag at `target`. The first event of a drag
    /// freezes playback under the cursor (remembering whether to
    /// resume) and starts blitting previews from the snapshot buffers.
    pub fn drag(&mut self, target: u32, replay: &ReplaySession) {
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
    pub fn refresh_thumb(&mut self, replay: &ReplaySession) {
        let Some(h) = self.hover else { return };
        if let Some(snap) = replay.nearest_snapshot(h.tick) {
            let snap_tick = snap.checkpoint.absolute_tick;
            if self.thumb.as_ref().map(|(t, _)| *t) != Some(snap_tick) {
                self.thumb = Some((snap_tick, super::thumbnail_handle(&snap.framebuffer)));
            }
        }
    }

    /// Drop all scrub state, drag and hover alike — used when the
    /// session closes.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

pub struct ReplaySession {
    game: &'static crate::game::Game,
    close_requested: Arc<AtomicBool>,
    stepper_state: tango_pvp::stepper::State,
    snapshots: SnapshotStore,
    /// Dense per-frame snapshot window trailing the playhead (see
    /// [`RewindBuffer`]); backward steps land on it exactly.
    rewind: RewindBuffer,
    prefetch_progress: Arc<AtomicU32>,
    /// Inter-round seek-bar marks: cumulative recorded input-pair counts,
    /// computed once at construction (see [`Self::round_boundaries`]).
    round_boundaries: Vec<u32>,
    total_ticks: u32,
    /// Shared display framebuffer + its wake handle, kept so
    /// [`Self::scrub_preview`] can blit snapshot framebuffers without
    /// going through the emulator at all.
    vbuf: Arc<Mutex<Vec<u8>>>,
    frame_notify: Arc<tokio::sync::Notify>,
    seek: Arc<SeekController>,
    /// Held so the audio binding survives for the session's lifetime;
    /// the LateBinder swaps back to silence when this Drops.
    _audio_binding: Option<crate::audio::Binding>,
    /// Field order matters — `_prefetcher`'s and `_seek_worker`'s Drops
    /// signal cancel and join their background threads before `thread`
    /// is torn down. All three come last so the frame callback and any
    /// in-flight seek chase (both running on `thread`) are dead by the
    /// time the earlier fields are freed.
    _prefetcher: Prefetcher,
    _seek_worker: SeekWorker,
    thread: mgba::thread::Thread,
}

impl ReplaySession {
    pub fn new(
        game: &'static crate::game::Game,
        rom: Arc<Vec<u8>>,
        remote_game: &'static crate::game::Game,
        remote_rom: Arc<Vec<u8>>,
        replay: Arc<tango_pvp::replay::Replay>,
        audio_binder: &crate::audio::LateBinder,
        frame_notify: Arc<tokio::sync::Notify>,
        vbuf: Arc<Mutex<Vec<u8>>>,
    ) -> anyhow::Result<Self> {
        let mut core = crate::session::new_gba_core(rom.as_ref())?;
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(replay.local_sram_dump()))?;

        let hooks = game.hooks;

        let completion_token = tango_pvp::hooks::CompletionToken::new();
        if replay.rounds.is_empty() {
            anyhow::bail!("replay has no rounds");
        }
        let replay_is_complete = replay.is_complete;
        let total_ticks = replay.rounds.iter().map(|r| r.len() as u32).sum::<u32>();

        let (stepper_state, shadow) = tango_pvp::stepper::State::new_for_replay(
            &replay,
            remote_rom.as_ref(),
            remote_game.hooks,
            Box::new({
                let completion_token = completion_token.clone();
                move || completion_token.complete()
            }),
        )?;

        hooks.install_on_stepper(&mut core, stepper_state.clone());

        let thread = mgba::thread::Thread::new(core);
        // Wipe the shared framebuffer so the previous session's
        // last frame doesn't flash through before mgba writes its
        // first one.
        vbuf.lock().unwrap().fill(0);

        // Inter-round marks live on the seek bar's coordinate (cumulative
        // inputs consumed = recorded-frame index), so they're just the
        // running sum of round lengths, exact and known up front — no
        // emulation needed. All but the last round; empty for one round.
        let round_boundaries = replay
            .rounds
            .iter()
            .take(replay.rounds.len().saturating_sub(1))
            .scan(0u32, |acc, r| {
                *acc += r.len() as u32;
                Some(*acc)
            })
            .collect::<Vec<_>>();

        let snapshots = SnapshotStore::new();
        let rewind = RewindBuffer::new();
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

        let seek = Arc::new(SeekController::new());

        thread.set_frame_callback({
            let vbuf = vbuf.clone();
            let completion_token = completion_token.clone();
            let stepper_state = stepper_state.clone();
            let snapshots = snapshots.clone();
            let rewind = rewind.clone();
            let shadow = shadow.clone();
            let frame_notify = frame_notify.clone();
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
                // display; publishing every intermediate catch-up frame
                // would strobe a fast-forward of everything in between.
                if seek.should_publish_frame(inputs_consumed) {
                    // Copy mgba's native BGR555 straight through; the framebuffer
                    // shader expands it to RGB on the GPU at draw time.
                    vbuf.lock().unwrap().copy_from_slice(video_buffer);
                    // the texture handle for this frame. See
                    // `singleplayer_session` for rationale.
                    frame_notify.notify_one();
                }

                // Capture every frame into the rewind window so backward
                // steps land exactly, and lift the sparse keyframes
                // (round starts + every MID_ROUND_SNAPSHOT_INTERVAL) out
                // of the same capture so those frames don't pay a second
                // save_state.
                if let Some(cp) = stepper_state.capture_replay_checkpoint() {
                    let keyframe_needed = snapshots.snapshot_needed(&cp);
                    if let Some(snap) = rewind.capture(cp, &mut core, &shadow, video_buffer) {
                        if keyframe_needed {
                            snapshots.push_arc(snap);
                        }
                    }
                }

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

        let seek_worker = SeekWorker::spawn(
            thread.handle(),
            seek.clone(),
            stepper_state.clone(),
            shadow.clone(),
            replay.clone(),
            snapshots.clone(),
            rewind.clone(),
            completion_token.clone(),
            {
                // Zero-frame seek landings (exact snapshot hits) never run
                // a frame, so the frame callback can't publish them — the
                // chase blits the snapshot's stored framebuffer instead.
                let vbuf = vbuf.clone();
                let frame_notify = frame_notify.clone();
                move |snap: &tango_pvp::stepper::ReplaySnapshot| {
                    {
                        let mut vbuf = vbuf.lock().unwrap();
                        if vbuf.len() != snap.framebuffer.len() {
                            return;
                        }
                        vbuf.copy_from_slice(&snap.framebuffer);
                    }
                    frame_notify.notify_one();
                }
            },
        );

        let audio_binding = audio_binder.bind_mgba(thread.handle(), "replay");

        Ok(Self {
            game,
            close_requested: Arc::new(AtomicBool::new(false)),
            stepper_state,
            snapshots,
            rewind,
            prefetch_progress,
            round_boundaries,
            total_ticks,
            vbuf,
            frame_notify,
            seek,
            _audio_binding: audio_binding,
            _prefetcher: prefetcher,
            _seek_worker: seek_worker,
            thread,
        })
    }

    pub fn game(&self) -> &'static crate::game::Game {
        self.game
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
        self.close_requested.store(true, Ordering::Release);
    }

    /// Playhead position on the seek bar: the recorded-frame index =
    /// cumulative input pairs consumed. Freezes during the input-less
    /// inter-round animation (so it rests on the round mark while it
    /// plays), and reaches `total_ticks` exactly when the replay finishes.
    pub fn current_tick(&self) -> u32 {
        self.stepper_state.lock_inner().inputs_consumed()
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

    /// Recorded-frame index of each inter-round transition — the marks the
    /// scrubber draws. These sit on the same scale as the playhead
    /// ([`Self::current_tick`]), so a mark coincides exactly with the
    /// playhead as it crosses, with no emulation needed: the seek bar is
    /// indexed by inputs consumed, and a round boundary is just the running
    /// sum of round lengths. Empty for a single-round replay.
    pub fn round_boundaries(&self) -> Vec<u32> {
        self.round_boundaries.clone()
    }

    /// Jump the playhead to `target`, asynchronously. Records the request
    /// on the seek controller and returns immediately; the seek worker
    /// runs the snapshot load + frame catch-up on the mgba thread, and
    /// newer requests supersede in-flight ones mid-chase. With
    /// `resume_after`, playback unpauses once the chase lands (unless a
    /// newer request took over) — used by scrub commits, which pause
    /// playback for the duration of the drag.
    pub fn seek_to(&self, target: u32, resume_after: bool) {
        self.seek.request(target.min(self.total_ticks), resume_after);
    }

    /// Target of the in-flight seek, if any — lets the UI draw the
    /// playhead where it's headed instead of snapping back to the
    /// pre-seek tick until the chase lands.
    pub fn pending_seek_target(&self) -> Option<u32> {
        self.seek.pending_target()
    }

    /// True while an in-flight seek will unpause playback on landing.
    /// The thread is paused for the chase's duration, but the session
    /// is logically still playing — the transport shouldn't flip to
    /// the paused state.
    pub fn seek_will_resume(&self) -> bool {
        self.seek.resume_pending()
    }

    /// Withdraw an in-flight seek's pending resume, keeping playback
    /// paused once it lands.
    pub fn cancel_seek_resume(&self) {
        self.seek.clear_resume();
    }

    /// The captured snapshot nearest `target`, if any — backs the hover
    /// thumbnail above the scrub bar and the drag preview blit. Near the
    /// playhead the rewind window supplies exact frames; elsewhere it's
    /// the store's keyframes. The snapshot's framebuffer is mgba-native
    /// BGR555, same as the shared display buffer.
    pub fn nearest_snapshot(&self, target: u32) -> Option<std::sync::Arc<tango_pvp::stepper::ReplaySnapshot>> {
        [self.snapshots.nearest(target), self.rewind.nearest(target)]
            .into_iter()
            .flatten()
            .min_by_key(|s| s.checkpoint.frame_index.abs_diff(target))
    }

    /// Blit the captured framebuffer of the snapshot nearest `target`
    /// straight into the shared display buffer — instant, emulation-free
    /// feedback while the user drags the scrubber. The exact landing
    /// happens on release via [`Self::seek_to`].
    ///
    /// Unless `force_keyframe`, the blit is skipped while the playhead's
    /// own (exact) frame is at least as close to `target` as the nearest
    /// snapshot — every drag starts by pressing on the handle, and
    /// jumping the display to a keyframe seconds away would glitch.
    /// Once a drag has swapped to keyframes the live frame is no longer
    /// in the buffer, so callers pass `force_keyframe` from then on.
    /// Returns whether a blit happened.
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

    /// Blit the frame at exactly `target`, if a snapshot holds it —
    /// the scrub *press*'s preview. Unlike [`Self::scrub_preview`] this
    /// never substitutes a nearby keyframe: a click seeks to whatever
    /// tick is under the cursor, and blitting the nearest keyframe
    /// there would flash a wrong frame for the chase's duration before
    /// snapping to the real one. Returns whether a blit happened.
    pub fn scrub_preview_exact(&self, target: u32) -> bool {
        match self.nearest_snapshot(target) {
            Some(snap) if snap.checkpoint.frame_index == target => self.blit_snapshot(&snap),
            _ => false,
        }
    }

    /// Copy `snap`'s stored framebuffer into the shared display buffer
    /// and wake the renderer. False if the buffer sizes disagree.
    fn blit_snapshot(&self, snap: &tango_pvp::stepper::ReplaySnapshot) -> bool {
        {
            let mut vbuf = self.vbuf.lock().unwrap();
            if vbuf.len() != snap.framebuffer.len() {
                return false;
            }
            vbuf.copy_from_slice(&snap.framebuffer);
        }
        self.frame_notify.notify_one();
        true
    }
}

/// Owns the seek worker thread driving [`SeekController`] requests
/// against the playback core. The worker — not the requester — eats the
/// blocking `run_on_core` round-trip per chase.
///
/// Drop cancels the controller (aborting any in-flight chase at its
/// next frame boundary) and joins.
struct SeekWorker {
    ctrl: Arc<SeekController>,
    join_handle: Option<std::thread::JoinHandle<()>>,
}

impl SeekWorker {
    fn spawn(
        handle: mgba::thread::Handle,
        ctrl: Arc<SeekController>,
        stepper_state: tango_pvp::stepper::State,
        shadow: Arc<Mutex<Shadow>>,
        replay: Arc<tango_pvp::replay::Replay>,
        snapshots: SnapshotStore,
        rewind: RewindBuffer,
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
        game: &'static crate::game::Game,
        remote_game: &'static crate::game::Game,
        snapshots: SnapshotStore,
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

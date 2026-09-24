//! Replay playback session: a recording re-simulated on the game's own
//! engine ([`tango_match::Replay`]) behind a mutex, paced by a drive
//! thread, with a second pass racing ahead of the playhead for the
//! match statistics and the keyframes that make seeking cheap.
//!
//! Seeks are asynchronous: requests land on a
//! [`SeekController`](tango_match::seek::SeekController) and a
//! dedicated worker chases the newest target a slice at a time, so the
//! UI never blocks on catch-up emulation. What the player keeps to
//! serve those chases — keyframes, a rewind ring — is the player's
//! business; this session only says where to go and how much time it
//! may take getting there.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use tango_match::seek::SeekController;

use crate::local::Surfaces;

mod engine;
mod speed;
mod workers;
pub use engine::{analyze, ConfigError, EngineReplay};
use speed::SpeedControl;
use workers::Playhead;
pub use workers::{DriveWorker, Driver, PrefetchWorker, SeekWorker, Workers};

/// What the input display overlay reads off a replay: every recorded
/// (local, remote) joyflags pair, flattened across rounds in playhead
/// order (index = the tick that consumed it) and masked to the
/// hardware bits, plus the two sides' nicknames for the chip captions.
struct InputDisplay {
    pairs: Vec<(u16, u16)>,
    /// Recorded (local, remote) touch positions per tick, in the touch
    /// screen's own pixels. Empty — not all-`None` — for a stream with
    /// no touches anywhere (every GBA replay), so the common case
    /// costs nothing.
    touches: Vec<(Option<(u16, u16)>, Option<(u16, u16)>)>,
    nicknames: (String, String),
}

pub struct ReplaySession {
    game: &'static tango_gamesupport::Game,
    /// Header carried by the recording. Playback itself only needs a
    /// handful of these fields, but the host's viewer uses the full header
    /// to identify the replay without reopening the file or reaching back
    /// into its library index.
    metadata: tango_replay::Metadata,
    /// Inter-round seek-bar marks (see [`Self::round_boundaries`]):
    /// either handed in by a host that already had the recording's
    /// analysis, or discovered from telemetry by the prefetch pass as it
    /// runs.
    round_boundaries: Arc<Mutex<Vec<u32>>>,
    total_ticks: u32,
    /// Input display lookup data ([`Self::input_at`] /
    /// [`Self::nicknames`]). Boxed to keep this struct — and with it
    /// the `Session` enum — small, same as the PvP variant.
    input_display: Box<InputDisplay>,
    /// The console's screens, as the local game's engine presents them
    /// — what the surfaces below are sized for.
    layout: tango_match::ScreenLayout,
    /// This session's display, kept so [`Self::scrub_preview`] can blit
    /// snapshot framebuffers without going through the emulator at all.
    /// The PiP (a per-session toggle on the transport bar) carries the
    /// opponent's screen.
    surfaces: Surfaces,
    /// Whether the main screen shows the opponent's perspective instead
    /// of the local one — a per-session toggle on the transport bar. The
    /// PiP, when also on, carries the local screen so the two surfaces
    /// always show both sides.
    swap_perspective: Arc<AtomicBool>,
    /// The selected transport rate plus the custom-screen override that
    /// derives the effective drive/audio rate from it.
    speed: Arc<SpeedControl>,
    /// The playback machinery (pair, workers, seek state).
    engine: Engine,
}

/// Playback: the recording's pair behind a mutex, paced by a host
/// drive thread; the seek worker chases targets through the engine's
/// own captures, and the prefetch worker races a second simulation
/// ahead for keyframes, statistics and round marks.
struct Engine {
    /// Which pair core is the replay's local perspective.
    local_player: usize,
    /// Lock-free playhead mirror for UI reads.
    cursor: Arc<AtomicU32>,
    paused: Arc<crate::PauseGate>,
    /// The recording as the game's engine offers it: the pair below and
    /// the statistics pass share the captures they lay down.
    set: Arc<tango_match::ReplaySet>,
    playback: SharedPlayback,
    /// How far the stats pass has run, mirrored per slice by the
    /// prefetch worker for UI reads.
    prefetch_progress: Arc<AtomicU32>,
    seek: Arc<SeekController>,
    /// Cancels the loops the host is running, on Drop; whoever is
    /// running them notices on their next tick (and a host with threads
    /// joins them afterwards).
    cancel: Arc<AtomicBool>,
    /// Set once the display pair is up. The session exists from the
    /// moment it is built, but its pair is booted and primed on the
    /// drive loop's first tick — seconds of emulation on a DS-class
    /// game — and there is nothing to show until then, which is what
    /// [`is_booting`](ReplaySession::is_booting) lets the host say.
    booted: Arc<AtomicBool>,
    /// Why that boot failed, if it did: a recording whose games can't
    /// be walked back into their battle plays nothing at all, and the
    /// session stays up on a black frame with only this to show for it
    /// ([`prime_error`](ReplaySession::prime_error)).
    prime_error: Arc<Mutex<Option<tango_match::Error>>>,
}

type SharedPlayback = Arc<Mutex<Option<tango_match::Replay>>>;

impl Drop for Engine {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        // The engine's own flag, checked per tick deep inside the stats
        // pass and the priming walks (and it wakes a stats pass parked
        // on the display boot) — without it a host joining its workers
        // waits out whatever slice or boot is in flight.
        self.set.cancel();
        // Release a gate-parked drive loop so the host's join is prompt.
        self.paused.set(false);
        self.seek.shutdown();
    }
}

impl ReplaySession {
    /// Build a playback session for an SIO-engine replay
    /// ([`tango_replay::VERSION`]): one continuous run of pair
    /// ticks, re-simulated on a linearly-driven pair. Both sides must
    /// have replay support from their engine. Returns
    /// immediately — boot + priming (a second or two) happens on the
    /// drive thread, with a black frame and silence until it's up.
    /// Also returns the session's audio stream (the shown perspective's
    /// core at `sample_rate`, following the drive loop's pacing) for the
    /// host to route to its output.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        games: [&'static tango_gamesupport::Game; 2],
        roms: [Vec<u8>; 2],
        replay: Arc<tango_replay::Replay>,
        sample_rate: u32,
        show_pip: bool,
        // Whether the prefetch pass should also fold the match-stats
        // analysis, for a host that wants [`PrefetchWorker::preview`] and
        // [`PrefetchWorker::finished`].
        want_stats: bool,
        // The recording's round boundaries, when the host already had
        // its analysis (a stats sidecar) — the scrub bar draws them from
        // the first frame instead of waiting out a pass that would only
        // rediscover them. Empty otherwise, and the pass fills them in.
        round_boundaries: Vec<u32>,
    ) -> Result<(Self, Workers, crate::audio::Stream), crate::Error> {
        let mut engine = EngineReplay::new(games, roms, &replay)?;
        let local_player = engine.config.local_player;
        let total_ticks = engine.total_ticks();

        // The engine gets a head start on the two pairs playback runs
        // (display + the keyframe pass's). The pairs boot lazily from
        // the host's tick loop, so by then the event loop has turned —
        // which is what a browser engine's worker startup needs.
        engine.backend.prepare(4);

        let nickname_of =
            |side: Option<&tango_replay::metadata::Side>| side.map(|s| s.nickname.clone()).unwrap_or_default();
        let widen_touch = |touch: Option<(u8, u8)>| touch.map(|(x, y)| (x as u16, y as u16));
        let input_display = Box::new(InputDisplay {
            pairs: replay
                .inputs
                .iter()
                .map(|&row| {
                    (
                        row[local_player].keys & tango_match::keys::MASK as u16,
                        row[1 - local_player].keys & tango_match::keys::MASK as u16,
                    )
                })
                .collect(),
            touches: if replay.inputs.iter().any(|row| row.iter().any(|i| i.touch.is_some())) {
                replay
                    .inputs
                    .iter()
                    .map(|&row| {
                        (
                            widen_touch(row[local_player].touch),
                            widen_touch(row[1 - local_player].touch),
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            },
            nicknames: (nickname_of(replay.local_side()), nickname_of(replay.remote_side())),
        });

        // The mode the recording was played in, which the re-primed
        // pair below walks back into and the pane is shaped by.
        let match_type = engine.config.match_type;
        let layout = engine
            .backend
            .screen_layout(tango_match::SessionMode::PvP { match_type });
        let surfaces = Surfaces::new(&layout, show_pip);
        let playback: SharedPlayback = Arc::new(Mutex::new(None));
        let cursor = Arc::new(AtomicU32::new(0));
        let paused = Arc::new(crate::PauseGate::new(false));
        let speed = Arc::new(SpeedControl::new(crate::local::Pacing::new(games[local_player])));
        let prefetch_progress = Arc::new(AtomicU32::new(0));
        // Inter-round marks. The recording holds none — where the rounds
        // fall is what the games' telemetry says on re-simulation — so
        // either the host handed in a finished analysis' boundaries or
        // the prefetch pass publishes them as it reaches them.
        let discover_marks = round_boundaries.is_empty();
        let round_marks = Arc::new(Mutex::new(round_boundaries));
        let seek = Arc::new(SeekController::new());
        let cancel = Arc::new(AtomicBool::new(false));
        let booted = Arc::new(AtomicBool::new(false));
        let prime_error = Arc::new(Mutex::new(None));
        let swap_perspective = Arc::new(AtomicBool::new(false));
        // Which seat is on screen and in the speakers. Kept as a number
        // rather than derived at each use, because the engine's audio
        // pull reads it per fill.
        let shown_seat = Arc::new(AtomicUsize::new(local_player));

        // The recording as the local game's engine offers it. Nothing
        // is simulated yet: the display pair boots on the drive worker,
        // and the prefetch worker's pass either reuses its primed first
        // state (parking until it lands) or — on an engine that can't
        // hand over a bare pair — walks its own prime concurrently.
        // The fold is also where round boundaries come from, so a
        // session that doesn't know them yet wants it even with no
        // host asking for the rest.
        engine.config.want_stats = want_stats || discover_marks;
        let set: Arc<tango_match::ReplaySet> = Arc::new(engine.backend.open_replay(engine.config)?);
        // The session's audio ring, made before the pair that feeds it
        // exists: the host binds the stream at construction, and the
        // ring simply reads empty — so the stream primes — through the
        // priming walk the boot runs.
        let (audio_in, audio_out) = crate::audio::ring();

        let perspective = Perspective {
            shown_seat: shown_seat.clone(),
            surfaces: surfaces.clone(),
            swap_perspective: swap_perspective.clone(),
            local_player,
        };

        // Audio: play the shown perspective's core, following the drive
        // loop's pacing (see [`crate::audio::stream`]).
        let audio = speed.pacing().audio_stream(audio_out, sample_rate);

        // The three loops this session needs run. Which of them get
        // threads is the host's call: a desktop gives each one
        // ([`Workers::split`]), a browser ticks them in turn
        // ([`Workers::into_driver`]).
        let workers = Workers {
            drive: DriveWorker {
                playhead: Playhead {
                    set: set.clone(),
                    playback: playback.clone(),
                    cursor: cursor.clone(),
                    paused: paused.clone(),
                    cancel: cancel.clone(),
                    perspective: perspective.clone(),
                    audio: Mutex::new(Some(audio_in)),
                    seat: shown_seat.clone(),
                    booted: booted.clone(),
                    prime_error: prime_error.clone(),
                    speed: speed.clone(),
                },
                speed: speed.clone(),
                paused: paused.clone(),
                cancel: cancel.clone(),
                booted: false,
                backend: engine.backend,
            },
            seek: SeekWorker {
                seek: seek.clone(),
                playback: playback.clone(),
                cursor: cursor.clone(),
                paused: paused.clone(),
                perspective,
                speed: speed.clone(),
            },
            prefetch: PrefetchWorker {
                set: set.clone(),
                round_marks: discover_marks.then(|| round_marks.clone()),
                progress: prefetch_progress.clone(),
                cancel: cancel.clone(),
                pass: None,
                finished: None,
                done: false,
            },
        };

        let session = Self {
            game: games[local_player],
            metadata: replay.metadata.clone(),
            round_boundaries: round_marks,
            total_ticks,
            input_display,
            layout,
            surfaces,
            swap_perspective,
            speed,
            engine: Engine {
                local_player,
                cursor,
                paused,
                set,
                playback,
                prefetch_progress,
                seek,
                cancel,
                booted,
                prime_error,
            },
        };
        Ok((session, workers, audio))
    }

    /// Turn the auxiliary opponent surface on or off. While playing, it
    /// appears on the next published frame; on a paused replay it is
    /// re-blitted from the current frame's snapshot immediately. The host
    /// decides whether that surface is an inset or an equal second pane.
    pub fn set_opponent_visible(&self, visible: bool) {
        self.surfaces.set_pip_visible(visible);
        self.refresh_paused_frame();
    }

    /// Whether the main screen shows the opponent's perspective — drives
    /// the transport bar toggle's lit state.
    pub fn swap_perspective(&self) -> bool {
        self.swap_perspective.load(Ordering::Relaxed)
    }

    /// Swap which perspective the main screen shows. Takes effect on
    /// the next published frame while playing, immediately while paused
    /// — like the PiP.
    pub fn toggle_swap_perspective(&self) {
        self.swap_perspective.fetch_xor(true, Ordering::Relaxed);
        self.refresh_paused_frame();
    }

    /// Re-blit the current frame's snapshot so a perspective toggle
    /// takes effect immediately on a paused replay — the frame callback
    /// won't run to repaint the surfaces until playback resumes.
    /// Reading the shadow's live video buffer instead would be wrong
    /// here: after a zero-frame seek landing the shadow core has loaded
    /// state but never run, so its buffer still holds pre-seek pixels.
    ///
    /// Nearest-within-2 rather than exact: a pause can land after the
    /// next frame's input was consumed but before that frame completed
    /// and published, leaving the playhead one tick ahead of both the
    /// displayed frame and the last capture — an exact lookup misses
    /// there (and the surfaces would silently stay stale, which is the
    /// bug this fixes). The bound keeps a genuine miss (the pre-round
    /// boot window, where no snapshots exist) from jumping the paused
    /// screen to some distant keyframe; those toggles just wait for the
    /// next published frame as before.
    fn refresh_paused_frame(&self) {
        if !self.is_paused() {
            return;
        }
        let tick = self.current_tick();
        if let Some(snap) = self.nearest_snapshot(tick) {
            if snap.frame_index().abs_diff(tick) <= 2 {
                self.blit_snapshot(&snap);
            }
        }
    }

    pub fn is_paused(&self) -> bool {
        self.engine.paused.paused()
    }

    /// `true` until the display pair is booted and primed — the window
    /// between the session being built and its first frame, which is a
    /// priming walk long (seconds, on a DS-class game) and shows black
    /// and silent. Hosts put a notice over it; nothing else about the
    /// session is meaningful yet.
    pub fn is_booting(&self) -> bool {
        !self.engine.booted.load(Ordering::Acquire)
    }

    /// Why the boot failed, ready to show, or `None` while it is still
    /// running or has succeeded. A recording that can't be re-primed
    /// plays nothing at all, and this is the only thing left to tell
    /// the user about a session that will never show a frame.
    pub fn prime_error(&self) -> Option<String> {
        self.engine.prime_error.lock().unwrap().as_ref().map(|e| e.to_string())
    }

    /// The transport's selected base factor (1.0 = realtime). The
    /// effective rate may temporarily be higher while custom-screen
    /// fast-forwarding is active.
    pub fn speed(&self) -> f32 {
        self.speed.factor()
    }

    /// Whether the replay temporarily runs at least 2× while either
    /// player's custom screen is open.
    pub fn custom_screen_speedup(&self) -> bool {
        self.speed.custom_screen_speedup()
    }

    pub fn set_custom_screen_speedup(&self, enabled: bool) {
        self.speed.set_custom_screen_speedup(enabled);
    }

    /// Toggle playback between paused and running.
    pub fn set_paused(&self, paused: bool) {
        // Unpausing at end-of-stream is a no-op — the drive loop
        // re-pauses before running a frame.
        self.engine.paused.set(paused);
    }

    /// Playhead position on the seek bar: the recorded-frame index =
    /// cumulative input pairs consumed. Freezes during the input-less
    /// inter-round animation (so it rests on the round mark while it
    /// plays), and reaches `total_ticks` exactly when the replay finishes.
    pub fn current_tick(&self) -> u32 {
        self.engine.cursor.load(Ordering::Relaxed)
    }

    pub fn total_ticks(&self) -> u32 {
        self.total_ticks
    }

    /// The recorded (local, remote) joyflags behind the frame at
    /// `tick`. The playhead coordinate counts input pairs consumed,
    /// so the pair that produced tick `t` is index `t - 1`;
    /// all-released at tick 0, before anything has been consumed.
    /// While the playhead is frozen (the input-less inter-round
    /// animation), this holds the round's last pair.
    pub fn input_at(&self, tick: u32) -> (u16, u16) {
        tick.checked_sub(1)
            .and_then(|i| self.input_display.pairs.get(i as usize))
            .copied()
            .unwrap_or((0, 0))
    }

    /// The recorded (local, remote) touch positions behind the frame
    /// at `tick`, in the touch screen's own pixels — same playhead
    /// coordinate as [`Self::input_at`]. `None` for a side that wasn't
    /// touching, and always for a console with no touch screen.
    pub fn touch_at(&self, tick: u32) -> (Option<(u16, u16)>, Option<(u16, u16)>) {
        tick.checked_sub(1)
            .and_then(|i| self.input_display.touches.get(i as usize))
            .copied()
            .unwrap_or((None, None))
    }

    /// (local, remote) nicknames from the replay metadata — the
    /// input display chips' captions. Either may be empty.
    pub fn nicknames(&self) -> (&str, &str) {
        (&self.input_display.nicknames.0, &self.input_display.nicknames.1)
    }

    /// The recording header, retained for viewer presentation (filename
    /// lives with the host, while game/mode/players/date live here).
    pub fn metadata(&self) -> &tango_replay::Metadata {
        &self.metadata
    }

    /// Highest tick the background prefetcher has reached, for the
    /// progress overlay on the scrub bar. Hits `total_ticks` when the
    /// prefetcher has run to completion.
    pub fn prefetch_progress(&self) -> u32 {
        self.engine.prefetch_progress.load(Ordering::Relaxed)
    }

    /// Recorded-frame index of each inter-round transition — the marks the
    /// scrubber draws. These sit on the same scale as the playhead
    /// ([`Self::current_tick`]), so a mark coincides exactly with the
    /// playhead as it crosses.
    ///
    /// A recording says nothing about its own rounds, so these come from
    /// the games' telemetry: complete from the first frame when the host
    /// had the recording's analysis to hand in, and otherwise arriving
    /// one at a time as the prefetch pass reaches them. Empty until the
    /// pass finds the second round — a single-round replay stays that
    /// way.
    pub fn round_boundaries(&self) -> Vec<u32> {
        self.round_boundaries.lock().unwrap().clone()
    }

    /// Jump the playhead to `target`, asynchronously. Records the request
    /// on the seek controller and returns immediately; the seek worker
    /// runs the capture load + frame catch-up on its own thread, and
    /// newer requests supersede in-flight ones mid-chase. With
    /// `resume_after`, playback unpauses once the chase lands (unless a
    /// newer request took over) — used by scrub commits, which pause
    /// playback for the duration of the drag.
    pub fn seek_to(&self, target: u32, resume_after: bool) {
        self.seek_ctrl().request(target.min(self.total_ticks), resume_after);
    }

    /// Target of the in-flight seek, if any — lets the UI draw the
    /// playhead where it's headed instead of snapping back to the
    /// pre-seek tick until the chase lands.
    pub fn pending_seek_target(&self) -> Option<u32> {
        self.seek_ctrl().pending_target()
    }

    /// True while an in-flight seek will unpause playback on landing.
    /// The thread is paused for the chase's duration, but the session
    /// is logically still playing — the transport shouldn't flip to
    /// the paused state.
    pub fn seek_will_resume(&self) -> bool {
        self.seek_ctrl().resume_pending()
    }

    /// Withdraw an in-flight seek's pending resume, keeping playback
    /// paused once it lands.
    pub fn cancel_seek_resume(&self) {
        self.seek_ctrl().clear_resume();
    }

    fn seek_ctrl(&self) -> &SeekController {
        &self.engine.seek
    }

    /// The whole-pair capture best suited to jump-start a clip export
    /// at playhead tick `start`: the latest one strictly *before* it
    /// (keyframe store ∪ rewind ring), so the clip's first frame is
    /// still produced by a stepped tick rather than promised from a
    /// framebuffer we can't re-emit. `None` means the export falls
    /// back to simulating from boot.
    pub fn clip_start_capture(&self, start: u32) -> Option<Arc<tango_match::Capture>> {
        self.engine
            .playback
            .lock()
            .unwrap()
            .as_ref()?
            .nearest_capture(start.checked_sub(1)?)
    }

    /// The captured snapshot nearest `target`, if any — backs the hover
    /// thumbnail above the scrub bar and the drag preview blit. Near the
    /// playhead the rewind window supplies exact frames; elsewhere it's
    /// the store's keyframes.
    pub fn nearest_snapshot(&self, target: u32) -> Option<NearestSnapshot> {
        let guard = self.engine.playback.lock().unwrap();
        let capture = guard.as_ref()?.nearest_capture(target)?;
        Some(NearestSnapshot {
            frames: capture.frames.clone(),
            local_player: self.engine.local_player,
        })
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
            let cur = self.current_tick();
            if cur.abs_diff(target) <= snap.frame_index().abs_diff(target) {
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
            Some(snap) if snap.frame_index() == target => self.blit_snapshot(&snap),
            _ => false,
        }
    }

    /// Copy `snap`'s stored framebuffers into the display surfaces —
    /// see [`Perspective`].
    fn blit_snapshot(&self, snap: &NearestSnapshot) -> bool {
        Perspective {
            shown_seat: Arc::new(AtomicUsize::new(snap.local_player)),
            surfaces: self.surfaces.clone(),
            swap_perspective: self.swap_perspective.clone(),
            local_player: snap.local_player,
        }
        .publish_frames(&snap.frames);
        true
    }
}

impl crate::Session for ReplaySession {
    fn local_game(&self) -> &'static tango_gamesupport::Game {
        self.game
    }

    fn frame(&self) -> Vec<u8> {
        self.surfaces.screen.read()
    }

    fn screen_layout(&self) -> tango_match::ScreenLayout {
        self.layout.clone()
    }

    fn wake(&self) -> Arc<tokio::sync::Notify> {
        self.surfaces.wake.clone()
    }

    /// The opponent's screen, or the local one while swapped — `None`
    /// while the PiP is off or before its first captured frame.
    fn pip_frame(&self) -> Option<Vec<u8>> {
        self.surfaces.pip_frame()
    }

    /// 0.5 = slow-mo. This changes the selected base rate; an enabled
    /// custom-screen override can temporarily raise the effective rate.
    fn set_speed(&self, factor: f32) {
        self.speed.set_factor(factor);
    }
}

/// A captured playback snapshot — what
/// [`ReplaySession::nearest_snapshot`] hands the scrub/hover UI.
pub struct NearestSnapshot {
    /// Both seats' frames as they were captured, owned as the seam's
    /// own capture type.
    frames: tango_match::LiveFrames,
    local_player: usize,
}

impl NearestSnapshot {
    /// The captured frame's position on the playhead scale.
    pub fn frame_index(&self) -> u32 {
        self.frames.tick
    }

    /// The local perspective's pixels, same RGBA8 as
    /// [`Session::frame`](crate::Session::frame). May be empty if the
    /// capture had no rendered frame.
    pub fn local_framebuffer(&self) -> Vec<u8> {
        self.frames.frames[self.local_player].clone()
    }
}

/// The display surfaces an SIO playback session publishes into, plus
/// the perspective toggles that pick which core lands where — shared
/// between the drive loop, the seek worker's landing publisher, and
/// paused-frame blits so the paths can't drift.
#[derive(Clone)]
struct Perspective {
    /// Mirrors `swap_perspective` as a seat number, for the engine's
    /// audio pull.
    shown_seat: Arc<AtomicUsize>,
    surfaces: Surfaces,
    swap_perspective: Arc<AtomicBool>,
    local_player: usize,
}

impl Perspective {
    /// Which seat the main screen currently shows.
    fn shown(&self) -> usize {
        let shown = if self.swap_perspective.load(Ordering::Relaxed) {
            1 - self.local_player
        } else {
            self.local_player
        };
        // The audio pull follows the picture.
        self.shown_seat.store(shown, Ordering::Relaxed);
        shown
    }

    /// Publish a capture's frames — live or landed, the player draws no
    /// distinction and neither does this.
    fn publish_frames(&self, frames: &tango_match::LiveFrames) {
        let shown = self.shown();
        fn pick(fb: &[u8]) -> Option<&[u8]> {
            (!fb.is_empty()).then_some(fb)
        }
        self.surfaces
            .publish(pick(&frames.frames[shown]), pick(&frames.frames[1 - shown]));
    }
}

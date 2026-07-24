//! Training-mode emulator session: a real link battle you fight
//! locally, against a **dummy controller** on the opponent core.
//!
//! Mechanically this is the PvP engine ([`tango_match::engine::Match`])
//! with the network cut out. Both cores run the player's own ROM + save
//! (a mirror match), primed all the way into their link battle exactly
//! as a netplay match would be — so training *starts in a battle*, not
//! at the title screen. The player drives one core; the other core's
//! input each tick comes from a [`TrainingController`].
//!
//! Out of the box that controller does nothing: the stock
//! [`NoopController`] presses no buttons, so the opponent just stands
//! there. The point of the mode is the seam, not any behaviour — it
//! exists so future work has one obvious place to hook in: implement
//! [`TrainingController`], read either core's state off the live pair in
//! [`TrainingController::poll`], and decide what the dummy should press.
//! A controller can be swapped in at any time with
//! [`TrainingSession::set_controller`].
//!
//! The battle runs entirely off in-memory SRAM, so nothing a training
//! session does is written back to the player's `.sav` on disk. There is
//! no netcode, no throttling and no rollback churn: the dummy's input for
//! each tick is supplied locally before that tick advances, so the pair
//! runs in perfect lockstep.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tango_match::engine::{Match, MatchConfig};
use tango_match::telemetry::RoundEvent;

/// GBA video framerate — the true link-battle rate (matches the PvP
/// engine), so the wall-clock pacer runs the battle at the right speed.
pub const EXPECTED_FPS: f32 = 16777216.0 / 280896.0;

/// Single battle. Training always fights one round against the dummy;
/// there's no lobby to pick a mode, and the default do-nothing opponent
/// makes best-of-N pointless.
const TRAINING_MATCH_TYPE: (u8, u8) = (0, 0);

/// What the drive loop hands a [`TrainingController`] each tick: the live
/// linked pair (read either core's RAM/video to decide what to do) and
/// which core is which. This is the whole integration surface — a
/// controller inspects the pair, then returns the joyflags the dummy
/// should hold for the tick about to advance.
pub struct ControllerContext<'a> {
    /// The live pair, parked at the newest simulated tick. Read the
    /// dummy's own core with `pair.core_mut(ctx.dummy_player)` or watch
    /// the human with `pair.core(ctx.human_player)`.
    pub pair: &'a mut tango_match::Link,
    /// The core the dummy drives (the non-human core).
    pub dummy_player: usize,
    /// The core the human drives.
    pub human_player: usize,
    /// Ticks elapsed since the battle started (0 on the first poll).
    pub frame: u64,
}

/// A pluggable per-tick input source for the training dummy — the one
/// extension point of training mode. The drive loop calls [`poll`] once
/// per tick, just before that tick advances, and feeds the returned
/// joyflags to the dummy's core as its input for the tick.
///
/// The stock implementation is [`NoopController`], which presses nothing.
/// Implement this to drive the dummy: read state off `ctx.pair`, return
/// the buttons to hold this tick.
///
/// [`poll`]: TrainingController::poll
pub trait TrainingController: Send {
    /// Produce the dummy's input for the tick about to advance. Return an
    /// mgba joyflag bitmap (the same word
    /// [`crate::Session::set_joyflags`] carries); return `0` to press
    /// nothing.
    fn poll(&mut self, ctx: &mut ControllerContext) -> u32;
}

/// The default dummy controller: presses nothing, every tick. A training
/// session built with it is a battle against an opponent that just
/// stands there — until a real [`TrainingController`] is installed.
pub struct NoopController;

impl TrainingController for NoopController {
    fn poll(&mut self, _ctx: &mut ControllerContext) -> u32 {
        0
    }
}

/// A boxed, hot-swappable training controller shared between the session
/// (which can replace it) and the drive thread (which polls it).
type SharedController = Arc<Mutex<Box<dyn TrainingController>>>;

pub struct TrainingSession {
    game: &'static tango_gamesupport::Game,
    /// Which core the human currently drives (0 or 1). The player starts
    /// on core 0 with the dummy on core 1; [`swap`](Self::toggle_swap)
    /// flips it so the human takes the other side. Read every tick by the
    /// drive loop (to route input) and by the audio pull (to follow the
    /// controlled core).
    controlled: Arc<AtomicUsize>,
    joyflags: Arc<AtomicU32>,
    controller: SharedController,
    /// Pacing target as f32 bits — realtime by default; `set_speed`
    /// raises it for fast-forward and the audio stream compresses to
    /// match.
    fps_bits: Arc<AtomicU32>,
    /// The most recent joyflags the dummy controller produced, for the
    /// host to observe.
    dummy_joyflags: Arc<AtomicU32>,
    /// Whether the opponent-screen picture-in-picture is on.
    show_pip: Arc<AtomicBool>,
    /// The non-controlled core's screen, written each tick while the PiP
    /// is on.
    pip: Arc<crate::Framebuffer>,
    /// Whether `pip` holds a frame from the current PiP activation
    /// (cleared while off, so a stale capture never flashes on re-toggle).
    pip_fresh: Arc<AtomicBool>,
    /// Latched once the battle's own match-end path fires — flips
    /// [`is_ended`](crate::Session::is_ended) so the host tears the
    /// session down.
    ended: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    screen: Arc<crate::Framebuffer>,
    wake: Arc<tokio::sync::Notify>,
    drive: Option<std::thread::JoinHandle<()>>,
}

impl TrainingSession {
    /// Boot a training battle with `controller` as the dummy's input
    /// source (pass `Box::new(NoopController)` for the do-nothing
    /// default). Both cores run `rom` + `save_sram` (a mirror match); the
    /// SRAM is in-memory, so nothing persists back to disk.
    ///
    /// Primes both games into their link battle before returning — a
    /// short burst of headless emulation — so a live session is already
    /// mid-battle. Also returns the session's audio stream (the human
    /// core's samples resampled to `sample_rate`) for the host to route
    /// to its output; dropping it just costs sound.
    pub fn new(
        game: &'static tango_gamesupport::Game,
        rom: Arc<Vec<u8>>,
        save_sram: Vec<u8>,
        sample_rate: u32,
        controller: Box<dyn TrainingController>,
    ) -> Result<(Self, crate::core_stream::CoreStream), crate::Error> {
        // The engine's local core is core 0; `advance` always feeds core
        // 0 and `add_remote_input` core 1. The player starts on core 0
        // (the dummy on core 1); a swap only re-routes which input source
        // feeds each core, so the engine's local_player stays 0. Both
        // cores run the same game (a mirror match) — training is local,
        // so there's no opponent selection.
        //
        // Present delay 0: the match is local and lockstep, so there's no
        // latency to hide and no speculation to roll back.
        let match_ = Match::new(MatchConfig {
            roms: [rom.as_ref().clone(), rom.as_ref().clone()],
            saves: [save_sram.clone(), save_sram],
            support: [game.pvp, game.pvp],
            match_type: TRAINING_MATCH_TYPE,
            rng_seed: rand::random(),
            rtc: std::time::SystemTime::now(),
            local_player: 0,
            present_delay: 0,
            disable_bgm: false,
        })?;

        // The rollback session frameskips every non-local core to spare
        // the software renderer (PvP only ever shows the local side).
        // Training shows both — the PiP and the side-swap — so turn
        // rendering back on for the whole pair. (Frameskip is unserialized
        // and invisible to the simulation, so this is rollback-safe.)
        match_.with_pair(|pair| {
            for i in 0..2 {
                pair.set_frameskip(i, 0);
            }
        });

        let controlled = Arc::new(AtomicUsize::new(0));
        let joyflags = Arc::new(AtomicU32::new(0));
        let controller: SharedController = Arc::new(Mutex::new(controller));
        let fps_bits = Arc::new(AtomicU32::new(EXPECTED_FPS.to_bits()));
        let dummy_joyflags = Arc::new(AtomicU32::new(0));
        let show_pip = Arc::new(AtomicBool::new(false));
        let pip = crate::Framebuffer::new();
        let pip_fresh = Arc::new(AtomicBool::new(false));
        let ended = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let screen = crate::Framebuffer::new();
        let wake = Arc::new(tokio::sync::Notify::new());

        // Audio pulls the controlled core straight off the pair (same
        // path as PvP), rate control following the pacing target. The
        // `player` closure is re-read every fill, so a swap moves the
        // sound to the side the player is now driving.
        let audio = crate::core_stream::CoreStream::new(
            crate::core_stream::PairCorePull {
                pair: match_.pair_handle(),
                player: {
                    let controlled = controlled.clone();
                    Box::new(move || controlled.load(Ordering::Relaxed))
                },
            },
            crate::core_stream::CoreStream::fps_from_bits(fps_bits.clone()),
            sample_rate,
        );

        let drive = std::thread::Builder::new().name("training".to_owned()).spawn({
            let ctx = DriveContext {
                match_,
                controlled: controlled.clone(),
                joyflags: joyflags.clone(),
                controller: controller.clone(),
                fps_bits: fps_bits.clone(),
                dummy_joyflags: dummy_joyflags.clone(),
                show_pip: show_pip.clone(),
                pip: pip.clone(),
                pip_fresh: pip_fresh.clone(),
                ended: ended.clone(),
                stop: stop.clone(),
                screen: screen.clone(),
                wake: wake.clone(),
            };
            move || ctx.run()
        })?;

        Ok((
            Self {
                game,
                controlled,
                joyflags,
                controller,
                fps_bits,
                dummy_joyflags,
                show_pip,
                pip,
                pip_fresh,
                ended,
                stop,
                screen,
                wake,
                drive: Some(drive),
            },
            audio,
        ))
    }

    /// Install a new dummy controller, replacing whatever is running.
    /// Takes effect on the next tick the drive loop polls. The other
    /// half of the extension point: build a session with
    /// [`NoopController`], then swap in real behaviour whenever it's
    /// ready.
    pub fn set_controller(&self, controller: Box<dyn TrainingController>) {
        *self.controller.lock().unwrap() = controller;
    }

    /// The joyflags the dummy controller produced on its most recent
    /// poll. `0` with the stock [`NoopController`].
    pub fn dummy_joyflags(&self) -> u32 {
        self.dummy_joyflags.load(Ordering::Relaxed)
    }

    /// Which core the human currently drives (0 or 1).
    pub fn controlled_player(&self) -> usize {
        self.controlled.load(Ordering::Relaxed)
    }

    /// Whether the player has swapped to the non-default side (control of
    /// core 1). `false` on the side the session booted on.
    pub fn is_swapped(&self) -> bool {
        self.controlled.load(Ordering::Relaxed) != 0
    }

    /// Swap which side the player controls: the human and the dummy trade
    /// cores. Takes effect on the next tick the drive loop routes input,
    /// and the audio + main screen follow the newly-controlled core.
    pub fn toggle_swap(&self) {
        self.controlled.fetch_xor(1, Ordering::Relaxed);
    }

    /// Whether the opponent-screen picture-in-picture is on.
    pub fn show_pip(&self) -> bool {
        self.show_pip.load(Ordering::Relaxed)
    }

    /// Toggle the opponent-screen picture-in-picture. Takes effect on the
    /// next published frame.
    pub fn toggle_pip(&self) {
        self.show_pip.fetch_xor(true, Ordering::Relaxed);
    }
}

impl crate::Session for TrainingSession {
    fn local_game(&self) -> &'static tango_gamesupport::Game {
        self.game
    }

    fn frame(&self) -> Vec<u8> {
        self.screen.read()
    }

    fn wake(&self) -> Arc<tokio::sync::Notify> {
        self.wake.clone()
    }

    /// The non-controlled core's screen — `None` while the PiP is off or
    /// before its first captured frame.
    fn pip_frame(&self) -> Option<Vec<u8>> {
        (self.show_pip.load(Ordering::Relaxed) && self.pip_fresh.load(Ordering::Relaxed)).then(|| self.pip.read())
    }

    fn set_joyflags(&self, joyflags: u32) {
        self.joyflags.store(joyflags, Ordering::Relaxed);
    }

    fn set_speed(&self, factor: f32) {
        self.fps_bits
            .store(crate::clamp_speed(EXPECTED_FPS, factor).to_bits(), Ordering::Relaxed);
    }

    /// True once the battle's own match-end path fired, so the host
    /// tears the session down instead of leaving the player on a hung
    /// post-match link screen.
    fn is_ended(&self) -> bool {
        self.ended.load(Ordering::Acquire)
    }
}

impl Drop for TrainingSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(drive) = self.drive.take() {
            let _ = drive.join();
        }
    }
}

/// Everything the drive thread owns for the session's life.
struct DriveContext {
    match_: Match,
    controlled: Arc<AtomicUsize>,
    joyflags: Arc<AtomicU32>,
    controller: SharedController,
    fps_bits: Arc<AtomicU32>,
    dummy_joyflags: Arc<AtomicU32>,
    show_pip: Arc<AtomicBool>,
    pip: Arc<crate::Framebuffer>,
    pip_fresh: Arc<AtomicBool>,
    ended: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    screen: Arc<crate::Framebuffer>,
    wake: Arc<tokio::sync::Notify>,
}

impl DriveContext {
    fn run(mut self) {
        let mask = tango_match::input::JOYFLAGS_MASK as u32;
        let mut frame: u64 = 0;
        let mut pacer = crate::Pacer::new();

        while !self.stop.load(Ordering::Relaxed) {
            // Which core the player drives this tick; the dummy takes the
            // other. A swap flips this between ticks.
            let controlled = self.controlled.load(Ordering::Relaxed);
            let dummy_player = 1 - controlled;

            // Poll the dummy controller for the tick about to advance. It
            // sees the pair parked at the newest simulated tick; its
            // output becomes the dummy core's input for this tick. The
            // stock NoopController returns 0.
            let controller = self.controller.clone();
            let dummy = self.match_.with_pair(|pair| {
                controller.lock().unwrap().poll(&mut ControllerContext {
                    pair,
                    dummy_player,
                    human_player: controlled,
                    frame,
                })
            }) & mask;
            self.dummy_joyflags.store(dummy, Ordering::Relaxed);

            // Route each input to its core, then feed the engine: core 0
            // via `advance`, core 1 via `add_remote_input` (the engine's
            // fixed local/remote slots). Whichever core the player drives
            // gets the pad; the other gets the dummy. Both inputs for the
            // tick are present before it advances, so the pair confirms it
            // immediately — lockstep, no rollback.
            let player = self.joyflags.load(Ordering::Relaxed) & mask;
            let core0 = if controlled == 0 { player } else { dummy };
            let core1 = if controlled == 0 { dummy } else { player };
            self.match_.add_remote_input(core1, 0);
            let (_outgoing, report) = match self.match_.advance(core0) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("training: advance failed: {e}");
                    self.ended.store(true, Ordering::Release);
                    self.wake.notify_one();
                    break;
                }
            };

            // Watch the confirmed telemetry for the games' own match-end
            // path so the session can tear down cleanly (with a
            // do-nothing dummy the player wins and the battle ends). We
            // don't fold stats — training records nothing.
            let (_samples, events) = self.match_.telemetry().lock().unwrap().drain_confirmed(report.confirmed);
            if events.iter().any(|(_, e)| matches!(e, RoundEvent::MatchEnded)) {
                self.ended.store(true, Ordering::Release);
                self.wake.notify_one();
                break;
            }

            // Publish the controlled core to the main screen; the other
            // core feeds the PiP while it's on.
            self.match_.with_pair(|pair| {
                if let Some(buf) = pair.video_buffer(controlled) {
                    self.screen.write(buf);
                }
                if self.show_pip.load(Ordering::Relaxed) {
                    if let Some(buf) = pair.video_buffer(dummy_player) {
                        self.pip.write(buf);
                        self.pip_fresh.store(true, Ordering::Relaxed);
                    }
                } else {
                    self.pip_fresh.store(false, Ordering::Relaxed);
                }
            });
            frame = frame.wrapping_add(1);
            self.wake.notify_one();

            // Pace at the target rate (realtime unless fast-forwarding).
            pacer.wait(f32::from_bits(self.fps_bits.load(Ordering::Relaxed)));
        }
    }
}

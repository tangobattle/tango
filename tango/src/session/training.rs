//! Training session: local sparring against a controllable dummy, run on
//! the live PvP machinery with the network replaced by an in-process
//! loopback.
//!
//! The whole PvP stack — [`tango_pvp::battle::Match`], the shadow co-sim,
//! the re-sim stepper, the per-game traps, replay recording — is reused
//! unchanged; only the transport differs. [`LoopbackSender`] implements
//! [`tango_pvp::net::Sender`] and answers each local input *synchronously,
//! inside the same primary trap fire*: it asks the [`DummyController`] for
//! the dummy's joyflags and injects them straight into the match's
//! remote-input queue via [`Match::inject_remote_input`]. The injected
//! input is drained later in the very same trap, so every tick confirms
//! immediately — the rollback engine runs in pure lockstep with zero
//! speculation, and there is no receive task, no async, and no reset race.
//!
//! On top of that sits the training toolkit: checkpoint save/restore
//! (via [`Match::training_checkpoint`] / `restore_training_checkpoint`),
//! an auto-captured round-start checkpoint for instant round restarts,
//! pause + frame advance, and a speed factor.
//!
//! [`Match`]: tango_pvp::battle::Match

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tango_pvp::battle::TrainingCheckpoint;

pub use tango_pvp::battle::EXPECTED_FPS;

/// How many manual checkpoint slots a session offers. Fighting-game
/// trainers converge on a small handful; the round-start restart is
/// separate and free.
pub const CHECKPOINT_SLOTS: usize = 3;

/// Speed steps offered by the HUD control. 1.0 must be present (the
/// default); PvP-style netplay never sees these — the factor only exists
/// because the "peer" is in-process.
pub const SPEED_STEPS: [f32; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

/// What drives the dummy's joyflags each tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DummyMode {
    /// Neutral input.
    Idle,
    /// The user's controller drives the dummy; the player side holds
    /// neutral. The classic single-controller trainer flow.
    Possess,
    /// Possess + capture the stream into the script.
    Record,
    /// Replay the recorded script (optionally looping); the user
    /// controls their own side again.
    Playback,
}

/// Snapshot of the dummy's state for the HUD (mode chip highlights,
/// record length, playback progress).
#[derive(Clone, Copy)]
pub struct DummyStatus {
    pub mode: DummyMode,
    pub script_len: usize,
    pub play_pos: usize,
    pub looping: bool,
}

/// Decides the dummy's joyflags, one tick at a time. Lives behind a
/// mutex shared between the loopback sender (emulator thread, once per
/// tick) and the UI (mode changes); neither holds it while calling into
/// the match, so there is no lock-order cycle with the round-state lock.
pub struct DummyController {
    mode: DummyMode,
    /// mgba-keys bitmap the user's controller currently holds *for the
    /// dummy* — written by the session's input routing while possessed
    /// (Possess/Record), ignored otherwise.
    held: u32,
    /// Recorded joyflags, one per tick, in wire (u16) form.
    script: Vec<u16>,
    play_pos: usize,
    looping: bool,
}

impl DummyController {
    fn new() -> Self {
        Self {
            mode: DummyMode::Idle,
            held: 0,
            script: Vec::new(),
            play_pos: 0,
            looping: true,
        }
    }

    /// One tick's dummy joyflags. Called from the loopback sender on the
    /// emulator thread, exactly once per local input.
    fn next(&mut self) -> u16 {
        match self.mode {
            DummyMode::Idle => 0,
            DummyMode::Possess => self.held as u16,
            DummyMode::Record => {
                let joyflags = self.held as u16;
                self.script.push(joyflags);
                joyflags
            }
            DummyMode::Playback => {
                if self.play_pos >= self.script.len() {
                    if self.looping && !self.script.is_empty() {
                        self.play_pos = 0;
                    } else {
                        return 0;
                    }
                }
                let joyflags = self.script[self.play_pos];
                self.play_pos += 1;
                joyflags
            }
        }
    }

    /// Switch modes, with the transition bookkeeping each entry implies:
    /// starting a recording begins a fresh take; starting playback rewinds
    /// the script.
    fn set_mode(&mut self, mode: DummyMode) {
        match mode {
            DummyMode::Record => {
                self.script.clear();
                self.play_pos = 0;
            }
            DummyMode::Playback => {
                self.play_pos = 0;
            }
            DummyMode::Idle | DummyMode::Possess => {}
        }
        self.mode = mode;
    }

    /// A round ended. Every active mode drops to Idle: a script (or a
    /// take) is positional within a fight, and a held possession would
    /// starve the real inter-round screens of local input. The recorded
    /// script itself is kept for the next round.
    fn on_round_end(&mut self) {
        self.mode = DummyMode::Idle;
        self.play_pos = 0;
        self.held = 0;
    }

    /// A checkpoint was restored. Restarting an active playback from the
    /// top is the core practice loop (save a situation, record an answer,
    /// re-run it against every retry); a recording mid-take is broken by
    /// the timeline branch, so it stops.
    fn on_restore(&mut self) {
        match self.mode {
            DummyMode::Playback => self.play_pos = 0,
            DummyMode::Record => self.mode = DummyMode::Idle,
            DummyMode::Idle | DummyMode::Possess => {}
        }
    }
}

/// The in-process "network": [`tango_pvp::net::Sender`] whose peer is the
/// dummy. Each local `Input` event is answered synchronously by injecting
/// the dummy's input for the same tick (echoing our own `tick_advantage`
/// back, so skew reads 0 and the throttler never engages); `EndOfRound`
/// is echoed so the peer-round tags track the local round counter. The
/// match is installed after construction via the [`OnceLock`] — events
/// before that (there are none: sends only happen inside rounds) drop
/// harmlessly.
struct LoopbackSender {
    match_: Arc<OnceLock<Arc<tango_pvp::battle::Match>>>,
    dummy: Arc<Mutex<DummyController>>,
    /// Last injected dummy joyflags, surfaced for the input display.
    last_dummy: Arc<AtomicU32>,
    /// Mirror of "the user is driving the dummy" for the input routing
    /// and the display swap — see [`TrainingSession::possessed`].
    possessed: Arc<AtomicBool>,
}

impl tango_pvp::net::Sender for LoopbackSender {
    fn send(&mut self, event: &tango_pvp::net::Event) -> std::io::Result<()> {
        let Some(match_) = self.match_.get() else {
            return Ok(());
        };
        match event {
            tango_pvp::net::Event::Input(input) => {
                let joyflags = self.dummy.lock().unwrap().next();
                self.last_dummy.store(joyflags as u32, Ordering::Relaxed);
                match_.inject_remote_input(tango_pvp::net::Input {
                    joyflags,
                    tick_advantage: input.tick_advantage,
                });
            }
            tango_pvp::net::Event::EndOfRound => {
                match_.inject_remote_end_of_round();
                // Any active dummy mode drops at the round boundary —
                // including possession: between rounds the local input
                // drives the real inter-round screens, and a possessed
                // controller would leave the user unable to advance them.
                self.dummy.lock().unwrap().on_round_end();
                self.possessed.store(false, Ordering::Relaxed);
                match_.set_shadow_rendering(false);
            }
        }
        Ok(())
    }
}

/// Everything the training HUD / keybinds can do to a running session.
/// Routed through `session::Message::Training`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    SetDummyMode(DummyMode),
    /// Toggle between a mode and Idle — what the keybinds want (press
    /// possess again to release), while the HUD chips use `SetDummyMode`.
    ToggleDummyMode(DummyMode),
    ToggleLoop,
    RestartRound,
    SelectSlot(usize),
    SaveSlot,
    LoadSlot,
    TogglePause,
    FrameAdvance,
    SetSpeed(f32),
    ToggleInputDisplay,
}

/// The live training session. A lean sibling of
/// [`PvpSession`](crate::session::pvp::PvpSession): same primary core,
/// trap set, and frame callback shape, but no network machinery at all —
/// no receive loop, no reconnect coordinator, no latency counter, no
/// end-of-match handshake. `is_ended` is the completion token, full stop.
pub struct TrainingSession {
    local_game: &'static crate::game::Game,
    local_player_index: u8,
    /// Player-side joyflags (the primary trap's input source).
    joyflags: Arc<AtomicU32>,
    /// Dummy brain, shared with the loopback sender.
    dummy: Arc<Mutex<DummyController>>,
    /// Last dummy joyflags actually injected — input-display fodder.
    last_dummy: Arc<AtomicU32>,
    completion_token: tango_pvp::hooks::CompletionToken,
    _audio_binding: Option<crate::audio::Binding>,
    thread: mgba::thread::Thread,
    /// Held directly (unlike PvP, where the receive task owns it): the
    /// checkpoint / speed / restore calls all go straight to the match.
    inner_match: Arc<tango_pvp::battle::Match>,
    match_handle: tango_pvp::hooks::MatchHandle,
    /// Cancelling makes [`MatchHandle::get`] go inert, which is what lets
    /// the mgba thread tear down without traps touching a dying match.
    cancellation_token: tokio_util::sync::CancellationToken,
    /// Auto-captured checkpoint at each round's first commit; cleared at
    /// round end. Written by the frame callback (emulator thread), read
    /// by [`Action::RestartRound`] (UI thread).
    round_start: Arc<Mutex<Option<TrainingCheckpoint>>>,
    /// Manual checkpoint slots.
    slots: Mutex<[Option<TrainingCheckpoint>; CHECKPOINT_SLOTS]>,
    /// Which slot the save/load keybinds and HUD buttons operate on.
    active_slot: AtomicUsize,
    /// Which slots are occupied, mirrored out of `slots` as a bitmask so
    /// the HUD can render fill state without taking the (checkpoint-sized)
    /// mutex every frame.
    slot_filled: AtomicUsize,
    /// Speed factor as f32 bits — the HUD's view of what was last set on
    /// the match.
    speed: AtomicU32,
    /// Whether the input-display overlay is on.
    show_inputs: AtomicBool,
    /// Whether the player's input routing is currently redirected to the
    /// dummy. An atomic mirror of the dummy mode, shared with the loopback
    /// sender (round-end drops possession) and the frame callback (which
    /// captures the shadow's render while it's set) so neither takes the
    /// controller mutex on their hot paths.
    possessed: Arc<AtomicBool>,
    /// The dummy's screen, copied from the shadow core once per frame
    /// while possessed — the PiP overlay's producer. Same BGR555 layout
    /// as the session vbuf.
    shadow_vbuf: Arc<Mutex<Vec<u8>>>,
    /// Whether `shadow_vbuf` holds a frame from the *current* possession
    /// (cleared whenever a frame passes unpossessed, so a stale capture
    /// never flashes when possession toggles back on).
    pip_fresh: Arc<AtomicBool>,
    pub local_loaded: Option<crate::selection::Loaded>,
    pub local_save_view: crate::save_view::State,
    pub opponent_loaded: Option<crate::selection::Loaded>,
    pub opponent_save_view: crate::save_view::State,
}

impl TrainingSession {
    /// Build and start the session. Fully synchronous: the loopback needs
    /// no handoff, so unlike PvP there is nothing to await.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        local_game: &'static crate::game::Game,
        rom: Arc<Vec<u8>>,
        local_save: Box<dyn tango_dataview::save::Save + Send + Sync>,
        opponent_save: Box<dyn tango_dataview::save::Save + Send + Sync>,
        match_type: (u8, u8),
        wanted_player_index: u8,
        rng_seed: [u8; 16],
        local_settings: crate::net::protocol::Settings,
        remote_settings: crate::net::protocol::Settings,
        replays_path: &std::path::Path,
        disable_bgm: bool,
        audio_binder: &crate::audio::LateBinder,
        frame_notify: Arc<tokio::sync::Notify>,
        vbuf: Arc<Mutex<Vec<u8>>>,
        local_loaded: Option<crate::selection::Loaded>,
        opponent_loaded: Option<crate::selection::Loaded>,
    ) -> anyhow::Result<Self> {
        let mut core = crate::session::new_gba_core(rom.as_ref())?;
        // In-memory SRAM like PvP: nothing a training match does should
        // write back to the user's .sav.
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(local_save.to_sram_dump()))?;
        // Pin the cart RTC to the session clock, exactly like a real match
        // pins the negotiated one — the shadow and each round's stepper get
        // the same instant below, and the replay records it, so exe45 stays
        // deterministic and its replays reproduce.
        let match_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let rtc_time = std::time::UNIX_EPOCH + std::time::Duration::from_millis(match_ts);
        core.set_rtc_fixed(rtc_time);

        let joyflags = Arc::new(AtomicU32::new(0));
        let local_hooks = local_game.hooks;
        let match_handle = tango_pvp::hooks::MatchHandle::new();
        let completion_token = tango_pvp::hooks::CompletionToken::new();

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

        // Side select without breaking RNG parity: `pick_local_player_index`
        // draws one bool from the shared stream and maps it to a side via
        // `is_offerer`. We control both knobs, so peek the draw and pick
        // `is_offerer` such that the draw lands on the requested side — the
        // stream stays byte-identical to a real match with this seed, so the
        // recorded replay is canonical.
        use rand::SeedableRng;
        let mut rng = rand_pcg::Mcg128Xsl64::from_seed(rng_seed);
        // Probe the draw through the canonical picker on a clone: with
        // `is_offerer = true`, index 0 means the polite side won the draw.
        let polite_win = tango_pvp::battle::Match::pick_local_player_index(&mut rng.clone(), true) == 0;
        let is_offerer = (wanted_player_index == 0) == polite_win;
        let local_player_index = tango_pvp::battle::Match::pick_local_player_index(&mut rng, is_offerer);
        debug_assert_eq!(local_player_index, wanted_player_index);

        let replay_writer = crate::session::pvp::build_replay_writer(
            replays_path,
            "training",
            &local_settings,
            &remote_settings,
            match_type,
            is_offerer,
            local_player_index,
            rng_seed,
            match_ts,
            local_save.as_ref(),
            opponent_save.as_ref(),
        )
        .map_err(|e| {
            log::warn!("training: replay writer open failed: {e}");
            e
        })
        .ok();

        let shadow = tango_pvp::shadow::Shadow::new(
            rom.as_ref(),
            opponent_save.as_ref(),
            local_hooks,
            match_type,
            is_offerer,
            local_player_index,
            rng.clone(),
            rtc_time,
        )?;

        let dummy = Arc::new(Mutex::new(DummyController::new()));
        let last_dummy = Arc::new(AtomicU32::new(0));
        let possessed = Arc::new(AtomicBool::new(false));
        let match_slot: Arc<OnceLock<Arc<tango_pvp::battle::Match>>> = Arc::new(OnceLock::new());
        let sender = LoopbackSender {
            match_: match_slot.clone(),
            dummy: dummy.clone(),
            last_dummy: last_dummy.clone(),
            possessed: possessed.clone(),
        };

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        let identity = tango_pvp::battle::MatchIdentity {
            match_type,
            is_offerer,
            local_player_index,
            rtc_time,
        };
        let inner_match = tango_pvp::battle::Match::new(
            rom.as_ref().clone(),
            local_hooks,
            thread.handle(),
            Box::new(sender),
            cancellation_token.clone(),
            rng,
            shadow,
            identity,
            tango_pvp::battle::ReplayConfig { writer: replay_writer },
            // The dummy's input arrives within the same trap fire, so the
            // display can hug the engine floor — no network to hide.
            Arc::new(AtomicU32::new(tango_pvp::battle::MIN_FRAME_DELAY)),
            disable_bgm,
        );
        let _ = match_slot.set(inner_match.clone());
        match_handle.set(inner_match.clone());

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        vbuf.lock().unwrap().fill(0);
        let audio_binding = audio_binder.bind_mgba(thread.handle(), "training");

        let round_start: Arc<Mutex<Option<TrainingCheckpoint>>> = Arc::default();

        let shadow_vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 2) as usize
        ]));
        let pip_fresh = Arc::new(AtomicBool::new(false));

        thread.set_frame_callback({
            let joyflags = joyflags.clone();
            let vbuf = vbuf.clone();
            let frame_notify = frame_notify.clone();
            let completion_token = completion_token.clone();
            let inner_match = inner_match.clone();
            let round_start = round_start.clone();
            let possessed = possessed.clone();
            let shadow_vbuf = shadow_vbuf.clone();
            let pip_fresh = pip_fresh.clone();
            // Finalize the replay exactly once when the match completes.
            let finished = AtomicBool::new(false);
            move |mut core, video_buffer, mut thread_handle| {
                core.set_keys(joyflags.load(Ordering::Relaxed));
                // The main screen always shows the player's perspective.
                vbuf.lock().unwrap().copy_from_slice(video_buffer);
                // While the user drives the dummy, capture the dummy's
                // screen for the PiP overlay: the shadow co-sim IS the
                // opponent's game, and its renderer is switched on for
                // exactly these modes.
                if possessed.load(Ordering::Relaxed) {
                    let got = inner_match.read_shadow_video_buffer(&mut shadow_vbuf.lock().unwrap());
                    if got {
                        pip_fresh.store(true, Ordering::Relaxed);
                    }
                } else {
                    pip_fresh.store(false, Ordering::Relaxed);
                }
                // Round-start checkpoint upkeep. While a round exists but the
                // capture hasn't landed (armed rounds decline), keep trying —
                // the first post-commit frame succeeds, which is the tick-0
                // settled bundle. When the round goes away, clear it.
                if inner_match.round_metrics().is_some() {
                    let mut slot = round_start.lock().unwrap();
                    if slot.is_none() {
                        *slot = inner_match.training_checkpoint();
                    }
                } else {
                    round_start.lock().unwrap().take();
                }
                frame_notify.notify_one();
                if completion_token.is_complete() {
                    if !finished.swap(true, Ordering::AcqRel) {
                        if let Err(e) = inner_match.finish_replay() {
                            log::error!("training: finish replay failed: {e}");
                        }
                    }
                    thread_handle.pause();
                }
            }
        });

        Ok(Self {
            local_game,
            local_player_index,
            joyflags,
            dummy,
            last_dummy,
            completion_token,
            _audio_binding: audio_binding,
            thread,
            inner_match,
            match_handle,
            cancellation_token,
            round_start,
            slots: Mutex::new(std::array::from_fn(|_| None)),
            active_slot: AtomicUsize::new(0),
            slot_filled: AtomicUsize::new(0),
            speed: AtomicU32::new(1.0f32.to_bits()),
            show_inputs: AtomicBool::new(false),
            possessed,
            shadow_vbuf,
            pip_fresh,
            local_loaded,
            local_save_view: crate::save_view::State::new(),
            opponent_loaded,
            opponent_save_view: crate::save_view::State::new(),
        })
    }

    pub fn game(&self) -> &'static crate::game::Game {
        self.local_game
    }

    /// Route the resolved controller state to whichever side the user is
    /// currently driving. While possessed the player side holds neutral
    /// (already zeroed by the possess transition), and vice versa.
    pub fn set_joyflags(&self, mgba_keys: u32) {
        if self.possessed.load(Ordering::Relaxed) {
            self.dummy.lock().unwrap().held = mgba_keys;
        } else {
            self.joyflags.store(mgba_keys, Ordering::Relaxed);
        }
    }

    /// The match ends when the per-game match-end hook fires — there is no
    /// peer to hand-shake with, so that's the whole story.
    pub fn is_ended(&self) -> bool {
        self.completion_token.is_complete()
    }

    pub fn is_paused(&self) -> bool {
        self.thread.handle().is_paused()
    }

    pub fn dummy_status(&self) -> DummyStatus {
        let dummy = self.dummy.lock().unwrap();
        DummyStatus {
            mode: dummy.mode,
            script_len: dummy.script.len(),
            play_pos: dummy.play_pos,
            looping: dummy.looping,
        }
    }

    pub fn active_slot(&self) -> usize {
        self.active_slot.load(Ordering::Relaxed)
    }

    pub fn slot_filled(&self, slot: usize) -> bool {
        self.slot_filled.load(Ordering::Relaxed) & (1 << slot) != 0
    }

    /// Whether the current round can be checkpointed / restored right now —
    /// drives the HUD buttons' enabled state.
    pub fn round_live(&self) -> bool {
        self.round_start.lock().unwrap().is_some()
    }

    pub fn speed(&self) -> f32 {
        f32::from_bits(self.speed.load(Ordering::Relaxed))
    }

    pub fn show_inputs(&self) -> bool {
        self.show_inputs.load(Ordering::Relaxed)
    }

    /// Latest dummy-side frame for the PiP overlay, as raw BGR555 —
    /// `None` whenever the dummy view isn't live (not possessing, or no
    /// shadow frame captured yet this possession).
    pub fn pip_pixels(&self) -> Option<Vec<u8>> {
        (self.possessed.load(Ordering::Relaxed) && self.pip_fresh.load(Ordering::Relaxed))
            .then(|| self.shadow_vbuf.lock().unwrap().clone())
    }

    /// Current per-side joyflags for the input display: `(player, dummy)`,
    /// in mgba-keys form. The player half reads the live atomic the trap
    /// consumes; the dummy half is the last injected input.
    pub fn input_display(&self) -> (u32, u32) {
        (
            self.joyflags.load(Ordering::Relaxed),
            self.last_dummy.load(Ordering::Relaxed),
        )
    }

    pub fn dummy_mode(&self) -> DummyMode {
        self.dummy.lock().unwrap().mode
    }

    fn set_dummy_mode(&self, mode: DummyMode) {
        {
            let mut dummy = self.dummy.lock().unwrap();
            dummy.set_mode(mode);
            // Zero both sides on any routing change so nothing stays
            // latched on the side the user just stopped driving.
            dummy.held = 0;
        }
        self.joyflags.store(0, Ordering::Relaxed);
        let possessing = matches!(mode, DummyMode::Possess | DummyMode::Record);
        self.possessed.store(possessing, Ordering::Relaxed);
        // Driving the dummy shows the dummy's screen: rasterize the shadow
        // for exactly these modes (it's frameskipped otherwise), and let
        // the frame callback swap the display.
        self.inner_match.set_shadow_rendering(possessing);
    }

    /// Restore a checkpoint: the match rewinds engine + cores + RNG, the
    /// dummy reacts (playback restarts, a live take stops), and — when
    /// paused — one frame is stepped so the restored state actually shows.
    fn restore(&self, checkpoint: &TrainingCheckpoint) {
        match self.inner_match.restore_training_checkpoint(checkpoint) {
            Ok(true) => {
                self.dummy.lock().unwrap().on_restore();
                if self.is_paused() && !self.is_ended() {
                    self.frame_advance();
                }
            }
            // Nothing to restore into right now (between rounds, armed
            // round, settled round end) — quietly do nothing; the HUD
            // buttons are disabled in those states anyway.
            Ok(false) => {}
            Err(e) => log::error!("training: restore failed: {e:#}"),
        }
    }

    fn frame_advance(&self) {
        let handle = self.thread.handle();
        if !handle.is_paused() {
            // First press pauses; subsequent presses step.
            handle.pause();
            return;
        }
        handle.run_on_core(|mut core| {
            core.run_frame();
        });
    }

    pub fn apply(&self, action: Action) {
        match action {
            Action::SetDummyMode(mode) => self.set_dummy_mode(mode),
            Action::ToggleDummyMode(mode) => {
                let current = self.dummy_mode();
                self.set_dummy_mode(if current == mode { DummyMode::Idle } else { mode });
            }
            Action::ToggleLoop => {
                let mut dummy = self.dummy.lock().unwrap();
                dummy.looping = !dummy.looping;
            }
            Action::RestartRound => {
                let checkpoint = self.round_start.lock().unwrap().clone();
                if let Some(checkpoint) = checkpoint {
                    self.restore(&checkpoint);
                }
            }
            Action::SelectSlot(slot) => {
                if slot < CHECKPOINT_SLOTS {
                    self.active_slot.store(slot, Ordering::Relaxed);
                }
            }
            Action::SaveSlot => {
                let slot = self.active_slot();
                if let Some(checkpoint) = self.inner_match.training_checkpoint() {
                    self.slots.lock().unwrap()[slot] = Some(checkpoint);
                    self.slot_filled.fetch_or(1 << slot, Ordering::Relaxed);
                }
            }
            Action::LoadSlot => {
                let checkpoint = self.slots.lock().unwrap()[self.active_slot()].clone();
                if let Some(checkpoint) = checkpoint {
                    self.restore(&checkpoint);
                }
            }
            Action::TogglePause => {
                let handle = self.thread.handle();
                if handle.is_paused() {
                    // Resuming a completed match would just run past the end
                    // screen the completion pause parked on.
                    if !self.is_ended() {
                        handle.unpause();
                    }
                } else {
                    handle.pause();
                }
            }
            Action::FrameAdvance => self.frame_advance(),
            Action::SetSpeed(factor) => {
                let factor = factor.clamp(SPEED_STEPS[0], SPEED_STEPS[SPEED_STEPS.len() - 1]);
                self.inner_match.set_speed_factor(factor);
                self.speed.store(factor.to_bits(), Ordering::Relaxed);
            }
            Action::ToggleInputDisplay => {
                self.show_inputs.fetch_xor(true, Ordering::Relaxed);
            }
        }
    }

    pub fn request_close(&self) {
        // Make the match handle go inert so traps stop driving the match,
        // and release a pause (user quitting while paused / at completion)
        // so the mgba thread can reach its exit.
        self.cancellation_token.cancel();
        self.thread.handle().unpause();
    }
}

impl Drop for TrainingSession {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
        self.thread.handle().unpause();
        self.match_handle.clear();
    }
}

impl std::fmt::Debug for TrainingSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrainingSession")
            .field("local_player_index", &self.local_player_index)
            .finish_non_exhaustive()
    }
}

/// User-facing knobs collected by the Play tab's training setup and
/// consumed by [`crate::session::spawn_training`].
#[derive(Clone, Debug)]
pub struct TrainingOptions {
    /// The dummy's save. Must be one of the scanner's saves for the same
    /// game (v1 keeps the opponent on the local game + patch; the session
    /// machinery itself already supports diverging ROMs when the setup UI
    /// grows that far).
    pub opponent_save_path: std::path::PathBuf,
    pub match_type: (u8, u8),
    /// Which side the user plays (0 = P1, 1 = P2).
    pub local_player_index: u8,
    /// Free-text seed; empty/None = random. The same text always derives
    /// the same match seed, so drills are shareable.
    pub seed: Option<String>,
}

/// Derive the 16-byte match seed from free-form seed text: FNV-1a folded
/// over the text twice (second lane offset and reversed so the halves
/// decorrelate). Not cryptographic — it just needs to be stable and
/// well-spread.
pub fn seed_from_text(text: &str) -> [u8; 16] {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut lo = OFFSET;
    for b in text.bytes() {
        lo = (lo ^ b as u64).wrapping_mul(PRIME);
    }
    let mut hi = lo ^ 0x9e3779b97f4a7c15;
    for b in text.bytes().rev() {
        hi = (hi ^ b as u64).wrapping_mul(PRIME);
    }
    let mut seed = [0u8; 16];
    seed[..8].copy_from_slice(&lo.to_le_bytes());
    seed[8..].copy_from_slice(&hi.to_le_bytes());
    seed
}

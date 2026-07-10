//! Training session: a drill loop against a dummy, run on the live PvP
//! machinery with the network replaced by an in-process loopback.
//!
//! The whole PvP stack — [`tango_pvp::battle::Match`], the shadow co-sim,
//! the re-sim stepper, the per-game traps, replay recording — is reused
//! unchanged; only the transport differs. [`LoopbackSender`] implements
//! [`tango_pvp::net::Sender`] and answers each local input *synchronously,
//! inside the same primary trap fire*: it asks the [`DummyController`] for
//! the dummy's joyflags and injects them straight into the match's
//! remote-input queue via [`Match::inject_remote_input`]. Every tick
//! confirms immediately — pure lockstep, no async, no rollbacks.
//!
//! The user-facing model is one concept, the **drill**:
//!
//! - **Drill point** — one checkpoint (defaults to the auto-captured round
//!   start). Everything snaps back to it.
//! - **Author** — restart from the drill point with the user acting *as
//!   the dummy* (screen swapped to its perspective), recording everything
//!   it does — chip picks included. Toggling off snaps back, and the dummy
//!   now replays the authored part every rep.
//! - **Reset** — snap back to the drill point. Also automatic when a round
//!   would end: the round-end trap is intercepted
//!   ([`Match::set_training_round_end_reset`]) so a KO restarts the rep
//!   instead of tearing the round down.
//!
//! Determinism is what makes this coherent: from a fixed checkpoint with a
//! fixed seed, the chip draw and all game state repeat exactly, so the
//! authored inputs — even menu cursor movement — replay faithfully.
//! Authored scripts are additionally segmented by phase (field vs the
//! dummy's chip pick, via the per-game custom-screen flags) and each
//! segment plays aligned to its phase, so a rep whose custom timing
//! drifts still lands picks in picks. Outside authored material the dummy
//! falls back to a simple behavior: stand, or tap A to use its chips.
//!
//! [`Match`]: tango_pvp::battle::Match

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tango_pvp::battle::TrainingCheckpoint;

pub use tango_pvp::battle::EXPECTED_FPS;

/// Speed steps offered by the HUD control. 1.0 must be present (the
/// default); real PvP never sees these — the factor only exists because
/// the "peer" is in-process.
pub const SPEED_STEPS: [f32; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

/// UseChips cadence, in ticks between A taps.
const ATTACK_INTERVAL: u32 = 40;

/// What the dummy does outside authored material.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Behavior {
    /// Neutral input.
    Stand,
    /// Tap A on a fixed cadence — fire whatever chips it's holding.
    UseChips,
}

/// Snapshot of the drill state for the HUD (author chip lit, rep
/// readout, behavior chip).
#[derive(Clone, Copy)]
pub struct DrillStatus {
    pub authoring: bool,
    /// Total authored ticks across all segments.
    pub script_ticks: usize,
    /// Authored ticks consumed so far this rep.
    pub played_ticks: usize,
    pub behavior: Behavior,
}

/// Decides the dummy's joyflags, one tick at a time. Lives behind a
/// mutex shared between the loopback sender (emulator thread, once per
/// tick) and the UI (drill actions); neither holds it while calling into
/// the match, so there is no lock-order cycle with the round-state lock.
pub struct DummyController {
    /// The user is acting as the dummy: capture their inputs as the
    /// script instead of playing one back.
    authoring: bool,
    /// mgba-keys bitmap the user's controller currently holds *for the
    /// dummy* — written by the session's input routing while authoring.
    held: u32,
    /// The authored script, segmented by phase: `(is_pick, joyflags per
    /// tick)`. Segments alternate by construction (a new one starts
    /// whenever the phase flips during authoring).
    segments: Vec<(bool, Vec<u16>)>,
    /// Playback cursor.
    seg: usize,
    pos: usize,
    behavior: Behavior,
    /// Tick counter for the UseChips cadence.
    pattern_tick: u32,
}

impl DummyController {
    fn new(behavior: Behavior) -> Self {
        Self {
            authoring: false,
            held: 0,
            segments: Vec::new(),
            seg: 0,
            pos: 0,
            behavior,
            pattern_tick: 0,
        }
    }

    /// One tick's dummy joyflags. Called from the loopback sender on the
    /// emulator thread, exactly once per local input. `picking` is
    /// whether the dummy's chip pick is open right now (always false on
    /// games without the custom-screen flag wiring — playback is then
    /// simply linear).
    fn next(&mut self, picking: bool) -> u16 {
        if self.authoring {
            let joyflags = self.held as u16;
            match self.segments.last_mut() {
                Some((phase, ticks)) if *phase == picking => ticks.push(joyflags),
                _ => self.segments.push((picking, vec![joyflags])),
            }
            return joyflags;
        }
        // Phase-aligned playback. Rules, per tick with current phase P:
        //   - a segment of phase P at the cursor plays;
        //   - a *partially consumed* segment of the other phase was
        //     interrupted by a phase change — abandon its remainder;
        //   - an untouched segment of the other phase waits (the game
        //     will reach that phase; e.g. an authored pick waits for the
        //     custom screen to open);
        //   - past the end of the script, fall back to the behavior.
        for _ in 0..=self.segments.len() {
            while self.pos > 0 && self.segments.get(self.seg).is_some_and(|(_, t)| self.pos >= t.len()) {
                self.seg += 1;
                self.pos = 0;
            }
            let Some((phase, ticks)) = self.segments.get(self.seg) else {
                return self.fallback(picking);
            };
            if *phase == picking {
                if let Some(&joyflags) = ticks.get(self.pos) {
                    self.pos += 1;
                    return joyflags;
                }
                // Empty/exhausted at pos 0 — step over it.
                self.seg += 1;
                continue;
            }
            if self.pos > 0 {
                self.seg += 1;
                self.pos = 0;
                continue;
            }
            return self.fallback(picking);
        }
        self.fallback(picking)
    }

    fn fallback(&mut self, picking: bool) -> u16 {
        if picking {
            return 0;
        }
        match self.behavior {
            Behavior::Stand => 0,
            Behavior::UseChips => {
                self.pattern_tick = self.pattern_tick.wrapping_add(1);
                if self.pattern_tick % ATTACK_INTERVAL == 0 {
                    mgba::input::keys::A as u16
                } else {
                    0
                }
            }
        }
    }

    /// Rewind the playback cursor to the top of the script — every
    /// applied reset does this, so each rep replays from the start.
    fn rewind(&mut self) {
        self.seg = 0;
        self.pos = 0;
        self.pattern_tick = 0;
    }

    /// Begin a fresh take (the previous script is discarded).
    fn start_author(&mut self) {
        self.segments.clear();
        self.rewind();
        self.authoring = true;
        self.held = 0;
    }

    fn stop_author(&mut self) {
        self.authoring = false;
        self.rewind();
        self.held = 0;
    }

    fn clear_script(&mut self) {
        self.segments.clear();
        self.rewind();
    }

    /// A round actually ended (auto-reset off, or nothing to reset to).
    /// The take/script positions are meaningless across the boundary.
    fn on_round_end(&mut self) {
        self.authoring = false;
        self.rewind();
        self.held = 0;
    }

    fn script_ticks(&self) -> usize {
        self.segments.iter().map(|(_, t)| t.len()).sum()
    }

    fn played_ticks(&self) -> usize {
        self.segments.iter().take(self.seg).map(|(_, t)| t.len()).sum::<usize>() + self.pos
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
    /// Mirror of "the user is authoring the dummy" — see
    /// [`TrainingSession::authoring`].
    authoring: Arc<AtomicBool>,
    /// Per-player custom-screen picking flags (see
    /// [`tango_pvp::hooks::PrimaryState::custom_screen_flags`]).
    custom_screen_flags: Arc<AtomicU8>,
    /// The dummy's absolute player index (`1 - local_player_index`).
    dummy_index: u8,
}

impl tango_pvp::net::Sender for LoopbackSender {
    fn send(&mut self, event: &tango_pvp::net::Event) -> std::io::Result<()> {
        let Some(match_) = self.match_.get() else {
            return Ok(());
        };
        match event {
            tango_pvp::net::Event::Input(input) => {
                let picking =
                    self.custom_screen_flags.load(Ordering::Relaxed) & (1 << self.dummy_index) != 0;
                let joyflags = self.dummy.lock().unwrap().next(picking);
                self.last_dummy.store(joyflags as u32, Ordering::Relaxed);
                match_.inject_remote_input(tango_pvp::net::Input {
                    joyflags,
                    tick_advantage: input.tick_advantage,
                });
            }
            tango_pvp::net::Event::EndOfRound => {
                // Only reachable when the drill loop didn't intercept the
                // end (auto-reset off, or nothing to reset to). Unwind any
                // authoring: the inter-round screens need the real
                // controller back.
                match_.inject_remote_end_of_round();
                self.dummy.lock().unwrap().on_round_end();
                self.authoring.store(false, Ordering::Relaxed);
                match_.set_shadow_rendering(false);
            }
        }
        Ok(())
    }
}

/// Everything the training HUD / hotkeys can do to a running session.
/// Routed through `session::Message::Training`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// Snap back to the drill point (finishing an in-progress take
    /// first — reset doubles as "done authoring, go").
    Reset,
    /// Toggle authoring: on = restart from the drill point acting as the
    /// dummy; off = keep the take and restart the rep against it.
    ToggleAuthor,
    /// Checkpoint the current state as the drill point (replacing the
    /// round-start default) and clear the authored script — it was
    /// relative to the old point.
    SetDrillPoint,
    /// Flip the fallback behavior (stand ↔ use chips).
    ToggleBehavior,
    /// Toggle round-end interception (KO → reset instead of round end).
    ToggleAutoReset,
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
    /// checkpoint / speed / reset calls all go straight to the match.
    inner_match: Arc<tango_pvp::battle::Match>,
    match_handle: tango_pvp::hooks::MatchHandle,
    /// Cancelling makes [`MatchHandle::get`] go inert, which is what lets
    /// the mgba thread tear down without traps touching a dying match.
    cancellation_token: tokio_util::sync::CancellationToken,
    /// Auto-captured checkpoint at each round's first commit — the
    /// default drill point. Written by the frame callback (emulator
    /// thread), read by the drill actions (UI thread).
    round_start: Arc<Mutex<Option<TrainingCheckpoint>>>,
    /// The user-set drill point, when they've placed one.
    drill: Mutex<Option<TrainingCheckpoint>>,
    /// Whether a round that would end resets the rep instead.
    auto_reset: Arc<AtomicBool>,
    /// Whether the user is currently acting as the dummy. While set, the
    /// input routing drives the dummy, the main screen shows the dummy's
    /// perspective, and the PiP shows the player's own.
    authoring: Arc<AtomicBool>,
    /// The *other* perspective's frame while authoring (the player's own
    /// screen, for the PiP). Same BGR555 layout as the session vbuf.
    alt_vbuf: Arc<Mutex<Vec<u8>>>,
    /// Whether `alt_vbuf` holds a frame from the current authoring
    /// stretch (cleared otherwise, so nothing stale ever shows).
    pip_fresh: Arc<AtomicBool>,
    /// Speed factor as f32 bits — the HUD's view of what was last set on
    /// the match.
    speed: AtomicU32,
    /// Whether the input-display overlay is on.
    show_inputs: AtomicBool,
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
        behavior: Behavior,
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

        // Per-player custom-screen picking flags, mirrored out by game
        // modules that know where the game keeps them (bn6 today). Games
        // without the wiring leave it zero: authored playback degrades to
        // linear, which is still exact when the rep doesn't shift timing.
        let custom_screen_flags = Arc::new(AtomicU8::new(0));
        local_hooks.install_on_primary(
            &mut core,
            tango_pvp::hooks::PrimaryState {
                joyflags: joyflags.clone(),
                match_: match_handle.clone(),
                completion_token: completion_token.clone(),
                disable_bgm,
                custom_screen_flags: custom_screen_flags.clone(),
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

        let dummy = Arc::new(Mutex::new(DummyController::new(behavior)));
        let last_dummy = Arc::new(AtomicU32::new(0));
        let authoring = Arc::new(AtomicBool::new(false));
        let dummy_index = 1 - local_player_index;
        let match_slot: Arc<OnceLock<Arc<tango_pvp::battle::Match>>> = Arc::new(OnceLock::new());
        let sender = LoopbackSender {
            match_: match_slot.clone(),
            dummy: dummy.clone(),
            last_dummy: last_dummy.clone(),
            authoring: authoring.clone(),
            custom_screen_flags: custom_screen_flags.clone(),
            dummy_index,
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
        let auto_reset = Arc::new(AtomicBool::new(true));
        let alt_vbuf = Arc::new(Mutex::new(vec![
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
            let auto_reset = auto_reset.clone();
            let authoring = authoring.clone();
            let alt_vbuf = alt_vbuf.clone();
            let pip_fresh = pip_fresh.clone();
            let dummy = dummy.clone();
            // Reset watchers (see below).
            let last_resets = AtomicU32::new(0);
            let last_round_end_resets = AtomicU32::new(0);
            // Finalize the replay exactly once when the match completes.
            let finished = AtomicBool::new(false);
            move |mut core, video_buffer, mut thread_handle| {
                core.set_keys(joyflags.load(Ordering::Relaxed));
                let is_authoring = authoring.load(Ordering::Relaxed);
                {
                    // While authoring, the main screen is the dummy's
                    // perspective — the user is playing IT — and the
                    // player's own screen goes to the PiP.
                    let mut vb = vbuf.lock().unwrap();
                    if !(is_authoring && inner_match.read_shadow_video_buffer(&mut vb)) {
                        vb.copy_from_slice(video_buffer);
                    }
                }
                if is_authoring {
                    alt_vbuf.lock().unwrap().copy_from_slice(video_buffer);
                    pip_fresh.store(true, Ordering::Relaxed);
                } else {
                    pip_fresh.store(false, Ordering::Relaxed);
                }
                // Every applied reset (manual or round-end interception)
                // rewinds the script so the rep replays from the top…
                let resets = inner_match.training_reset_count();
                if resets != last_resets.swap(resets, Ordering::Relaxed) {
                    dummy.lock().unwrap().rewind();
                }
                // …and an interception during authoring also ends the take
                // (the KO ended it; the take up to here is kept).
                let intercepts = inner_match.training_round_end_reset_count();
                if intercepts != last_round_end_resets.swap(intercepts, Ordering::Relaxed)
                    && authoring.load(Ordering::Relaxed)
                {
                    dummy.lock().unwrap().stop_author();
                    authoring.store(false, Ordering::Relaxed);
                    joyflags.store(0, Ordering::Relaxed);
                    inner_match.set_shadow_rendering(false);
                }
                // Round-start checkpoint upkeep: the default drill point.
                // Capturing it also arms the round-end interceptor when no
                // user drill point overrides it.
                if inner_match.round_metrics().is_some() {
                    let mut slot = round_start.lock().unwrap();
                    if slot.is_none() {
                        *slot = inner_match.training_checkpoint();
                        if let Some(cp) = slot.as_ref() {
                            if auto_reset.load(Ordering::Relaxed) {
                                inner_match.set_training_round_end_reset(Some(cp.clone()));
                            }
                        }
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
            drill: Mutex::new(None),
            auto_reset,
            authoring,
            alt_vbuf,
            pip_fresh,
            speed: AtomicU32::new(1.0f32.to_bits()),
            show_inputs: AtomicBool::new(false),
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
    /// currently driving. While authoring, the player side holds neutral.
    pub fn set_joyflags(&self, mgba_keys: u32) {
        if self.authoring.load(Ordering::Relaxed) {
            self.dummy.lock().unwrap().held = mgba_keys;
        } else {
            self.joyflags.store(mgba_keys, Ordering::Relaxed);
        }
    }

    /// The match ends when the per-game match-end hook fires — there is no
    /// peer to hand-shake with, so that's the whole story. (With auto-reset
    /// on, rounds never end, so this is reached via Esc-quit or with
    /// auto-reset off.)
    pub fn is_ended(&self) -> bool {
        self.completion_token.is_complete()
    }

    pub fn is_paused(&self) -> bool {
        self.thread.handle().is_paused()
    }

    pub fn is_authoring(&self) -> bool {
        self.authoring.load(Ordering::Relaxed)
    }

    pub fn drill_status(&self) -> DrillStatus {
        let dummy = self.dummy.lock().unwrap();
        DrillStatus {
            authoring: dummy.authoring,
            script_ticks: dummy.script_ticks(),
            played_ticks: dummy.played_ticks(),
            behavior: dummy.behavior,
        }
    }

    /// Whether the drill actions have something to act on — the HUD
    /// buttons' enabled state.
    pub fn round_live(&self) -> bool {
        self.round_start.lock().unwrap().is_some()
    }

    /// Whether the user has placed their own drill point (vs the
    /// round-start default).
    pub fn has_drill_point(&self) -> bool {
        self.drill.lock().unwrap().is_some()
    }

    pub fn auto_reset(&self) -> bool {
        self.auto_reset.load(Ordering::Relaxed)
    }

    pub fn speed(&self) -> f32 {
        f32::from_bits(self.speed.load(Ordering::Relaxed))
    }

    pub fn show_inputs(&self) -> bool {
        self.show_inputs.load(Ordering::Relaxed)
    }

    /// The player's own screen while authoring, for the PiP overlay —
    /// `None` outside authoring.
    pub fn pip_pixels(&self) -> Option<Vec<u8>> {
        (self.authoring.load(Ordering::Relaxed) && self.pip_fresh.load(Ordering::Relaxed))
            .then(|| self.alt_vbuf.lock().unwrap().clone())
    }

    /// Current per-side joyflags for the input display: `(player, dummy)`,
    /// in mgba-keys form.
    pub fn input_display(&self) -> (u32, u32) {
        (
            self.joyflags.load(Ordering::Relaxed),
            self.last_dummy.load(Ordering::Relaxed),
        )
    }

    /// The checkpoint drill actions operate on: the user's drill point,
    /// else the round start.
    fn drill_checkpoint(&self) -> Option<TrainingCheckpoint> {
        self.drill
            .lock()
            .unwrap()
            .clone()
            .or_else(|| self.round_start.lock().unwrap().clone())
    }

    /// Push the current drill point / auto-reset state into the match's
    /// round-end interceptor.
    fn sync_interceptor(&self) {
        let checkpoint = self
            .auto_reset
            .load(Ordering::Relaxed)
            .then(|| self.drill_checkpoint())
            .flatten();
        self.inner_match.set_training_round_end_reset(checkpoint);
    }

    fn restore(&self, checkpoint: &TrainingCheckpoint) {
        match self.inner_match.restore_training_checkpoint(checkpoint) {
            // The reset-counter watcher in the frame callback rewinds the
            // script; when paused, step one frame so the restored state
            // actually shows (and the watcher runs).
            Ok(true) => {
                if self.is_paused() && !self.is_ended() {
                    self.frame_advance();
                }
            }
            // Nothing to restore into right now (armed round) — quietly do
            // nothing; the HUD gates on round_live anyway.
            Ok(false) => {}
            Err(e) => log::error!("training: reset failed: {e:#}"),
        }
    }

    /// End an in-progress take and stop routing input to the dummy.
    /// Returns whether a take was actually in progress.
    fn finish_authoring(&self) -> bool {
        let was = {
            let mut dummy = self.dummy.lock().unwrap();
            let was = dummy.authoring;
            if was {
                dummy.stop_author();
            }
            was
        };
        if was {
            self.authoring.store(false, Ordering::Relaxed);
            self.joyflags.store(0, Ordering::Relaxed);
            self.inner_match.set_shadow_rendering(false);
        }
        was
    }

    fn toggle_author(&self) {
        let Some(checkpoint) = self.drill_checkpoint() else {
            return;
        };
        if !self.finish_authoring() {
            // Start a take: act as the dummy from the drill point.
            self.dummy.lock().unwrap().start_author();
            self.authoring.store(true, Ordering::Relaxed);
            self.joyflags.store(0, Ordering::Relaxed);
            self.inner_match.set_shadow_rendering(true);
        }
        // Either way the rep restarts from the drill point: into the take,
        // or immediately against it.
        self.restore(&checkpoint);
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
            Action::Reset => {
                // Reset doubles as "done authoring, go".
                self.finish_authoring();
                if let Some(checkpoint) = self.drill_checkpoint() {
                    self.restore(&checkpoint);
                }
            }
            Action::ToggleAuthor => self.toggle_author(),
            Action::SetDrillPoint => {
                if let Some(checkpoint) = self.inner_match.training_checkpoint() {
                    *self.drill.lock().unwrap() = Some(checkpoint);
                    // The authored script was relative to the old point.
                    self.dummy.lock().unwrap().clear_script();
                    self.sync_interceptor();
                }
            }
            Action::ToggleBehavior => {
                let mut dummy = self.dummy.lock().unwrap();
                dummy.behavior = match dummy.behavior {
                    Behavior::Stand => Behavior::UseChips,
                    Behavior::UseChips => Behavior::Stand,
                };
            }
            Action::ToggleAutoReset => {
                self.auto_reset.fetch_xor(true, Ordering::Relaxed);
                self.sync_interceptor();
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
    /// What the dummy does outside authored material.
    pub behavior: Behavior,
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

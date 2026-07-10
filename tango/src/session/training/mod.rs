//! Training session: a drill loop against a scripted dummy, run on the
//! live PvP machinery with the network replaced by an in-process source.
//!
//! The whole PvP stack — [`tango_pvp::battle::Match`], the shadow co-sim,
//! the re-sim stepper, the per-game traps, replay recording — is reused
//! unchanged; the match is simply constructed with
//! [`Remote::Training`](tango_pvp::battle::Remote) instead of a networked
//! peer: the live round asks the installed source for the dummy's
//! joyflags once per local input, inside the same primary trap fire, with
//! the live core in hand. Every tick confirms immediately — pure
//! lockstep, no async, no rollbacks. The [`LoopbackSender`] is what's
//! left of the "network": it counts real round ends for the script's rep
//! epoch and drops everything else.
//!
//! The dummy starts scriptless ([`NeutralDummy`] — stands still); a
//! script is picked, switched, and reloaded live from the training bar. A
//! [`script::ScriptDummy`] reads whatever game memory it wants and
//! answers synchronously — see [`script`] for the API scripts get.
//!
//! The user-facing model is one concept, the **drill**:
//!
//! - **Drill point** — one checkpoint (defaults to the auto-captured round
//!   start). Everything snaps back to it.
//! - **Reset** — snap back to the drill point. Also automatic when a round
//!   would end: the round-end trap is intercepted
//!   ([`Match::set_training_round_end_reset`]) so a KO restarts the rep
//!   instead of tearing the round down. Every reset rewinds the script (a
//!   fresh rep: `on_reset` fires, the script RNG reseeds) and picks up an
//!   edited script file — edit, hit Reset, and the new behavior runs
//!   against the identical checkpoint.
//!
//! Determinism is what makes this coherent: from a fixed checkpoint with a
//! fixed seed, the chip draw and all game state repeat exactly, and a pure
//! script replays exactly with them; `rand()` draws vary per rep but
//! reproducibly (seeded from the match seed + rep counter).
//!
//! [`Match`]: tango_pvp::battle::Match
//! [`Match::set_training_round_end_reset`]: tango_pvp::battle::Match::set_training_round_end_reset

pub mod script;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tango_pvp::battle::TrainingCheckpoint;

pub use tango_pvp::battle::EXPECTED_FPS;

/// Speed steps offered by the HUD control. 1.0 must be present (the
/// default); real PvP never sees these — the factor only exists because
/// the "peer" is in-process.
pub const SPEED_STEPS: [f32; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

/// A dummy script on disk (`*.lua` / `*.rhai`, from the user's scripts
/// dir). The extension picks the backend; the file hot-reloads on Reset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptSource(pub std::path::PathBuf);

impl ScriptSource {
    /// The name the backend dispatches on (extension) and the HUD shows.
    pub fn label(&self) -> String {
        self.0
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.0.display().to_string())
    }

    fn read(&self) -> anyhow::Result<String> {
        Ok(std::fs::read_to_string(&self.0)?)
    }

    /// File mtime, for the reload-on-Reset check. `None` on stat errors
    /// (a vanished file just stops hot-reloading).
    fn mtime(&self) -> Option<std::time::SystemTime> {
        std::fs::metadata(&self.0).and_then(|m| m.modified()).ok()
    }
}

/// Every script the bar picker can offer: `dir`'s `*.lua` / `*.rhai`,
/// sorted by file name. The dir is created on the way (so there's an
/// obvious place to drop scripts) but a create/read failure just means an
/// empty list.
pub fn scan_scripts(dir: &std::path::Path) -> Vec<ScriptSource> {
    let _ = std::fs::create_dir_all(dir);
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("lua") | Some("rhai")))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files.into_iter().map(ScriptSource).collect()
}

/// The scriptless dummy: stands still. Some source must always be
/// installed — the lockstep engine pairs exactly one remote input with
/// every local one — so "no script" is a source that answers neutral.
struct NeutralDummy;

impl tango_pvp::battle::TrainingRemoteSource for NeutralDummy {
    fn next_joyflags(&mut self, _core: mgba::core::CoreMutRef<'_>) -> u16 {
        0
    }
}

/// Possession: while `active`, the user's controller drives the dummy.
/// Written by the UI thread (the bar toggle + input routing), read by
/// [`DummySource`] on the emulator thread.
#[derive(Default)]
struct PossessState {
    active: AtomicBool,
    /// mgba-keys bitmap the controller currently holds *for the dummy*.
    joyflags: AtomicU32,
}

/// What's actually installed on the match: the picked brain (script or
/// neutral), with possession layered over it. While possessed the script
/// isn't ticked at all — its rep clock pauses and resumes untouched.
struct DummySource {
    inner: Box<dyn tango_pvp::battle::TrainingRemoteSource>,
    possess: Arc<PossessState>,
}

impl tango_pvp::battle::TrainingRemoteSource for DummySource {
    fn next_joyflags(&mut self, core: mgba::core::CoreMutRef<'_>) -> u16 {
        if self.possess.active.load(Ordering::Relaxed) {
            return self.possess.joyflags.load(Ordering::Relaxed) as u16;
        }
        self.inner.next_joyflags(core)
    }
}

/// The in-process "network": [`tango_pvp::net::Sender`] whose peer is the
/// dummy. `Input` events need no answer here — the round asks the match's
/// [`Remote::Training`](tango_pvp::battle::Remote) source inside the same
/// trap fire — so this only counts `EndOfRound`s, folding real round ends
/// into the script's rep epoch so a new round reads as a new rep.
struct LoopbackSender {
    rounds_ended: Arc<AtomicU32>,
}

impl tango_pvp::net::Sender for LoopbackSender {
    fn send(&mut self, event: &tango_pvp::net::Event) -> std::io::Result<()> {
        match event {
            tango_pvp::net::Event::Input(_) => {}
            tango_pvp::net::Event::EndOfRound => {
                self.rounds_ended.fetch_add(1, Ordering::Release);
            }
        }
        Ok(())
    }
}

/// Everything the training HUD / hotkeys can do to a running session.
/// Routed through `session::Message::Training`.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Snap back to the drill point. Also hot-reloads the script when its
    /// file changed on disk — Reset is the drill loop's "go again".
    Reset,
    /// Reload the current script unconditionally, then snap back to the
    /// drill point. No-op on a scriptless dummy.
    ReloadScript,
    /// Switch the dummy to a different script — or none — from the bar's
    /// picker, then snap back to the drill point.
    SetScript(Option<ScriptSource>),
    /// Checkpoint the current state as the drill point (replacing the
    /// round-start default).
    SetDrillPoint,
    /// Toggle round-end interception (KO → reset instead of round end).
    ToggleAutoReset,
    TogglePause,
    FrameAdvance,
    SetSpeed(f32),
    /// Toggle possession: the user's controller drives the dummy, the
    /// main screen swaps to its perspective (own screen in the PiP slot),
    /// and the script pauses. Dropped automatically when a round really
    /// ends — the inter-round screens need the real controller back.
    TogglePossess,
    /// Toggle the dummy-screen picture-in-picture (the shadow's render).
    /// Per-session, unlike the replay PiP — it doesn't touch the config
    /// preference.
    TogglePip,
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
    /// The dummy's current script — `None` is the scriptless stand-still
    /// dummy. Picked and switched live from the training bar.
    script_source: Mutex<Option<ScriptSource>>,
    /// Mtime of the script file at (re)load, for the reload-on-Reset
    /// check.
    script_mtime: Mutex<Option<std::time::SystemTime>>,
    /// The user's scripts dir, for the bar picker's rescans.
    scripts_dir: std::path::PathBuf,
    /// The scripts dir contents, for the bar picker. Scanned at
    /// construction and rescanned on the drill actions (Reset / reload /
    /// switch) — a new file shows up after the next F1.
    available_scripts: Mutex<Vec<ScriptSource>>,
    /// Error + last-joyflags surface shared with the running
    /// [`script::ScriptDummy`]; both threads only take short locks.
    script_status: Arc<script::ScriptStatus>,
    /// The script's rep epoch: applied resets + real round ends. Written
    /// by the frame callback (which owns the round-end counter half);
    /// the dummy notices a change on its next tick and rewinds (see
    /// [`script::ScriptDummy`]).
    reset_epoch: Arc<AtomicU32>,
    rng_seed: [u8; 16],
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
    /// Possession state, shared with the installed [`DummySource`].
    possess: Arc<PossessState>,
    /// Whether the dummy-screen PiP is on (a per-session toggle on the
    /// training bar; flips the shadow's rasterizer with it).
    show_pip: Arc<AtomicBool>,
    /// The shadow's screen, copied once per frame while the PiP is on.
    /// Same BGR555 layout as the session vbuf.
    pip_vbuf: Arc<Mutex<Vec<u8>>>,
    /// Whether `pip_vbuf` holds a frame from the current PiP activation
    /// (cleared while off, so a stale capture never flashes on re-toggle).
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
    /// no handoff, so unlike PvP there is nothing to await. The dummy
    /// starts scriptless; the training bar picks its script live.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        local_game: &'static crate::game::Game,
        rom: Arc<Vec<u8>>,
        local_save: Box<dyn tango_dataview::save::Save + Send + Sync>,
        opponent_save: Box<dyn tango_dataview::save::Save + Send + Sync>,
        match_type: (u8, u8),
        wanted_player_index: u8,
        rng_seed: [u8; 16],
        scripts_dir: std::path::PathBuf,
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

        let rounds_ended = Arc::new(AtomicU32::new(0));
        let possess: Arc<PossessState> = Arc::default();
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
            Box::new(LoopbackSender {
                rounds_ended: rounds_ended.clone(),
            }),
            tango_pvp::battle::Remote::Training(Box::new(DummySource {
                inner: Box::new(NeutralDummy),
                possess: possess.clone(),
            })),
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
        match_handle.set(inner_match.clone());

        let reset_epoch = Arc::new(AtomicU32::new(0));
        let script_status = Arc::new(script::ScriptStatus::default());

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        vbuf.lock().unwrap().fill(0);
        let audio_binding = audio_binder.bind_mgba(thread.handle(), "training");

        let round_start: Arc<Mutex<Option<TrainingCheckpoint>>> = Arc::default();
        let auto_reset = Arc::new(AtomicBool::new(true));
        let show_pip = Arc::new(AtomicBool::new(false));
        let pip_vbuf = Arc::new(Mutex::new(vec![
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
            let reset_epoch = reset_epoch.clone();
            let rounds_ended = rounds_ended.clone();
            let show_pip = show_pip.clone();
            let pip_vbuf = pip_vbuf.clone();
            let pip_fresh = pip_fresh.clone();
            let possess = possess.clone();
            // Auto-unpossess watcher (see below).
            let last_rounds_ended = AtomicU32::new(0);
            // Finalize the replay exactly once when the match completes.
            let finished = AtomicBool::new(false);
            move |mut core, video_buffer, mut thread_handle| {
                core.set_keys(joyflags.load(Ordering::Relaxed));
                // A real round end drops possession: the inter-round
                // screens need the real controller back. (Round-end
                // interception doesn't come through here — possession
                // survives the drill loop's resets.)
                let ended = rounds_ended.load(Ordering::Acquire);
                if ended != last_rounds_ended.swap(ended, Ordering::Relaxed) {
                    possess.active.store(false, Ordering::Relaxed);
                    possess.joyflags.store(0, Ordering::Relaxed);
                }
                let possessing = possess.active.load(Ordering::Relaxed);
                {
                    // While possessing, the main screen is the dummy's
                    // perspective — the user is playing IT — and the
                    // player's own screen goes to the PiP slot.
                    let mut vb = vbuf.lock().unwrap();
                    if !(possessing && inner_match.read_shadow_video_buffer(&mut vb)) {
                        vb.copy_from_slice(video_buffer);
                    }
                }
                // The PiP slot: the player's own screen while possessing,
                // else the shadow's render while the toggle is on.
                if possessing {
                    pip_vbuf.lock().unwrap().copy_from_slice(video_buffer);
                    pip_fresh.store(true, Ordering::Relaxed);
                } else if show_pip.load(Ordering::Relaxed) {
                    if inner_match.read_shadow_video_buffer(&mut pip_vbuf.lock().unwrap()) {
                        pip_fresh.store(true, Ordering::Relaxed);
                    }
                } else {
                    pip_fresh.store(false, Ordering::Relaxed);
                }
                // The script's rep epoch: every applied reset (manual or
                // round-end interception) and every real round end starts a
                // fresh rep — the dummy notices the bump on its next tick
                // and rewinds its script state + RNG.
                reset_epoch.store(
                    inner_match
                        .training_reset_count()
                        .wrapping_add(rounds_ended.load(Ordering::Acquire)),
                    Ordering::Release,
                );
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

        let available_scripts = Mutex::new(scan_scripts(&scripts_dir));
        Ok(Self {
            local_game,
            local_player_index,
            joyflags,
            script_source: Mutex::new(None),
            script_mtime: Mutex::new(None),
            scripts_dir,
            available_scripts,
            script_status,
            reset_epoch,
            rng_seed,
            completion_token,
            _audio_binding: audio_binding,
            thread,
            inner_match,
            match_handle,
            cancellation_token,
            round_start,
            drill: Mutex::new(None),
            auto_reset,
            possess,
            show_pip,
            pip_vbuf,
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
    /// currently driving. While possessing, the player side holds neutral.
    pub fn set_joyflags(&self, mgba_keys: u32) {
        if self.possess.active.load(Ordering::Relaxed) {
            self.possess.joyflags.store(mgba_keys, Ordering::Relaxed);
        } else {
            self.joyflags.store(mgba_keys, Ordering::Relaxed);
        }
    }

    /// Whether the user is currently driving the dummy — the bar chip's
    /// lit state.
    pub fn is_possessing(&self) -> bool {
        self.possess.active.load(Ordering::Relaxed)
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

    /// The dummy's current script — the bar picker's selection. `None` is
    /// the scriptless stand-still dummy.
    pub fn script_source(&self) -> Option<ScriptSource> {
        self.script_source.lock().unwrap().clone()
    }

    /// Every script the bar picker offers. Rescanned on the drill
    /// actions, so it's fresh whenever the user just did something.
    pub fn available_scripts(&self) -> Vec<ScriptSource> {
        self.available_scripts.lock().unwrap().clone()
    }

    fn rescan_scripts(&self) {
        *self.available_scripts.lock().unwrap() = scan_scripts(&self.scripts_dir);
    }

    /// The latched script error, if the dummy is dead — the HUD's error
    /// chip. Cleared by a reset or a successful reload.
    pub fn script_error(&self) -> Option<String> {
        self.script_status.error.lock().unwrap().clone()
    }

    /// Whether the dummy-screen PiP is on — drives the bar toggle's lit
    /// state.
    pub fn show_pip(&self) -> bool {
        self.show_pip.load(Ordering::Relaxed)
    }

    /// Latest dummy-side frame for the PiP overlay, as raw BGR555 —
    /// `None` while the PiP is off or before its first captured frame.
    pub fn pip_pixels(&self) -> Option<Vec<u8>> {
        (self.show_pip.load(Ordering::Relaxed) && self.pip_fresh.load(Ordering::Relaxed))
            .then(|| self.pip_vbuf.lock().unwrap().clone())
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

    /// Current per-side joyflags for the input display: `(player, dummy)`,
    /// in mgba-keys form.
    pub fn input_display(&self) -> (u32, u32) {
        if self.possess.active.load(Ordering::Relaxed) {
            (0, self.possess.joyflags.load(Ordering::Relaxed))
        } else {
            (
                self.joyflags.load(Ordering::Relaxed),
                self.script_status.last_joyflags.load(Ordering::Relaxed),
            )
        }
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
            // The reset bumps the script's rep epoch via the frame
            // callback; when paused, step one frame so the restored state
            // actually shows (and the epoch update runs).
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

    /// Load `source` and install it as the dummy's brain. On a load
    /// failure the dummy goes neutral with the error latched on the HUD —
    /// the selection sticks, so the user sees which file is broken.
    fn set_script(&self, source: Option<ScriptSource>) {
        *self.script_mtime.lock().unwrap() = source.as_ref().and_then(|s| s.mtime());
        *self.script_source.lock().unwrap() = source.clone();
        let inner: Box<dyn tango_pvp::battle::TrainingRemoteSource> = match &source {
            None => Box::new(NeutralDummy),
            Some(src) => {
                let loaded = src
                    .read()
                    .and_then(|text| script::load_script(&src.label(), &text, 1 - self.local_player_index, self.rng_seed));
                match loaded {
                    Ok(loaded) => Box::new(script::ScriptDummy::new(
                        loaded,
                        self.rng_seed,
                        self.reset_epoch.clone(),
                        self.script_status.clone(),
                    )),
                    Err(e) => {
                        log::warn!("training: script load failed: {e:#}");
                        *self.script_status.error.lock().unwrap() = Some(format!("{e:#}"));
                        self.script_status.last_joyflags.store(0, Ordering::Relaxed);
                        self.install_source(Box::new(NeutralDummy));
                        return;
                    }
                }
            }
        };
        *self.script_status.error.lock().unwrap() = None;
        self.script_status.last_joyflags.store(0, Ordering::Relaxed);
        self.install_source(inner);
    }

    /// Wrap a brain in the possession layer and swap it into the match.
    fn install_source(&self, inner: Box<dyn tango_pvp::battle::TrainingRemoteSource>) {
        self.inner_match.set_training_remote_source(Box::new(DummySource {
            inner,
            possess: self.possess.clone(),
        }));
    }

    /// The shadow rasterizes whenever something shows its pixels: the
    /// dummy-screen PiP, or the main screen while possessing.
    fn sync_shadow_rendering(&self) {
        self.inner_match
            .set_shadow_rendering(self.possess.active.load(Ordering::Relaxed) || self.show_pip.load(Ordering::Relaxed));
    }

    /// Reload on Reset iff the script file changed since the last load —
    /// the edit-run loop without ever leaving the game.
    fn maybe_reload_script(&self) {
        let source = self.script_source.lock().unwrap().clone();
        let Some(src) = source else {
            return;
        };
        if src.mtime() != *self.script_mtime.lock().unwrap() {
            self.set_script(Some(src));
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

    /// The drill actions all end the same way: rescan the picker's list
    /// (the user is at the keyboard — a new file should show up) and
    /// restart the rep from the drill point.
    fn rescan_and_restore(&self) {
        self.rescan_scripts();
        if let Some(checkpoint) = self.drill_checkpoint() {
            self.restore(&checkpoint);
        }
    }

    pub fn apply(&self, action: Action) {
        match action {
            Action::Reset => {
                self.maybe_reload_script();
                self.rescan_and_restore();
            }
            Action::ReloadScript => {
                if let Some(src) = self.script_source.lock().unwrap().clone() {
                    self.set_script(Some(src));
                }
                self.rescan_and_restore();
            }
            Action::SetScript(source) => {
                self.set_script(source);
                self.rescan_and_restore();
            }
            Action::SetDrillPoint => {
                if let Some(checkpoint) = self.inner_match.training_checkpoint() {
                    *self.drill.lock().unwrap() = Some(checkpoint);
                    self.sync_interceptor();
                }
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
            Action::TogglePossess => {
                self.possess.active.fetch_xor(true, Ordering::Relaxed);
                // Both sides start neutral across the handoff — whichever
                // side the controller was driving, its held keys don't
                // carry over.
                self.possess.joyflags.store(0, Ordering::Relaxed);
                self.joyflags.store(0, Ordering::Relaxed);
                self.sync_shadow_rendering();
            }
            Action::TogglePip => {
                self.show_pip.fetch_xor(true, Ordering::Relaxed);
                self.sync_shadow_rendering();
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
/// consumed by [`crate::session::spawn_training`]. The dummy's script
/// isn't one of them — it's picked live from the training bar.
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

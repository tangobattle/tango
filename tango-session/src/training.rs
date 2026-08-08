//! Training-mode emulator session: a real link battle you fight
//! locally, against a **dummy controller** on the opponent core.
//!
//! Mechanically this is a netplay match with the network cut out: the
//! game's own registration starts it, and both seats' input is supplied
//! locally before the tick advances. Both cores run the player's own ROM + save
//! (a mirror match), primed all the way into their link battle exactly
//! as a netplay match would be — so training *starts in a battle*, not
//! at the title screen. The player drives one core; the other core's
//! input each tick comes from a [`TrainingController`].
//!
//! Out of the box that controller does nothing: the stock
//! [`NoopController`] presses no buttons, so the opponent just stands
//! there. The driver layers the session's [`DummyPolicy`] on top —
//! by default it closes the dummy's custom screen automatically (a
//! link battle only resumes once BOTH players confirm, so a dummy that
//! never confirms soft-locks the match on its first chip select), or
//! it can hand the player over to the dummy for the pick instead. A
//! richer controller can be swapped in at any time with
//! [`TrainingSession::set_controller`]; it gets the battle facts the
//! driver already tracks through [`ControllerContext`].
//!
//! The battle runs entirely off in-memory SRAM, so nothing a training
//! session does is written back to the player's `.sav` on disk. There is
//! no netcode, no throttling and no rollback churn: the dummy's input for
//! each tick is supplied locally before that tick advances, so the pair
//! runs in perfect lockstep.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tango_match::telemetry::Event;

/// Single battle. Training always fights one round against the dummy;
/// there's no lobby to pick a mode, and the default do-nothing opponent
/// makes best-of-N pointless.
const TRAINING_MATCH_TYPE: (u8, u8) = (0, 0);

/// What the drive loop hands a [`TrainingController`] each tick: which
/// core is which, plus the battle facts the driver tracks off the
/// pair's confirmed telemetry. A controller reads these and returns
/// the joyflags the dummy should hold for the tick about to advance.
pub struct ControllerContext {
    /// The core the dummy drives (the non-human core).
    pub dummy_player: usize,
    /// The core the human drives.
    pub human_player: usize,
    /// Ticks elapsed since the battle started (0 on the first poll).
    pub frame: u64,
    /// Whether each player's custom (chip-select) screen stands open,
    /// by absolute player — a tick or two behind the frontier (it
    /// comes off the confirmed telemetry), which no multi-frame
    /// reaction cares about.
    pub custom_open: [bool; 2],
}

/// How the dummy's custom screen gets handled — the one part of a
/// battle a do-nothing dummy cannot be allowed to do nothing about: a
/// link battle only resumes once BOTH players confirm their picks.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DummyPolicy {
    /// The driver confirms for the dummy: once its screen has stood
    /// open a moment, a scripted START→A closes it with the game's
    /// own picks (which a forced hand then overwrites). The default.
    #[default]
    AutoConfirm,
    /// Control switches to the dummy while its screen is open — the
    /// player makes the dummy's picks — and hands back on confirm.
    AutoPossess,
    /// Nothing: the player deals with the dummy's screen by swapping
    /// manually (today's behaviour, and what a custom controller that
    /// handles the screen itself wants).
    Manual,
}

impl DummyPolicy {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => DummyPolicy::AutoPossess,
            2 => DummyPolicy::Manual,
            _ => DummyPolicy::AutoConfirm,
        }
    }

    /// The next policy in the cycle a single toggle button walks.
    pub fn next(self) -> Self {
        match self {
            DummyPolicy::AutoConfirm => DummyPolicy::AutoPossess,
            DummyPolicy::AutoPossess => DummyPolicy::Manual,
            DummyPolicy::Manual => DummyPolicy::AutoConfirm,
        }
    }
}

/// Where the dummy's recorded drill stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DrillMode {
    /// No drill running (there may still be a recorded take).
    #[default]
    Off,
    /// The player is on the dummy's seat and their inputs are being
    /// captured as the take.
    Recording,
    /// The dummy is replaying the take, looping.
    Playing,
}

impl DrillMode {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => DrillMode::Recording,
            2 => DrillMode::Playing,
            _ => DrillMode::Off,
        }
    }
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
    /// Produce the dummy's input for the tick about to advance. Return a
    /// joyflag bitmap (the pad half of what
    /// [`crate::Session::set_input`] carries); return `0` to press
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
    /// The engine's native frame rate — what the speed dial's 1.0× means.
    expected_fps: f32,
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
    /// The console's screens, as the game's engine presents them —
    /// what the session's surfaces are sized for.
    layout: tango_match::ScreenLayout,
    /// Training's write-side control (see [`tango_match::trainer`]):
    /// forced hands land here and the engine's per-game trainer applies
    /// them. Wired iff this game's support offers a trainer.
    control: Arc<tango_match::trainer::TrainerControl>,
    /// The standing [`DummyPolicy`], as its discriminant.
    policy: Arc<AtomicU8>,
    /// The standing [`DrillMode`], as its discriminant.
    drill_mode: Arc<AtomicU8>,
    /// The recorded take: raw joyflags per battle tick (ticks under
    /// the shared custom pause excluded on both sides of the trip).
    drill: Arc<Mutex<Vec<u32>>>,
    /// Bumped on every manual swap. The driver's auto-possession
    /// records it going in and drops its restore if it moved — a
    /// player who swapped mid-possession chose a side, and the
    /// hand-back must not fight them.
    swap_generation: Arc<AtomicU32>,
    /// Latched once the battle's own match-end path fires — flips
    /// [`is_ended`](crate::Session::is_ended) so the host tears the
    /// session down.
    ended: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    /// Cleared by [`TrainingBoot`] once the pair is up. The session is
    /// installed and on screen while the priming walk runs on the drive
    /// thread, so the host reads this to show its priming notice.
    booting: Arc<AtomicBool>,
    /// Aborts a priming walk still in flight when the session closes,
    /// so the host's drive-thread join doesn't wait out the emulation
    /// the user just walked away from.
    boot_cancel: Arc<AtomicBool>,
    /// Why the boot failed, for the host to show — the session stays up
    /// with nothing to run (see [`TrainingBoot`]).
    prime_error: Arc<Mutex<Option<crate::Error>>>,
    screen: Arc<crate::Framebuffer>,
    wake: Arc<tokio::sync::Notify>,
}

impl TrainingSession {
    /// Set up a training battle with `controller` as the dummy's input
    /// source (pass `Box::new(NoopController)` for the do-nothing
    /// default). Both cores run `rom` + `save_sram` (a mirror match); the
    /// SRAM is in-memory, so nothing persists back to disk.
    ///
    /// The pair boots and primes on the drive thread's first tick (the
    /// returned [`TrainingBoot`] — hand it to the drive thread), so the
    /// session is installed and on screen while the seconds-long walk
    /// runs; the host shows its priming notice off
    /// [`is_booting`](Self::is_booting) until it lands. Also returns
    /// the session's audio stream (the controlled core's samples
    /// resampled to `sample_rate`) for the host to route to its output;
    /// dropping it just costs sound.
    pub fn new(
        game: &'static tango_gamesupport::Game,
        rom: Arc<Vec<u8>>,
        save_sram: Vec<u8>,
        rtc: std::time::SystemTime,
        rng_seed: [u8; 16],
        expected_fps: f32,
        sample_rate: u32,
        controller: Box<dyn TrainingController>,
    ) -> Result<(Self, TrainingBoot, crate::audio::Stream), crate::Error> {
        // The engine gets a head start on the pair the boot will run —
        // a browser engine's worker threads need event-loop turns that
        // happen between here and the boot's first tick.
        game.pvp.prepare(2);

        // The session's audio ring, made before the pair that feeds it
        // exists: the host binds this stream at construction, and the
        // ring just reads empty until the boot hands the producing end
        // to the pair.
        let (audio_in, audio_out) = crate::audio::ring();
        let control = tango_match::trainer::TrainerControl::new();
        let controlled = Arc::new(AtomicUsize::new(0));
        let joyflags = Arc::new(AtomicU32::new(0));
        let controller: SharedController = Arc::new(Mutex::new(controller));
        let policy = Arc::new(AtomicU8::new(DummyPolicy::default() as u8));
        let drill_mode = Arc::new(AtomicU8::new(DrillMode::default() as u8));
        let drill = Arc::new(Mutex::new(Vec::new()));
        let swap_generation = Arc::new(AtomicU32::new(0));
        let fps_bits = Arc::new(AtomicU32::new(expected_fps.to_bits()));
        let dummy_joyflags = Arc::new(AtomicU32::new(0));
        let show_pip = Arc::new(AtomicBool::new(false));
        // A primed pair, same as netplay — the dummy seat is the pair's
        // other console, not a solo boot.
        let layout = game.pvp.screen_layout(tango_match::SessionMode::PvP {
            match_type: TRAINING_MATCH_TYPE,
        });
        let pip = crate::Framebuffer::new(&layout);
        let pip_fresh = Arc::new(AtomicBool::new(false));
        let ended = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let screen = crate::Framebuffer::new(&layout);
        let wake = Arc::new(tokio::sync::Notify::new());

        // Audio comes off whichever core the player is driving (same
        // path as PvP), rate control following the pacing target. A swap
        // tells the match to listen to the other seat, so the sound
        // follows the player without anything here being rebuilt.
        let audio = crate::audio::Stream::new(
            audio_out,
            expected_fps,
            crate::audio::Stream::fps_from_bits(fps_bits.clone()),
            sample_rate,
        );

        let booting = Arc::new(AtomicBool::new(true));
        let boot_cancel = Arc::new(AtomicBool::new(false));
        let prime_error = Arc::new(Mutex::new(None));
        let boot = TrainingBoot {
            pending: Some(BootPieces {
                game,
                rom,
                save_sram,
                rtc,
                rng_seed,
                control: control.clone(),
                audio: audio_in,
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
                policy: policy.clone(),
                drill_mode: drill_mode.clone(),
                drill: drill.clone(),
                swap_generation: swap_generation.clone(),
            }),
            driver: None,
            prime_error: prime_error.clone(),
            booting: booting.clone(),
            boot_cancel: boot_cancel.clone(),
            wake: wake.clone(),
            fps_bits: fps_bits.clone(),
            backend: game.pvp,
        };

        Ok((
            Self {
                game,
                controlled,
                joyflags,
                controller,
                expected_fps,
                fps_bits,
                dummy_joyflags,
                show_pip,
                pip,
                pip_fresh,
                control,
                policy,
                drill_mode,
                drill,
                swap_generation,
                ended,
                stop,
                booting,
                boot_cancel,
                prime_error,
                layout,
                screen,
                wake,
            },
            boot,
            audio,
        ))
    }

    /// `true` while the pair is still booting/priming on the drive
    /// thread — the session is on screen with no frame and no sound
    /// until it lands, so the host shows its priming notice.
    pub fn is_booting(&self) -> bool {
        self.booting.load(Ordering::Acquire)
    }

    /// Why the boot failed, ready to show, or `None` while it is still
    /// running or has succeeded. A failed boot leaves the session up
    /// with nothing to run: there is no frame coming, and this is the
    /// only thing left to tell the user.
    pub fn prime_error(&self) -> Option<String> {
        self.prime_error.lock().unwrap().as_ref().map(|e| e.to_string())
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
        self.swap_generation.fetch_add(1, Ordering::Relaxed);
        self.controlled.fetch_xor(1, Ordering::Relaxed);
    }

    /// How the dummy's custom screen is handled (see [`DummyPolicy`]).
    pub fn policy(&self) -> DummyPolicy {
        DummyPolicy::from_u8(self.policy.load(Ordering::Relaxed))
    }

    /// Set the dummy-screen policy. Takes effect on the next tick.
    pub fn set_policy(&self, policy: DummyPolicy) {
        self.policy.store(policy as u8, Ordering::Relaxed);
    }

    /// Step to the next policy in the cycle (the bar's toggle).
    pub fn cycle_policy(&self) {
        self.set_policy(self.policy().next());
    }

    /// Where the dummy's drill stands (see [`DrillMode`]).
    pub fn drill_mode(&self) -> DrillMode {
        DrillMode::from_u8(self.drill_mode.load(Ordering::Relaxed))
    }

    /// Whether a recorded take exists (playable even while off).
    pub fn has_drill(&self) -> bool {
        !self.drill.lock().unwrap().is_empty()
    }

    /// Start or stop recording a drill. Starting swaps the player onto
    /// the dummy's seat (their inputs become the take) and discards
    /// any previous take; stopping swaps back and, if anything was
    /// captured, starts the dummy looping it immediately.
    pub fn toggle_record(&self) {
        match self.drill_mode() {
            DrillMode::Recording => {
                let has_take = self.has_drill();
                self.drill_mode.store(
                    if has_take { DrillMode::Playing } else { DrillMode::Off } as u8,
                    Ordering::Relaxed,
                );
                self.toggle_swap();
            }
            _ => {
                self.drill.lock().unwrap().clear();
                self.drill_mode.store(DrillMode::Recording as u8, Ordering::Relaxed);
                self.toggle_swap();
            }
        }
    }

    /// Toggle the dummy looping the recorded take. Ignored while
    /// recording or with nothing recorded.
    pub fn toggle_playback(&self) {
        match self.drill_mode() {
            DrillMode::Playing => self.drill_mode.store(DrillMode::Off as u8, Ordering::Relaxed),
            DrillMode::Off if self.has_drill() => {
                self.drill_mode.store(DrillMode::Playing as u8, Ordering::Relaxed)
            }
            _ => {}
        }
    }

    /// Whether chip forcing works in this session — the game's engine
    /// support offered a trainer. `false` means the picker has nothing
    /// to drive and the host should say so instead of offering it.
    pub fn chip_forcing_available(&self) -> bool {
        self.control.is_wired()
    }

    /// Set or clear the forced hand for absolute player `player`: up
    /// to 6 chip ids in fire order, overwriting that player's pick at
    /// every custom-screen close until cleared.
    pub fn set_forced_hand(&self, player: usize, hand: Option<Vec<u16>>) {
        self.control.set_forced_hand(player, hand);
    }

    /// The forced hand standing for absolute player `player`, if any.
    pub fn forced_hand(&self, player: usize) -> Option<Vec<u16>> {
        self.control.forced_hand(player)
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

    fn screen_layout(&self) -> tango_match::ScreenLayout {
        self.layout.clone()
    }

    fn wake(&self) -> Arc<tokio::sync::Notify> {
        self.wake.clone()
    }

    /// The non-controlled core's screen — `None` while the PiP is off or
    /// before its first captured frame.
    fn pip_frame(&self) -> Option<Vec<u8>> {
        (self.show_pip.load(Ordering::Relaxed) && self.pip_fresh.load(Ordering::Relaxed)).then(|| self.pip.read())
    }

    fn set_input(&self, input: crate::HostInput) {
        // A training pair is GBA-only, so the stylus has nowhere to go.
        self.joyflags.store(input.keys, Ordering::Relaxed);
    }

    fn request_close(&self) {
        // Stop the driver, and abort a priming walk still in flight —
        // without the cancel, closing during the walk waits it out.
        self.stop.store(true, Ordering::Relaxed);
        self.boot_cancel.store(true, Ordering::Release);
    }

    fn set_speed(&self, factor: f32) {
        self.fps_bits.store(
            crate::clamp_speed(self.expected_fps, factor).to_bits(),
            Ordering::Relaxed,
        );
    }

    /// True once the battle's own match-end path fired, so the host
    /// tears the session down instead of leaving the player on a hung
    /// post-match link screen.
    fn is_ended(&self) -> bool {
        self.ended.load(Ordering::Acquire)
    }
}

impl Drop for TrainingSession {
    /// Tell whoever is driving to stop; the next `tick` returns false.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// What [`TrainingBoot`]'s first tick consumes to bring the pair up:
/// the pair's ingredients plus clones of everything the driver shares
/// with the session.
struct BootPieces {
    game: &'static tango_gamesupport::Game,
    rom: Arc<Vec<u8>>,
    save_sram: Vec<u8>,
    rtc: std::time::SystemTime,
    rng_seed: [u8; 16],
    control: Arc<tango_match::trainer::TrainerControl>,
    /// The producing end of the ring the host's stream is already bound
    /// to. Until the pair takes it the ring reads empty and the stream
    /// primes.
    audio: tango_match::AudioIn,
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
    policy: Arc<AtomicU8>,
    drill_mode: Arc<AtomicU8>,
    drill: Arc<Mutex<Vec<u32>>>,
    swap_generation: Arc<AtomicU32>,
}

impl BootPieces {
    /// Boot the pair, prime it into its link battle, and hand back the
    /// live driver. Seconds of blocking emulation — which is exactly
    /// why it runs on the drive thread, not at session construction.
    fn boot(self, cancel: &AtomicBool) -> Result<Driver, tango_match::Error> {
        // The engine's local core is core 0; `advance` always feeds core
        // 0 and `add_remote_input` core 1. The player starts on core 0
        // (the dummy on core 1); a swap only re-routes which input source
        // feeds each core, so the engine's local_player stays 0. Both
        // cores run the same game (a mirror match) — training is local,
        // so there's no opponent selection.
        //
        // Present delay 0: the match is local and lockstep, so there's no
        // latency to hide and no speculation to roll back.
        let mut match_ = self.game.pvp.start(tango_match::StartConfig {
            roms: [self.rom.as_ref(), self.rom.as_ref()],
            saves: [Some(&self.save_sram), Some(&self.save_sram)],
            match_type: TRAINING_MATCH_TYPE,
            rng_seed: self.rng_seed,
            rtc: self.rtc,
            // A mirror match, so the peer's cartridge is this one.
            peer_rom: tango_match::PeerRom {
                code: *self.game.rom_code,
                revision: self.game.revision,
            },
            local_player: 0,
            present_delay: 0,
            disable_bgm: false,
            audio: Some(self.audio),
            cancel: Some(cancel),
            // Training is the one lockstep session, so it is the one
            // place a trainer is sound — the engine installs the
            // game's hook over this control if the game offers one.
            trainer: Some(self.control),
        })?;

        // A netplay match renders only the local side. Training shows
        // both — the PiP and the side-swap — so ask for the whole pair.
        match_.render_seats();

        Ok(Driver {
            match_,
            controlled: self.controlled,
            joyflags: self.joyflags,
            controller: self.controller,
            fps_bits: self.fps_bits,
            dummy_joyflags: self.dummy_joyflags,
            show_pip: self.show_pip,
            pip: self.pip,
            pip_fresh: self.pip_fresh,
            ended: self.ended,
            stop: self.stop,
            screen: self.screen,
            wake: self.wake,
            frame: 0,
            policy: self.policy,
            drill_mode: self.drill_mode,
            drill: self.drill,
            drill_cursor: 0,
            swap_generation: self.swap_generation,
            custom_open: [false; 2],
            dummy_custom_ticks: 0,
            possession: None,
        })
    }
}

/// The drive-thread half of a training session: its first tick boots
/// and primes the pair (so the session is installed and on screen
/// while the walk runs), then it becomes the driver. The same shape as
/// PvP's boot.
pub struct TrainingBoot {
    /// What the first tick needs to bring the pair up, taken there.
    /// `None` once the boot has run, whichever way it went.
    pending: Option<BootPieces>,
    /// The live battle, once the boot has produced one.
    driver: Option<Driver>,
    /// Where a failed boot leaves its reason, for the session to
    /// publish ([`TrainingSession::prime_error`]).
    prime_error: Arc<Mutex<Option<crate::Error>>>,
    /// Cleared once the pair is up — the session's
    /// [`is_booting`](TrainingSession::is_booting).
    booting: Arc<AtomicBool>,
    boot_cancel: Arc<AtomicBool>,
    /// Repaint wake, so a failure reaches a host whose session is
    /// otherwise sitting on a frame that will never come.
    wake: Arc<tokio::sync::Notify>,
    fps_bits: Arc<AtomicU32>,
    /// The engine the boot will run on, for its readiness gate.
    backend: &'static (dyn tango_match::Backend + Send + Sync),
}

impl crate::Drive for TrainingBoot {
    fn tick(&mut self) -> bool {
        // What `prepare` started may still be coming up — a browser
        // engine's worker threads finish starting only between ticks,
        // while the host's loop yields. The priming notice is already
        // up either way.
        if self.pending.is_some() && !self.backend.ready(2) {
            return true;
        }
        if let Some(pieces) = self.pending.take() {
            match pieces.boot(&self.boot_cancel) {
                Ok(driver) => {
                    self.driver = Some(driver);
                    self.booting.store(false, Ordering::Release);
                    // The boot produced no frame yet — wake the host
                    // itself so the notice comes down promptly.
                    self.wake.notify_one();
                }
                // Cancelled is the session being torn down mid-walk,
                // not a failure to report: there is nobody left to
                // read it.
                Err(tango_match::Error::Cancelled) => return false,
                Err(e) => {
                    log::error!("training: boot failed: {e}");
                    *self.prime_error.lock().unwrap() = Some(e.into());
                    self.wake.notify_one();
                    return false;
                }
            }
        }
        match self.driver.as_mut() {
            Some(driver) => driver.tick(),
            // A boot that failed leaves the session up with its reason
            // on screen; there is simply nothing left to drive.
            None => false,
        }
    }

    /// The pacing target once the battle runs; before that, the rate
    /// the pacer should idle the boot at.
    fn fps_target(&self) -> f32 {
        f32::from_bits(self.fps_bits.load(Ordering::Relaxed))
    }
}

/// Everything the driver owns for the session's life.
pub struct Driver {
    match_: tango_match::Match,
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
    /// Ticks run, handed to the dummy controller so it can time its
    /// takes.
    frame: u64,
    policy: Arc<AtomicU8>,
    drill_mode: Arc<AtomicU8>,
    drill: Arc<Mutex<Vec<u32>>>,
    /// Playback position in the take; wraps for the loop, rewinds
    /// whenever the drill isn't playing.
    drill_cursor: usize,
    swap_generation: Arc<AtomicU32>,
    /// Whether each player's custom screen stands open, by absolute
    /// player, off the confirmed telemetry samples (a tick or two
    /// behind the frontier — the policies all react over multiple
    /// frames anyway).
    custom_open: [bool; 2],
    /// Consecutive ticks the DUMMY seat's screen has stood open — the
    /// auto-confirm debounce-then-script clock.
    dummy_custom_ticks: u32,
    /// A live auto-possession: the seat to hand back to and the swap
    /// generation it began under (a manual swap moves the generation
    /// and the hand-back is dropped — the player's choice wins).
    possession: Option<(usize, u32)>,
}

impl crate::Drive for Driver {
    fn tick(&mut self) -> bool {
        Driver::tick(self)
    }

    fn fps_target(&self) -> f32 {
        f32::from_bits(self.fps_bits.load(Ordering::Relaxed))
    }
}

impl Driver {
    /// How long the dummy's custom screen must stand open before the
    /// auto-confirm script engages — long enough for the screen to
    /// settle and for a quick manual swap to pre-empt it.
    const CONFIRM_DEBOUNCE: u32 = 20;

    /// Advance the battle one tick: resolve the dummy policy, poll the
    /// dummy, route both inputs, step the pair, publish the screens.
    /// `false` once the session has ended — the battle's own match-end
    /// path, a failed advance, or the session being dropped.
    pub fn tick(&mut self) -> bool {
        let frame = self.frame;
        if self.stop.load(Ordering::Relaxed) {
            return false;
        }
        {
            let policy = DummyPolicy::from_u8(self.policy.load(Ordering::Relaxed));
            let drill = DrillMode::from_u8(self.drill_mode.load(Ordering::Relaxed));

            // A live auto-possession hands back the moment the
            // possessed seat's screen closes — unless a manual swap
            // moved the generation in between, in which case the
            // player's choice stands and the record just drops.
            // Suspended entirely while recording: the player is on the
            // dummy's seat on purpose, and a possession flip would
            // corrupt the take.
            if let Some((return_seat, generation)) = self.possession {
                if !self.custom_open[self.controlled.load(Ordering::Relaxed)] {
                    if self.swap_generation.load(Ordering::Relaxed) == generation {
                        self.controlled.store(return_seat, Ordering::Relaxed);
                    }
                    self.possession = None;
                }
            } else if policy == DummyPolicy::AutoPossess && drill != DrillMode::Recording {
                let controlled = self.controlled.load(Ordering::Relaxed);
                if self.custom_open[1 - controlled] {
                    // The dummy's screen is open: take its seat for the
                    // pick. Screen, audio and input routing all follow
                    // `controlled`, so this one store is the whole
                    // perspective flip.
                    self.possession = Some((controlled, self.swap_generation.load(Ordering::Relaxed)));
                    self.controlled.store(1 - controlled, Ordering::Relaxed);
                }
            }

            // Which core the player drives this tick; the dummy takes the
            // other. A swap flips this between ticks.
            let controlled = self.controlled.load(Ordering::Relaxed);
            let dummy_player = 1 - controlled;
            // The sound follows the player across a swap; the pair drops
            // whatever the seat being left had queued, so the old side's
            // tail never plays under the new one.
            self.match_.listen_to(controlled);

            // Poll the dummy controller for the tick about to advance. It
            // sees the battle facts as of the newest confirmed tick; its
            // output becomes the dummy core's input for this tick. The
            // stock NoopController returns 0.
            let controller = self.controller.clone();
            let mut dummy = controller.lock().unwrap().poll(&mut ControllerContext {
                dummy_player,
                human_player: controlled,
                frame,
                custom_open: self.custom_open,
            });

            // Auto-confirm: whoever holds the dummy seat right now is by
            // definition not the human (a human swapping onto a seat
            // makes it the controlled one), so the injection can never
            // fight the pad — the corollary being that swapping away
            // from your own open custom screen abandons it to the
            // script. START jumps the cursor to OK, A confirms; held 3
            // ticks each with gaps so the game sees clean edges.
            if self.custom_open[dummy_player] {
                self.dummy_custom_ticks += 1;
            } else {
                self.dummy_custom_ticks = 0;
            }
            if policy == DummyPolicy::AutoConfirm && self.dummy_custom_ticks > Self::CONFIRM_DEBOUNCE {
                dummy = match (self.dummy_custom_ticks - Self::CONFIRM_DEBOUNCE) % 12 {
                    0..=2 => tango_match::keys::START,
                    6..=8 => tango_match::keys::A,
                    _ => 0,
                };
            }

            // The drill. Recording: the player is ON the dummy's seat
            // (toggle_record swapped them), so the take is their own
            // pad, captured outside the shared custom pause — pause
            // ticks are dead air on both sides of the trip. Playing:
            // the take drives the dummy seat, suspended through the
            // pause (auto-confirm still answers the screens), looping
            // at the end.
            let paused = self.custom_open.iter().any(|&c| c);
            match drill {
                DrillMode::Recording => {
                    if !paused {
                        self.drill.lock().unwrap().push(self.joyflags.load(Ordering::Relaxed));
                    }
                    self.drill_cursor = 0;
                }
                DrillMode::Playing => {
                    if !paused {
                        let take = self.drill.lock().unwrap();
                        if !take.is_empty() {
                            if self.drill_cursor >= take.len() {
                                self.drill_cursor = 0;
                            }
                            dummy = take[self.drill_cursor];
                            self.drill_cursor += 1;
                        }
                    }
                }
                DrillMode::Off => {
                    self.drill_cursor = 0;
                }
            }
            self.dummy_joyflags.store(dummy, Ordering::Relaxed);

            // Route each input to its core, then feed the engine: core 0
            // via `advance`, core 1 via `add_remote_input` (the engine's
            // fixed local/remote slots). Whichever core the player drives
            // gets the pad; the other gets the dummy. Both inputs for the
            // tick are present before it advances, so the pair confirms it
            // immediately — lockstep, no rollback.
            let player = self.joyflags.load(Ordering::Relaxed);
            let core0 = if controlled == 0 { player } else { dummy };
            let core1 = if controlled == 0 { dummy } else { player };
            self.match_.add_remote_input(tango_match::HostInput::keys(core1), 0);
            let _outgoing = match self.match_.advance(tango_match::HostInput::keys(core0)) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("training: advance failed: {e}");
                    self.ended.store(true, Ordering::Release);
                    self.wake.notify_one();
                    return false;
                }
            };

            // Drain the confirmed telemetry: the custom flags feed the
            // dummy policies, and the games' own match-end path tears
            // the session down cleanly. We don't fold stats — training
            // records nothing. (Under lockstep the confirmed boundary
            // tracks the frontier, so the flags run at most a couple of
            // ticks behind what's on screen.)
            let (samples, events) = match self.match_.telemetry() {
                Some(store) => store.lock().unwrap().drain_confirmed(self.match_.confirmed()),
                None => (Vec::new(), Vec::new()),
            };
            if let Some((_, obs)) = samples.last() {
                self.custom_open = obs.custom;
            }
            if events
                .iter()
                .any(|(_, e)| matches!(e, Event::RoundEnded { .. } | Event::MatchEnded))
            {
                // Samples stop at a round's verdict while the battle
                // structs linger stale-live — don't let a stale open
                // flag drive the script into the result screens.
                self.custom_open = [false; 2];
            }
            if events.iter().any(|(_, e)| matches!(e, Event::MatchEnded)) {
                self.ended.store(true, Ordering::Release);
                self.wake.notify_one();
                return false;
            }

            // Publish the controlled core to the main screen; the other
            // core feeds the PiP while it's on.
            if let Some(buf) = self.match_.seat_frame(controlled) {
                self.screen.write(&buf);
            }
            if self.show_pip.load(Ordering::Relaxed) {
                if let Some(buf) = self.match_.seat_frame(dummy_player) {
                    self.pip.write(&buf);
                    self.pip_fresh.store(true, Ordering::Relaxed);
                }
            } else {
                self.pip_fresh.store(false, Ordering::Relaxed);
            }
            self.frame = frame.wrapping_add(1);
            self.wake.notify_one();
        }
        true
    }
}

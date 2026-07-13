pub use crate::input::{Input, PartialInput};

mod state;

pub(crate) use state::InnerState;
pub use state::{CapturedBoundary, ReplayCheckpoint, ReplaySnapshot, SharedShadow, State};

/// Source of the remote peer's link packet for one tick of simulation.
///
/// The stepper can only re-simulate this side of the link; the opponent's
/// per-tick packets aren't on the wire, so each step asks this source to
/// produce them. The production implementation is the shared
/// [`Shadow`](crate::shadow::Shadow) co-sim: `resolve` runs the opponent's
/// core forward one tick over the (real or predicted) remote joyflags in
/// `pair` and returns the link packet the opponent's game emitted.
///
/// This names a data flow that used to be an anonymous closure threaded
/// through three layers: `MgbaWorld::step` → [`Stepper`] → per-game stepper
/// trap (`InnerState::apply_shadow_input`) → `Shadow::apply_input`.
///
/// `resolve` takes `&self` so the source can be held as a plain shared
/// `Arc` everywhere it's needed (the production impl synchronizes
/// internally via its mutex).
pub trait RemotePacketSource: Send + Sync {
    fn resolve(&self, pair: (Input, PartialInput)) -> anyhow::Result<Vec<u8>>;
}

impl RemotePacketSource for std::sync::Mutex<crate::shadow::Shadow> {
    fn resolve(&self, pair: (Input, PartialInput)) -> anyhow::Result<Vec<u8>> {
        self.lock().unwrap().apply_input(pair)
    }
}

/// Outcome of a single round, as detected by the per-game `round_end_*` traps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde_repr::Serialize_repr, serde_repr::Deserialize_repr)]
#[repr(i8)]
pub enum BattleOutcome {
    Draw = -1,
    Loss = 0,
    Win = 1,
}

/// Round-ending progression within a single Fastforwarder window. (Replay
/// mode tracks its richer per-round lifecycle in its own `PlaybackPhase`;
/// this one only answers the per-game `is_round_ending` / `is_round_ended`
/// gates during a re-sim step.)
#[derive(Clone, Copy, PartialEq)]
pub(super) enum RoundPhase {
    InProgress,
    Ending,
    Ended,
}

/// Outcome bundled with the tick at which the GAME signaled it.
#[derive(Clone, Copy)]
pub struct RoundResult {
    pub tick: u32,
    pub outcome: BattleOutcome,
}

/// Both players' in-battle HP at one tick, indexed by absolute player
/// index (not local/remote). Reported by the per-game stepper traps via
/// [`InnerState::set_battle_hp`] on games with known HP offsets; games
/// without them simply never report and consumers see no samples.
pub type BattleHp = [u16; 2];

/// Output of a single stepper run.
pub struct StepperResult {
    /// The boundary the run halted at: its tick and outgoing packet. The per-game
    /// stepper trap fires `main_read_joyflags` once the input window is exhausted
    /// and halts the core there (`end_run_loop`), poised at the start of that tick
    /// with r4 (local joyflags) left unset.
    ///
    /// The matching mgba state is *not* bundled — [`Stepper::step`] leaves the
    /// core parked at this boundary and the caller materializes the snapshot on
    /// demand via [`Stepper::save`], so a rollback that re-steps a whole tail only
    /// saves the one state it keeps. r4 is supplied by the consumer: the live core
    /// via
    /// [`Hooks::inject_joyflags_on_primary`](crate::hooks::Hooks::inject_joyflags_on_primary)
    /// after loading the snapshot, and the next run by re-priming r4 at its first
    /// `main_read_joyflags` (its PC is rewound there by `prepare_for_next_input`).
    pub boundary: CapturedBoundary,
    pub round_result: Option<RoundResult>,
    /// Both players' HP during the tick this run simulated, if the per-game
    /// traps reported it (see [`BattleHp`]).
    pub hp: Option<BattleHp>,
    /// Whether the custom screen was open during the tick this run
    /// simulated, if the per-game traps reported it.
    pub custom: Option<bool>,
    /// Each player's loaded chip id during the tick this run simulated
    /// (0xFFFF = none), if the per-game traps reported it.
    pub chips: Option<[u16; 2]>,
}

/// Single per-frame re-sim core for the rollback engine: a dedicated headless
/// mgba core plus the drive loop that runs the per-game trap set one tick at a
/// time, halting at the boundary tick (`capture_tick`) each tick — where the
/// caller materializes the snapshot on demand via [`save`](Self::save). It advances
/// via [`step`](Self::step) and rewinds via [`restore`](Self::restore) only when
/// the engine rolls back to re-simulate a mispredicted tail. In steady state —
/// and after promotions — `step` resumes forward from wherever the previous call
/// parked the core (`end_run_loop` halts exactly at the boundary
/// `main_read_joyflags` with r4 unset; see e.g. `game/bn6/stepper.rs`), so no
/// reload happens. Folds together what used to be two cores (a reload-each-frame
/// speculative fork and a forward-only authoritative core).
pub struct Stepper {
    core: mgba::core::Core,
    state: State,
    hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    identity: crate::battle::MatchIdentity,
    /// The tick the core is currently parked at — advanced by one per
    /// [`step`](Self::step) and reset by [`restore`](Self::restore). [`step`]
    /// re-sims from here, so the caller never threads the tick back in.
    parked_tick: u32,
    /// Remote-packet source set at construction; each [`step`](Self::step)
    /// hands a clone to its run state, where the per-game traps reach it via
    /// `apply_shadow_input`.
    packet_source: std::sync::Arc<dyn RemotePacketSource>,
    /// Whether each [`step`](Self::step)'s run state mutes battle BGM (the
    /// per-game play-music trap skips the call). The snapshots this core
    /// produces carry the sound driver's RAM into the live core, so this is
    /// where a PvP music mute actually takes hold.
    disable_bgm: bool,
}

impl Stepper {
    /// Build the stepper seeded at the round's first-committed state — tick 0,
    /// where the live core hands off. The core is then parked at that boundary,
    /// ready for the first [`step`](Self::step).
    pub fn new(
        rom: &[u8],
        hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        identity: crate::battle::MatchIdentity,
        initial_state: &mgba::state::State,
        packet_source: std::sync::Arc<dyn RemotePacketSource>,
        disable_bgm: bool,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango", &mgba::core::Options { ..Default::default() })?;
        let rom_vf = mgba::vfile::VFile::from_vec(rom.to_vec());
        core.as_mut().load_rom(rom_vf)?;

        let state = State(std::sync::Arc::new(std::sync::Mutex::new(None)));

        hooks.install_on_stepper(&mut core, state.clone());
        // Pin the cart RTC to the match clock: re-sim ticks must read the
        // same values the live core (and the peer) read, or RTC-reading
        // games (exe45) diverge on every rollback.
        core.set_rtc_fixed(identity.rtc_time);
        core.as_mut().reset();
        // Headless re-sim core: never rasterize. Its pixels are never shown, so
        // skipping drawScanline cuts a large constant off the dominant cost. Set
        // after reset() — which zeroes frameskip — and it sticks (frameskip isn't
        // serialized).
        core.as_mut().gba_mut().set_frameskip(i32::MAX);

        core.as_mut().load_state(initial_state)?;

        Ok(Stepper {
            core,
            state,
            hooks,
            identity,
            parked_tick: 0,
            packet_source,
            disable_bgm,
        })
    }

    /// Rewind the core to `state`, which is poised at `tick`, before a rollback
    /// re-sim. The caller positions the shadow alongside.
    ///
    /// Returns `false` without touching the core when it's already parked at
    /// `tick` — a re-sim that promoted every speculation up to here leaves the
    /// core exactly where this `state` would put it, so the reload is skipped and
    /// steady-state settles stay forward-only. The caller can mirror this: a
    /// `false` means its own side state (shadow, outgoing packet) is already
    /// current too.
    pub fn restore(&mut self, state: &mgba::state::State, tick: u32) -> anyhow::Result<bool> {
        if self.parked_tick == tick {
            return Ok(false);
        }
        self.core.as_mut().load_state(state)?;
        self.parked_tick = tick;
        Ok(true)
    }

    /// Advance exactly one tick from where the core is parked, applying `input`
    /// and halting at the next boundary (`parked_tick + 1`). `last_local_packet`
    /// is this side's outgoing link packet at the parked tick (seeds the link
    /// exchange); the [`RemotePacketSource`] given at construction resolves the
    /// remote packet by co-simulating the shadow. Drives the core until the
    /// per-game stepper trap reaches the boundary and halts, advancing the
    /// parked tick; call [`save`](Self::save) afterward to snapshot the core
    /// there.
    pub fn step(
        &mut self,
        input: (PartialInput, PartialInput),
        last_local_packet: &[u8],
    ) -> anyhow::Result<StepperResult> {
        self.hooks.prepare_for_next_input(self.core.as_mut());
        *self.state.0.lock().unwrap() = Some(InnerState::for_fastforward(
            self.identity.match_type,
            self.identity.local_player_index,
            vec![input],
            self.parked_tick,
            last_local_packet.to_vec(),
            self.packet_source.clone(),
            self.disable_bgm,
        ));

        let result = loop {
            {
                let mut guard = self.state.0.lock().unwrap();
                let inner = guard.as_mut().unwrap();
                if inner.has_captured_snapshot() {
                    break guard.take().expect("state").into_stepper_result();
                }
                let _ = inner.take_error();
            }
            self.core.as_mut().run_loop();
            let mut guard = self.state.0.lock().unwrap();
            if let Some(err) = guard.as_mut().expect("state").take_error() {
                guard.take();
                return Err(anyhow::format_err!("replayer: {}", err));
            }
        };

        self.parked_tick = result.boundary.tick;
        Ok(result)
    }

    /// Snapshot the core at the boundary the last [`step`](Self::step) parked it
    /// at. `step` halts the core exactly at the capture boundary — the per-game
    /// `main_read_joyflags` trap calls `end_run_loop` right there, with r4 unset —
    /// so a save taken now is byte-identical to one taken inside that trap. The
    /// engine folds the speculative and authoritative cores into this one, so the
    /// parked core *is* the snapshot; deferring the save to here means a rollback
    /// that re-steps N ticks only saves the one final state it keeps, not a state
    /// per re-simulated tick. Returns the snapshot bundled with the tick it's
    /// poised at (the parked tick), so the caller can checkpoint both together.
    pub fn save(&mut self) -> anyhow::Result<(Box<mgba::state::State>, u32)> {
        self.save_reusing(mgba::state::State::new_uninit())
    }

    /// [`save`](Self::save), but writing into a recycled state buffer instead
    /// of allocating a fresh one. The rollback engine discards a snapshot of
    /// this size every promoted tick; reusing it keeps the per-frame save out
    /// of the page allocator.
    pub fn save_reusing(
        &mut self,
        buf: Box<std::mem::MaybeUninit<mgba::state::State>>,
    ) -> anyhow::Result<(Box<mgba::state::State>, u32)> {
        Ok((self.core.as_mut().save_state_reusing(buf)?, self.parked_tick))
    }
}

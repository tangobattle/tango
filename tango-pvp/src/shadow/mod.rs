//! Shadow emulator: simulates the remote peer locally so the live primary
//! can advance lockstep without waiting on the network. Each [`Shadow`] runs
//! the opponent's ROM + SRAM and answers the primary fastforwarder's
//! `apply_shadow_input` calls with predicted remote packets.
//!
//! - [`State`] is the shared handle the per-game shadow traps lock to read
//!   and modify the round state, RNG, and applied snapshots.
//! - [`Round`] holds the per-round mutable state.

mod round;
mod state;
mod worker;

pub use round::Round;
pub use state::State;
pub use worker::Worker;

/// Captures the full shadow-side state for replay-mode seeking. The
/// playback session pairs this with the stepper's `ReplayCheckpoint` +
/// stepper-core mgba state so that loading a snapshot restores both
/// sides — without this, the shadow would still be at its pre-seek tick
/// and would feed misaligned packets after the seek.
#[derive(Clone)]
pub struct ShadowSnapshot {
    pub mgba_state: Box<mgba::state::State>,
    pub rng: rand_pcg::Mcg128Xsl64,
    pub round: Option<Round>,
    pub result_is_in: bool,
}

use crate::input::{Input, PartialInput};

/// Shadow-mode emulator that mirrors the remote peer locally. The visible
/// primary calls into this to advance the predicted opponent state via
/// [`apply_input`](Shadow::apply_input) and to capture initial /
/// end-of-round snapshots.
pub struct Shadow {
    core: mgba::core::Core,
    state: State,
    hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
}

impl Shadow {
    pub fn new(
        rom: &[u8],
        save: &(dyn tango_dataview::save::Save + Send + Sync),
        hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        match_type: (u8, u8),
        is_offerer: bool,
        local_player_index: u8,
        rng: rand_pcg::Mcg128Xsl64,
        rtc_time: std::time::SystemTime,
    ) -> anyhow::Result<Self> {
        Self::new_from_sram(
            rom,
            &save.to_sram_dump(),
            hooks,
            match_type,
            is_offerer,
            local_player_index,
            rng,
            rtc_time,
        )
    }

    /// Build a shadow for replay-style reconstruction (playback, export,
    /// the golden suite). Pulls remote sram + match_type +
    /// is_offerer + local_player_index from `replay`, seeds the RNG from
    /// `replay.rng_seed`, and advances it past the one-bool draw that
    /// [`crate::battle::Match::pick_local_player_index`] would have
    /// consumed during the live match — so the shadow's per-game
    /// RNG-handling traps stay in sync with the recorded run.
    ///
    /// Live PvP uses [`Shadow::new`] instead: there, the bool draw
    /// happens during `pick_local_player_index` itself, and the
    /// post-draw RNG is what gets passed in.
    pub fn new_for_replay(
        rom: &[u8],
        replay: &crate::replay::Replay,
        hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    ) -> anyhow::Result<Self> {
        use rand::SeedableRng;
        let mut rng = rand_pcg::Mcg128Xsl64::from_seed(replay.rng_seed);
        // Advance past the one-bool polite-win draw that the live match made
        // in `pick_local_player_index`; the index itself comes from `replay`.
        let _ = crate::battle::Match::pick_local_player_index(&mut rng, replay.is_offerer);
        let match_type = (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8);
        Self::new_from_sram(
            rom,
            &replay.remote_sram,
            hooks,
            match_type,
            replay.is_offerer,
            replay.local_player_index,
            rng,
            replay.rtc_time(),
        )
    }

    /// Same as [`Shadow::new`] but takes the SRAM dump directly. Used by the
    /// replay-via-shadow playback path, where the remote-side save is
    /// stored as raw bytes inside the replay file rather than as a parsed
    /// Save object.
    pub fn new_from_sram(
        rom: &[u8],
        save_sram: &[u8],
        hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        match_type: (u8, u8),
        is_offerer: bool,
        local_player_index: u8,
        rng: rand_pcg::Mcg128Xsl64,
        rtc_time: std::time::SystemTime,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango", &mgba::core::Options { ..Default::default() })?;
        // A video buffer is always attached (it's just a render target;
        // game logic never sees it), but rasterization stays off via the
        // frameskip below. The replay PiP turns rendering on to show the
        // opponent's perspective.
        core.enable_video_buffer();

        core.as_mut().load_rom(mgba::vfile::VFile::from_vec(rom.to_vec()))?;
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(save_sram.to_vec()))?;
        // Pin the cart RTC to the match clock so RTC-reading games (exe45)
        // derive the same values here as on the primary — and as on the
        // peer's pair of cores.
        core.set_rtc_fixed(rtc_time);

        let state = State::new(match_type, is_offerer, local_player_index, rng);

        hooks.install_on_shadow(&mut core, state.clone());
        core.as_mut().reset();
        // The shadow only derives the remote side's packets (game logic); its
        // pixels are never shown, so skip rasterization. Set after reset() (which
        // zeroes frameskip); it sticks, as frameskip isn't serialized.
        core.as_mut().gba_mut().set_frameskip(i32::MAX);

        Ok(Shadow { core, hooks, state })
    }

    /// Turn rasterization on/off. Off (the default) skips drawScanline
    /// entirely — the shadow's pixels are normally never shown. The
    /// replay PiP flips it on to show the opponent's screen. Frameskip
    /// isn't serialized, so the setting survives every `load_state`.
    pub fn set_rendering(&mut self, on: bool) {
        self.core
            .as_mut()
            .gba_mut()
            .set_frameskip(if on { 0 } else { i32::MAX });
    }

    /// Copy the shadow's most recently rendered frame into `buf`.
    /// `false` (buf untouched) when no buffer is attached or sizes
    /// mismatch. Only meaningful while rendering is on.
    pub fn read_video_buffer(&self, buf: &mut [u8]) -> bool {
        match self.core.video_buffer() {
            Some(vb) if vb.len() == buf.len() => {
                buf.copy_from_slice(vb);
                true
            }
            _ => false,
        }
    }

    pub fn save_state(&mut self) -> anyhow::Result<ShadowSnapshot> {
        self.save_state_reusing(mgba::state::State::new_uninit())
    }

    /// [`save_state`](Self::save_state), but writing the mgba state into a
    /// recycled buffer instead of allocating a fresh one.
    pub fn save_state_reusing(
        &mut self,
        buf: Box<std::mem::MaybeUninit<mgba::state::State>>,
    ) -> anyhow::Result<ShadowSnapshot> {
        let mgba_state = self.core.as_mut().save_state_reusing(buf)?;
        let shared = self.state.lock();
        Ok(ShadowSnapshot {
            mgba_state,
            rng: shared.rng.clone(),
            round: shared.round.clone(),
            result_is_in: shared.result_is_in,
        })
    }

    pub fn load_state(&mut self, snapshot: &ShadowSnapshot) -> anyhow::Result<()> {
        self.core.as_mut().load_state(&snapshot.mgba_state)?;
        let mut shared = self.state.lock();
        shared.rng = snapshot.rng.clone();
        shared.round = snapshot.round.clone();
        shared.result_is_in = snapshot.result_is_in;
        // input_applied and error are per-run scratch; clear so the next
        // apply_input / round-end run doesn't pick up stale values that don't
        // correspond to the just-restored core state.
        shared.input_applied = false;
        drop(shared);
        self.state.clear_error();
        Ok(())
    }

    /// The shared drive-loop shape: run the core in bursts, draining the
    /// trap error channel after each, until `done` observes the wanted
    /// state transition. The per-game traps perform the transitions while
    /// the core runs; this just polls for them.
    fn run_core_until(&mut self, mut done: impl FnMut(&State) -> bool) -> anyhow::Result<()> {
        loop {
            self.core.as_mut().run_loop();
            if let Some(err) = self.state.0.error.lock().unwrap().take() {
                return Err(anyhow::format_err!("shadow: {}", err));
            }
            if done(&self.state) {
                return Ok(());
            }
        }
    }

    /// Run the shadow until the per-game traps mark this round's first
    /// committed state. `end_run_loop` parks the core right there, so there's
    /// nothing to load back — the next apply_input run continues from here.
    pub fn advance_until_first_committed_state(&mut self) -> anyhow::Result<()> {
        log::info!("advancing shadow until first committed state");
        self.run_core_until(|state| {
            let mut shared = state.lock();
            let Some(round) = shared.round.as_mut() else {
                return false;
            };
            if !round.has_first_committed_state() {
                return false;
            }
            round.current_tick = 0;
            true
        })
    }

    /// Run the shadow until `end_round` drops the round state. `end_run_loop`
    /// in `round_end_entry` parks the core right at round end, so there's
    /// nothing to load back.
    pub fn advance_until_round_end(&mut self) -> anyhow::Result<()> {
        log::info!("advancing shadow until round end");
        self.hooks.prepare_for_next_input(self.core.as_mut());
        self.run_core_until(|state| state.lock().round.is_none())
    }

    /// Inject the given input pair as the next shadow input, then run the
    /// shadow forward one tick from wherever it is parked, until the per-game
    /// trap signals the input was applied: the core has reached the next tick's
    /// `main_read_joyflags`, where the trap calls `end_run_loop`, which parks the
    /// core exactly at that boundary. This call only ever advances; a rollback
    /// rewinds the shadow beforehand via [`load_state`](Self::load_state) (the
    /// rollback engine drives the primary and shadow cores in lockstep), so each
    /// `apply_input` resumes from the rewound position. Returns the remote
    /// packet queued before this run.
    pub fn apply_input(&mut self, ip: (Input, PartialInput)) -> anyhow::Result<Vec<u8>> {
        let pending_remote_packet = self.begin_apply_input(ip)?;
        self.finish_apply_input()?;
        Ok(pending_remote_packet)
    }

    /// First half of [`apply_input`](Self::apply_input): queue `ip` as the
    /// shadow's next input and return this tick's remote packet — which the
    /// shadow's *previous* run buffered (`set_remote_packet` stamps it
    /// `current_tick + 1`), so it is available before the core advances at
    /// all. The core itself has not moved;
    /// [`finish_apply_input`](Self::finish_apply_input) runs it forward to
    /// consume the queued input (and buffer the *next* tick's packet). Split
    /// out so the live path can hand the packet to the primary immediately
    /// and run the shadow's tick concurrently on the [`Worker`].
    pub fn begin_apply_input(&mut self, ip: (Input, PartialInput)) -> anyhow::Result<Vec<u8>> {
        let pending_remote_packet = {
            let mut shared = self.state.lock();
            let round = shared.round.as_mut().expect("round");
            round.set_pending_shadow_input(ip);
            round.peek_remote_packet().expect("pending remote packet").to_vec()
        };
        // Discard any stale "input applied" signal before the coming run. The
        // per-game trap sets it whenever `take_input_injected()` fires, which
        // also happens outside apply_input — e.g. while
        // `advance_until_round_end` runs the game through round-end link-cable
        // exchanges. The old shared `applied_snapshot` signal was cleared by
        // whichever of apply_input / advance_until_round_end `.take()`'d it;
        // the split into `input_applied` lost that, so a leftover `true` would
        // make the next round's first apply_input return before it actually
        // applied its input. (Nothing runs the core between this clear and
        // `finish_apply_input`, so clearing here covers the split path too.)
        self.state.take_input_applied();
        Ok(pending_remote_packet)
    }

    /// Second half of [`apply_input`](Self::apply_input): run the shadow
    /// forward from wherever it is parked until the per-game trap signals the
    /// input queued by [`begin_apply_input`](Self::begin_apply_input) was
    /// applied, parking the core at the next tick's boundary.
    pub fn finish_apply_input(&mut self) -> anyhow::Result<()> {
        self.hooks.prepare_for_next_input(self.core.as_mut());
        self.run_core_until(|state| state.take_input_applied())
    }
}

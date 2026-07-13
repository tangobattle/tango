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
pub use state::{Halt, State};
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
    /// The shadow's most recently rasterized frame at capture time;
    /// `None` while rendering is off (the buffer holds stale pixels
    /// then — see [`Shadow::set_rendering`]). Purely a preview asset,
    /// ignored by [`Shadow::load_state`]: it lets replay snapshot blits
    /// refresh shadow-showing surfaces (the scrub drag's PiP) without
    /// emulating a frame, mirroring `ReplaySnapshot::framebuffer` on
    /// the primary side.
    pub framebuffer: Option<Vec<u8>>,
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
    /// Whether rasterization is on (see [`Self::set_rendering`]) —
    /// gates the framebuffer capture in [`Self::save_state`], since the
    /// video buffer holds stale pixels while frameskip is active.
    rendering: bool,
}

impl Shadow {
    pub fn new(
        rom: &[u8],
        save: &(dyn tango_dataview::save::Save + Send + Sync),
        hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        identity: crate::battle::MatchIdentity,
        rng: rand_pcg::Mcg128Xsl64,
    ) -> anyhow::Result<Self> {
        Self::new_from_sram(rom, &save.to_sram_dump(), hooks, identity, rng)
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
        let identity = crate::battle::MatchIdentity {
            match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
            is_offerer: replay.is_offerer,
            local_player_index: replay.local_player_index,
            rtc_time: replay.rtc_time(),
        };
        Self::new_from_sram(rom, &replay.remote_sram, hooks, identity, rng)
    }

    /// Same as [`Shadow::new`] but takes the SRAM dump directly. Used by the
    /// replay-via-shadow playback path, where the remote-side save is
    /// stored as raw bytes inside the replay file rather than as a parsed
    /// Save object.
    pub fn new_from_sram(
        rom: &[u8],
        save_sram: &[u8],
        hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        identity: crate::battle::MatchIdentity,
        rng: rand_pcg::Mcg128Xsl64,
    ) -> anyhow::Result<Self> {
        let mut core = mgba::core::Core::new_gba("tango", &mgba::core::Options { ..Default::default() })?;
        // A video buffer is always attached (it's just a render target;
        // game logic never sees it), but rasterization stays off via the
        // frameskip below. Replay playback and prefetch turn rendering on
        // so the opponent's perspective can be shown (the PiP) and
        // captured into snapshots.
        core.enable_video_buffer();

        core.as_mut().load_rom(mgba::vfile::VFile::from_vec(rom.to_vec()))?;
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(save_sram.to_vec()))?;
        // Pin the cart RTC to the match clock so RTC-reading games (exe45)
        // derive the same values here as on the primary — and as on the
        // peer's pair of cores.
        core.set_rtc_fixed(identity.rtc_time);

        let state = State::new(
            identity.match_type,
            identity.is_offerer,
            identity.local_player_index,
            rng,
        );

        hooks.install_on_shadow(&mut core, state.clone());
        core.as_mut().reset();
        // The shadow only derives the remote side's packets (game logic); its
        // pixels are never shown, so skip rasterization. Set after reset() (which
        // zeroes frameskip); it sticks, as frameskip isn't serialized.
        core.as_mut().gba_mut().set_frameskip(i32::MAX);

        Ok(Shadow {
            core,
            hooks,
            state,
            rendering: false,
        })
    }

    /// Turn rasterization on/off. Off (the default) skips drawScanline
    /// entirely — live PvP never shows the shadow's pixels. Replay
    /// playback and prefetch flip it on so the opponent's screen can be
    /// shown (the PiP) and captured into snapshots. Frameskip isn't
    /// serialized, so the setting survives every `load_state`.
    pub fn set_rendering(&mut self, on: bool) {
        self.rendering = on;
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

    /// Run `f` over the shadow core's audio buffer — the remote-perspective
    /// samples, accumulating whether or not anyone reads them. Closure
    /// style because the buffer ref borrows from a temporary
    /// [`CoreMutRef`](mgba::core::CoreMutRef).
    pub fn with_audio_buffer<R>(&mut self, f: impl FnOnce(&mut mgba::audio::AudioBufferMutRef<'_>) -> R) -> R {
        let mut core = self.core.as_mut();
        let mut buf = core.audio_buffer();
        f(&mut buf)
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
        // Only meaningful while rasterizing — under frameskip the buffer
        // still holds whatever frame last rendered, which may be from a
        // different tick entirely.
        let framebuffer = if self.rendering {
            self.core.video_buffer().map(|vb| vb.to_vec())
        } else {
            None
        };
        let shared = self.state.lock();
        Ok(ShadowSnapshot {
            mgba_state,
            rng: shared.rng.clone(),
            round: shared.round.clone(),
            result_is_in: shared.result_is_in,
            framebuffer,
        })
    }

    pub fn load_state(&mut self, snapshot: &ShadowSnapshot) -> anyhow::Result<()> {
        self.core.as_mut().load_state(&snapshot.mgba_state)?;
        let mut shared = self.state.lock();
        shared.rng = snapshot.rng.clone();
        shared.round = snapshot.round.clone();
        shared.result_is_in = snapshot.result_is_in;
        drop(shared);
        // A pending halt or error is per-run scratch belonging to the run
        // this restore just threw away; clear it so the next drive loop
        // doesn't consume a reason that doesn't correspond to the restored
        // core state.
        self.state.clear_halt();
        Ok(())
    }

    /// Run the core until a per-game trap parks it with a typed [`Halt`] (or
    /// reports an error, which rides the same slot). The per-game traps
    /// perform the state transitions while the core runs; every
    /// `end_run_loop` in the shadow traps goes through [`State::halt`], so a
    /// return from here always says *why* the core stopped — the drive
    /// methods below match on it instead of polling flags.
    fn run(&mut self) -> anyhow::Result<Halt> {
        loop {
            self.core.as_mut().run_loop();
            if let Some(halt) = self.state.take_halt() {
                return halt.map_err(|err| anyhow::format_err!("shadow: {}", err));
            }
            // The burst hit its natural event-batch end without any trap
            // parking the core; keep running.
        }
    }

    /// Run the shadow until the per-game traps mark this round's first
    /// committed state. `end_run_loop` parks the core right there, so there's
    /// nothing to load back — the next apply_input run continues from here.
    pub fn advance_until_first_committed_state(&mut self) -> anyhow::Result<()> {
        match self.run()? {
            Halt::FirstCommit => {
                // The commit trap fires with the game's own tick still mid-
                // transition; anchor the round at tick 0, where the packet
                // buffered by set_first_committed is stamped to land.
                let mut shared = self.state.lock();
                shared.round.as_mut().expect("round").current_tick = 0;
                Ok(())
            }
            halt => Err(anyhow::format_err!(
                "shadow: unexpected halt before first commit: {halt:?}"
            )),
        }
    }

    /// Run the shadow until `end_round` drops the round state. `end_run_loop`
    /// in the round-end trap parks the core right at round end, so there's
    /// nothing to load back. The game keeps exchanging link data through the
    /// round-end screens, so completed exchanges ([`Halt::InputApplied`])
    /// park the core along the way — nobody is waiting on them here; just
    /// keep running.
    pub fn advance_until_round_end(&mut self) -> anyhow::Result<()> {
        self.hooks.prepare_for_next_input(self.core.as_mut());
        loop {
            match self.run()? {
                Halt::RoundEnded => return Ok(()),
                Halt::InputApplied => continue,
                halt => {
                    return Err(anyhow::format_err!(
                        "shadow: unexpected halt while advancing to round end: {halt:?}"
                    ))
                }
            }
        }
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
        // No stale-signal clear needed: every halt — including the
        // `InputApplied`s raised by round-end link chatter outside
        // apply_input — is consumed by the drive loop of the very run that
        // produced it, so nothing can linger into the run
        // `finish_apply_input` is about to make.
        Ok(pending_remote_packet)
    }

    /// Second half of [`apply_input`](Self::apply_input): run the shadow
    /// forward from wherever it is parked until the per-game trap reports the
    /// input queued by [`begin_apply_input`](Self::begin_apply_input) was
    /// applied ([`Halt::InputApplied`]), parking the core at the next tick's
    /// boundary.
    pub fn finish_apply_input(&mut self) -> anyhow::Result<()> {
        self.hooks.prepare_for_next_input(self.core.as_mut());
        match self.run()? {
            Halt::InputApplied => Ok(()),
            halt => Err(anyhow::format_err!(
                "shadow: unexpected halt while applying input: {halt:?}"
            )),
        }
    }
}

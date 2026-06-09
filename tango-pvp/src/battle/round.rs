use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::input::PartialInput;

use super::world::{MgbaState, MgbaWorld};
use super::EXPECTED_FPS;

/// Per-side input-queue capacity.
const MAX_QUEUE_LENGTH: usize = 120;

/// One round of live PvP. A thin shell around the generic
/// [`getgud::Session`]: it owns the rollback state machine plus the
/// mgba-specific I/O the engine deliberately knows nothing about — the network
/// sender (for shipping the local input each frame) and the live core's thread
/// handle (to restore the frame-rate target when the round ends).
pub struct Round {
    /// The rollback engine. `None` while the round is "armed" but hasn't reached
    /// its first commit; created — seeded with the first committed state — by
    /// [`start_session`](Round::start_session) on the first `main_read_joyflags`.
    session: Option<getgud::Session<MgbaWorld>>,
    /// This side's player index. A game/host concept, not the engine's — the
    /// per-game traps read it to drive p1/p2 register writes.
    local_player_index: u8,
    /// Per-game hooks for the running ROM. Held so the live render path can
    /// prime the loaded snapshot's local-joyflags register (r4) via
    /// [`inject_joyflags_on_primary_snapshot`](crate::hooks::Hooks::inject_joyflags_on_primary_snapshot).
    hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    /// Outbound network input channel. The engine no longer sends; we ship the
    /// local input ourselves (with the engine's frame advantage attached)
    /// before stepping it.
    sender: Arc<Mutex<Box<dyn crate::net::Sender + Send + Sync>>>,
    /// This side's live frame delay, owned by `PvpSession` and adjustable via
    /// the footer slider. Re-read into the engine each frame so a mid-round
    /// change takes effect on the next render. The engine itself just holds a
    /// plain value; this atomic is purely the host-side sharing mechanism.
    frame_delay: Arc<AtomicU32>,
    /// Handle to the live core's mgba thread, held so `Drop` can reset its
    /// `fps_target` when the round ends.
    primary_thread_handle: mgba::thread::Handle,
    /// Time-sync throttler. Its EMA state carries across frames;
    /// `add_local_input_and_fastforward` feeds it the engine's skew each frame to
    /// turn it into an fps target for the live core.
    throttler: super::throttler::Throttler,
    /// Tick of the last state loaded into the live core — the tick returned by
    /// the most recent [`Session::advance`](getgud::Session::advance), or 0
    /// before the first load. Read by the per-game `round_post_increment_tick`
    /// traps via [`last_loaded_tick`](Self::last_loaded_tick).
    last_loaded_tick: u32,
    /// Chip-select deliberation timer for this round, armed lazily by the
    /// per-game primary trap (it supplies the [`CustomScreenHooks`] adapter).
    /// `None` until armed; stays `None` all round if the match disabled it.
    /// [`crate::custom_screen::CustomScreenHooks`].
    custom_screen: Option<crate::custom_screen::CustomScreenTimer>,
    /// The match's deliberation cap (`None` = disabled), copied at construction.
    custom_screen_tick_limit: Option<u32>,
}

impl Round {
    pub(super) fn new(match_: &super::Match) -> Self {
        Self {
            session: None,
            local_player_index: match_.local_player_index(),
            hooks: match_.local_hooks(),
            sender: match_.sender_handle(),
            frame_delay: match_.frame_delay(),
            primary_thread_handle: match_.primary_thread_handle(),
            throttler: super::throttler::Throttler::new(),
            last_loaded_tick: 0,
            custom_screen: None,
            custom_screen_tick_limit: match_.custom_screen_tick_limit(),
        }
    }

    /// True if the match enabled the chip-select timer and it hasn't been armed
    /// yet — the per-game primary trap checks this to know when to call
    /// [`arm_custom_screen`](Self::arm_custom_screen).
    pub fn needs_custom_screen_armed(&self) -> bool {
        self.custom_screen_tick_limit.is_some() && self.custom_screen.is_none()
    }

    /// Arm this round's chip-select timer with the game's [`CustomScreenHooks`]
    /// adapter. No-op if the match disabled the timer.
    pub fn arm_custom_screen(&mut self, game: Box<dyn crate::custom_screen::CustomScreenHooks + Send>) {
        if let Some(limit) = self.custom_screen_tick_limit {
            self.custom_screen = Some(crate::custom_screen::CustomScreenTimer::new(game, limit));
        }
    }

    /// Run the chip-select timer one tick at this core's synced Tango tick
    /// (`last_loaded_tick+1`, the tick about to be processed — equal to the
    /// game's own tick, as the `round_post_increment_tick` checks assert).
    /// Returns the confirm joyflags to OR into the local input this frame (`0`
    /// when the timer is disabled/unarmed or not confirming).
    pub fn enforce_custom_screen(&mut self, core: mgba::core::CoreMutRef) -> u16 {
        let tick = self.last_loaded_tick + 1;
        self.custom_screen.as_mut().map_or(0, |t| t.enforce(core, tick))
    }

    /// Battle ticks left in the custom screen, or `None` when not in it / the
    /// timer is inactive. For the GUI countdown.
    pub fn custom_screen_remaining(&self) -> Option<u32> {
        self.custom_screen.as_ref().and_then(|t| t.remaining())
    }

    /// Build the rollback session from the round's first committed state, seeding
    /// the engine's settled checkpoint at tick 0. Called once per round from
    /// [`Match::record_first_commit`](super::Match::record_first_commit) when the
    /// live core reaches the first commit tick. The heavy
    /// [`Stepper`](crate::stepper::Stepper) is built here rather than at round
    /// start — it isn't needed until the first re-sim, which is post-commit.
    /// `shadow_snapshot` captures the opponent co-sim at its matching
    /// first-committed state so a rollback to tick 0 rewinds both cores.
    pub(super) fn start_session(
        &mut self,
        match_: &super::Match,
        local_state: Box<mgba::state::State>,
        first_packet: &[u8],
        shadow_snapshot: crate::shadow::ShadowSnapshot,
    ) -> anyhow::Result<()> {
        let hooks = match_.local_hooks();
        let stepper = crate::stepper::Stepper::new(
            match_.rom(),
            hooks,
            match_.match_type(),
            self.local_player_index,
            local_state.as_ref(),
        )?;
        let world = MgbaWorld {
            stepper,
            shadow: match_.shadow_handle(),
            last_outgoing: first_packet.to_vec(),
            replay_writer: match_.replay_writer_handle(),
            local_player_index: self.local_player_index,
        };
        self.session = Some(getgud::Session::new(getgud::SessionParams {
            present_delay: self.frame_delay.load(Ordering::Relaxed),
            initial_remote: PartialInput { joyflags: 0 },
            initial_state: MgbaState {
                primary: local_state,
                outgoing: first_packet.to_vec(),
                shadow_snapshot,
                tick: 0,
            },
            world,
        }));
        Ok(())
    }

    /// The opponent co-sim snapshot at the authoritative settled tick, cloned for
    /// re-anchoring the shared shadow before its round-end advance (the simulator
    /// may have parked the shadow ahead on a speculative tick). `None` before the
    /// first commit.
    pub(super) fn settled_shadow_snapshot(&self) -> Option<&crate::shadow::ShadowSnapshot> {
        self.session.as_ref().map(|s| &s.settled_state().shadow_snapshot)
    }

    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    /// Netcode frontier — advances one per wall-frame via the live core's
    /// post-tick hook.
    pub(crate) fn frontier(&self) -> u32 {
        self.session.as_ref().map_or(0, |s| s.local_frontier())
    }

    /// Tick of the last `present_state` loaded into the live core (0 before any
    /// load). Per-game `round_post_increment_tick` traps compare the game's
    /// tick against this.
    pub fn last_loaded_tick(&self) -> u32 {
        self.last_loaded_tick
    }

    /// Whether the round has reached its first commit and the rollback session
    /// is live. Until then the round is armed but not yet running.
    pub fn has_settled_snapshot(&self) -> bool {
        self.session.is_some()
    }

    pub fn local_frame_advantage(&self) -> i16 {
        self.session.as_ref().map_or(0, |s| s.local_tick_advantage())
    }

    pub fn last_remote_frame_advantage(&self) -> i16 {
        self.session.as_ref().map_or(0, |s| s.last_remote_tick_advantage())
    }

    /// Per-frame misprediction depth shown in the UI: how many speculative frames
    /// the most recent step discarded and re-simulated because a confirmed remote
    /// input contradicted the prediction. 0 on a clean frame; spikes on a
    /// rollback. See [`misprediction_depth`](getgud::Session::misprediction_depth).
    pub fn misprediction_depth(&self) -> u32 {
        self.session.as_ref().map_or(0, |s| s.last_misprediction_depth())
    }

    /// Called once per `main_read_joyflags` fire on the live primary. Ships the
    /// local input over the network (with the engine's frame advantage), then
    /// advances the rollback engine one displayed frame, loading the chosen
    /// state into `core`.
    pub async fn add_local_input_and_fastforward(
        &mut self,
        mut core: mgba::core::CoreMutRef<'_>,
        joyflags: u16,
    ) -> anyhow::Result<()> {
        let frame_advantage = self.local_frame_advantage();
        self.sender
            .lock()
            .await
            .send(&crate::net::Input {
                joyflags,
                frame_advantage,
            })
            .await?;

        // The session exists by now: the primary's first `main_read_joyflags`
        // calls `start_session` before this in the same trap fire.
        let session = self.session.as_mut().expect("round committed before stepping");
        if session.local_queue_length() >= MAX_QUEUE_LENGTH {
            anyhow::bail!("local overflowed our input buffer");
        }

        // Push the host-side live frame delay into the engine before stepping,
        // so a footer-slider change takes effect on this frame.
        session.set_present_delay(self.frame_delay.load(Ordering::Relaxed));

        // Sample skew before `advance` enqueues this tick's local input, so our
        // half matches the advantage we shipped the peer above (reading it after
        // would fold in the just-enqueued input and bias the skew up by one).
        let skew = session.skew();
        let frame = session.advance(PartialInput { joyflags })?;
        core.load_state(&frame.state.primary).expect("load present state");
        // The snapshot is poised at the start of `frame.tick` with its local
        // joyflags register (r4) unset — the engine carries that input on the
        // frame instead of baking it in. Prime it now so the live core resumes
        // the displayed tick with the right local input.
        let (local, _) = frame.input;
        self.hooks.inject_joyflags_on_primary_snapshot(core, local.joyflags);
        self.last_loaded_tick = frame.tick;
        // `frame`'s borrow of `session` ends here, freeing it to be re-queried.

        let slowdown = self.throttler.step(skew);
        core.gba_mut()
            .sync_mut()
            .expect("set fps target")
            .set_fps_target(EXPECTED_FPS - slowdown);
        Ok(())
    }

    pub fn add_remote_input(&mut self, input: crate::net::Input) {
        log::debug!("remote input: {:?}", input);
        self.session.as_mut().expect("round committed").add_remote_input(
            PartialInput {
                joyflags: input.joyflags,
            },
            input.frame_advantage,
        );
    }

    pub(super) fn can_add_remote_input(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|s| s.remote_queue_length() < MAX_QUEUE_LENGTH)
    }
}

impl Drop for Round {
    fn drop(&mut self) {
        // HACK: This is the only safe way to set the FPS without clogging
        // everything else up.
        self.primary_thread_handle
            .lock_audio()
            .sync_mut()
            .set_fps_target(EXPECTED_FPS);
    }
}

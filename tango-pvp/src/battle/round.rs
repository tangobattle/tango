use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::input::PartialInput;

use super::world::{MgbaState, MgbaWorld};
use super::{EXPECTED_FPS, MAX_QUEUE_LENGTH, RECONNECT_QUEUE_LENGTH};

/// One round of live PvP. A thin shell around the generic
/// [`getgud::Session`]: it owns the rollback state machine plus the
/// mgba-specific I/O the engine deliberately knows nothing about — the network
/// sender (for shipping the local input each frame) and the live core's thread
/// handle (to restore the frame-rate target when the round ends).
pub struct Round {
    /// The rollback engine. A round is allocated at `round_start_ret`, but
    /// the engine can't be built until the live core reaches the round's
    /// first commit — it must be seeded with that state — so this is `None`
    /// ("armed") for the round's early frames, until
    /// [`start_session`](Round::start_session) runs on the first
    /// `main_read_joyflags`. While armed, engine-metric accessors answer 0
    /// and remote inputs are held off
    /// ([`try_add_remote_input`](Round::try_add_remote_input)).
    session: Option<getgud::Session<MgbaWorld>>,
    /// Which local round this is (count of locally-ended rounds at creation).
    /// The drain below admits only queue entries tagged with this index:
    /// smaller tags are stale tails from rounds we already closed, larger
    /// ones belong to a future round a racing peer has started.
    round_idx: u32,
    /// Handoff queue from the net receive task, shared with the Match.
    remote_inputs: Arc<super::match_::RemoteInputs>,
    /// This side's player index. A game/host concept, not the engine's — the
    /// per-game traps read it to drive p1/p2 register writes.
    local_player_index: u8,
    /// Per-game hooks for the running ROM. Held so the live render path can
    /// prime the loaded snapshot's local-joyflags register (r4) via
    /// [`inject_joyflags_on_primary`](crate::hooks::Hooks::inject_joyflags_on_primary).
    hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    /// This side's live frame delay, owned by `PvpSession` and adjustable via
    /// the footer slider. Re-read into the engine each frame so a mid-round
    /// change takes effect on the next render. The engine itself just holds a
    /// plain value; this atomic is purely the host-side sharing mechanism.
    frame_delay: Arc<AtomicU32>,
    /// Handle to the live core's mgba thread, held so `Drop` can reset its
    /// `fps_target` when the round ends.
    primary_thread_handle: mgba::thread::Handle,
    /// Time-sync throttler. Its EMA state carries across frames;
    /// `add_local_input_and_fastforward` feeds it the engine's skew and
    /// speculation balance each frame to turn them into an fps target for the
    /// live core.
    throttler: super::throttler::Throttler,
    /// Tick of the last state loaded into the live core — the tick returned by
    /// the most recent [`Session::advance`](getgud::Session::advance), or 0
    /// before the first load. Read by the per-game `round_post_increment_tick`
    /// traps via [`last_loaded_tick`](Self::last_loaded_tick).
    last_loaded_tick: u32,
    /// Signals the session's reconnect coordinator the first frame the local
    /// input queue reaches [`RECONNECT_QUEUE_LENGTH`] (a dead link). Cloned from
    /// the [`Match`](super::Match) so it outlives the round.
    local_stall: Arc<tokio::sync::Notify>,
    /// Whether [`local_stall`](Self::local_stall) has already fired for the
    /// current above-threshold episode, so the rising edge signals once instead
    /// of every frame. Cleared when the queue drops back below the threshold —
    /// which, after a reconnect, the peer's resent inputs do, re-arming the
    /// signal for any later drop. Persisting on the round (which survives the
    /// reconnect pause) is what makes that re-arm automatic.
    stall_signaled: bool,
}

impl Round {
    pub(super) fn new(match_: &super::Match) -> Self {
        Self {
            session: None,
            round_idx: match_.current_local_round_idx(),
            remote_inputs: match_.remote_inputs_handle(),
            local_player_index: match_.local_player_index(),
            hooks: match_.local_hooks(),
            frame_delay: match_.frame_delay(),
            primary_thread_handle: match_.primary_thread_handle(),
            throttler: super::throttler::Throttler::new(),
            last_loaded_tick: 0,
            local_stall: match_.local_stall_handle(),
            stall_signaled: false,
        }
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
        // Wrap the shared shadow in its concurrent driver for the round. As
        // the stepper's remote-packet source it answers each re-sim tick's
        // trap immediately (the packet is buffered by the shadow's previous
        // run) and completes the shadow's own tick on its worker thread,
        // overlapping the rest of the primary's tick.
        let shadow = std::sync::Arc::new(crate::shadow::Worker::new(match_.shadow_handle()));
        let stepper = crate::stepper::Stepper::new(
            match_.rom(),
            hooks,
            match_.match_type(),
            self.local_player_index,
            local_state.as_ref(),
            shadow.clone(),
            match_.disable_bgm(),
        )?;
        let world = MgbaWorld {
            stepper,
            shadow,
            last_outgoing: first_packet.to_vec(),
            replay_writer: match_.replay_writer_handle(),
            local_player_index: self.local_player_index,
            state_pool: Vec::new(),
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

    fn local_tick_advantage(&self) -> i16 {
        self.session.as_ref().map_or(0, |s| s.local_tick_advantage())
    }

    /// Engine metrics for the host status bar; all zero while armed.
    pub(super) fn metrics(&self) -> super::RoundMetrics {
        super::RoundMetrics {
            local_tick_advantage: self.local_tick_advantage(),
            remote_tick_advantage: self.session.as_ref().map_or(0, |s| s.last_remote_tick_advantage()),
            misprediction_depth: self.session.as_ref().map_or(0, |s| s.last_misprediction_depth()),
        }
    }

    /// Called once per `main_read_joyflags` fire on the live primary. Ships the
    /// local input over `sender` (the match's outbound channel, with the
    /// engine's frame advantage attached — the engine itself never sends),
    /// then advances the rollback engine one displayed frame, loading the
    /// chosen state into `core`.
    pub fn add_local_input_and_fastforward(
        &mut self,
        sender: &super::SenderMutex,
        mut core: mgba::core::CoreMutRef<'_>,
        joyflags: u16,
    ) -> anyhow::Result<()> {
        let tick_advantage = self.local_tick_advantage();
        sender
            .lock()
            .unwrap()
            .send(&crate::net::Event::Input(crate::net::Input {
                joyflags,
                tick_advantage,
            }))?;

        // The engine exists by now: the primary's first `main_read_joyflags`
        // calls `start_session` before this in the same trap fire. Bail (the
        // per-game trap logs + cancels the match) rather than panicking the
        // emulator thread if a game's traps ever fire out of order.
        let Some(session) = self.session.as_mut() else {
            anyhow::bail!("add_local_input_and_fastforward on an armed round (no first commit yet)");
        };
        let queue_length = session.local_queue_length();
        if queue_length >= MAX_QUEUE_LENGTH {
            anyhow::bail!("local overflowed our input buffer");
        }
        // A dead link keeps this queue climbing ~one per frame (nothing arrives
        // to match against), so its depth is the liveness signal: at
        // RECONNECT_QUEUE_LENGTH wake the session's reconnect coordinator to
        // pause and rebuild, well short of the overflow bail above. Rising-edge —
        // signal once per episode, not every frame it sits high (e.g. before a
        // reconnect's resent inputs drain it) — and re-arm once it drops back.
        if queue_length >= RECONNECT_QUEUE_LENGTH {
            if !self.stall_signaled {
                self.stall_signaled = true;
                self.local_stall.notify_one();
            }
        } else {
            self.stall_signaled = false;
        }

        // Drain peer inputs that arrived since last frame into the engine.
        // The engine only consults remote inputs inside `advance`, so
        // draining here (instead of the net task pushing them the moment
        // they arrive) changes nothing about when they take effect.
        self.remote_inputs.drain(self.round_idx, |input| {
            log::debug!("remote input: {:?}", input);
            if session.remote_queue_length() >= MAX_QUEUE_LENGTH {
                anyhow::bail!("remote overflowed our input buffer");
            }
            session.add_remote_input(
                PartialInput {
                    joyflags: input.joyflags,
                },
                input.tick_advantage,
            );
            Ok(())
        })?;

        // Push the host-side live frame delay into the engine before stepping,
        // so a footer-slider change takes effect on this frame.
        session.set_present_delay(self.frame_delay.load(Ordering::Relaxed));

        // Sample skew before `advance` enqueues this tick's local input, so our
        // half matches the advantage we shipped the peer above (reading it after
        // would fold in the just-enqueued input and bias the skew up by one).
        let skew = session.skew();
        let frame = session.advance(PartialInput { joyflags })?;
        core.load_state(&frame.state.primary)?;
        // The snapshot is poised at the start of `frame.tick` with its local
        // joyflags register (r4) unset — the engine carries that input on the
        // frame instead of baking it in. Prime it now so the live core resumes
        // the displayed tick with the right local input.
        let (local, _) = frame.input;
        self.hooks.inject_joyflags_on_primary(core, local.joyflags);
        self.last_loaded_tick = frame.tick;
        // `frame`'s borrow of `session` ends here, freeing it to be re-queried.

        // Frames presented with the lead still inside the present delay are
        // fully confirmed — running ahead by that much costs no presentation
        // quality, so the throttler stays disengaged until the speculation
        // balance crosses the boundary, instead of shaving fps the player
        // can feel.
        let slowdown = self.throttler.step(skew, session.speculation_balance());
        core.gba_mut()
            .sync_mut()
            .expect("set fps target")
            .set_fps_target(EXPECTED_FPS - slowdown);
        Ok(())
    }
}

impl Drop for Round {
    fn drop(&mut self) {
        // Reset the throttler's shaved fps target so inter-round screens run
        // at full speed again. Handed to a detached thread: `Round` drops on
        // the emulator thread (`end_round`'s trap), and `Handle::lock_audio`
        // takes the same handle mutex that `Handle::pause` holds for its
        // entire wait on the emulator thread reaching a pause point — taking
        // it here inverts that lock order and deadlocks both threads if a
        // pause (e.g. the reconnect coordinator's) lands at round end. On a
        // thread nothing joins, the cycle can't close; the reset landing a
        // beat late at worst overwrites one frame of the next round's
        // target, which its own per-frame set immediately corrects.
        let handle = self.primary_thread_handle.clone();
        std::thread::spawn(move || {
            handle.lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);
        });
    }
}

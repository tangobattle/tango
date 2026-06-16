use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::input::PartialInput;

use super::world::{MgbaState, MgbaWorld};
use super::EXPECTED_FPS;

/// Per-side input-queue capacity: how many local inputs may sit unmatched
/// against remote ones (and vice versa) before the engine bails and cancels
/// the match. Public because it's the backpressure bound other layers size
/// against — anything queueing inputs upstream of the engine (e.g. the host's
/// send pump) can hold a bit more than this and rely on the engine's bail
/// firing first.
pub const MAX_QUEUE_LENGTH: usize = 2 * 60; // ~2 seconds

/// How long the local frontier may sit stalled above [`STALL_HIGH_WATER`] with
/// no confirmation progress before the round tears down. The emergency brake
/// holds a merely *slow* peer indefinitely (it keeps confirming, just below
/// realtime); this only fires once a peer has gone genuinely silent, turning
/// the old unrecoverable overflow bail into a deliberate timeout. Sized to the
/// session's peer-end grace.
const STALL_TIMEOUT_SECS: u64 = 5;
const STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(STALL_TIMEOUT_SECS);

/// Lead (unmatched local inputs — the same quantity the overflow bail guards)
/// at which the emergency time-sync brake engages. Derived from the watchdog,
/// not picked as a fraction of the cap: it sits far enough below the cap that a
/// peer going silent the instant the brake engages still can't overrun the cap
/// before the [`STALL_TIMEOUT`] watchdog tears the round down — the headroom is
/// the frozen frontier's floor creep over the grace window (see
/// [`stall_headroom`](super::throttler::stall_headroom)). So the recoverable
/// timeout always beats the unrecoverable overflow bail.
pub(super) const STALL_HIGH_WATER: usize =
    MAX_QUEUE_LENGTH - super::throttler::stall_headroom(STALL_TIMEOUT_SECS as f32);

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
    /// [`inject_joyflags_on_primary_snapshot`](crate::hooks::Hooks::inject_joyflags_on_primary_snapshot).
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
    /// Confirmation watchdog for the emergency stall. `last_confirm_frontier`
    /// is the count of settled (matched) pairs at the last observed progress;
    /// `stall_watch` is when that progress happened. While the lead sits above
    /// [`STALL_HIGH_WATER`] and confirmation hasn't advanced for
    /// [`STALL_TIMEOUT`], the peer has gone silent and the round tears down —
    /// the emergency brake has frozen the frontier but there is nothing to
    /// catch up to. Refreshed every frame during healthy play, so it only ever
    /// ages during an actual stall.
    last_confirm_frontier: u32,
    stall_watch: std::time::Instant,
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
            throttler: super::throttler::Throttler::new(STALL_HIGH_WATER as i32),
            last_loaded_tick: 0,
            last_confirm_frontier: 0,
            stall_watch: std::time::Instant::now(),
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

    pub(crate) fn local_player_index(&self) -> u8 {
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
    pub(crate) fn last_loaded_tick(&self) -> u32 {
        self.last_loaded_tick
    }

    /// Whether the round has reached its first commit and the rollback session
    /// is live. Until then the round is armed but not yet running.
    pub(crate) fn has_settled_snapshot(&self) -> bool {
        self.session.is_some()
    }

    fn local_frame_advantage(&self) -> i16 {
        self.session.as_ref().map_or(0, |s| s.local_tick_advantage())
    }

    /// Engine metrics for the host status bar; all zero while armed.
    pub(super) fn metrics(&self) -> super::RoundMetrics {
        super::RoundMetrics {
            local_frame_advantage: self.local_frame_advantage(),
            remote_frame_advantage: self.session.as_ref().map_or(0, |s| s.last_remote_tick_advantage()),
            misprediction_depth: self.session.as_ref().map_or(0, |s| s.last_misprediction_depth()),
        }
    }

    /// Called once per `main_read_joyflags` fire on the live primary. Ships the
    /// local input over `sender` (the match's outbound channel, with the
    /// engine's frame advantage attached — the engine itself never sends),
    /// then advances the rollback engine one displayed frame, loading the
    /// chosen state into `core`.
    pub(crate) async fn add_local_input_and_fastforward(
        &mut self,
        sender: &super::SenderMutex,
        mut core: mgba::core::CoreMutRef<'_>,
        joyflags: u16,
    ) -> anyhow::Result<()> {
        let frame_advantage = self.local_frame_advantage();
        sender
            .lock()
            .await
            .send(&crate::net::Event::Input(crate::net::Input {
                joyflags,
                frame_advantage,
            }))
            .await?;

        // The engine exists by now: the primary's first `main_read_joyflags`
        // calls `start_session` before this in the same trap fire. Bail (the
        // per-game trap logs + cancels the match) rather than panicking the
        // emulator thread if a game's traps ever fire out of order.
        let Some(session) = self.session.as_mut() else {
            anyhow::bail!("add_local_input_and_fastforward on an armed round (no first commit yet)");
        };
        if session.local_queue_length() >= MAX_QUEUE_LENGTH {
            anyhow::bail!("local overflowed our input buffer");
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
                input.frame_advantage,
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
        self.hooks.inject_joyflags_on_primary_snapshot(core, local.joyflags);
        self.last_loaded_tick = frame.tick;
        // `frame`'s borrow of `session` ends here, freeing it to be re-queried.

        // Emergency-stall watchdog. `lead` is the unmatched-local count — how
        // close we are to the overflow bail — and `confirm_frontier` is the
        // count of settled pairs, which advances as the peer's inputs match
        // ours. The emergency brake below freezes the frontier once the lead
        // crosses STALL_HIGH_WATER, holding a slow-but-live peer below the cap
        // indefinitely (confirmation keeps advancing, refreshing the watch). If
        // confirmation instead flatlines for STALL_TIMEOUT while parked up
        // here, the peer has gone silent: tear down deliberately rather than
        // freezing forever (or waiting to overrun the cap).
        let lead = session.local_queue_length();
        let confirm_frontier = session.local_frontier().saturating_sub(lead as u32);
        let now = std::time::Instant::now();
        if confirm_frontier > self.last_confirm_frontier {
            self.last_confirm_frontier = confirm_frontier;
            self.stall_watch = now;
        }
        if lead >= STALL_HIGH_WATER && now.duration_since(self.stall_watch) >= STALL_TIMEOUT {
            anyhow::bail!("peer stalled: {lead} frames ahead with no confirmation for {STALL_TIMEOUT:?}");
        }

        // Frames presented with the lead still inside the present delay are
        // fully confirmed — running ahead by that much costs no presentation
        // quality, so the throttler stays disengaged until the speculation
        // balance crosses the boundary, instead of shaving fps the player
        // can feel. Past STALL_HIGH_WATER the same call brakes hard toward a
        // near-full stall regardless, on the raw `lead`.
        let slowdown = self.throttler.step(skew, session.speculation_balance(), lead as i32);
        core.gba_mut()
            .sync_mut()
            .expect("set fps target")
            .set_fps_target(EXPECTED_FPS - slowdown);
        Ok(())
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

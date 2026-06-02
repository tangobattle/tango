use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::input::PartialInput;

use super::world::{MgbaPredictor, MgbaPresenter, MgbaSimulator, MgbaState, MgbaWorld, ReplayObserver};
use super::EXPECTED_FPS;

/// Per-side input-queue capacity.
const MAX_QUEUE_LENGTH: usize = 120;

/// One round of live PvP. A thin shell around the generic
/// [`getgud::Session`]: it owns the rollback state machine plus the
/// mgba-specific I/O the engine deliberately knows nothing about — the network
/// sender (for shipping the local input each frame) and the live core's thread
/// handle (to restore the frame-rate target when the round ends).
pub struct Round {
    session: getgud::Session<MgbaWorld>,
    /// This side's player index. A game/host concept, not the engine's — the
    /// per-game traps read it to drive p1/p2 register writes.
    local_player_index: u8,
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
}

impl Round {
    pub(super) fn new(match_: &super::Match) -> anyhow::Result<Self> {
        let hooks = match_.local_hooks();
        let local_player_index = match_.local_player_index();

        let ff = crate::stepper::Fastforwarder::new(match_.rom(), hooks, match_.match_type(), local_player_index)?;
        let simulator = Box::new(MgbaSimulator {
            ff,
            shadow: match_.shadow_handle(),
            hooks,
            last_remote_packet: vec![0u8; hooks.packet_size()],
        });
        let predictor: Arc<dyn getgud::Predictor<MgbaWorld>> = Arc::new(MgbaPredictor);
        let observer: Box<dyn getgud::CommitObserver<MgbaWorld>> = Box::new(ReplayObserver {
            writer: match_.replay_writer_handle(),
            local_player_index,
        });

        let frame_delay = match_.frame_delay();
        let session = getgud::Session::new(getgud::SessionParams {
            frame_delay: frame_delay.load(Ordering::Relaxed),
            max_queue: MAX_QUEUE_LENGTH,
            initial_remote: PartialInput { joyflags: 0 },
            simulator,
            predictor,
            observer: Some(observer),
            throttler: match_.build_throttler(),
        });

        Ok(Self {
            session,
            local_player_index,
            sender: match_.sender_handle(),
            frame_delay,
            primary_thread_handle: match_.primary_thread_handle(),
        })
    }

    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    /// Netcode frontier — advances one per wall-frame via the live core's
    /// post-tick hook.
    pub(crate) fn frontier(&self) -> u32 {
        self.session.frontier()
    }

    /// Tick of the last `present_state` loaded into the live core (0 before any
    /// load). Per-game `round_post_increment_tick` traps compare the game's
    /// tick against this.
    pub fn last_loaded_tick(&self) -> u32 {
        self.session.presented_tick()
    }

    /// Called from each per-game `round_post_increment_tick` trap to keep the
    /// netcode frontier in lockstep with the wall clock.
    pub fn advance_frontier(&mut self) {
        self.session.advance_frontier();
    }

    pub fn set_first_settled_state(&mut self, local_state: Box<mgba::state::State>, first_packet: &[u8]) {
        self.session.set_first_settled_state(MgbaState {
            core: local_state,
            outgoing: first_packet.to_vec(),
        });
    }

    pub fn has_committed_state(&self) -> bool {
        self.session.has_committed_state()
    }

    pub fn local_frame_advantage(&self) -> i16 {
        self.session.local_frame_advantage()
    }

    pub fn last_remote_frame_advantage(&self) -> i16 {
        self.session.last_remote_frame_advantage()
    }

    pub fn speculative_depth(&self) -> u32 {
        self.session.speculative_depth()
    }

    /// Called once per `main_read_joyflags` fire on the live primary. Ships the
    /// local input over the network (with the engine's frame advantage), then
    /// advances the rollback engine one displayed frame, loading the chosen
    /// state into `core`.
    pub async fn add_local_input_and_fastforward(
        &mut self,
        core: mgba::core::CoreMutRef<'_>,
        joyflags: u16,
    ) -> anyhow::Result<()> {
        let frame_advantage = self.session.local_frame_advantage();
        self.sender
            .lock()
            .await
            .send(&crate::net::Input { joyflags, frame_advantage })
            .await?;

        // Push the host-side live frame delay into the engine before stepping,
        // so a footer-slider change takes effect on this frame.
        self.session.set_frame_delay(self.frame_delay.load(Ordering::Relaxed));

        let mut presenter = MgbaPresenter { core };
        self.session.advance(&mut presenter, PartialInput { joyflags })
    }

    pub fn add_remote_input(&mut self, input: crate::net::Input) {
        log::debug!("remote input: {:?}", input);
        self.session.add_remote_input(PartialInput { joyflags: input.joyflags }, input.frame_advantage);
    }

    pub(super) fn can_add_remote_input(&self) -> bool {
        self.session.can_add_remote_input()
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

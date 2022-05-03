use rand::Rng;

use crate::audio;
use crate::fastforwarder;
use crate::game;
use crate::hooks;
use crate::input;
use crate::protocol;
use crate::replay;
use crate::shadow;
use crate::transport;

pub const TURN_TX_DELAY: u8 = 64;

#[derive(Clone, Debug)]
pub struct Settings {
    pub ice_servers: Vec<String>,
    pub matchmaking_connect_addr: String,
    pub session_id: String,
    pub replays_path: std::path::PathBuf,
    pub shadow_save_path: std::path::PathBuf,
    pub replay_metadata: Vec<u8>,
    pub match_type: u16,
    pub input_delay: u32,
}

pub struct RoundState {
    pub number: u8,
    pub round: Option<Round>,
    pub won_last_round: bool,
}

impl RoundState {
    pub async fn end_round(&mut self) -> anyhow::Result<()> {
        match self.round.take() {
            Some(mut round) => {
                round
                    .replay_writer
                    .take()
                    .unwrap()
                    .finish()
                    .expect("finish");
            }
            None => {
                return Ok(());
            }
        }
        log::info!("round ended");
        Ok(())
    }

    pub fn set_won_last_round(&mut self, did_win: bool) {
        self.won_last_round = did_win;
    }
}

pub struct Match {
    shadow: std::sync::Arc<tokio::sync::Mutex<shadow::Shadow>>,
    audio_supported_config: cpal::SupportedStreamConfig,
    rom_path: std::path::PathBuf,
    hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
    _peer_conn: datachannel_wrapper::PeerConnection,
    dc_rx: tokio::sync::Mutex<datachannel_wrapper::DataChannelReceiver>,
    transport: std::sync::Arc<tokio::sync::Mutex<transport::Transport>>,
    rng: tokio::sync::Mutex<rand_pcg::Mcg128Xsl64>,
    settings: Settings,
    is_offerer: bool,
    round_state: tokio::sync::Mutex<RoundState>,
    primary_thread_handle: mgba::thread::Handle,
    audio_mux: audio::mux_stream::MuxStream,
}

#[derive(Debug)]
pub enum NegotiationError {
    ExpectedHello,
    ExpectedHola,
    IdenticalCommitment,
    ProtocolVersionMismatch,
    MatchTypeMismatch,
    IncompatibleGames,
    InvalidCommitment,
    Other(anyhow::Error),
}

impl From<anyhow::Error> for NegotiationError {
    fn from(err: anyhow::Error) -> Self {
        NegotiationError::Other(err)
    }
}

impl From<datachannel_wrapper::Error> for NegotiationError {
    fn from(err: datachannel_wrapper::Error) -> Self {
        NegotiationError::Other(err.into())
    }
}

impl From<std::io::Error> for NegotiationError {
    fn from(err: std::io::Error) -> Self {
        NegotiationError::Other(err.into())
    }
}

impl std::fmt::Display for NegotiationError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            NegotiationError::ExpectedHello => write!(f, "expected hello"),
            NegotiationError::ExpectedHola => write!(f, "expected hola"),
            NegotiationError::IdenticalCommitment => write!(f, "identical commitment"),
            NegotiationError::ProtocolVersionMismatch => write!(f, "protocol version mismatch"),
            NegotiationError::MatchTypeMismatch => write!(f, "match type mismatch"),
            NegotiationError::IncompatibleGames => write!(f, "game mismatch"),
            NegotiationError::InvalidCommitment => write!(f, "invalid commitment"),
            NegotiationError::Other(e) => write!(f, "other error: {}", e),
        }
    }
}

impl std::error::Error for NegotiationError {}

pub enum NegotiationFailure {
    ProtocolVersionMismatch,
    MatchTypeMismatch,
    IncompatibleGames,
    Unknown,
}

pub enum NegotiationStatus {
    Ready,
    NotReady(NegotiationProgress),
    Failed(NegotiationFailure),
}

#[derive(Clone, Debug)]
pub enum NegotiationProgress {
    NotStarted,
    Signalling,
    Handshaking,
}

const MAX_QUEUE_LENGTH: usize = 120;

impl Match {
    pub fn new(
        audio_supported_config: cpal::SupportedStreamConfig,
        rom_path: std::path::PathBuf,
        hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
        audio_mux: audio::mux_stream::MuxStream,
        peer_conn: datachannel_wrapper::PeerConnection,
        dc: datachannel_wrapper::DataChannel,
        mut rng: rand_pcg::Mcg128Xsl64,
        is_offerer: bool,
        primary_thread_handle: mgba::thread::Handle,
        settings: Settings,
    ) -> anyhow::Result<std::sync::Arc<Self>> {
        let (dc_rx, dc_tx) = dc.split();
        let did_polite_win_last_round = rng.gen::<bool>();
        let won_last_round = did_polite_win_last_round == is_offerer;
        let match_ = std::sync::Arc::new(Self {
            shadow: std::sync::Arc::new(tokio::sync::Mutex::new(shadow::Shadow::new(
                &rom_path,
                &settings.shadow_save_path,
                hooks,
                settings.match_type,
                is_offerer,
                won_last_round,
                rng.clone(),
            )?)),
            audio_supported_config,
            rom_path,
            hooks,
            _peer_conn: peer_conn,
            dc_rx: tokio::sync::Mutex::new(dc_rx),
            transport: std::sync::Arc::new(tokio::sync::Mutex::new(transport::Transport::new(
                dc_tx,
            ))),
            rng: tokio::sync::Mutex::new(rng),
            settings,
            round_state: tokio::sync::Mutex::new(RoundState {
                number: 0,
                round: None,
                won_last_round,
            }),
            is_offerer,
            audio_mux,
            primary_thread_handle,
        });
        Ok(match_)
    }

    pub async fn advance_shadow_until_round_end(&self) -> anyhow::Result<()> {
        self.shadow.lock().await.advance_until_round_end()
    }

    pub async fn exchange_init_with_shadow(&self, local_init: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        log::info!("local init: {:?}", local_init);
        let remote_init = self.shadow.lock().await.exchange_init(local_init)?;
        log::info!("remote init: {:?}", remote_init);
        Ok(remote_init)
    }

    pub async fn advance_shadow_until_first_committed_state(
        &self,
    ) -> anyhow::Result<mgba::state::State> {
        self.shadow
            .lock()
            .await
            .advance_until_first_committed_state()
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let mut dc_rx = self.dc_rx.lock().await;
        loop {
            match protocol::Packet::deserialize(
                match dc_rx.receive().await {
                    None => break,
                    Some(buf) => buf,
                }
                .as_slice(),
            )? {
                protocol::Packet::Input(input) => {
                    let first_state_committed_rx = {
                        let mut round_state = self.round_state.lock().await;

                        if input.round_number != round_state.number {
                            log::info!("round number mismatch, dropping input");
                            continue;
                        }

                        let round = match &mut round_state.round {
                            None => {
                                log::info!("no round in progress, dropping input");
                                continue;
                            }
                            Some(b) => b,
                        };
                        round.first_state_committed_rx.take()
                    };

                    if let Some(first_state_committed_rx) = first_state_committed_rx {
                        first_state_committed_rx.await.unwrap();
                    }

                    let mut round_state = self.round_state.lock().await;
                    let round = match &mut round_state.round {
                        None => {
                            log::info!("no round in progress, dropping input");
                            continue;
                        }
                        Some(b) => b,
                    };

                    if !round.can_add_remote_input() {
                        anyhow::bail!("remote overflowed our input buffer");
                    }

                    round.add_remote_input(input::PartialInput {
                        local_tick: input.local_tick,
                        remote_tick: input.remote_tick,
                        joyflags: input.joyflags as u16,
                    });
                }
                p => anyhow::bail!("unknown packet: {:?}", p),
            }
        }

        Ok(())
    }

    pub async fn lock_round_state(&self) -> tokio::sync::MutexGuard<'_, RoundState> {
        self.round_state.lock().await
    }

    pub async fn lock_rng(&self) -> tokio::sync::MutexGuard<'_, rand_pcg::Mcg128Xsl64> {
        self.rng.lock().await
    }

    pub fn match_type(&self) -> u16 {
        self.settings.match_type
    }

    pub fn is_offerer(&self) -> bool {
        self.is_offerer
    }

    pub async fn start_round(
        self: &std::sync::Arc<Self>,
        core: mgba::core::CoreMutRef<'_>,
    ) -> anyhow::Result<()> {
        let mut round_state = self.round_state.lock().await;
        round_state.number += 1;
        let local_player_index = if round_state.won_last_round { 0 } else { 1 };
        log::info!(
            "starting round: local_player_index = {}",
            local_player_index
        );
        let mut replay_filename = self.settings.replays_path.clone();
        replay_filename.push(format!("round{}.tangoreplay", round_state.number));
        let replay_filename = std::path::Path::new(&replay_filename);
        let replay_file = std::fs::File::create(&replay_filename)?;
        log::info!("opened replay: {}", replay_filename.display());

        log::info!("starting audio core");
        let mut audio_core = mgba::core::Core::new_gba("tango")?;
        let audio_save_state_holder = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let rom_vf = mgba::vfile::VFile::open(&self.rom_path, mgba::vfile::flags::O_RDONLY)?;
        audio_core.as_mut().load_rom(rom_vf)?;
        audio_core.set_traps(
            self.hooks
                .audio_traps(audio_save_state_holder.clone(), local_player_index),
        );

        log::info!("starting audio thread");
        let audio_core_thread = mgba::thread::Thread::new(audio_core);
        audio_core_thread.start()?;
        let audio_core_handle = audio_core_thread.handle();

        audio_core_handle.pause();
        let audio_core_mux_handle =
            self.audio_mux
                .open_stream(audio::mgba_stream::MGBAStream::new(
                    audio_core_handle.clone(),
                    self.audio_supported_config.sample_rate(),
                ));

        log::info!("loading our state into audio core");
        {
            let audio_core_mux_handle = audio_core_mux_handle.clone();
            let save_state = core.save_state()?;
            audio_core_handle.run_on_core(move |mut core| {
                core.gba_mut()
                    .sync_mut()
                    .as_mut()
                    .expect("sync")
                    .set_fps_target(game::EXPECTED_FPS as f32);
                core.load_state(&save_state).expect("load state");
            });
            audio_core_mux_handle.switch();
        }
        audio_core_handle.unpause();

        log::info!("preparing round state");

        let (first_state_committed_tx, first_state_committed_rx) = tokio::sync::oneshot::channel();
        round_state.round = Some(Round {
            local_player_index,
            iq: input::PairQueue::new(MAX_QUEUE_LENGTH, self.settings.input_delay),
            remote_delay: 0,
            is_accepting_input: false,
            last_committed_remote_input: input::Input {
                local_tick: 0,
                remote_tick: 0,
                joyflags: 0,
                custom_screen_state: 0,
                turn: vec![],
            },
            last_input: None,
            first_state_committed_tx: Some(first_state_committed_tx),
            first_state_committed_rx: Some(first_state_committed_rx),
            committed_state: None,
            local_pending_turn: None,
            replay_writer: Some(replay::Writer::new(
                Box::new(replay_file),
                &self.settings.replay_metadata,
                local_player_index,
            )?),
            fastforwarder: fastforwarder::Fastforwarder::new(
                &self.rom_path,
                self.hooks,
                local_player_index,
            )?,
            audio_save_state_holder,
            _audio_core_thread: audio_core_thread,
            _audio_core_mux_handle: audio_core_mux_handle,
            primary_thread_handle: self.primary_thread_handle.clone(),
            transport: self.transport.clone(),
            shadow: self.shadow.clone(),
        });
        log::info!("round has started");
        Ok(())
    }
}

struct PendingTurn {
    tx_buf: Vec<u8>,
    ticks_left: u8,
}

pub struct Round {
    local_player_index: u8,
    iq: input::PairQueue<input::Input, input::PartialInput>,
    remote_delay: u32,
    is_accepting_input: bool,
    last_committed_remote_input: input::Input,
    last_input: Option<input::Pair<input::Input, input::Input>>,
    first_state_committed_tx: Option<tokio::sync::oneshot::Sender<()>>,
    first_state_committed_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    committed_state: Option<mgba::state::State>,
    local_pending_turn: Option<PendingTurn>,
    replay_writer: Option<replay::Writer>,
    fastforwarder: fastforwarder::Fastforwarder,
    audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
    primary_thread_handle: mgba::thread::Handle,
    _audio_core_thread: mgba::thread::Thread,
    _audio_core_mux_handle: audio::mux_stream::MuxHandle,
    transport: std::sync::Arc<tokio::sync::Mutex<transport::Transport>>,
    shadow: std::sync::Arc<tokio::sync::Mutex<shadow::Shadow>>,
}

impl Round {
    pub fn fastforwarder(&mut self) -> &mut fastforwarder::Fastforwarder {
        &mut self.fastforwarder
    }

    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    pub fn remote_player_index(&self) -> u8 {
        1 - self.local_player_index
    }

    pub fn set_first_committed_state(
        &mut self,
        state: mgba::state::State,
        remote_state: mgba::state::State,
    ) {
        self.replay_writer
            .as_mut()
            .unwrap()
            .write_state(&state)
            .expect("write local state");
        self.replay_writer
            .as_mut()
            .unwrap()
            .write_state(&remote_state)
            .expect("write remote state");
        self.committed_state = Some(state);
        if let Some(tx) = self.first_state_committed_tx.take() {
            let _ = tx.send(());
        }
    }

    pub fn fill_input_delay(&mut self, current_tick: u32) {
        log::info!(
            "local delay = {}, remote_delay = {}",
            self.local_delay(),
            self.remote_delay()
        );
        for i in 0..self.local_delay() {
            self.add_local_input(input::Input {
                local_tick: current_tick + i,
                remote_tick: 0,
                joyflags: 0,
                custom_screen_state: 0,
                turn: vec![],
            });
        }
        for i in 0..self.remote_delay() {
            self.add_remote_input(input::PartialInput {
                local_tick: current_tick + i,
                remote_tick: 0,
                joyflags: 0,
            });
        }
    }

    pub async fn add_local_input_and_fastforward(
        &mut self,
        mut core: mgba::core::CoreMutRef<'_>,
        round_number: u8,
        current_tick: u32,
        joyflags: u16,
        custom_screen_state: u8,
        turn: Vec<u8>,
    ) -> bool {
        let local_tick = current_tick + self.local_delay();
        let remote_tick = self.last_committed_remote_input().local_tick;

        // We do it in this order such that:
        // 1. We make sure that the input buffer does not overflow if we were to add an input.
        // 2. We try to send it to the peer: if it fails, we don't end up desyncing the opponent as we haven't added the input ourselves yet.
        // 3. We add the input to our buffer: no overflow is guaranteed because we already checked ahead of time.
        //
        // This is all done while the self is locked, so there are no TOCTTOU issues.
        if !self.can_add_local_input() {
            log::warn!("local input buffer overflow!");
            return false;
        }

        if let Err(e) = self
            .transport
            .lock()
            .await
            .send_input(round_number, local_tick, remote_tick, joyflags)
            .await
        {
            log::warn!("failed to send input: {}", e);
            return false;
        }

        self.add_local_input(input::Input {
            local_tick,
            remote_tick,
            joyflags,
            custom_screen_state,
            turn,
        });

        let (input_pairs, left) = self.consume_and_peek_local().await;

        let committed_state = self
            .committed_state()
            .as_ref()
            .expect("committed state")
            .clone();
        let last_committed_remote_input = self.last_committed_remote_input();

        let (committed_state, dirty_state, last_input) = match self.fastforwarder().fastforward(
            &committed_state,
            &input_pairs,
            last_committed_remote_input,
            &left,
        ) {
            Ok(t) => t,
            Err(e) => {
                log::error!("fastforwarder failed with error: {}", e);
                return false;
            }
        };

        core.load_state(&dirty_state).expect("load dirty state");

        self.set_audio_save_state(dirty_state);

        // const RENDEZVOUS_AUDIO_EVERY: u32 = 10;
        // if current_tick % RENDEZVOUS_AUDIO_EVERY == 0 {
        //     self
        //         .wait_for_audio_rendezvous()
        //         .expect("wait for audio rendezvous");
        // }

        self.set_committed_state(committed_state);
        self.set_last_input(last_input);

        core.gba_mut()
            .sync_mut()
            .expect("set fps target")
            .set_fps_target((game::EXPECTED_FPS as i32 + self.tps_adjustment()) as f32);

        true
    }

    pub fn has_committed_state(&mut self) -> bool {
        self.committed_state.is_some()
    }

    pub fn set_committed_state(&mut self, state: mgba::state::State) {
        self.committed_state = Some(state);
    }

    pub fn set_audio_save_state(&mut self, state: mgba::state::State) {
        *self.audio_save_state_holder.lock() = Some(state);
    }

    pub fn set_last_input(&mut self, inp: input::Pair<input::Input, input::Input>) {
        self.last_input = Some(inp);
    }

    pub fn take_last_input(&mut self) -> Option<input::Pair<input::Input, input::Input>> {
        self.last_input.take()
    }

    pub fn local_delay(&self) -> u32 {
        self.iq.local_delay()
    }

    pub fn set_remote_delay(&mut self, delay: u32) {
        self.remote_delay = delay;
    }

    pub fn remote_delay(&self) -> u32 {
        self.remote_delay
    }

    pub fn local_queue_length(&self) -> usize {
        self.iq.local_queue_length()
    }

    pub fn remote_queue_length(&self) -> usize {
        self.iq.remote_queue_length()
    }

    pub fn start_accepting_input(&mut self) {
        self.is_accepting_input = true;
    }

    pub fn is_accepting_input(&self) -> bool {
        self.is_accepting_input
    }

    pub fn last_committed_remote_input(&self) -> input::Input {
        self.last_committed_remote_input.clone()
    }

    pub fn committed_state(&self) -> &Option<mgba::state::State> {
        &self.committed_state
    }

    pub async fn consume_and_peek_local(
        &mut self,
    ) -> (
        Vec<input::Pair<input::Input, input::Input>>,
        Vec<input::Input>,
    ) {
        let (partial_input_pairs, left) = self.iq.consume_and_peek_local();

        let mut shadow = self.shadow.lock().await;
        let input_pairs = partial_input_pairs
            .into_iter()
            .map(|pair| shadow.apply_input(pair).expect("apply input to shadow"))
            .collect::<Vec<_>>();

        if let Some(last) = input_pairs.last() {
            self.last_committed_remote_input = last.remote.clone();
        }

        for ip in &input_pairs {
            self.replay_writer
                .as_mut()
                .unwrap()
                .write_input(self.local_player_index, ip)
                .expect("write input");
        }

        (input_pairs, left)
    }

    pub fn can_add_local_input(&mut self) -> bool {
        self.iq.local_queue_length() < MAX_QUEUE_LENGTH
    }

    pub fn add_local_input(&mut self, input: input::Input) {
        log::debug!("local input: {:?}", input);
        self.iq.add_local_input(input);
    }

    pub fn can_add_remote_input(&mut self) -> bool {
        self.iq.remote_queue_length() < MAX_QUEUE_LENGTH
    }

    pub fn add_remote_input(&mut self, input: input::PartialInput) {
        log::debug!("remote input: {:?}", input);
        self.iq.add_remote_input(input);
    }

    pub fn add_local_pending_turn(&mut self, tx_buf: Vec<u8>) {
        self.local_pending_turn = Some(PendingTurn {
            ticks_left: TURN_TX_DELAY,
            tx_buf,
        })
    }

    pub fn take_local_pending_turn(&mut self) -> Vec<u8> {
        match &mut self.local_pending_turn {
            Some(pt) => {
                if pt.ticks_left > 0 {
                    pt.ticks_left -= 1;
                    if pt.ticks_left != 0 {
                        return vec![];
                    }
                    self.local_pending_turn.take().unwrap().tx_buf
                } else {
                    vec![]
                }
            }
            None => vec![],
        }
    }

    pub fn tps_adjustment(&self) -> i32 {
        let last_local_input = match &self.last_input {
            Some(input::Pair { local, .. }) => local,
            None => {
                return 0;
            }
        };
        (last_local_input.lag() - self.last_committed_remote_input.lag())
            - (self.local_delay() as i32 - self.remote_delay() as i32)
    }
}

impl Drop for Round {
    fn drop(&mut self) {
        // HACK: This is the only safe way to set the FPS without clogging everything else up.
        self.primary_thread_handle
            .lock_audio()
            .core_mut()
            .gba_mut()
            .sync_mut()
            .expect("sync")
            .set_fps_target(game::EXPECTED_FPS as f32);
    }
}

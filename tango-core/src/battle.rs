use rand::Rng;

use crate::audio;
use crate::facade;
use crate::fastforwarder;
use crate::game;
use crate::hooks;
use crate::input;
use crate::protocol;
use crate::replay;

#[derive(Clone, Debug)]
pub struct Settings {
    pub ice_servers: Vec<String>,
    pub matchmaking_connect_addr: String,
    pub session_id: String,
    pub replays_path: std::path::PathBuf,
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
}

pub struct Match {
    audio_supported_config: cpal::SupportedStreamConfig,
    rom_path: std::path::PathBuf,
    hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
    _peer_conn: datachannel_wrapper::PeerConnection,
    dc_rx: tokio::sync::Mutex<datachannel_wrapper::DataChannelReceiver>,
    dc_tx: tokio::sync::Mutex<datachannel_wrapper::DataChannelSender>,
    rng: tokio::sync::Mutex<rand_pcg::Mcg128Xsl64>,
    settings: Settings,
    round_state: tokio::sync::Mutex<RoundState>,
    remote_init_sender: tokio::sync::mpsc::Sender<protocol::Init>,
    remote_init_receiver: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<protocol::Init>>,
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
    ) -> Self {
        let (remote_init_sender, remote_init_receiver) = tokio::sync::mpsc::channel(1);
        let (dc_rx, dc_tx) = dc.split();
        let did_polite_win_last_round = rng.gen::<bool>();
        Self {
            audio_supported_config,
            rom_path,
            hooks,
            _peer_conn: peer_conn,
            dc_rx: tokio::sync::Mutex::new(dc_rx),
            dc_tx: tokio::sync::Mutex::new(dc_tx),
            rng: tokio::sync::Mutex::new(rng),
            settings,
            round_state: tokio::sync::Mutex::new(RoundState {
                number: 0,
                round: None,
                won_last_round: did_polite_win_last_round == is_offerer,
            }),
            remote_init_sender,
            remote_init_receiver: tokio::sync::Mutex::new(remote_init_receiver),
            audio_mux,
            primary_thread_handle,
        }
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
                protocol::Packet::Init(init) => {
                    self.remote_init_sender.send(init).await?;
                }
                protocol::Packet::Input(input) => {
                    let state_committed_rx = {
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
                        round.state_committed_rx.take()
                    };

                    if let Some(state_committed_rx) = state_committed_rx {
                        state_committed_rx.await.unwrap();
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

                    round.add_remote_input(input::Input {
                        local_tick: input.local_tick,
                        remote_tick: input.remote_tick,
                        joyflags: input.joyflags as u16,
                        custom_screen_state: input.custom_screen_state as u8,
                        turn: input.turn,
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

    pub async fn receive_remote_init(&self) -> Option<protocol::Init> {
        let mut remote_init_receiver = self.remote_init_receiver.lock().await;
        remote_init_receiver.recv().await
    }

    pub async fn send_init(
        &self,
        round_number: u8,
        input_delay: u32,
        marshaled: &[u8],
    ) -> anyhow::Result<()> {
        self.dc_tx
            .lock()
            .await
            .send(
                protocol::Packet::Init(protocol::Init {
                    round_number,
                    input_delay,
                    marshaled: marshaled.to_vec(),
                })
                .serialize()?
                .as_slice(),
            )
            .await?;
        Ok(())
    }

    pub async fn send_input(
        &self,
        round_number: u8,
        local_tick: u32,
        remote_tick: u32,
        joyflags: u16,
        custom_screen_state: u8,
        turn: Vec<u8>,
    ) -> anyhow::Result<()> {
        self.dc_tx
            .lock()
            .await
            .send(
                protocol::Packet::Input(protocol::Input {
                    round_number,
                    local_tick,
                    remote_tick,
                    joyflags,
                    custom_screen_state,
                    turn,
                })
                .serialize()?
                .as_slice(),
            )
            .await?;
        Ok(())
    }

    pub async fn lock_rng(&self) -> tokio::sync::MutexGuard<'_, rand_pcg::Mcg128Xsl64> {
        self.rng.lock().await
    }

    pub fn match_type(&self) -> u16 {
        self.settings.match_type
    }

    pub async fn start_round(&self, core: mgba::core::CoreMutRef<'_>) -> anyhow::Result<()> {
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
        audio_core.set_traps(self.hooks.audio_traps(facade::AudioFacade::new(
            audio_save_state_holder.clone(),
            local_player_index,
        )));

        log::info!("starting audio thread");
        let audio_core_thread = mgba::thread::Thread::new(audio_core);
        audio_core_thread.start()?;
        let audio_core_handle = audio_core_thread.handle();

        audio_core_handle.pause();
        let audio_core_mux_handle =
            self.audio_mux
                .open_stream(audio::timewarp_stream::TimewarpStream::new(
                    audio_core_handle.clone(),
                    self.audio_supported_config.sample_rate(),
                    self.audio_supported_config.channels(),
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

        let (state_committed_tx, state_committed_rx) = tokio::sync::oneshot::channel();
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
            state_committed_tx: Some(state_committed_tx),
            state_committed_rx: Some(state_committed_rx),
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
        });
        log::info!("round has started");
        Ok(())
    }
}

struct LocalPendingTurn {
    marshaled: Vec<u8>,
    ticks_left: u8,
}

pub struct Round {
    local_player_index: u8,
    iq: input::PairQueue<input::Input>,
    remote_delay: u32,
    is_accepting_input: bool,
    last_committed_remote_input: input::Input,
    last_input: Option<input::Pair<input::Input>>,
    state_committed_tx: Option<tokio::sync::oneshot::Sender<()>>,
    state_committed_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    committed_state: Option<mgba::state::State>,
    local_pending_turn: Option<LocalPendingTurn>,
    replay_writer: Option<replay::Writer>,
    fastforwarder: fastforwarder::Fastforwarder,
    audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
    primary_thread_handle: mgba::thread::Handle,
    _audio_core_thread: mgba::thread::Thread,
    _audio_core_mux_handle: audio::mux_stream::MuxHandle,
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

    pub fn set_committed_state(&mut self, state: mgba::state::State) {
        if self.committed_state.is_none() {
            self.replay_writer
                .as_mut()
                .unwrap()
                .write_state(&state)
                .expect("write state");
            self.replay_writer
                .as_mut()
                .unwrap()
                .write_state_placeholder()
                .expect("write state");
        }
        self.committed_state = Some(state);
        if let Some(tx) = self.state_committed_tx.take() {
            let _ = tx.send(());
        }
    }

    pub fn set_audio_save_state(&mut self, state: mgba::state::State) {
        *self.audio_save_state_holder.lock() = Some(state);
    }

    pub fn set_last_input(&mut self, inp: input::Pair<input::Input>) {
        self.last_input = Some(inp);
    }

    pub fn take_last_input(&mut self) -> Option<input::Pair<input::Input>> {
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

    pub fn mark_accepting_input(&mut self) {
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

    pub fn consume_and_peek_local(
        &mut self,
    ) -> (Vec<input::Pair<input::Input>>, Vec<input::Input>) {
        let (input_pairs, left) = self.iq.consume_and_peek_local();
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

    pub fn add_remote_input(&mut self, input: input::Input) {
        log::debug!("remote input: {:?}", input);
        self.iq.add_remote_input(input);
    }

    pub fn add_local_pending_turn(&mut self, marshaled: Vec<u8>) {
        self.local_pending_turn = Some(LocalPendingTurn {
            ticks_left: 64,
            marshaled,
        })
    }

    pub fn take_local_pending_turn(&mut self) -> Vec<u8> {
        match &mut self.local_pending_turn {
            Some(lpt) => {
                if lpt.ticks_left > 0 {
                    lpt.ticks_left -= 1;
                    if lpt.ticks_left != 0 {
                        return vec![];
                    }
                    self.local_pending_turn.take().unwrap().marshaled
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

use rand::Rng;

use crate::audio;
use crate::datachannel;
use crate::facade;
use crate::fastforwarder;
use crate::game;
use crate::hooks;
use crate::input;
use crate::protocol;
use crate::replay;
use crate::transport;

#[derive(Clone, Debug)]
pub struct Settings {
    pub ice_servers: Vec<webrtc::ice_transport::ice_server::RTCIceServer>,
    pub matchmaking_connect_addr: String,
    pub session_id: String,
    pub replays_path: std::path::PathBuf,
    pub replay_metadata: Vec<u8>,
    pub match_type: u16,
    pub input_delay: u32,
}

pub struct BattleState {
    pub number: u8,
    pub battle: Option<Battle>,
    pub won_last_battle: bool,
}

impl BattleState {
    pub async fn end_battle(&mut self) -> anyhow::Result<()> {
        log::info!("battle ended");
        self.battle = None;
        Ok(())
    }
}

pub struct Match {
    audio_supported_config: cpal::SupportedStreamConfig,
    rom_path: std::path::PathBuf,
    hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
    dc: std::sync::Arc<datachannel::DataChannel>,
    rng: tokio::sync::Mutex<rand_pcg::Mcg128Xsl64>,
    settings: Settings,
    battle_state: tokio::sync::Mutex<BattleState>,
    remote_init_sender: tokio::sync::mpsc::Sender<protocol::Init>,
    remote_init_receiver: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<protocol::Init>>,
    remote_state_chunk_sender: tokio::sync::mpsc::Sender<Vec<u8>>,
    remote_state_chunk_receiver: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>,
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

impl From<webrtc::Error> for NegotiationError {
    fn from(err: webrtc::Error) -> Self {
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
        dc: std::sync::Arc<datachannel::DataChannel>,
        mut rng: rand_pcg::Mcg128Xsl64,
        side: tango_matchmaking::client::ConnectionSide,
        primary_thread_handle: mgba::thread::Handle,
        settings: Settings,
    ) -> Self {
        let (remote_init_sender, remote_init_receiver) = tokio::sync::mpsc::channel(1);
        let (remote_state_chunk_sender, remote_state_chunk_receiver) =
            tokio::sync::mpsc::channel(1);
        let did_polite_win_last_battle = rng.gen::<bool>();
        Self {
            audio_supported_config,
            rom_path,
            hooks,
            dc,
            rng: tokio::sync::Mutex::new(rng),
            settings,
            battle_state: tokio::sync::Mutex::new(BattleState {
                number: 0,
                battle: None,
                won_last_battle: did_polite_win_last_battle
                    == (side == tango_matchmaking::client::ConnectionSide::Polite),
            }),
            remote_init_sender,
            remote_init_receiver: tokio::sync::Mutex::new(remote_init_receiver),
            remote_state_chunk_sender,
            remote_state_chunk_receiver: tokio::sync::Mutex::new(remote_state_chunk_receiver),
            audio_mux,
            primary_thread_handle,
        }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        loop {
            match protocol::Packet::deserialize(
                match self.dc.receive().await {
                    None => break,
                    Some(buf) => buf,
                }
                .as_slice(),
            )? {
                protocol::Packet::Init(init) => {
                    self.remote_init_sender.send(init).await?;
                }
                protocol::Packet::StateChunk(state_chunk) => {
                    self.remote_state_chunk_sender
                        .send(state_chunk.chunk)
                        .await?;
                }
                protocol::Packet::Input(input) => {
                    let state_committed_rx = {
                        let mut battle_state = self.battle_state.lock().await;

                        if input.battle_number != battle_state.number {
                            log::info!("battle number mismatch, dropping input");
                            continue;
                        }

                        let battle = match &mut battle_state.battle {
                            None => {
                                log::info!("no battle in progress, dropping input");
                                continue;
                            }
                            Some(b) => b,
                        };
                        battle.state_committed_rx.take()
                    };

                    if let Some(state_committed_rx) = state_committed_rx {
                        state_committed_rx.await.unwrap();
                    }

                    let mut battle_state = self.battle_state.lock().await;

                    let battle = match &mut battle_state.battle {
                        None => {
                            log::info!("no battle in progress, dropping input");
                            continue;
                        }
                        Some(b) => b,
                    };

                    if !battle.can_add_remote_input() {
                        anyhow::bail!("remote overflowed our input buffer");
                    }

                    battle.add_remote_input(input::Input {
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

    pub async fn lock_battle_state(&self) -> tokio::sync::MutexGuard<'_, BattleState> {
        self.battle_state.lock().await
    }

    pub async fn receive_remote_init(&self) -> Option<protocol::Init> {
        let mut remote_init_receiver = self.remote_init_receiver.lock().await;
        remote_init_receiver.recv().await
    }

    pub async fn receive_remote_state_chunk(&self) -> Option<Vec<u8>> {
        let mut remote_state_chunk_receiver = self.remote_state_chunk_receiver.lock().await;
        remote_state_chunk_receiver.recv().await
    }

    pub fn transport(&self) -> anyhow::Result<transport::Transport> {
        Ok(transport::Transport::new(self.dc.clone()))
    }

    pub async fn lock_rng(&self) -> tokio::sync::MutexGuard<'_, rand_pcg::Mcg128Xsl64> {
        self.rng.lock().await
    }

    pub fn match_type(&self) -> u16 {
        self.settings.match_type
    }

    pub async fn start_battle(&self, core: mgba::core::CoreMutRef<'_>) -> anyhow::Result<()> {
        let mut battle_state = self.battle_state.lock().await;
        battle_state.number += 1;
        let local_player_index = if battle_state.won_last_battle { 0 } else { 1 };
        log::info!(
            "starting battle: local_player_index = {}",
            local_player_index
        );
        let mut replay_filename = self.settings.replays_path.clone();
        replay_filename.push(format!("battle{}.tangoreplay", battle_state.number));
        let replay_filename = std::path::Path::new(&replay_filename);
        let replay_file = std::fs::File::create(&replay_filename)?;
        log::info!("opened replay: {}", replay_filename.display());

        let mut audio_core = mgba::core::Core::new_gba("tango")?;
        let audio_save_state_holder = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let (audio_rendezvous_tx, audio_rendezvous_rx) = std::sync::mpsc::sync_channel(0);
        let rom_vf = mgba::vfile::VFile::open(&self.rom_path, mgba::vfile::flags::O_RDONLY)?;
        audio_core.as_mut().load_rom(rom_vf)?;
        audio_core.as_mut().reset();
        audio_core.set_traps(self.hooks.audio_traps(facade::AudioFacade::new(
            audio_save_state_holder.clone(),
            std::sync::Arc::new(audio_rendezvous_rx),
            local_player_index,
        )));

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
                audio_core_mux_handle.switch();
            });
        }
        audio_core_handle.unpause();

        let (state_committed_tx, state_committed_rx) = tokio::sync::oneshot::channel();
        battle_state.battle = Some(Battle {
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
            audio_rendezvous_tx,
            committed_state: None,
            local_pending_turn: None,
            replay_writer: replay::Writer::new(
                Box::new(replay_file),
                &self.settings.replay_metadata,
                local_player_index,
            )?,
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
        log::info!("battle has started");
        Ok(())
    }
}

struct LocalPendingTurn {
    marshaled: Vec<u8>,
    ticks_left: u8,
}

pub struct Battle {
    local_player_index: u8,
    iq: input::PairQueue<input::Input>,
    remote_delay: u32,
    is_accepting_input: bool,
    last_committed_remote_input: input::Input,
    last_input: Option<input::Pair<input::Input>>,
    state_committed_tx: Option<tokio::sync::oneshot::Sender<()>>,
    state_committed_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    audio_rendezvous_tx: std::sync::mpsc::SyncSender<()>,
    committed_state: Option<mgba::state::State>,
    local_pending_turn: Option<LocalPendingTurn>,
    replay_writer: replay::Writer,
    fastforwarder: fastforwarder::Fastforwarder,
    audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
    primary_thread_handle: mgba::thread::Handle,
    _audio_core_thread: mgba::thread::Thread,
    _audio_core_mux_handle: audio::mux_stream::MuxHandle,
}

impl Battle {
    pub fn fastforwarder(&mut self) -> &mut fastforwarder::Fastforwarder {
        &mut self.fastforwarder
    }

    pub fn replay_writer(&mut self) -> &mut replay::Writer {
        &mut self.replay_writer
    }

    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    pub fn remote_player_index(&self) -> u8 {
        1 - self.local_player_index
    }

    pub fn set_committed_state(&mut self, state: mgba::state::State) {
        self.committed_state = Some(state);
        if let Some(tx) = self.state_committed_tx.take() {
            let _ = tx.send(());
        }
    }

    pub fn set_audio_save_state(&mut self, state: mgba::state::State) {
        *self.audio_save_state_holder.lock() = Some(state);
    }

    pub fn wait_for_audio_rendezvous(&mut self) -> anyhow::Result<()> {
        self.audio_rendezvous_tx.send(())?;
        Ok(())
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
        let local_tps_adjustment = last_local_input.lag() - self.local_delay() as i32;
        let remote_tps_adjustment =
            self.last_committed_remote_input.lag() - self.remote_delay() as i32;
        local_tps_adjustment - remote_tps_adjustment
    }
}

impl Drop for Battle {
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

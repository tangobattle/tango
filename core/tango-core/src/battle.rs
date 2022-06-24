use rand::Rng;

use crate::game;
use crate::hooks;
use crate::input;
use crate::protocol;
use crate::replay;
use crate::replayer;
use crate::shadow;
use crate::transport;

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum BattleResult {
    Loss,
    Win,
}

#[derive(Clone)]
pub struct CommittedState {
    state: mgba::state::State,
    tick: u32,
}

pub struct MatchInit {
    pub dc: datachannel_wrapper::DataChannel,
    pub peer_conn: datachannel_wrapper::PeerConnection,
    pub settings: Settings,
}

pub struct Settings {
    pub replays_path: std::path::PathBuf,
    pub shadow_save_path: std::path::PathBuf,
    pub shadow_rom_path: std::path::PathBuf,
    pub replay_metadata: Vec<u8>,
    pub match_type: u8,
    pub input_delay: u32,
    pub shadow_input_delay: u32,
    pub rng_seed: Vec<u8>,
    pub opponent_nickname: Option<String>,
}

pub struct RoundState {
    pub number: u8,
    pub round: Option<Round>,
    pub last_result: Option<BattleResult>,
}

impl RoundState {
    pub async fn end_round(&mut self) -> anyhow::Result<()> {
        log::info!("round ended");

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
        Ok(())
    }

    pub fn set_last_result(&mut self, last_result: BattleResult) {
        self.last_result = Some(last_result);
    }
}

pub struct Match {
    shadow: std::sync::Arc<tokio::sync::Mutex<shadow::Shadow>>,
    rom: Vec<u8>,
    hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
    _peer_conn: datachannel_wrapper::PeerConnection,
    transport: std::sync::Arc<tokio::sync::Mutex<transport::Transport>>,
    rng: tokio::sync::Mutex<rand_pcg::Mcg128Xsl64>,
    settings: Settings,
    is_offerer: bool,
    round_state: tokio::sync::Mutex<RoundState>,
    primary_thread_handle: mgba::thread::Handle,
    round_started_tx: tokio::sync::mpsc::Sender<u8>,
    round_started_rx: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<u8>>,
    transport_rendezvous_tx: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
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

const MAX_QUEUE_LENGTH: usize = 1200;

impl Match {
    pub fn new(
        rom: Vec<u8>,
        hooks: &'static Box<dyn hooks::Hooks + Send + Sync>,
        peer_conn: datachannel_wrapper::PeerConnection,
        dc_tx: datachannel_wrapper::DataChannelSender,
        mut rng: rand_pcg::Mcg128Xsl64,
        is_offerer: bool,
        primary_thread_handle: mgba::thread::Handle,
        settings: Settings,
    ) -> anyhow::Result<std::sync::Arc<Self>> {
        let shadow_rom = std::fs::read(&settings.shadow_rom_path)?;

        let (round_started_tx, round_started_rx) = tokio::sync::mpsc::channel(1);
        let (transport_rendezvous_tx, transport_rendezvous_rx) = tokio::sync::oneshot::channel();
        let did_polite_win_last_round = rng.gen::<bool>();
        let last_result = if did_polite_win_last_round == is_offerer {
            BattleResult::Win
        } else {
            BattleResult::Loss
        };
        let match_ = std::sync::Arc::new(Self {
            shadow: std::sync::Arc::new(tokio::sync::Mutex::new(shadow::Shadow::new(
                &shadow_rom,
                &settings.shadow_save_path,
                settings.match_type,
                is_offerer,
                last_result,
                rng.clone(),
            )?)),
            rom,
            hooks,
            _peer_conn: peer_conn,
            transport: std::sync::Arc::new(tokio::sync::Mutex::new(transport::Transport::new(
                dc_tx,
                transport_rendezvous_rx,
            ))),
            transport_rendezvous_tx: tokio::sync::Mutex::new(Some(transport_rendezvous_tx)),
            rng: tokio::sync::Mutex::new(rng),
            settings,
            round_state: tokio::sync::Mutex::new(RoundState {
                number: 0,
                round: None,
                last_result: Some(last_result),
            }),
            is_offerer,
            primary_thread_handle,
            round_started_tx,
            round_started_rx: tokio::sync::Mutex::new(round_started_rx),
        });
        Ok(match_)
    }

    pub async fn advance_shadow_until_round_end(&self) -> anyhow::Result<()> {
        self.shadow.lock().await.advance_until_round_end()
    }

    pub async fn advance_shadow_until_first_committed_state(
        &self,
    ) -> anyhow::Result<mgba::state::State> {
        self.shadow
            .lock()
            .await
            .advance_until_first_committed_state()
    }

    pub async fn run(
        &self,
        mut dc_rx: datachannel_wrapper::DataChannelReceiver,
    ) -> anyhow::Result<()> {
        let mut last_round_number = 0;
        loop {
            match protocol::Packet::deserialize(
                match dc_rx.receive().await {
                    None => {
                        log::info!("data channel closed");
                        break;
                    }
                    Some(buf) => buf,
                }
                .as_slice(),
            )? {
                protocol::Packet::Input(input) => {
                    // We need to sync on the first input so we don't end up wildly out of sync.
                    if let Some(transport_rendezvous_tx) =
                        self.transport_rendezvous_tx.lock().await.take()
                    {
                        transport_rendezvous_tx.send(()).unwrap();
                    }

                    // We need to wait for the next round to start to avoid dropping inputs on the floor.
                    if input.round_number != last_round_number {
                        let round_number =
                            if let Some(number) = self.round_started_rx.lock().await.recv().await {
                                number
                            } else {
                                break;
                            };
                        assert!(round_number == input.round_number);
                        last_round_number = input.round_number;
                    }

                    // We need to wait for the first state to be committed before we can add remote input.
                    //
                    // This is because we don't know what tick to add the input at, and the input queue has not been filled up with delay frames yet.
                    let first_state_committed_rx = {
                        let mut round_state = self.round_state.lock().await;

                        if input.round_number != round_state.number {
                            log::error!(
                                "round number mismatch, dropping input: this is probably bad!"
                            );
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
                    if input.round_number != round_state.number {
                        log::error!(
                            "round number mismatch, dropping input: this is probably not what you"
                        );
                        continue;
                    }

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
                        remote_tick: (input.local_tick as i64 + input.tick_diff as i64) as u32,
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

    pub fn match_type(&self) -> u8 {
        self.settings.match_type
    }

    pub fn is_offerer(&self) -> bool {
        self.is_offerer
    }

    pub async fn start_round(self: &std::sync::Arc<Self>) -> anyhow::Result<()> {
        let mut round_state = self.round_state.lock().await;
        round_state.number += 1;
        let local_player_index = match round_state.last_result.take().unwrap() {
            BattleResult::Win => 0,
            BattleResult::Loss => 1,
        };
        log::info!(
            "starting round: local_player_index = {}",
            local_player_index
        );
        let mut replay_filename = self.settings.replays_path.clone().as_os_str().to_owned();
        replay_filename.push(format!(
            "-round{}-p{}.tangoreplay",
            round_state.number,
            local_player_index + 1
        ));
        let replay_filename = std::path::Path::new(&replay_filename);
        let replay_file = std::fs::File::create(&replay_filename)?;
        log::info!("opened replay: {}", replay_filename.display());

        log::info!("preparing round state");

        let (first_state_committed_tx, first_state_committed_rx) = tokio::sync::oneshot::channel();

        let mut tx_queue = std::collections::VecDeque::with_capacity(MAX_QUEUE_LENGTH);
        tx_queue.push_front(Tx {
            for_tick: 0,
            tx: self.hooks.placeholder_rx(),
        });

        let mut iq = input::PairQueue::new(MAX_QUEUE_LENGTH, self.settings.input_delay);
        log::info!(
            "filling input delay: local = {}, remote = {}",
            self.settings.input_delay,
            self.settings.shadow_input_delay
        );
        for i in 0..self.settings.input_delay {
            iq.add_local_input(input::PartialInput {
                local_tick: i,
                remote_tick: 0,
                joyflags: 0,
            });
        }
        for i in 0..self.settings.shadow_input_delay {
            iq.add_remote_input(input::PartialInput {
                local_tick: i,
                remote_tick: 0,
                joyflags: 0,
            });
        }

        round_state.round = Some(Round {
            number: round_state.number,
            local_player_index,
            current_tick: 0,
            iq,
            tx_queue,
            remote_delay: self.settings.shadow_input_delay,
            last_committed_remote_input: input::Input {
                local_tick: 0,
                remote_tick: 0,
                joyflags: 0,
                rx: self.hooks.placeholder_rx(),
                is_prediction: false,
            },
            last_input: None,
            first_state_committed_tx: Some(first_state_committed_tx),
            first_state_committed_rx: Some(first_state_committed_rx),
            committed_state: None,
            replay_writer: Some(replay::Writer::new(
                Box::new(replay_file),
                &self.settings.replay_metadata,
                local_player_index,
                self.hooks.placeholder_rx().len() as u8,
            )?),
            replayer: replayer::Fastforwarder::new(
                &self.rom,
                self.hooks,
                local_player_index,
                &self.settings.opponent_nickname,
            )?,
            primary_thread_handle: self.primary_thread_handle.clone(),
            transport: self.transport.clone(),
            shadow: self.shadow.clone(),
        });
        self.round_started_tx.send(round_state.number).await?;
        log::info!("round has started");
        Ok(())
    }
}

#[derive(Clone)]
struct Tx {
    for_tick: u32,
    tx: Vec<u8>,
}

pub struct Round {
    number: u8,
    local_player_index: u8,
    current_tick: u32,
    iq: input::PairQueue<input::PartialInput, input::PartialInput>,
    tx_queue: std::collections::VecDeque<Tx>,
    remote_delay: u32,
    last_committed_remote_input: input::Input,
    last_input: Option<input::Pair<input::Input, input::Input>>,
    first_state_committed_tx: Option<tokio::sync::oneshot::Sender<()>>,
    first_state_committed_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    committed_state: Option<CommittedState>,
    replay_writer: Option<replay::Writer>,
    replayer: replayer::Fastforwarder,
    primary_thread_handle: mgba::thread::Handle,
    transport: std::sync::Arc<tokio::sync::Mutex<transport::Transport>>,
    shadow: std::sync::Arc<tokio::sync::Mutex<shadow::Shadow>>,
}

impl Round {
    pub fn on_draw_result(&self) -> BattleResult {
        match self.local_player_index {
            0 => BattleResult::Win,
            1 => BattleResult::Loss,
            _ => unreachable!(),
        }
    }

    pub fn current_tick(&self) -> u32 {
        self.current_tick
    }

    pub fn increment_current_tick(&mut self) {
        self.current_tick += 1;
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
        self.committed_state = Some(CommittedState { state, tick: 0 });
        if let Some(tx) = self.first_state_committed_tx.take() {
            let _ = tx.send(());
        }
    }

    pub async fn add_local_input_and_fastforward(
        &mut self,
        mut core: mgba::core::CoreMutRef<'_>,
        joyflags: u16,
    ) -> bool {
        let local_tick = self.current_tick + self.local_delay();
        let remote_tick = self.last_committed_remote_input().local_tick;

        // We do it in this order such that:
        // 1. We make sure that the input buffer does not overflow if we were to add an input.
        // 2. We try to send it to the peer: if it fails, we don't end up desyncing the opponent as we haven't added the input ourselves yet.
        // 3. We add the input to our buffer: no overflow is guaranteed because we already checked ahead of time.
        //
        // This is all done while the self is locked, so there are no TOCTTOU issues.
        if !self.can_add_local_input() {
            log::error!("local input buffer overflow!");
            return false;
        }

        if let Err(e) = self
            .transport
            .lock()
            .await
            .send_input(
                self.number,
                local_tick,
                (remote_tick as i32 - local_tick as i32) as i8,
                joyflags,
            )
            .await
        {
            log::error!("failed to send input: {}", e);
            return false;
        }

        self.add_local_input(input::PartialInput {
            local_tick,
            remote_tick,
            joyflags,
        });

        let (input_pairs, left) = match self.consume_and_peek_local().await {
            Ok(r) => r,
            Err(e) => {
                log::error!("failed to consume input: {}", e);
                return false;
            }
        };

        let last_committed_state = self.committed_state.take().expect("committed state");
        let last_committed_remote_input = self.last_committed_remote_input();

        let (committed_state, dirty_state, last_input) = match self.replayer.fastforward(
            &last_committed_state.state,
            last_committed_state.tick,
            &input_pairs,
            last_committed_remote_input,
            &left,
        ) {
            Ok(t) => t,
            Err(e) => {
                log::error!("replayer failed with error: {}", e);
                return false;
            }
        };

        core.load_state(&dirty_state).expect("load dirty state");

        self.committed_state = Some(CommittedState {
            state: committed_state,
            tick: last_committed_state.tick + input_pairs.len() as u32,
        });
        self.last_input = Some(last_input);

        core.gba_mut()
            .sync_mut()
            .expect("set fps target")
            .set_fps_target(game::EXPECTED_FPS as f32 + self.tps_adjustment());

        true
    }

    pub fn has_committed_state(&mut self) -> bool {
        self.committed_state.is_some()
    }

    pub fn peek_last_input(&self) -> &Option<input::Pair<input::Input, input::Input>> {
        &self.last_input
    }

    pub fn local_delay(&self) -> u32 {
        self.iq.local_delay()
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
    pub fn last_committed_remote_input(&self) -> input::Input {
        self.last_committed_remote_input.clone()
    }

    pub fn queue_tx(&mut self, for_tick: u32, tx: Vec<u8>) {
        self.tx_queue.push_back(Tx { for_tick, tx });
    }

    pub async fn consume_and_peek_local(
        &mut self,
    ) -> anyhow::Result<(
        Vec<input::Pair<input::Input, input::Input>>,
        Vec<input::Input>,
    )> {
        let (partial_input_pairs, left) = self.iq.consume_and_peek_local();

        let partial_input_pairs_len = partial_input_pairs.len();
        assert!(
            self.tx_queue.len() >= partial_input_pairs_len + left.len(),
            "not enough tx queue inputs"
        );

        let input_pairs = partial_input_pairs
            .into_iter()
            .zip(self.tx_queue.drain(..partial_input_pairs_len))
            .map(|(pair, tx)| {
                assert!(
                    tx.for_tick == pair.local.local_tick,
                    "tx input did not match current tick: {} != {}",
                    tx.for_tick,
                    pair.local.local_tick
                );
                input::Pair {
                    local: input::Input {
                        local_tick: pair.local.local_tick,
                        remote_tick: pair.local.remote_tick,
                        joyflags: pair.local.joyflags,
                        rx: tx.tx,
                        is_prediction: false,
                    },
                    remote: pair.remote,
                }
            })
            .collect::<Vec<_>>();

        let left = left
            .into_iter()
            .zip(self.tx_queue.iter().cloned())
            .map(|(pi, tx)| {
                assert!(
                    tx.for_tick == pi.local_tick,
                    "tx input did not match current tick: {} != {}",
                    tx.for_tick,
                    pi.local_tick
                );
                input::Input {
                    local_tick: pi.local_tick,
                    remote_tick: pi.remote_tick,
                    joyflags: pi.joyflags,
                    rx: tx.tx,
                    is_prediction: true,
                }
            })
            .collect::<Vec<_>>();

        let mut shadow = self.shadow.lock().await;
        let input_pairs = input_pairs
            .into_iter()
            .map(|pair| shadow.apply_input(pair))
            .collect::<Result<Vec<_>, _>>()?;

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

        Ok((input_pairs, left))
    }

    pub fn can_add_local_input(&mut self) -> bool {
        self.iq.local_queue_length() < MAX_QUEUE_LENGTH
    }

    pub fn add_local_input(&mut self, input: input::PartialInput) {
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

    pub fn tps_adjustment(&self) -> f32 {
        let last_local_input = match &self.last_input {
            Some(input::Pair { local, .. }) => local,
            None => {
                return 0.0;
            }
        };
        let dtick = last_local_input.lag() - self.last_committed_remote_input.lag();
        let ddelay = self.local_delay() as i32 - self.remote_delay() as i32;
        ((dtick + ddelay) * game::EXPECTED_FPS as i32) as f32 / MAX_QUEUE_LENGTH as f32
    }
}

impl Drop for Round {
    fn drop(&mut self) {
        // HACK: This is the only safe way to set the FPS without clogging everything else up.
        self.primary_thread_handle
            .lock_audio()
            .sync_mut()
            .set_fps_target(game::EXPECTED_FPS as f32);
    }
}

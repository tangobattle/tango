use rand::Rng;

use crate::games;
use crate::lockstep;
use crate::net;
use crate::replay;
use crate::replayer;
use crate::session;
use crate::shadow;

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum BattleResult {
    Loss,
    Win,
}

#[derive(Clone)]
pub struct CommittedState {
    pub state: mgba::state::State,
    pub tick: u32,
    pub packet: Vec<u8>,
}

pub struct RoundState {
    pub number: u8,
    pub round: Option<Round>,
    pub last_result: Option<BattleResult>,
}

impl RoundState {
    pub async fn end_round(&mut self) -> anyhow::Result<()> {
        match self.round.take() {
            Some(round) => {
                log::info!("round ended at {:x}", round.current_tick);
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
    shadow: std::sync::Arc<parking_lot::Mutex<shadow::Shadow>>,
    rom: Vec<u8>,
    link_code: String,
    local_game: &'static (dyn games::Game + Send + Sync),
    local_settings: net::protocol::Settings,
    remote_game: &'static (dyn games::Game + Send + Sync),
    remote_settings: net::protocol::Settings,
    sender: std::sync::Arc<tokio::sync::Mutex<net::Sender>>,
    _peer_conn: datachannel_wrapper::PeerConnection,
    rng: tokio::sync::Mutex<rand_pcg::Mcg128Xsl64>,
    cancellation_token: tokio_util::sync::CancellationToken,
    replays_path: std::path::PathBuf,
    match_type: (u8, u8),
    input_delay: u32,
    max_queue_length: usize,
    is_offerer: bool,
    round_state: tokio::sync::Mutex<RoundState>,
    primary_thread_handle: mgba::thread::Handle,
    round_started_tx: tokio::sync::mpsc::Sender<u8>,
    round_started_rx: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<u8>>,
}

impl Match {
    pub fn new(
        link_code: String,
        rom: Vec<u8>,
        local_game: &'static (dyn games::Game + Send + Sync),
        local_settings: net::protocol::Settings,
        remote_game: &'static (dyn games::Game + Send + Sync),
        remote_settings: net::protocol::Settings,
        sender: net::Sender,
        peer_conn: datachannel_wrapper::PeerConnection,
        mut rng: rand_pcg::Mcg128Xsl64,
        is_offerer: bool,
        primary_thread_handle: mgba::thread::Handle,
        remote_rom: &[u8],
        remote_save: &[u8],
        replays_path: std::path::PathBuf,
        match_type: (u8, u8),
        input_delay: u32,
        max_queue_length: usize,
    ) -> anyhow::Result<std::sync::Arc<Self>> {
        let (round_started_tx, round_started_rx) = tokio::sync::mpsc::channel(1);
        let did_polite_win_last_round = rng.gen::<bool>();
        let last_result = if did_polite_win_last_round == is_offerer {
            BattleResult::Win
        } else {
            BattleResult::Loss
        };
        let match_ = std::sync::Arc::new(Self {
            shadow: std::sync::Arc::new(parking_lot::Mutex::new(shadow::Shadow::new(
                &remote_rom,
                &remote_save,
                match_type,
                is_offerer,
                last_result,
                rng.clone(),
            )?)),
            link_code,
            local_game,
            local_settings,
            remote_game,
            remote_settings,
            rom,
            sender: std::sync::Arc::new(tokio::sync::Mutex::new(sender)),
            _peer_conn: peer_conn,
            rng: tokio::sync::Mutex::new(rng),
            cancellation_token: tokio_util::sync::CancellationToken::new(),
            replays_path,
            match_type,
            input_delay,
            max_queue_length,
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

    pub fn cancel(&self) {
        self.cancellation_token.cancel()
    }

    pub fn cancelled(&self) -> tokio_util::sync::WaitForCancellationFuture {
        self.cancellation_token.cancelled()
    }

    pub async fn advance_shadow_until_round_end(&self) -> anyhow::Result<()> {
        self.shadow.lock().advance_until_round_end()
    }

    pub async fn advance_shadow_until_first_committed_state(
        &self,
    ) -> anyhow::Result<mgba::state::State> {
        self.shadow.lock().advance_until_first_committed_state()
    }

    pub async fn run(&self, mut receiver: net::Receiver) -> anyhow::Result<()> {
        let mut last_round_number = 0;
        loop {
            match receiver.receive().await? {
                net::protocol::Packet::Ping(ping) => {
                    self.sender.lock().await.send_pong(ping.ts).await?;
                }
                net::protocol::Packet::Pong(_pong) => {
                    // TODO
                }
                net::protocol::Packet::Input(input) => {
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
                        log::error!("round number mismatch, dropping input: this is probably bad!");
                        continue;
                    }

                    let round = match &mut round_state.round {
                        None => {
                            log::info!("no round in progress, dropping input");
                            continue;
                        }
                        Some(b) => b,
                    };

                    if !round.iq.can_add_remote_input() {
                        anyhow::bail!("remote overflowed our input buffer");
                    }

                    round.add_remote_input(lockstep::PartialInput {
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

    pub fn match_type(&self) -> (u8, u8) {
        self.match_type
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
        let replay_filename = self.replays_path.join(
            format!(
                "{}-{}-{}-vs-{}-round{}-p{}.tangoreplay",
                time::OffsetDateTime::from(std::time::SystemTime::now())
                    .format(time::macros::format_description!(
                        "[year padding:zero][month padding:zero repr:numerical][day padding:zero][hour padding:zero][minute padding:zero][second padding:zero]"
                    ))
                    .expect("format time"),
                self.link_code,
                self.local_game.family_and_variant().0,
                self.remote_settings.nickname,
                round_state.number,
                local_player_index + 1
            )
            .chars()
            .filter(|c| "/\\?%*:|\"<>. ".chars().any(|c2| c2 != *c))
            .collect::<String>(),
        );
        log::info!("open replay: {}", replay_filename.display());

        let replay_file = std::fs::File::create(&replay_filename)?;

        log::info!("preparing round state");

        let (first_state_committed_local_packet, first_state_committed_rx) =
            tokio::sync::oneshot::channel();

        let mut iq = lockstep::PairQueue::new(self.max_queue_length, self.input_delay);
        log::info!("filling {} ticks of input delay", self.input_delay,);

        {
            let mut sender = self.sender.lock().await;
            for i in 0..self.input_delay {
                iq.add_local_input(lockstep::PartialInput {
                    local_tick: i,
                    remote_tick: 0,
                    joyflags: 0,
                });
                sender.send_input(round_state.number, i, 0, 0).await?;
            }
        }

        let hooks = self.local_game.hooks();
        let local_family_and_variant = self.local_game.family_and_variant();
        let remote_family_and_variant = self.remote_game.family_and_variant();
        round_state.round = Some(Round {
            hooks,
            number: round_state.number,
            local_player_index,
            current_tick: 0,
            dtick: 0,
            iq,
            last_committed_remote_input: lockstep::Input {
                local_tick: 0,
                remote_tick: 0,
                joyflags: 0,
                packet: vec![0u8; hooks.packet_size()],
            },
            first_state_committed_local_packet: Some(first_state_committed_local_packet),
            first_state_committed_rx: Some(first_state_committed_rx),
            committed_state: None,
            replay_writer: Some(replay::Writer::new(
                Box::new(replay_file),
                tango_protos::replay::ReplayMetadata {
                    ts: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                    link_code: self.link_code.clone(),
                    local_side: Some(tango_protos::replay::replay_metadata::Side {
                        nickname: self.local_settings.nickname.clone(),
                        game_info: Some(tango_protos::replay::replay_metadata::GameInfo {
                            rom_family: local_family_and_variant.0.to_string(),
                            rom_variant: local_family_and_variant.1 as u32,
                            patch: None, // TODO
                        }),
                        reveal_setup: self.local_settings.reveal_setup,
                    }),
                    remote_side: Some(tango_protos::replay::replay_metadata::Side {
                        nickname: self.remote_settings.nickname.clone(),
                        game_info: Some(tango_protos::replay::replay_metadata::GameInfo {
                            rom_family: remote_family_and_variant.0.to_string(),
                            rom_variant: remote_family_and_variant.1 as u32,
                            patch: None, // TODO
                        }),
                        reveal_setup: self.remote_settings.reveal_setup,
                    }),
                },
                local_player_index,
                hooks.packet_size() as u8,
            )?),
            replayer: replayer::Fastforwarder::new(&self.rom, hooks, local_player_index)?,
            primary_thread_handle: self.primary_thread_handle.clone(),
            sender: self.sender.clone(),
            shadow: self.shadow.clone(),
        });
        self.round_started_tx.send(round_state.number).await?;
        log::info!("round has started");
        Ok(())
    }
}

pub struct Round {
    hooks: &'static (dyn games::Hooks + Send + Sync),
    number: u8,
    local_player_index: u8,
    current_tick: u32,
    dtick: i32,
    iq: lockstep::PairQueue<lockstep::PartialInput, lockstep::PartialInput>,
    last_committed_remote_input: lockstep::Input,
    first_state_committed_local_packet: Option<tokio::sync::oneshot::Sender<()>>,
    first_state_committed_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    committed_state: Option<CommittedState>,
    replay_writer: Option<replay::Writer>,
    replayer: replayer::Fastforwarder,
    primary_thread_handle: mgba::thread::Handle,
    sender: std::sync::Arc<tokio::sync::Mutex<net::Sender>>,
    shadow: std::sync::Arc<parking_lot::Mutex<shadow::Shadow>>,
}

impl Round {
    pub fn current_tick(&self) -> u32 {
        self.current_tick
    }

    pub fn increment_current_tick(&mut self) {
        self.current_tick += 1;
    }

    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    #[allow(dead_code)] // TODO
    pub fn remote_player_index(&self) -> u8 {
        1 - self.local_player_index
    }

    pub fn set_first_committed_state(
        &mut self,
        state: mgba::state::State,
        remote_state: mgba::state::State,
        first_packet: &[u8],
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
        self.committed_state = Some(CommittedState {
            state,
            tick: 0,
            packet: first_packet.to_vec(),
        });
        if let Some(tx) = self.first_state_committed_local_packet.take() {
            let _ = tx.send(());
        }
    }

    pub async fn add_local_input_and_fastforward(
        &mut self,
        mut core: mgba::core::CoreMutRef<'_>,
        joyflags: u16,
    ) -> anyhow::Result<Option<BattleResult>> {
        let local_tick = self.current_tick + self.local_delay();
        let remote_tick = self.last_committed_remote_input.local_tick;

        // We do it in this order such that:
        // 1. We make sure that the input buffer does not overflow if we were to add an input.
        // 2. We try to send it to the peer: if it fails, we don't end up desyncing the opponent as we haven't added the input ourselves yet.
        // 3. We add the input to our buffer: no overflow is guaranteed because we already checked ahead of time.
        //
        // This is all done while the self is locked, so there are no TOCTTOU issues.
        if !self.iq.can_add_local_input() {
            anyhow::bail!("local input buffer overflow!");
        }

        self.sender
            .lock()
            .await
            .send_input(
                self.number,
                local_tick,
                (remote_tick as i32 - local_tick as i32) as i8,
                joyflags,
            )
            .await?;

        self.add_local_input(lockstep::PartialInput {
            local_tick,
            remote_tick,
            joyflags,
        });

        let (committable, predict_required) = self.iq.consume_and_peek_local();

        let last_committed_state = self.committed_state.take().expect("committed state");

        let commit_tick = last_committed_state.tick + committable.len() as u32;
        let dirty_tick = commit_tick + predict_required.len() as u32 - 1;

        let input_pairs = committable
            .into_iter()
            .chain(predict_required.into_iter().map(|local| {
                let local_tick = local.local_tick;
                let remote_tick = local.remote_tick;
                lockstep::Pair {
                    local,
                    remote: lockstep::PartialInput {
                        local_tick,
                        remote_tick,
                        joyflags: {
                            let mut joyflags = 0;
                            if self.last_committed_remote_input.joyflags
                                & mgba::input::keys::A as u16
                                != 0
                            {
                                joyflags |= mgba::input::keys::A as u16;
                            }
                            if self.last_committed_remote_input.joyflags
                                & mgba::input::keys::B as u16
                                != 0
                            {
                                joyflags |= mgba::input::keys::B as u16;
                            }
                            joyflags
                        },
                    },
                }
            }))
            .collect::<Vec<lockstep::Pair<lockstep::PartialInput, lockstep::PartialInput>>>();
        let last_local_input = input_pairs.last().unwrap().local.clone();

        let ff_result = self.replayer.fastforward(
            &last_committed_state.state,
            input_pairs,
            last_committed_state.tick,
            commit_tick,
            dirty_tick,
            &last_committed_state.packet,
            Box::new({
                let shadow = self.shadow.clone();
                let hooks = self.hooks;
                let mut last_commit = self.last_committed_remote_input.packet.clone();
                move |ip| {
                    let local_tick = ip.local.local_tick;
                    Ok(if ip.local.local_tick < commit_tick {
                        let r = shadow.lock().apply_input(ip)?;
                        assert!(
                            r.tick == local_tick,
                            "shadow input did not match current tick: {} != {}",
                            r.tick,
                            local_tick
                        );
                        last_commit = r.packet.clone();
                        r.packet
                    } else {
                        hooks.predict_rx(&mut last_commit);
                        last_commit.clone()
                    })
                }
            }),
        )?;

        for ip in &ff_result.output_pairs {
            if ip.local.local_tick >= commit_tick {
                break;
            }

            if ff_result
                .round_result
                .map(|rr| ip.local.local_tick < rr.tick)
                .unwrap_or(true)
            {
                if let Some(replay_writer) = self.replay_writer.as_mut() {
                    replay_writer
                        .write_input(self.local_player_index, ip)
                        .expect("write input");
                }
            }
            self.last_committed_remote_input = ip.remote.clone();
        }

        core.load_state(&ff_result.dirty_state.state)
            .expect("load dirty state");
        self.committed_state = Some(ff_result.committed_state);

        self.dtick = last_local_input.lag() - self.last_committed_remote_input.lag();

        core.gba_mut()
            .sync_mut()
            .expect("set fps target")
            .set_fps_target(session::EXPECTED_FPS as f32 + self.tps_adjustment());

        let round_result = if let Some(round_result) = ff_result.round_result {
            round_result
        } else {
            return Ok(None);
        };

        if round_result.tick >= commit_tick {
            return Ok(None);
        }

        if let Some(replay_writer) = self.replay_writer.take() {
            replay_writer.finish().expect("finish");
            log::info!(
                "replay finished at {:x} (real tick {:x})",
                round_result.tick,
                self.current_tick
            );
        }

        Ok(Some(match round_result.result {
            replayer::BattleResult::Draw => self.on_draw_result(),
            replayer::BattleResult::Loss => BattleResult::Loss,
            replayer::BattleResult::Win => BattleResult::Win,
        }))
    }

    pub fn on_draw_result(&self) -> BattleResult {
        match self.local_player_index {
            0 => BattleResult::Win,
            1 => BattleResult::Loss,
            _ => unreachable!(),
        }
    }

    pub fn has_committed_state(&mut self) -> bool {
        self.committed_state.is_some()
    }

    pub fn local_delay(&self) -> u32 {
        self.iq.local_delay()
    }

    pub fn local_queue_length(&self) -> usize {
        self.iq.local_queue_length()
    }

    pub fn remote_queue_length(&self) -> usize {
        self.iq.remote_queue_length()
    }

    pub fn add_local_input(&mut self, input: lockstep::PartialInput) {
        log::debug!("local input: {:?}", input);
        self.iq.add_local_input(input);
    }

    pub fn add_remote_input(&mut self, input: lockstep::PartialInput) {
        log::debug!("remote input: {:?}", input);
        self.iq.add_remote_input(input);
    }

    pub fn tps_adjustment(&self) -> f32 {
        (self.dtick * session::EXPECTED_FPS as i32) as f32 / self.iq.max_length() as f32
    }
}

impl Drop for Round {
    fn drop(&mut self) {
        // HACK: This is the only safe way to set the FPS without clogging everything else up.
        self.primary_thread_handle
            .lock_audio()
            .sync_mut()
            .set_fps_target(session::EXPECTED_FPS as f32);
    }
}

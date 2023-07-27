use rand::Rng;

pub const EXPECTED_FPS: f32 = 16777216.0 / 280896.0;

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum BattleOutcome {
    Loss,
    Win,
}

#[derive(Clone)]
pub struct CommittedState {
    pub state: Box<mgba::state::State>,
    pub tick: u32,
    pub packet: Vec<u8>,
}

pub struct RoundState {
    pub number: u8,
    pub round: Option<Round>,
    pub last_outcome: Option<BattleOutcome>,
}

impl RoundState {
    pub fn end_round(&mut self) -> anyhow::Result<()> {
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

    pub fn set_last_outcome(&mut self, last_outcome: BattleOutcome) {
        self.last_outcome = Some(last_outcome);
    }
}

pub struct Match {
    shadow: std::sync::Arc<parking_lot::Mutex<crate::shadow::Shadow>>,
    rom: Vec<u8>,
    local_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    sender: std::sync::Arc<tokio::sync::Mutex<Box<dyn crate::net::Sender + Send + Sync>>>,
    rng: tokio::sync::Mutex<rand_pcg::Mcg128Xsl64>,
    cancellation_token: tokio_util::sync::CancellationToken,
    match_type: (u8, u8),
    input_delay: u32,
    is_offerer: bool,
    round_state: tokio::sync::Mutex<RoundState>,
    primary_thread_handle: mgba::thread::Handle,
    round_started_tx: tokio::sync::mpsc::Sender<u8>,
    round_started_rx: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<u8>>,
    replay_writer_factory: Box<
        dyn Fn(
                /* round_number */ u8,
                /* local_player_index */ u8,
            ) -> std::io::Result<Option<crate::replay::Writer>>
            + Send
            + Sync,
    >,
    on_replay_complete: std::sync::Arc<dyn Fn(&mut dyn std::io::Read) -> anyhow::Result<()> + Send + Sync>,
}

impl Match {
    pub fn new(
        rom: Vec<u8>,
        local_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        remote_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        cancellation_token: tokio_util::sync::CancellationToken,
        sender: Box<dyn crate::net::Sender + Send + Sync>,
        mut rng: rand_pcg::Mcg128Xsl64,
        is_offerer: bool,
        primary_thread_handle: mgba::thread::Handle,
        remote_rom: &[u8],
        remote_save: &(dyn tango_dataview::save::Save + Send + Sync),
        match_type: (u8, u8),
        input_delay: u32,
        replay_writer_factory: impl Fn(
                /* round_number */ u8,
                /* local_player_index */ u8,
            ) -> std::io::Result<Option<crate::replay::Writer>>
            + Send
            + Sync
            + 'static,
        on_replay_complete: impl Fn(&mut dyn std::io::Read) -> anyhow::Result<()> + Send + Sync + 'static,
    ) -> anyhow::Result<std::sync::Arc<Self>> {
        let (round_started_tx, round_started_rx) = tokio::sync::mpsc::channel(1);
        let did_polite_win_last_round = rng.gen::<bool>();
        let last_outcome = if did_polite_win_last_round == is_offerer {
            BattleOutcome::Win
        } else {
            BattleOutcome::Loss
        };
        let match_ = std::sync::Arc::new(Self {
            shadow: std::sync::Arc::new(parking_lot::Mutex::new(crate::shadow::Shadow::new(
                &remote_rom,
                remote_save,
                remote_hooks,
                match_type,
                is_offerer,
                last_outcome,
                rng.clone(),
            )?)),
            local_hooks,
            rom,
            sender: std::sync::Arc::new(tokio::sync::Mutex::new(sender)),
            rng: tokio::sync::Mutex::new(rng),
            cancellation_token,
            match_type,
            input_delay,
            round_state: tokio::sync::Mutex::new(RoundState {
                number: 0,
                round: None,
                last_outcome: Some(last_outcome),
            }),
            is_offerer,
            primary_thread_handle,
            round_started_tx,
            round_started_rx: tokio::sync::Mutex::new(round_started_rx),
            replay_writer_factory: Box::new(replay_writer_factory),
            on_replay_complete: std::sync::Arc::new(on_replay_complete),
        });
        Ok(match_)
    }

    pub fn cancel(&self) {
        self.cancellation_token.cancel()
    }

    pub fn cancelled(&self) -> tokio_util::sync::WaitForCancellationFuture {
        self.cancellation_token.cancelled()
    }

    pub fn advance_shadow_until_round_end(&self) -> anyhow::Result<()> {
        self.shadow.lock().advance_until_round_end()
    }

    pub fn advance_shadow_until_first_committed_state(&self) -> anyhow::Result<Box<mgba::state::State>> {
        self.shadow.lock().advance_until_first_committed_state()
    }

    pub async fn run(&self, mut receiver: Box<dyn crate::net::Receiver + Send + Sync>) -> anyhow::Result<()> {
        let mut last_round_number = 0;
        loop {
            let input = receiver.receive().await?;

            // We need to wait for the next round to start to avoid dropping inputs on the floor.
            if input.round_number != last_round_number {
                let round_number = if let Some(number) = self.round_started_rx.lock().await.recv().await {
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

            let now = std::time::Instant::now();
            round.add_remote_input(crate::input::PartialInput {
                local_tick: input.local_tick,
                remote_tick: (input.local_tick as i64 + input.tick_diff as i64) as u32,
                joyflags: input.joyflags as u16,
                dt: now - round.last_remote_input_time,
            });
            round.last_remote_input_time = now;
        }

        Ok(())
    }

    pub fn lock_round_state(&self) -> tokio::sync::MutexGuard<'_, RoundState> {
        self.round_state.blocking_lock()
    }

    pub fn lock_rng(&self) -> tokio::sync::MutexGuard<'_, rand_pcg::Mcg128Xsl64> {
        self.rng.blocking_lock()
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
        let local_player_index = match round_state.last_outcome.take().unwrap() {
            BattleOutcome::Win => 0,
            BattleOutcome::Loss => 1,
        };
        log::info!("starting round: local_player_index = {}", local_player_index);

        let replay_writer = (self.replay_writer_factory)(round_state.number, local_player_index)?;

        log::info!("preparing round state");

        let (first_state_committed_local_packet, first_state_committed_rx) = tokio::sync::oneshot::channel();

        const MAX_QUEUE_LENGTH: usize = 300;
        let mut iq = crate::input::PairQueue::new(MAX_QUEUE_LENGTH, self.input_delay);
        log::info!("filling {} ticks of input delay", self.input_delay);

        {
            let mut sender = self.sender.lock().await;
            for i in 0..self.input_delay {
                iq.add_local_input(crate::input::PartialInput {
                    local_tick: i,
                    remote_tick: 0,
                    joyflags: 0,
                    dt: std::time::Duration::ZERO,
                });
                sender
                    .send(&crate::net::Input {
                        round_number: round_state.number,
                        local_tick: i,
                        tick_diff: 0,
                        joyflags: 0,
                    })
                    .await?;
            }
        }

        let now = std::time::Instant::now();
        round_state.round = Some(Round {
            hooks: self.local_hooks,
            number: round_state.number,
            local_player_index,
            current_tick: 0,
            dtick: 0,
            iq,
            last_committed_remote_input: crate::input::Input {
                local_tick: 0,
                remote_tick: 0,
                joyflags: 0,
                packet: vec![0u8; self.local_hooks.packet_size()],
                dt: std::time::Duration::ZERO,
            },
            first_state_committed_local_packet: Some(first_state_committed_local_packet),
            first_state_committed_rx: Some(first_state_committed_rx),
            committed_state: None,
            stepper: crate::stepper::Fastforwarder::new(
                &self.rom,
                self.local_hooks,
                self.match_type,
                local_player_index,
            )?,
            replay_writer,
            primary_thread_handle: self.primary_thread_handle.clone(),
            sender: self.sender.clone(),
            shadow: self.shadow.clone(),
            on_replay_complete: self.on_replay_complete.clone(),
            last_local_input_time: now,
            last_remote_input_time: now,
        });
        self.round_started_tx.send(round_state.number).await?;
        log::info!("round has started");
        Ok(())
    }
}

pub struct Round {
    hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    number: u8,
    local_player_index: u8,
    current_tick: u32,
    dtick: i32,
    iq: crate::input::PairQueue<crate::input::PartialInput, crate::input::PartialInput>,
    last_committed_remote_input: crate::input::Input,
    first_state_committed_local_packet: Option<tokio::sync::oneshot::Sender<()>>,
    first_state_committed_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    committed_state: Option<CommittedState>,
    stepper: crate::stepper::Fastforwarder,
    replay_writer: Option<crate::replay::Writer>,
    primary_thread_handle: mgba::thread::Handle,
    sender: std::sync::Arc<tokio::sync::Mutex<Box<dyn crate::net::Sender + Send + Sync>>>,
    shadow: std::sync::Arc<parking_lot::Mutex<crate::shadow::Shadow>>,
    on_replay_complete: std::sync::Arc<dyn Fn(&mut dyn std::io::Read) -> anyhow::Result<()> + Send + Sync>,
    last_local_input_time: std::time::Instant,
    last_remote_input_time: std::time::Instant,
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

    pub fn set_first_committed_state(
        &mut self,
        local_state: Box<mgba::state::State>,
        remote_state: Box<mgba::state::State>,
        first_packet: &[u8],
    ) {
        if let Some(replay_writer) = self.replay_writer.as_mut() {
            replay_writer.write_state(&local_state).expect("write local state");
            replay_writer.write_state(&remote_state).expect("write remote state");
        }

        self.committed_state = Some(CommittedState {
            state: local_state,
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
    ) -> anyhow::Result<Option<BattleOutcome>> {
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
            .send(&crate::net::Input {
                round_number: self.number,
                local_tick,
                tick_diff: (remote_tick as i32 - local_tick as i32) as i8,
                joyflags,
            })
            .await?;

        let now = std::time::Instant::now();
        self.add_local_input(crate::input::PartialInput {
            local_tick,
            remote_tick,
            joyflags,
            dt: now - self.last_local_input_time,
        });
        self.last_local_input_time = now;

        let (committable, predict_required) = self.iq.consume_and_peek_local();

        let last_committed_state = self.committed_state.take().expect("committed state");

        let commit_tick = last_committed_state.tick + committable.len() as u32;
        let dirty_tick = commit_tick + predict_required.len() as u32 - 1;

        let input_pairs = committable
            .into_iter()
            .chain(predict_required.into_iter().map(|local| {
                let local_tick = local.local_tick;
                let remote_tick = local.remote_tick;
                let dt = local.dt;
                crate::input::Pair {
                    local,
                    remote: crate::input::PartialInput {
                        local_tick,
                        remote_tick,
                        joyflags: {
                            let mut joyflags = 0;
                            if self.last_committed_remote_input.joyflags & mgba::input::keys::A as u16 != 0 {
                                joyflags |= mgba::input::keys::A as u16;
                            }
                            if self.last_committed_remote_input.joyflags & mgba::input::keys::B as u16 != 0 {
                                joyflags |= mgba::input::keys::B as u16;
                            }
                            joyflags
                        },
                        dt,
                    },
                }
            }))
            .collect::<Vec<crate::input::Pair<crate::input::PartialInput, crate::input::PartialInput>>>();
        let last_local_input = input_pairs.last().unwrap().local.clone();

        let ff_result = self.stepper.fastforward(
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
                        .write_input(self.local_player_index, &ip.clone().into())
                        .expect("write input");
                }
            }
            self.last_committed_remote_input = ip.remote.clone();
        }

        core.load_state(&ff_result.dirty_state.state).expect("load dirty state");
        self.committed_state = Some(ff_result.committed_state);

        self.dtick = last_local_input.lag() - self.last_committed_remote_input.lag();

        core.gba_mut().sync_mut().expect("set fps target").set_fps_target(
            match EXPECTED_FPS as f32 + self.tps_adjustment() {
                fps_target if fps_target <= 0.0 => f32::MIN,
                fps_target if fps_target == f32::INFINITY => f32::MAX,
                fps_target => fps_target,
            },
        );

        let round_result = if let Some(round_result) = ff_result.round_result {
            round_result
        } else {
            return Ok(None);
        };

        if round_result.tick >= commit_tick {
            return Ok(None);
        }

        if let Some(replay_writer) = self.replay_writer.take() {
            let mut r = replay_writer.finish()?;
            log::info!(
                "replay finished at {:x} (real tick {:x})",
                round_result.tick,
                self.current_tick
            );

            r.seek(std::io::SeekFrom::Start(0))?;
            if let Err(e) = (self.on_replay_complete)(&mut r) {
                log::error!("on_replay_complete failed: {}", e);
            }
        }

        Ok(Some(match round_result.outcome {
            crate::stepper::BattleOutcome::Draw => self.on_draw_outcome(),
            crate::stepper::BattleOutcome::Loss => BattleOutcome::Loss,
            crate::stepper::BattleOutcome::Win => BattleOutcome::Win,
        }))
    }

    pub fn on_draw_outcome(&self) -> BattleOutcome {
        match self.local_player_index {
            0 => BattleOutcome::Win,
            1 => BattleOutcome::Loss,
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

    pub fn add_local_input(&mut self, input: crate::input::PartialInput) {
        log::debug!("local input: {:?}", input);
        self.iq.add_local_input(input);
    }

    pub fn add_remote_input(&mut self, input: crate::input::PartialInput) {
        log::debug!("remote input: {:?}", input);
        self.iq.add_remote_input(input);
    }

    pub fn tps_adjustment(&self) -> f32 {
        // This is (dtick / 1.5) ^ (7.0 / 3.0), but we can't do a precise cube root so we do this awkward sign copying thing.
        (self.dtick.abs() as f32 / 15.0)
            .powf(7.0 / 3.0)
            .copysign(self.dtick as f32)
    }
}

impl Drop for Round {
    fn drop(&mut self) {
        // HACK: This is the only safe way to set the FPS without clogging everything else up.
        self.primary_thread_handle
            .lock_audio()
            .sync_mut()
            .set_fps_target(EXPECTED_FPS);
    }
}

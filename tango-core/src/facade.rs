use crate::{battle, game, input};

pub struct BattleStateFacadeGuard<'a> {
    guard: tokio::sync::MutexGuard<'a, battle::BattleState>,
    match_: std::sync::Arc<battle::Match>,
}

impl<'a> BattleStateFacadeGuard<'a> {
    pub fn add_local_pending_turn(&mut self, local_turn: Vec<u8>) {
        self.guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!")
            .add_local_pending_turn(local_turn);
    }

    pub async fn end_battle(&mut self) {
        if let Some(battle) = &mut self.guard.battle {
            battle.replay_writer().write_eor().expect("write eor");
        }
        self.guard.end_battle().await.expect("end battle");
    }

    pub fn has_committed_state(&self) -> bool {
        self.guard
            .battle
            .as_ref()
            .expect("attempted to get battle information while no battle was active!")
            .committed_state()
            .is_some()
    }

    pub async fn add_local_input_and_fastforward(
        &mut self,
        mut core: mgba::core::CoreMutRef<'_>,
        current_tick: u32,
        joyflags: u16,
        custom_screen_state: u8,
        turn: Vec<u8>,
    ) -> bool {
        let battle_number = self.guard.number;

        let battle = self
            .guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!");

        let local_player_index = battle.local_player_index();
        let local_tick = current_tick + battle.local_delay();
        let remote_tick = battle.last_committed_remote_input().local_tick;

        // We do it in this order such that:
        // 1. We make sure that the input buffer does not overflow if we were to add an input.
        // 2. We try to send it to the peer: if it fails, we don't end up desyncing the opponent as we haven't added the input ourselves yet.
        // 3. We add the input to our buffer: no overflow is guaranteed because we already checked ahead of time.
        //
        // This is all done while the battle is locked, so there are no TOCTTOU issues.
        if !battle.can_add_local_input() {
            log::warn!("local input buffer overflow!");
            return false;
        }

        if let Err(e) = self
            .match_
            .transport()
            .expect("transport not available")
            .send_input(
                battle_number,
                local_tick,
                remote_tick,
                joyflags,
                custom_screen_state,
                turn.clone(),
            )
            .await
        {
            log::warn!("failed to send input: {}", e);
            return false;
        }

        battle.add_local_input(input::Input {
            local_tick,
            remote_tick,
            joyflags,
            custom_screen_state,
            turn,
        });

        let (input_pairs, left) = battle.consume_and_peek_local();

        for ip in &input_pairs {
            battle
                .replay_writer()
                .write_input(local_player_index, ip)
                .expect("write input");
        }

        let committed_state = battle
            .committed_state()
            .as_ref()
            .expect("committed state")
            .clone();
        let last_committed_remote_input = battle.last_committed_remote_input();

        let (committed_state, dirty_state, last_input) = match battle.fastforwarder().fastforward(
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

        battle.set_audio_save_state(dirty_state);

        // const RENDEZVOUS_AUDIO_EVERY: u32 = 10;
        // if current_tick % RENDEZVOUS_AUDIO_EVERY == 0 {
        //     battle
        //         .wait_for_audio_rendezvous()
        //         .expect("wait for audio rendezvous");
        // }

        battle.set_committed_state(committed_state);
        battle.set_last_input(last_input);

        core.gba_mut()
            .sync_mut()
            .expect("set fps target")
            .set_fps_target((game::EXPECTED_FPS as i32 + battle.tps_adjustment()) as f32);

        true
    }

    pub async fn set_committed_state(&mut self, state: mgba::state::State) {
        let battle = self
            .guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!");
        if battle.committed_state().is_none() {
            battle
                .replay_writer()
                .write_state(&state)
                .expect("write state");
            const STATE_CHUNK_SIZE: usize = 48 * 1024;
            let mut remote_state = vec![];
            for chunk in state.as_slice().chunks(STATE_CHUNK_SIZE) {
                self.match_
                    .transport()
                    .expect("transport")
                    .send_state_chunk(chunk)
                    .await
                    .expect("send state");

                remote_state.extend(
                    self.match_
                        .receive_remote_state_chunk()
                        .await
                        .expect("state chunk")
                        .iter(),
                );
            }
            battle
                .replay_writer()
                .write_state(&mgba::state::State::from_slice(&remote_state))
                .expect("write state");
        }
        battle.set_committed_state(state);
    }

    pub async fn fill_input_delay(&mut self, current_tick: u32) {
        let battle = self
            .guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!");
        for i in 0..battle.local_delay() {
            battle.add_local_input(input::Input {
                local_tick: current_tick + i,
                remote_tick: 0,
                joyflags: 0,
                custom_screen_state: 0,
                turn: vec![],
            });
        }
        for i in 0..battle.remote_delay() {
            battle.add_remote_input(input::Input {
                local_tick: current_tick + i,
                remote_tick: 0,
                joyflags: 0,
                custom_screen_state: 0,
                turn: vec![],
            });
        }
    }

    pub async fn send_init(&mut self, init: &[u8]) {
        let local_delay = self
            .guard
            .battle
            .as_ref()
            .expect("attempted to get battle information while no battle was active!")
            .local_delay();

        self.match_
            .transport()
            .expect("no transport")
            .send_init(self.guard.number, local_delay, init)
            .await
            .expect("send init");
        log::info!("sent local init: {:?}", init);
    }

    pub async fn receive_init(&mut self) -> Option<Vec<u8>> {
        let init = match self.match_.receive_remote_init().await {
            Some(init) => init,
            None => {
                return None;
            }
        };
        log::info!("received remote init: {:?}", init);

        if init.battle_number != self.guard.number {
            log::warn!(
                "expected battle number {} but got {}",
                self.guard.number,
                init.battle_number,
            )
        }

        self.guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!")
            .set_remote_delay(init.input_delay);

        Some(init.marshaled)
    }

    pub fn is_active(&self) -> bool {
        self.guard.battle.is_some()
    }

    pub fn mark_accepting_input(&mut self) {
        self.guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!")
            .mark_accepting_input()
    }

    pub fn is_accepting_input(&self) -> bool {
        self.guard
            .battle
            .as_ref()
            .expect("attempted to get battle information while no battle was active!")
            .is_accepting_input()
    }

    pub fn local_player_index(&self) -> u8 {
        self.guard
            .battle
            .as_ref()
            .expect("attempted to get battle information while no battle was active!")
            .local_player_index()
    }

    pub fn remote_player_index(&self) -> u8 {
        self.guard
            .battle
            .as_ref()
            .expect("attempted to get battle information while no battle was active!")
            .remote_player_index()
    }

    pub fn take_last_input(&mut self) -> Option<input::Pair<input::Input>> {
        self.guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!")
            .take_last_input()
    }

    pub fn take_local_pending_turn(&mut self) -> Vec<u8> {
        self.guard
            .battle
            .as_mut()
            .expect("attempted to get battle information while no battle was active!")
            .take_local_pending_turn()
    }

    pub fn set_won_last_battle(&mut self, did_win: bool) {
        self.guard.won_last_battle = did_win;
    }
}

impl MatchFacade {
    pub async fn lock_battle_state(&self) -> BattleStateFacadeGuard<'_> {
        let guard = self.arc.lock_battle_state().await;
        BattleStateFacadeGuard {
            guard,
            match_: self.arc.clone(),
        }
    }

    pub async fn start_battle(&self, core: mgba::core::CoreMutRef<'_>) {
        self.arc.start_battle(core).await.expect("start battle");
    }

    pub async fn lock_rng(&self) -> tokio::sync::MutexGuard<'_, rand_pcg::Mcg128Xsl64> {
        self.arc.lock_rng().await
    }

    pub fn match_type(&self) -> u16 {
        self.arc.match_type()
    }
}

#[derive(Clone)]
pub struct MatchFacade {
    arc: std::sync::Arc<battle::Match>,
}

struct InnerFacade {
    match_: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<battle::Match>>>>,
    joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    cancellation_token: tokio_util::sync::CancellationToken,
}

#[derive(Clone)]
pub struct Facade(std::rc::Rc<std::cell::RefCell<InnerFacade>>);

impl Facade {
    pub fn new(
        match_: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<battle::Match>>>>,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Self {
        Self(std::rc::Rc::new(std::cell::RefCell::new(InnerFacade {
            match_,
            joyflags,
            cancellation_token,
        })))
    }
    pub async fn match_(&self) -> Option<MatchFacade> {
        let match_ = match &*self.0.borrow().match_.lock().await {
            Some(match_) => match_.clone(),
            None => {
                return None;
            }
        };
        Some(MatchFacade { arc: match_ })
    }

    pub fn joyflags(&self) -> u32 {
        self.0
            .borrow()
            .joyflags
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub async fn abort_match(&self) {
        *self.0.borrow().match_.lock().await = None;
        self.0.borrow().cancellation_token.cancel();
    }

    pub async fn end_match(&self) {
        std::process::exit(0);
    }
}

#[derive(Clone)]
pub struct AudioFacade {
    audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
    audio_rendezvous_rx: std::sync::Arc<std::sync::mpsc::Receiver<()>>,
    local_player_index: u8,
}

impl AudioFacade {
    pub fn new(
        audio_save_state_holder: std::sync::Arc<parking_lot::Mutex<Option<mgba::state::State>>>,
        audio_rendezvous_rx: std::sync::Arc<std::sync::mpsc::Receiver<()>>,
        local_player_index: u8,
    ) -> Self {
        Self {
            audio_save_state_holder,
            audio_rendezvous_rx,
            local_player_index,
        }
    }

    pub fn take_audio_save_state(&mut self) -> Option<mgba::state::State> {
        //let _ = self.audio_rendezvous_rx.try_recv();
        self.audio_save_state_holder.lock().take()
    }

    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }
}

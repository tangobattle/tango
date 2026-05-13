#[derive(Clone)]
pub struct CompletionToken {
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl CompletionToken {
    pub fn new() -> Self {
        Self {
            flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn is_complete(&self) -> bool {
        self.flag.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn complete(&self) {
        self.flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn reset(&self) {
        self.flag.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// A registered trap: a ROM PC and the closure to run when it fires.
pub type Trap = (u32, Box<dyn Fn(mgba::core::CoreMutRef)>);

/// Shared handle to the live PvP Match (None until the session is constructed).
/// Primary-mode traps clone this Arc and lock it inside their closure.
pub type MatchHandle = std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<crate::battle::Match>>>>;

/// Trap helper that locks the match handle and runs the closure with a
/// reference to the live `Arc<Match>`. Returns early if the match isn't set
/// yet (e.g. before the session has finished constructing it). The Arc is
/// passed (rather than `&Match`) because `Match::start_round` takes
/// `self: &Arc<Self>`.
pub fn match_trap<F>(addr: u32, match_: &MatchHandle, f: F) -> Trap
where
    F: Fn(&std::sync::Arc<crate::battle::Match>, mgba::core::CoreMutRef) + 'static,
{
    let match_ = match_.clone();
    (
        addr,
        Box::new(move |core| {
            let guard = match_.blocking_lock();
            let Some(match_) = guard.as_ref() else { return };
            f(match_, core);
        }),
    )
}

/// Trap helper that locks the match and the in-progress round. Returns
/// early if either is missing.
pub fn match_round_trap<F>(addr: u32, match_: &MatchHandle, f: F) -> Trap
where
    F: Fn(&std::sync::Arc<crate::battle::Match>, &mut crate::battle::Round, mgba::core::CoreMutRef) + 'static,
{
    let match_ = match_.clone();
    (
        addr,
        Box::new(move |core| {
            let guard = match_.blocking_lock();
            let Some(match_) = guard.as_ref() else { return };
            let mut round_state = match_.lock_round_state();
            let Some(round) = round_state.round.as_mut() else {
                return;
            };
            f(match_, round, core);
        }),
    )
}

/// Trap helper for shadow-mode traps that don't need the round.
pub fn shadow_trap<F>(addr: u32, shadow_state: &crate::shadow::State, f: F) -> Trap
where
    F: Fn(&crate::shadow::State, mgba::core::CoreMutRef) + 'static,
{
    let shadow_state = shadow_state.clone();
    (addr, Box::new(move |core| f(&shadow_state, core)))
}

/// Trap helper that locks the shadow round. Returns early if no round is in
/// progress.
pub fn shadow_round_trap<F>(addr: u32, shadow_state: &crate::shadow::State, f: F) -> Trap
where
    F: Fn(&crate::shadow::State, &mut crate::shadow::Round, mgba::core::CoreMutRef) + 'static,
{
    let shadow_state = shadow_state.clone();
    (
        addr,
        Box::new(move |core| {
            let mut round_state = shadow_state.lock_round_state();
            let Some(round) = round_state.round.as_mut() else {
                return;
            };
            f(&shadow_state, round, core);
        }),
    )
}

/// Trap helper that locks the stepper InnerState. The state is always
/// present in both Fastforwarder and replay modes once construction is done,
/// so no unwrap is needed.
pub fn stepper_trap<F>(addr: u32, stepper_state: &crate::stepper::State, f: F) -> Trap
where
    F: Fn(&mut crate::stepper::InnerState, mgba::core::CoreMutRef) + 'static,
{
    let stepper_state = stepper_state.clone();
    (
        addr,
        Box::new(move |core| {
            let mut inner = stepper_state.lock_inner();
            f(&mut inner, core);
        }),
    )
}

/// Build the shadow-mode "result is decided" trap set: every passed PC just
/// flips `result_is_in` on the round state. Per-game callers list whichever
/// `round_end_*` and `round_end_damage_judge_*` offsets they have.
pub fn shadow_result_is_in_traps(shadow_state: &crate::shadow::State, addrs: &[u32]) -> Vec<Trap> {
    addrs
        .iter()
        .map(|&addr| {
            shadow_trap(addr, shadow_state, |shadow_state, _core| {
                shadow_state.lock_round_state().set_result_is_in();
            })
        })
        .collect()
}

/// Build the stepper-mode "round outcome decided" trap set. Each entry pairs
/// a PC with the outcome that PC implies, and the trap records that outcome
/// on the stepper state.
pub fn stepper_round_outcome_traps(
    stepper_state: &crate::stepper::State,
    entries: &[(u32, crate::stepper::BattleOutcome)],
) -> Vec<Trap> {
    entries
        .iter()
        .map(|&(addr, outcome)| {
            stepper_trap(addr, stepper_state, move |state, _core| {
                state.set_round_result(outcome);
            })
        })
        .collect()
}

pub trait Hooks {
    fn patch(&self, _core: mgba::core::CoreMutRef) {}

    fn common_traps(&self) -> Vec<Trap>;

    fn stepper_traps(&self, stepper_state: crate::stepper::State) -> Vec<Trap>;

    fn shadow_traps(&self, shadow_state: crate::shadow::State) -> Vec<Trap>;

    fn primary_traps(
        &self,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        match_: MatchHandle,
        completion_token: CompletionToken,
    ) -> Vec<Trap>;

    fn packet_size(&self) -> usize {
        0x10
    }

    fn prepare_for_fastforward(&self, core: mgba::core::CoreMutRef);

    fn predict_rx(&self, _rx: &mut Vec<u8>) {}
}

pub fn hooks_for_gamedb_entry(entry: &tango_gamedb::Game) -> Option<&'static (dyn Hooks + Send + Sync)> {
    Some(match entry.rom_code_and_revision {
        (b"AREJ", 0x00) => &crate::game::bn1::AREJ_00,
        (b"AREE", 0x00) => &crate::game::bn1::AREE_00,

        (b"AE2J", 0x00) => &crate::game::bn2::AE2J_00_AC,
        (b"AE2E", 0x00) => &crate::game::bn2::AE2E_00,

        (b"A6BJ", 0x01) => &crate::game::bn3::A6BJ_01,
        (b"A3XJ", 0x01) => &crate::game::bn3::A3XJ_01,
        (b"A6BE", 0x00) => &crate::game::bn3::A6BE_00,
        (b"A3XE", 0x00) => &crate::game::bn3::A3XE_00,

        (b"B4WJ", 0x01) => &crate::game::bn4::B4WJ_01,
        (b"B4BJ", 0x01) => &crate::game::bn4::B4BJ_01,
        (b"B4WE", 0x00) => &crate::game::bn4::B4WE_00,
        (b"B4BE", 0x00) => &crate::game::bn4::B4BE_00,

        (b"BRBJ", 0x00) => &crate::game::bn5::BRBJ_00,
        (b"BRKJ", 0x00) => &crate::game::bn5::BRKJ_00,
        (b"BRBE", 0x00) => &crate::game::bn5::BRBE_00,
        (b"BRKE", 0x00) => &crate::game::bn5::BRKE_00,

        (b"BR5J", 0x00) => &crate::game::bn6::BR5J_00,
        (b"BR6J", 0x00) => &crate::game::bn6::BR6J_00,
        (b"BR5E", 0x00) => &crate::game::bn6::BR5E_00,
        (b"BR6E", 0x00) => &crate::game::bn6::BR6E_00,

        (b"BR4J", 0x00) => &crate::game::exe45::BR4J_00,

        _ => {
            return None;
        }
    })
}

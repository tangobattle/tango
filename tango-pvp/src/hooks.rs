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
        self.flag.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn complete(&self) {
        self.flag.store(true, std::sync::atomic::Ordering::Release);
    }

    pub fn reset(&self) {
        self.flag.store(false, std::sync::atomic::Ordering::Release);
    }
}

pub type Trap = (u32, Box<dyn Fn(mgba::core::CoreMutRef)>);

/// Slot through which the per-game primary traps reach the live
/// [`Match`](crate::battle::Match).
///
/// Traps are registered at core setup, before the Match exists, so the slot
/// starts empty; the host session installs the Match once it's built
/// ([`set`](Self::set)) and empties the slot at teardown
/// ([`clear`](Self::clear)). Trap bodies call [`get`](Self::get), which
/// clones the inner `Arc` under a momentary read lock — the trap then holds
/// no lock at all, so a multi-millisecond fastforward inside a trap body
/// can't stall a teardown waiting on the write lock.
#[derive(Clone, Default)]
pub struct MatchHandle(std::sync::Arc<std::sync::RwLock<Option<std::sync::Arc<crate::battle::Match>>>>);

impl MatchHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// The live match as the traps see it, or `None` before
    /// [`set`](Self::set) / after [`clear`](Self::clear). Traps treat `None`
    /// as "no match running" and return early. Crate-private on purpose:
    /// the host installs and clears the slot but goes through host-facing
    /// API like [`round_metrics`](Self::round_metrics) to observe the match
    /// — it never gets the trap view.
    ///
    /// A cancelled match is `None` here too: `Match::cancel` only fires the
    /// cancellation token, and the host's [`clear`](Self::clear) runs
    /// asynchronously after it, so the emu thread keeps hitting traps in
    /// between. Worse, the cancel may come from a trap that aborted
    /// mid-frame (e.g. `add_local_input_and_fastforward` failing before it
    /// loads a state), leaving the round's tick bookkeeping behind the game
    /// — the tick-invariant panics in the per-game traps would fire on
    /// state that's no longer expected to be consistent. Going inert at
    /// `cancel` instead of `clear` closes that window.
    pub fn get(&self) -> Option<TrapMatch> {
        self.0
            .read()
            .unwrap()
            .clone()
            .filter(|m| !m.is_cancelled())
            .map(TrapMatch)
    }

    /// True iff a match is installed (and not cancelled — see
    /// [`get`](Self::get)). Cheaper than `get` for the per-frame traps that
    /// only test presence (no `Arc` clone+drop).
    pub fn is_set(&self) -> bool {
        self.0.read().unwrap().as_ref().is_some_and(|m| !m.is_cancelled())
    }

    /// Host-facing: engine metrics of the live round, or `None` when no
    /// match is installed / no round is running.
    pub fn round_metrics(&self) -> Option<crate::battle::RoundMetrics> {
        self.0.read().unwrap().as_ref()?.round_metrics()
    }

    pub fn set(&self, match_: std::sync::Arc<crate::battle::Match>) {
        *self.0.write().unwrap() = Some(match_);
    }

    pub fn clear(&self) {
        *self.0.write().unwrap() = None;
    }
}

/// The per-game traps' view of the live match: exactly the methods trap
/// bodies need, nothing from the host lifecycle (`run`, `finish_replay`,
/// `Match::new`, ...). Only [`MatchHandle::get`] — itself crate-private —
/// produces one, so the host crate can't reach this surface at all, and
/// trap code can't reach the host's.
pub struct TrapMatch(std::sync::Arc<crate::battle::Match>);

impl TrapMatch {
    pub fn lock_round_state(&self) -> std::sync::MutexGuard<'_, Option<crate::battle::Round>> {
        self.0.lock_round_state()
    }

    pub fn lock_rng(&self) -> std::sync::MutexGuard<'_, rand_pcg::Mcg128Xsl64> {
        self.0.lock_rng()
    }

    /// The match's outbound network channel, for
    /// [`Round::add_local_input_and_fastforward`](crate::battle::Round::add_local_input_and_fastforward).
    pub fn sender(&self) -> &crate::battle::SenderMutex {
        self.0.sender()
    }

    pub fn match_type(&self) -> (u8, u8) {
        self.0.match_type()
    }

    pub fn is_offerer(&self) -> bool {
        self.0.is_offerer()
    }

    pub fn record_first_commit(
        &self,
        round: &mut crate::battle::Round,
        core: mgba::core::CoreMutRef,
        first_packet: &[u8],
    ) -> anyhow::Result<()> {
        self.0.record_first_commit(round, core, first_packet)
    }

    pub fn end_round_or_cancel(&self, core: mgba::core::CoreMutRef) {
        self.0.end_round_or_cancel(core)
    }

    pub fn start_round_or_cancel(&self) {
        self.0.start_round_or_cancel()
    }

    pub fn cancel(&self) {
        self.0.cancel()
    }
}

/// The live-primary install parameters, bundled like
/// [`shadow::State`](crate::shadow::State) /
/// [`stepper::State`](crate::stepper::State): the shared local-joyflags cell the
/// primary trap reads each frame, the [`MatchHandle`] the traps drive the
/// running match through, the [`CompletionToken`] they flip when the match
/// completes, and whether battle BGM is muted.
#[derive(Clone)]
pub struct PrimaryState {
    pub joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    pub match_: MatchHandle,
    pub completion_token: CompletionToken,
    pub disable_bgm: bool,
}

pub trait Hooks {
    fn install_on_primary(&self, core: &mut mgba::core::Core, primary_state: PrimaryState);

    fn install_on_shadow(&self, core: &mut mgba::core::Core, shadow_state: crate::shadow::State);

    fn install_on_stepper(&self, core: &mut mgba::core::Core, stepper_state: crate::stepper::State);

    fn prepare_for_next_input(&self, core: mgba::core::CoreMutRef);

    fn inject_joyflags_on_primary(&self, core: mgba::core::CoreMutRef, joyflags: u16);
}

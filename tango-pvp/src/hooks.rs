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
    pub(crate) fn get(&self) -> Option<TrapMatch> {
        self.0.read().unwrap().clone().filter(|m| !m.is_cancelled()).map(TrapMatch)
    }

    /// True iff a match is installed (and not cancelled — see
    /// [`get`](Self::get)). Cheaper than `get` for the per-frame traps that
    /// only test presence (no `Arc` clone+drop).
    pub(crate) fn is_set(&self) -> bool {
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
pub(crate) struct TrapMatch(std::sync::Arc<crate::battle::Match>);

impl TrapMatch {
    pub(crate) fn lock_round_state(&self) -> std::sync::MutexGuard<'_, Option<crate::battle::Round>> {
        self.0.lock_round_state()
    }

    pub(crate) fn lock_rng(&self) -> std::sync::MutexGuard<'_, rand_pcg::Mcg128Xsl64> {
        self.0.lock_rng()
    }

    /// The match's outbound network channel, for
    /// [`Round::add_local_input_and_fastforward`](crate::battle::Round::add_local_input_and_fastforward).
    pub(crate) fn sender(&self) -> &crate::battle::SenderMutex {
        self.0.sender()
    }

    pub(crate) fn match_type(&self) -> (u8, u8) {
        self.0.match_type()
    }

    pub(crate) fn is_offerer(&self) -> bool {
        self.0.is_offerer()
    }

    /// How many rounds the live match has locally ended — 0 until the first
    /// round closes. Per-game traps that seed RNG once at match start (bn1)
    /// gate on this being 0 at `round_start_entry`.
    pub(crate) fn current_local_round_idx(&self) -> u32 {
        self.0.current_local_round_idx()
    }

    pub(crate) fn record_first_commit(
        &self,
        round: &mut crate::battle::Round,
        core: mgba::core::CoreMutRef,
        first_packet: &[u8],
    ) -> anyhow::Result<()> {
        self.0.record_first_commit(round, core, first_packet)
    }

    pub(crate) fn end_round_or_cancel(&self) {
        self.0.end_round_or_cancel()
    }

    pub(crate) fn start_round_or_cancel(&self) {
        self.0.start_round_or_cancel()
    }

    pub(crate) fn cancel(&self) {
        self.0.cancel()
    }
}

pub trait Hooks {
    fn patch(&self, _core: mgba::core::CoreMutRef) {}

    fn common_traps(&self) -> Vec<Trap>;

    fn stepper_traps(&self, stepper_state: crate::stepper::State) -> Vec<Trap>;

    fn shadow_traps(&self, shadow_state: crate::shadow::State) -> Vec<Trap>;

    /// `disable_bgm` arms the battle-start play-music trap (same one the
    /// stepper installs, switched by its stepper state instead): when set,
    /// battle BGM never starts; sound effects are unaffected.
    fn primary_traps(
        &self,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        match_: MatchHandle,
        completion_token: CompletionToken,
        disable_bgm: bool,
    ) -> Vec<Trap>;

    fn prepare_for_fastforward(&self, core: mgba::core::CoreMutRef);

    /// Prime `core`'s local-joyflags register (r4) from `joyflags` after a
    /// fastforwarder snapshot is loaded into the live primary core. The snapshot
    /// is captured poised at the start of its tick with r4 left unset (the
    /// boundary input is no longer baked in), so the live core — which resumes
    /// straight from the captured PC, past `main_read_joyflags` — must inject the
    /// displayed tick's local joyflags itself before stepping. (A fastforwarder
    /// run doesn't need this: `prepare_for_fastforward` rewinds its PC to
    /// `main_read_joyflags`, which re-primes r4 from the input window.)
    fn inject_joyflags_on_primary_snapshot(&self, core: mgba::core::CoreMutRef, joyflags: u16);
}

pub fn hooks_for_gamedb_entry(
    entry: &(dyn tango_gamedb::Game + Send + Sync),
) -> Option<&'static (dyn Hooks + Send + Sync)> {
    Some(match entry.rom_code_and_revision() {
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

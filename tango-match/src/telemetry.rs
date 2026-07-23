//! Per-tick RAM-poll telemetry over the pair, with rollback revocation.
//!
//! Battle VALUES (HP, custom flags, chips) are pure observations:
//! per-core [`CorePoller`]s read the same EWRAM addresses after every
//! simulated tick, driven through
//! [`mgba_rollback::session::TickObserver`]. Round LIFECYCLE (round
//! start, match end) is trap-driven instead: the games' own
//! battle-start and match-end code paths are ROM anchors the per-game
//! support traps ([`GameSupport::primer_traps`] wires them for core 0),
//! and each firing latches into a [`LifecycleSink`] the observer
//! drains and stamps with the tick it fired in. Polling can't see
//! those boundaries reliably — the battle structs the pollers read
//! stay stale-live across round teardown on the older families.
//!
//! The revocation model covers both: traps re-fire on rollback
//! re-simulation exactly as they fired the first time (the pair is
//! deterministic), so samples AND events from speculative ticks are
//! recorded eagerly and truncated again when a rollback rewinds past
//! them — what stands at any moment is exactly what the current
//! timeline has simulated, and everything at or below the session's
//! confirmed boundary is final.
//!
//! Each core gets its own poller (its own game variant's offsets — the
//! two sides of a crossplay pair are different ROMs), and each poller
//! answers for **its own player** where a game only knows its local side
//! (bn1–3's custom-screen flag). Absolute-indexed values (HP, chips, the
//! battle tick) are taken from player 0's core; under lockstep the two
//! games compute the same battle, so the views agree at every settled
//! boundary. Round-start and verdict traps live on core 0 only, for the
//! same reason — but MATCH-END anchors live on both cores: each game
//! exits the link session through its own path, and on a one-sided
//! decline only the decliner's game exits (the other waits at its menu
//! forever — a link cable has no detach signal), so whichever core
//! leaves first is the match end.
//!
//! [`GameSupport::primer_traps`]: crate::GameSupport::primer_traps

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Sentinel for "no chip loaded" in [`BattleObs::chips`] — the games' own
/// in-memory sentinel (same as `battle::NO_CHIP`).
pub const NO_CHIP: u16 = 0xffff;

/// How a finished round came out, in **absolute** player terms; hosts
/// reorient by local player index.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    P0Win,
    P1Win,
    Draw,
}

/// One core's view of one simulated tick, absolute player order where a
/// field covers both players.
#[derive(Clone, Copy, Debug)]
pub struct CoreObs {
    /// Both players' in-battle HP.
    pub hp: [u16; 2],
    /// Whether **this core's own player** has the custom (chip-select)
    /// screen open.
    pub custom_self: bool,
    /// Both players' loaded-chip tokens ([`NO_CHIP`] = none/not reported).
    pub chips: [u16; 2],
    /// The round's standing result, once this game has decided it.
    pub outcome: Option<Outcome>,
}

/// The per-game data-side reader for one core of the pair: polls one
/// tick's worth of battle state out of that game's RAM. Implementations
/// live in the gamesupport crates and are pure reads — a poller must
/// never write game memory, and must return the same result for the same
/// core state (it runs on speculative ticks and again on their
/// re-simulation).
///
/// Returns `None` when this game has no live battle state to read
/// (menus, the round intro before unit init). Round BOUNDARIES do not
/// come from this — see [`LifecycleSink`].
pub trait CorePoller: Send {
    fn poll(&mut self, core: &mut mgba::core::Core) -> Option<CoreObs>;
}

/// Any `FnMut` over a core is a poller — game support hands
/// [`GameSupport::core_poller`](crate::GameSupport::core_poller) a plain
/// closure reading its RAM offsets instead of defining a struct.
impl<F: FnMut(&mut mgba::core::Core) -> Option<CoreObs> + Send> CorePoller for F {
    fn poll(&mut self, core: &mut mgba::core::Core) -> Option<CoreObs> {
        self(core)
    }
}

/// Where the per-game lifecycle traps report: `round_started` at the
/// game's battle-start-complete anchor (core 0 only), `match_ended` at
/// its match-end anchor (both cores — either game leaving the link
/// session ends the match). Firings latch until the tick's
/// [`Telemetry::on_tick`] drains them (a trap fires mid-tick; the
/// observer runs right after), which stamps the event with the tick —
/// and rollback truncation + deterministic re-firing keep the record
/// consistent with the current timeline.
///
/// A latch set during PRIMING (round 1 starts before the session on
/// most families — priming runs until the battle is live) is drained by
/// [`Telemetry::new`] into a baseline event at tick 0, which no rewind
/// can truncate.
#[derive(Clone, Default)]
pub struct LifecycleSink {
    round_started: Arc<AtomicBool>,
    /// 0 = no report; otherwise `OUTCOME_*`.
    round_outcome: Arc<std::sync::atomic::AtomicU8>,
    match_ended: Arc<AtomicBool>,
}

const OUTCOME_P0_WIN: u8 = 1;
const OUTCOME_P1_WIN: u8 = 2;
const OUTCOME_DRAW: u8 = 3;

impl LifecycleSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Trap-side: the game's battle-start routine completed.
    pub fn round_started(&self) {
        self.round_started.store(true, Ordering::Release);
    }

    /// Trap-side: the game's own result-deciding code path ran (the
    /// win/loss/judge sites) — the standing verdict for the round in
    /// progress. Rounds have no end event; the verdict is stamped onto
    /// the round when it closes at the next round start or the match
    /// end. There is deliberately no HP-based fallback: a round with no
    /// announced verdict reports `None`.
    pub fn round_outcome(&self, outcome: Outcome) {
        let v = match outcome {
            Outcome::P0Win => OUTCOME_P0_WIN,
            Outcome::P1Win => OUTCOME_P1_WIN,
            Outcome::Draw => OUTCOME_DRAW,
        };
        self.round_outcome.store(v, Ordering::Release);
    }

    /// Trap-side: the game's own match-end path ran — the players left
    /// the battle loop for good.
    pub fn match_ended(&self) {
        self.match_ended.store(true, Ordering::Release);
    }

    fn take_round_started(&self) -> bool {
        self.round_started.swap(false, Ordering::AcqRel)
    }

    fn take_round_outcome(&self) -> Option<Outcome> {
        match self.round_outcome.swap(0, Ordering::AcqRel) {
            OUTCOME_P0_WIN => Some(Outcome::P0Win),
            OUTCOME_P1_WIN => Some(Outcome::P1Win),
            OUTCOME_DRAW => Some(Outcome::Draw),
            _ => None,
        }
    }

    fn take_match_ended(&self) -> bool {
        self.match_ended.swap(false, Ordering::AcqRel)
    }
}

/// Both players' merged observation for one simulated tick: absolute
/// values from player 0's core, per-player custom flags from each side's
/// own core.
#[derive(Clone, Copy, Debug)]
pub struct BattleObs {
    pub hp: [u16; 2],
    pub custom: [bool; 2],
    pub chips: [u16; 2],
    pub outcome: Option<Outcome>,
}

/// A lifecycle event, stamped from the games' own trapped code paths.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoundEvent {
    /// The game's battle-start routine completed for a round.
    Started,
    /// A round closed — stamped when the next round starts or the match
    /// ends. Carries the verdict the game's own result code path
    /// announced during the round ([`LifecycleSink::round_outcome`]),
    /// `None` if none fired.
    Ended { outcome: Option<Outcome> },
    /// The game's own match-end path ran: the match is over. Always
    /// preceded by the final round's `Ended`.
    MatchEnded,
}

/// The revocable telemetry buffers, shared between the engine-owned
/// observer and the host via [`TelemetryHandle`]. Under a mutex because
/// the observer writes it from inside `session.advance` (on the engine
/// thread) while the host reads it between advances.
#[derive(Default)]
pub struct Store {
    /// In-battle samples, tick ascending, dense within a round.
    samples: Vec<(u32, BattleObs)>,
    /// Lifecycle events, tick ascending. Never drained — a match only
    /// has a handful, and [`on_rewind`](Telemetry::on_rewind)
    /// re-derives the open-round state from this history, so consuming
    /// it would corrupt that derivation (the first rollback after the
    /// baseline `Started` was handed out would silently close the
    /// round). [`drain_confirmed`](Store::drain_confirmed) hands out
    /// clones past a cursor instead.
    events: Vec<(u32, RoundEvent)>,
    /// How many leading `events` entries [`drain_confirmed`] has already
    /// handed out.
    events_drained: usize,
    /// Whether a round is open (a `Started` without a closing `Ended`).
    round_open: bool,
    /// Tick the open round started at (0 for the priming baseline).
    round_started_at: u32,
    /// The games' announced verdicts, tick-stamped for revocation. Read
    /// — never consumed — at round close (the last report inside the
    /// closing round wins); a rewound close must be able to re-read
    /// them, so pruning would race the rollback horizon. A handful of
    /// entries per match.
    outcome_reports: Vec<(u32, Outcome)>,
}

impl Store {
    /// All standing samples, tick ascending. Ticks above the session's
    /// confirmed boundary are still speculative.
    pub fn samples(&self) -> &[(u32, BattleObs)] {
        &self.samples
    }

    /// All standing lifecycle events, tick ascending. Same speculation
    /// caveat as [`samples`](Store::samples).
    pub fn events(&self) -> &[(u32, RoundEvent)] {
        &self.events
    }

    /// Hand out every sample and event at or below `tick` (a confirmed
    /// boundary) not handed out before: they are final. Samples are
    /// physically drained (they're the bulk, and nothing re-derives
    /// from them); events are cloned past a cursor — the event history
    /// must survive for [`on_rewind`](Telemetry::on_rewind)'s
    /// open-round re-derivation.
    pub fn drain_confirmed(&mut self, tick: u32) -> (Vec<(u32, BattleObs)>, Vec<(u32, RoundEvent)>) {
        let s = self.samples.partition_point(|(t, _)| *t <= tick);
        let e = self
            .events
            .partition_point(|(t, _)| *t <= tick)
            .max(self.events_drained);
        let events = self.events[self.events_drained..e].to_vec();
        self.events_drained = e;
        (self.samples.drain(..s).collect(), events)
    }

    /// Close the open round, if any, stamping the last verdict the
    /// game's result code announced inside it (`None` if none fired —
    /// there is deliberately no HP-based inference).
    fn close_round(&mut self, tick: u32) {
        if self.round_open {
            let started_at = self.round_started_at;
            let outcome = self
                .outcome_reports
                .iter()
                .rev()
                .find(|(t, _)| *t >= started_at)
                .map(|(_, o)| *o);
            self.events.push((tick, RoundEvent::Ended { outcome }));
            self.round_open = false;
        }
    }
}

/// A host-side handle to the shared telemetry [`Store`].
pub type TelemetryHandle = std::sync::Arc<std::sync::Mutex<Store>>;

/// The [`TickObserver`](mgba_rollback::session::TickObserver) that turns
/// a pair of [`CorePoller`]s plus the trap-fed [`LifecycleSink`] into
/// revocable telemetry in a shared [`Store`]: dense per-tick samples
/// plus lifecycle events, both truncated on rewind.
pub struct Telemetry {
    pollers: [Box<dyn CorePoller>; 2],
    lifecycle: LifecycleSink,
    store: TelemetryHandle,
}

impl Telemetry {
    /// `pollers[i]` reads core `i` (player `i`'s game); `lifecycle` is
    /// the sink the pair's core-0 traps report into. Returns the
    /// observer (hand to `Session::set_observer`) and a handle onto the
    /// shared store for the host to read.
    ///
    /// A round start latched during priming (round 1 begins before the
    /// session on most families) becomes a baseline `Started` at tick 0
    /// here — rewinds can't reach below tick 0, so it can never be
    /// truncated (nor would its trap re-fire if it were).
    pub fn new(pollers: [Box<dyn CorePoller>; 2], lifecycle: LifecycleSink) -> (Self, TelemetryHandle) {
        let store: TelemetryHandle = Default::default();
        if lifecycle.take_round_started() {
            let mut s = store.lock().unwrap();
            s.events.push((0, RoundEvent::Started));
            s.round_open = true;
            s.round_started_at = 0;
        }
        (
            Telemetry {
                pollers,
                lifecycle,
                store: store.clone(),
            },
            store,
        )
    }
}

impl mgba_rollback::session::TickObserver for Telemetry {
    fn on_tick(&mut self, pair: &mut mgba_rollback::Link, tick: u32) {
        let obs0 = self.pollers[0].poll(pair.core_mut(0));
        let obs1 = self.pollers[1].poll(pair.core_mut(1));
        let obs = match (obs0, obs1) {
            (Some(c0), Some(c1)) => Some(BattleObs {
                hp: c0.hp,
                custom: [c0.custom_self, c1.custom_self],
                chips: c0.chips,
                outcome: c0.outcome.or(c1.outcome),
            }),
            _ => None,
        };

        let mut store = self.store.lock().unwrap();
        // Order matters when several fire in one tick: the verdict lands
        // before the close that reads it, and the match end comes last.
        if let Some(outcome) = self.lifecycle.take_round_outcome() {
            store.outcome_reports.push((tick, outcome));
        }
        if self.lifecycle.take_round_started() {
            store.close_round(tick);
            store.events.push((tick, RoundEvent::Started));
            store.round_open = true;
            store.round_started_at = tick;
        }
        if self.lifecycle.take_match_ended() {
            // Match-end anchors live on BOTH cores (either game leaving
            // the link session ends the match — on a one-sided decline
            // only the decliner's game exits; the other waits at its
            // menu for a peer that isn't coming back). When both games
            // do exit (mutual decline), the second core's firing lands a
            // few ticks after the first — the event history (never
            // drained) dedups it, and rewind truncation keeps the check
            // timeline-consistent.
            if !store.events.iter().any(|(_, e)| matches!(e, RoundEvent::MatchEnded)) {
                store.close_round(tick);
                store.events.push((tick, RoundEvent::MatchEnded));
            }
        }
        if store.round_open {
            // No samples past the round's announced verdict: the battle
            // is decided, and what follows is result screens and the
            // rematch conversation — whose battle structs linger
            // stale-live on the older families, which would smear a
            // long flat tail onto every round. The verdict tick's own
            // sample still records (the KO frame's HP drop). Derived
            // from `outcome_reports`, so rewind truncation + re-fire
            // keep it rollback-deterministic.
            let decided = store
                .outcome_reports
                .iter()
                .rev()
                .any(|&(t, _)| t >= store.round_started_at && t < tick);
            if !decided {
                if let Some(obs) = obs {
                    store.samples.push((tick, obs));
                }
            }
        }
    }

    fn on_rewind(&mut self, tick: u32) {
        let mut store = self.store.lock().unwrap();
        let s = store.samples.partition_point(|(t, _)| *t <= tick);
        store.samples.truncate(s);
        // A rewind can never reach below the confirmed boundary, and
        // only confirmed events are handed out — the clamp just keeps
        // the drain cursor coherent if that invariant ever slips.
        let e = store
            .events
            .partition_point(|(t, _)| *t <= tick)
            .max(store.events_drained);
        store.events.truncate(e);
        let r = store.outcome_reports.partition_point(|(t, _)| *t <= tick);
        store.outcome_reports.truncate(r);
        // Re-derive the open-round state at the rewind point from the
        // surviving event tail; the re-simulation re-fires whatever the
        // truncation dropped. (The sink's latches are always empty at a
        // rewind — every tick's firings are drained by its own on_tick.)
        match store.events.last() {
            Some(&(t, RoundEvent::Started)) => {
                store.round_open = true;
                store.round_started_at = t;
            }
            _ => {
                store.round_open = false;
            }
        }
    }
}

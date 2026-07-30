//! Per-tick telemetry over the pair, with rollback revocation.
//!
//! A game reports two kinds of things, and the pipeline keeps them
//! distinct end to end:
//!
//! * **Levels** — instantaneous facts polled out of RAM after every
//!   simulated tick: HP, tile, whether the custom screen stands open.
//!   They arrive as [`CoreObs`] from the per-core [`CorePoller`]s and
//!   land in the store as dense per-tick samples.
//! * **Edges** — things that HAPPEN: a round starting, a round's
//!   verdict being announced, the match ending, a chip being used.
//!   Several can fire in one tick. They all go through one
//!   [`EventSink`], whoever detects them: a PC-sited trap firing at the
//!   game's own code path (the mgba families' lifecycle anchors), or
//!   the poller itself catching a RAM transition as it reads the tick's
//!   levels (chip fires everywhere; round/match lifecycle on the DS
//!   engine, whose anchors are RAM facts rather than PC sites). The
//!   collector drains the sink once per tick, stamps each report with
//!   the tick, and lands them in the store as [`Event`]s.
//!
//! The revocation model covers both: traps re-fire on rollback
//! re-simulation exactly as they fired the first time (the pair is
//! deterministic), and a poller's edge detection is a pure function of
//! (previous state, this tick's RAM) whose previous state the store
//! carries per tick (see [`CorePoller::save`]) — so samples AND
//! events from speculative ticks are recorded eagerly and truncated
//! again when a rollback rewinds past them, and everything at or below
//! the session's confirmed boundary is final.
//!
//! Each core gets its own poller (its own game variant's offsets — the
//! two sides of a crossplay pair are different ROMs), and a poll splits
//! along what the game's RAM actually holds. The battle SIM is
//! both-sided on every core — under lockstep each game simulates both
//! units, and `unit_owner` resolves slot to absolute player — so
//! [`CoreObs::units`] carries both players and the merge takes them from
//! player 0's core, arbitrarily but consistently (core 1's copy is the
//! same values; splicing one player from each core would assemble a
//! state neither game ever computed if they ever disagreed). LOCAL state
//! has no second copy to read: bn1–3 keep the custom screen as this
//! side's battle-mode handler value, so [`CoreObs::custom_self`] answers
//! for the polling core's own player only and the merge pairs the two
//! cores' answers. Chip-use events follow the same rule from the other
//! direction: core `p` reports player `p`'s uses only — bn1 records its
//! picks per console and a peer's are nowhere in the other's RAM, and
//! on the games whose sim carries both, each core answering for its own
//! player is what keeps a use from being reported twice.
//!
//! Round-start and verdict traps live on core 0 only, for the same
//! reason — but MATCH-END anchors live on both cores: each game exits
//! the link session through its own path, and on a one-sided decline
//! only the decliner's game exits (the other waits at its menu forever —
//! a link cable has no detach signal), so whichever core leaves first is
//! the match end.

use std::sync::{Arc, Mutex};

/// How a finished round came out, in **absolute** player terms; hosts
/// reorient by local player index.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    P0Win,
    P1Win,
    Draw,
}

/// One player's unit in the battle sim, as one core sees it. Every field
/// here comes out of the shared simulation, so both cores of a pair read
/// the same values at a settled tick — new per-player battle readings
/// belong in here.
#[derive(Clone, Copy, Debug)]
pub struct UnitObs {
    /// In-battle HP.
    pub hp: u16,
    /// The tile the unit stands on, `(x, y)`, 1-based over the whole
    /// field: x 1..=6 left to right (columns 1-3 are the left player's
    /// side), y 1..=3 top to bottom. Where the unit IS — a move in
    /// flight reads as its origin until it lands.
    pub tile: (u8, u8),
}

/// One core's view of one simulated tick — the LEVELS half of a game's
/// reporting. Edges go through the [`EventSink`] instead.
#[derive(Clone, Copy, Debug)]
pub struct CoreObs {
    /// Both players' units, absolute player order.
    pub units: [UnitObs; 2],
    /// Whether **this core's own player** has the custom (chip-select)
    /// screen open — the one reading a game may only know for its local
    /// side (bn1–3), hence the per-core shape.
    pub custom_self: bool,
}

/// One edge report, as a game hands it to the [`EventSink`]. Private:
/// the sink's methods are the reporting surface, and the store's
/// [`Event`]s are the reading one.
#[derive(Clone, Copy, Debug)]
enum Report {
    RoundStarted,
    RoundEnded,
    RoundOutcome(Outcome),
    MatchEnded,
    ChipUsed { player: usize, chip: u16 },
}

/// Where every edge-triggered report lands, whoever detects it: the
/// primer traps (the mgba families' round/verdict/match-end anchors)
/// and the pollers (chip fires; the DS engine's poll-derived lifecycle)
/// share one sink per pair. Reports queue — several may fire in one
/// tick — until the tick's [`Telemetry::observe`] drains them, stamping
/// each with the tick; rollback truncation plus deterministic re-firing
/// keep the record consistent with the current timeline.
///
/// A report queued during PRIMING (round 1 starts before the session on
/// most families — priming runs until the battle is live) is drained by
/// [`Telemetry::new`] into a baseline event at tick 0, which no rewind
/// can truncate.
///
/// Firings are host-side signals only — they never touch core state, so
/// they can't perturb the simulation.
#[derive(Clone, Default)]
pub struct EventSink {
    queue: Arc<Mutex<Vec<Report>>>,
}

impl EventSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// The game's battle-start routine completed: a round is live.
    pub fn round_started(&self) {
        self.queue.lock().unwrap().push(Report::RoundStarted);
    }

    /// The game left its battle for the between-rounds interlude — only
    /// engines that can see the teardown as it happens report this (the
    /// DS engine's poll-derived lifecycle). The mgba families have no
    /// teardown anchor; their open round closes at the next
    /// [`round_started`](EventSink::round_started) or
    /// [`match_ended`](EventSink::match_ended) instead.
    pub fn round_ended(&self) {
        self.queue.lock().unwrap().push(Report::RoundEnded);
    }

    /// The game's own result-deciding code path ran (the win/loss/judge
    /// sites) — the standing verdict for the round in progress. The
    /// verdict is stamped onto the round when it closes; there is
    /// deliberately no HP-based fallback, so a round with no announced
    /// verdict reports `None`.
    pub fn round_outcome(&self, outcome: Outcome) {
        self.queue.lock().unwrap().push(Report::RoundOutcome(outcome));
    }

    /// The game's own match-end path ran — the players left the battle
    /// loop for good.
    pub fn match_ended(&self) {
        self.queue.lock().unwrap().push(Report::MatchEnded);
    }

    /// `player` used chip `chip` this tick. Report only for the
    /// reporting core's OWN player — on the games whose sim carries both
    /// players' chips, each core answering for its own player is what
    /// keeps a use from landing twice.
    pub fn chip_used(&self, player: usize, chip: u16) {
        self.queue.lock().unwrap().push(Report::ChipUsed { player, chip });
    }

    fn take(&self) -> Vec<Report> {
        std::mem::take(&mut self.queue.lock().unwrap())
    }
}

/// The per-game reader for one core of the pair: polls one tick's worth
/// of battle LEVELS out of that game's RAM, and pushes any EDGES it
/// detects while reading into `events`. Implementations live in the
/// gamesupport crates and must never write game memory.
///
/// A poller runs on speculative ticks and again on their re-simulation,
/// so everything it does must be a pure function of (its cross-tick
/// state, the core's state) — and that state must rewind when the
/// engine does, or re-simulated ticks would re-detect edges from
/// speculated history. [`save`](CorePoller::save)/[`restore`](CorePoller::restore)
/// are that contract: the collector snapshots every tick and hands the
/// right snapshot back after a rewind. What's inside is entirely the
/// poller's — the collector never looks into a [`Scratch`]. A poller
/// with cross-tick state MUST cover ALL of it in `save`; anything left
/// out sails through rollbacks.
///
/// `round` is how many rounds the collector has seen start (its store's
/// own count, itself rollback-consistent) — trackers use it to scope
/// their per-round state without any reset call reaching into them.
///
/// Returns `None` when this game has no live battle state to read
/// (menus, the round intro before unit init).
pub trait CorePoller<Core>: Send {
    fn poll(&mut self, core: &mut Core, events: &EventSink, round: u32) -> Option<CoreObs>;

    /// Snapshot the poller's cross-tick state. Stateless pollers keep
    /// the default.
    fn save(&self) -> Scratch {
        Scratch::none()
    }

    /// Restore a snapshot from [`save`](CorePoller::save) —
    /// [`Scratch::none`] means the fresh pre-first-tick state (a rewind
    /// to before the first observation).
    fn restore(&mut self, scratch: &Scratch) {
        let _ = scratch;
    }
}

/// One poller's cross-tick state, opaque to the collector: it only ever
/// records these and hands them back.
pub struct Scratch(Option<Box<dyn std::any::Any + Send>>);

impl Scratch {
    pub fn new<T: std::any::Any + Send>(state: T) -> Self {
        Scratch(Some(Box::new(state)))
    }

    /// The no-state snapshot — what stateless pollers save, and what a
    /// rewind to before the first observation restores.
    pub fn none() -> Self {
        Scratch(None)
    }

    /// The snapshot back at its real type, `None` if this is
    /// [`Scratch::none`] (restore the fresh state) or a foreign type.
    pub fn get<T: std::any::Any>(&self) -> Option<&T> {
        self.0.as_deref().and_then(|s| s.downcast_ref())
    }
}

/// Any stateless `FnMut` over a core is a poller — game support without
/// edges to report hands back a plain closure reading its RAM offsets.
impl<Core, F: FnMut(&mut Core) -> Option<CoreObs> + Send> CorePoller<Core> for F {
    fn poll(&mut self, core: &mut Core, _events: &EventSink, _round: u32) -> Option<CoreObs> {
        self(core)
    }
}

/// Both players' merged observation for one simulated tick: the shared
/// sim's units from player 0's core, per-player custom flags from each
/// side's own core. Absolute player order.
#[derive(Clone, Copy, Debug)]
pub struct BattleObs {
    pub units: [UnitObs; 2],
    pub custom: [bool; 2],
}

/// A tick-stamped event, as the store records it. Lifecycle events are
/// synthesized from the games' reports (an `Ended` closes the open
/// round, carrying the last verdict announced inside it); chip uses are
/// recorded as reported.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    /// The game's battle-start routine completed for a round.
    RoundStarted,
    /// A round closed — at the game's own teardown report where the
    /// engine has one, else at the next round start or the match end.
    /// Carries the verdict the game's own result code path announced
    /// during the round ([`EventSink::round_outcome`]), `None` if none
    /// fired.
    RoundEnded { outcome: Option<Outcome> },
    /// The game's own match-end path ran: the match is over. Always
    /// preceded by the final round's `RoundEnded`.
    MatchEnded,
    /// `player` (absolute) used chip `chip`. Only within an open,
    /// undecided round — chip motion on the result screens and the
    /// between-rounds interlude (where the older families' battle
    /// structs stay stale-live) is teardown, not play.
    ChipUsed { player: usize, chip: u16 },
}

/// The revocable telemetry buffers, shared between the engine-owned
/// observer and the host via [`TelemetryHandle`]. Under a mutex because
/// the observer writes it from inside `session.advance` (on the engine
/// thread) while the host reads it between advances.
#[derive(Default)]
pub struct Store {
    /// In-battle samples, tick ascending, dense within a round.
    samples: Vec<(u32, BattleObs)>,
    /// Events, tick ascending. Never drained — a match only has a
    /// handful of lifecycle entries plus its chip uses, and
    /// [`on_rewind`](Telemetry::on_rewind) re-derives the open-round
    /// state from this history, so consuming it would corrupt that
    /// derivation. [`drain_confirmed`](Store::drain_confirmed) hands out
    /// clones past a cursor instead.
    events: Vec<(u32, Event)>,
    /// How many leading `events` entries [`drain_confirmed`] has already
    /// handed out.
    events_drained: usize,
    /// Whether a round is open (a `RoundStarted` without a closing
    /// `RoundEnded`).
    round_open: bool,
    /// Tick the open round started at (0 for the priming baseline).
    round_started_at: u32,
    /// The games' announced verdicts, tick-stamped for revocation. Read
    /// — never consumed — at round close (the last report inside the
    /// closing round wins); a rewound close must be able to re-read
    /// them, so pruning would race the rollback horizon. A handful of
    /// entries per match.
    outcome_reports: Vec<(u32, Outcome)>,
    /// The pollers' cross-tick state after each tick, opaque to this
    /// store — the ring that makes stateful edge detection
    /// rollback-transparent: a rewind to tick T hands the pollers back
    /// exactly the state they held after T, so the re-simulation
    /// re-detects exactly what the first pass did. Truncated with
    /// everything else on rewind; pruned below the confirmed boundary
    /// on drain (a rewind can never reach below it).
    scratch: Vec<(u32, [Scratch; 2])>,
}

impl Store {
    /// All standing samples, tick ascending. Ticks above the session's
    /// confirmed boundary are still speculative.
    pub fn samples(&self) -> &[(u32, BattleObs)] {
        &self.samples
    }

    /// All standing events, tick ascending. Same speculation caveat as
    /// [`samples`](Store::samples).
    pub fn events(&self) -> &[(u32, Event)] {
        &self.events
    }

    /// Hand out every sample and event at or below `tick` (a confirmed
    /// boundary) not handed out before: they are final. Samples are
    /// physically drained (they're the bulk, and nothing re-derives
    /// from them); events are cloned past a cursor — the event history
    /// must survive for [`on_rewind`](Telemetry::on_rewind)'s
    /// open-round re-derivation.
    pub fn drain_confirmed(&mut self, tick: u32) -> (Vec<(u32, BattleObs)>, Vec<(u32, Event)>) {
        let s = self.samples.partition_point(|(t, _)| *t <= tick);
        let e = self
            .events
            .partition_point(|(t, _)| *t <= tick)
            .max(self.events_drained);
        let events = self.events[self.events_drained..e].to_vec();
        self.events_drained = e;
        // Scratch below the confirmed boundary can never be restored
        // again (a rewind can't reach below it) — keep the boundary
        // entry itself, a rewind can land exactly there.
        let p = self.scratch.partition_point(|(t, _)| *t < tick);
        self.scratch.drain(..p);
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
            self.events.push((tick, Event::RoundEnded { outcome }));
            self.round_open = false;
        }
    }

    /// Whether the open round's verdict was announced BEFORE `tick` —
    /// the gate that stops samples and chip events once the battle is
    /// decided: what follows is result screens and the rematch
    /// conversation, whose battle structs linger stale-live on the older
    /// families. The verdict tick's own readings still record (the KO
    /// frame's HP drop). Derived from `outcome_reports`, so rewind
    /// truncation + re-fire keep it rollback-deterministic.
    fn decided(&self, tick: u32) -> bool {
        self.outcome_reports
            .iter()
            .rev()
            .any(|&(t, _)| t >= self.round_started_at && t < tick)
    }
}

/// A host-side handle to the shared telemetry [`Store`].
pub type TelemetryHandle = std::sync::Arc<std::sync::Mutex<Store>>;

/// The tick observer that turns a pair of [`CorePoller`]s plus the
/// shared [`EventSink`] into revocable telemetry in a shared [`Store`]:
/// dense per-tick samples plus tick-stamped events, both truncated on
/// rewind.
pub struct Telemetry<Core> {
    pollers: [Box<dyn CorePoller<Core>>; 2],
    events: EventSink,
    store: TelemetryHandle,
    /// Rounds seen start so far (the baseline included) — handed to the
    /// pollers so their trackers can scope per-round state. Recomputed
    /// from the event history on rewind, like everything else.
    rounds: u32,
}

impl<Core> Telemetry<Core> {
    /// Read one console's battle state. Engines call this per core —
    /// a link hands out one core at a time, so the two reads cannot
    /// share a borrow.
    pub fn poll(&mut self, player: usize, core: &mut Core) -> Option<CoreObs> {
        self.pollers[player].poll(core, &self.events, self.rounds)
    }

    /// Fold one tick's readings into the store: merge the levels,
    /// drain the sink's edge reports and stamp them with the tick.
    /// Everything from here on is the same arithmetic for any console.
    pub fn observe(&mut self, obs0: Option<CoreObs>, obs1: Option<CoreObs>, tick: u32) {
        let obs = match (obs0, obs1) {
            (Some(c0), Some(c1)) => Some(BattleObs {
                // The sim's own readings come from player 0's core; the
                // per-core custom answers pair up.
                units: c0.units,
                custom: [c0.custom_self, c1.custom_self],
            }),
            _ => None,
        };

        let reports = self.events.take();
        let mut store = self.store.lock().unwrap();
        // Reports process by kind, not arrival order — the one order
        // that composes when several fire in one tick: the verdict
        // lands before the close that reads it, chip uses belong to the
        // round that was live when they fired (so they precede any
        // close), a start closes the previous round, and the match end
        // comes last.
        for r in &reports {
            if let Report::RoundOutcome(outcome) = r {
                store.outcome_reports.push((tick, *outcome));
            }
        }
        for r in &reports {
            if let Report::ChipUsed { player, chip } = r {
                if store.round_open && !store.decided(tick) {
                    store.events.push((tick, Event::ChipUsed {
                        player: *player,
                        chip: *chip,
                    }));
                }
            }
        }
        if reports.iter().any(|r| matches!(r, Report::RoundEnded)) {
            store.close_round(tick);
        }
        if reports.iter().any(|r| matches!(r, Report::RoundStarted)) {
            store.close_round(tick);
            store.events.push((tick, Event::RoundStarted));
            store.round_open = true;
            store.round_started_at = tick;
            self.rounds += 1;
        }
        if reports.iter().any(|r| matches!(r, Report::MatchEnded)) {
            // Match-end anchors live on BOTH cores (either game leaving
            // the link session ends the match - on a one-sided decline
            // only the decliner's game exits; the other waits at its
            // menu for a peer that isn't coming back). When both games
            // do exit (mutual decline), the second core's firing lands a
            // few ticks after the first - the event history (never
            // drained) dedups it, and rewind truncation keeps the check
            // timeline-consistent.
            if !store.events.iter().any(|(_, e)| matches!(e, Event::MatchEnded)) {
                store.close_round(tick);
                store.events.push((tick, Event::MatchEnded));
            }
        }
        if store.round_open && !store.decided(tick) {
            if let Some(obs) = obs {
                store.samples.push((tick, obs));
            }
        }
        let scratch = [self.pollers[0].save(), self.pollers[1].save()];
        store.scratch.push((tick, scratch));
    }

    /// Revoke everything an engine speculated past `tick`. Called when
    /// a rollback discards work: also engine-neutral.
    pub fn on_rewind(&mut self, tick: u32) {
        let mut store = self.store.lock().unwrap();
        let s = store.samples.partition_point(|(t, _)| *t <= tick);
        store.samples.truncate(s);
        // A rewind can never reach below the confirmed boundary, and
        // only confirmed events are handed out - the clamp just keeps
        // the drain cursor coherent if that invariant ever slips.
        let e = store
            .events
            .partition_point(|(t, _)| *t <= tick)
            .max(store.events_drained);
        store.events.truncate(e);
        let r = store.outcome_reports.partition_point(|(t, _)| *t <= tick);
        store.outcome_reports.truncate(r);
        // Re-derive the open-round state and the round count at the
        // rewind point from the surviving event tail; the re-simulation
        // re-fires whatever the truncation dropped. (The sink's queue is
        // always empty at a rewind - every tick's reports are drained by
        // its own observe.)
        match store
            .events
            .iter()
            .rev()
            .find(|(_, e)| !matches!(e, Event::ChipUsed { .. }))
        {
            Some(&(t, Event::RoundStarted)) => {
                store.round_open = true;
                store.round_started_at = t;
            }
            _ => {
                store.round_open = false;
            }
        }
        self.rounds = store
            .events
            .iter()
            .filter(|(_, e)| matches!(e, Event::RoundStarted))
            .count() as u32;
        // Hand the pollers back the edge-detection state they held
        // after the restored tick, so re-simulated ticks re-detect
        // exactly what the first pass did.
        let p = store.scratch.partition_point(|(t, _)| *t <= tick);
        store.scratch.truncate(p);
        let fresh = [Scratch::none(), Scratch::none()];
        let scratch = store.scratch.last().map(|(_, s)| s).unwrap_or(&fresh);
        for (poller, s) in self.pollers.iter_mut().zip(scratch) {
            poller.restore(s);
        }
    }

    /// `pollers[i]` reads core `i` (player `i`'s game); `events` is the
    /// sink the pair's traps and pollers report into. Returns the
    /// observer (hand to the engine's link) and a handle onto the
    /// shared store for the host to read.
    ///
    /// A round start queued during priming (round 1 begins before the
    /// session on most families) becomes a baseline `RoundStarted` at
    /// tick 0 here — rewinds can't reach below tick 0, so it can never
    /// be truncated (nor would its trap re-fire if it were). Any other
    /// report queued by the walk is boot noise and is dropped with the
    /// drain.
    pub fn new(pollers: [Box<dyn CorePoller<Core>>; 2], events: EventSink) -> (Self, TelemetryHandle) {
        let store: TelemetryHandle = Default::default();
        let mut rounds = 0;
        if events.take().iter().any(|r| matches!(r, Report::RoundStarted)) {
            let mut s = store.lock().unwrap();
            s.events.push((0, Event::RoundStarted));
            s.round_open = true;
            s.round_started_at = 0;
            rounds = 1;
        }
        (
            Telemetry {
                pollers,
                events,
                store: store.clone(),
                rounds,
            },
            store,
        )
    }
}

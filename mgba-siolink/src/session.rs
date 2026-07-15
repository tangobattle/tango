//! The rollback loop around a [`Link`], built on the [`getgud`] engine:
//! repeat-last prediction for each remote side, speculative snapshots that
//! are promoted when the predictions held and rolled back when they
//! didn't, and a present delay instead of a negotiated input delay.
//!
//! All peers construct the SAME link (same ROM/save/RTC for every player,
//! in player order) and run one `Session` each, differing only in
//! `local_player`. Each frame the caller feeds the local joypad and
//! whatever remote input packets arrived — **in order** per peer, one per
//! remote tick, untagged — and the session presents
//! `frontier - present_delay`, rolling back transparently as corrections
//! land. The present delay is purely local (each peer picks its own;
//! nothing is negotiated), and the packets carry each side's tick
//! advantage so the clocks can be held together: feed
//! [`skew`](Session::skew) and [`speculation_balance`](Session::speculation_balance)
//! into a [`Throttler`](crate::throttler::Throttler) and shave the tick
//! rate by its output.
//!
//! Because the link is deterministic, sessions fed each other's
//! outgoing packets converge on the identical settled trajectory — which
//! [`checkpoint`](Session::checkpoint) digests make checkable on the wire.

use std::sync::{Arc, Mutex};

use crate::{Link, Snapshot, MAX_PLAYERS};

/// An input packet to forward to every peer: the local player's keys for
/// local tick `tick`, plus this side's tick advantage for clock sync.
///
/// The receiving side feeds packets to
/// [`add_remote_input`](Session::add_remote_input) strictly in `tick`
/// order per sender, exactly once each — `tick` exists to let the
/// transport deduplicate and order (e.g. as a sequence number), not to
/// schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outgoing {
    pub tick: u32,
    pub keys: u32,
    /// How far this side's local input stream leads its furthest-behind
    /// remote one, in ticks — the sender's half of the clock-sync
    /// handshake.
    pub tick_advantage: i16,
}

/// What one [`advance`](Session::advance) did, for pacing and diagnostics.
#[derive(Debug, Clone, Copy, Default)]
pub struct Report {
    /// Speculative ticks discarded and re-simulated because a confirmed
    /// remote input contradicted the prediction (0 = no rollback).
    pub rolled_back: u32,
    /// Local ticks fed in so far (the newest local tick).
    pub frontier: u32,
    /// Ticks [0, confirmed) have real inputs from every side and can
    /// never be rolled back again.
    pub confirmed: u32,
    /// The tick whose state this frame presents
    /// (`frontier - present_delay`, clamped to what has been simulated).
    pub presented: u32,
}

/// Observes the simulation as it advances, from inside the engine's
/// callbacks — the seam for anything that wants to read game RAM every
/// simulated tick (telemetry, round detection) without the session layer
/// knowing what a game is.
///
/// Contract: [`on_tick`](TickObserver::on_tick) fires for every simulated
/// tick in simulation order, with the link parked at that tick's boundary.
/// Ticks are **speculative until confirmed**: after
/// [`on_rewind`](TickObserver::on_rewind)`(t)`, every observation for
/// ticks > `t` is revoked, and the re-simulation will re-report them
/// (possibly with different values). Ticks at or below the session's
/// confirmed boundary are final and will never be rewound.
pub trait TickObserver: Send {
    /// The link just simulated `tick` and is parked at its boundary.
    fn on_tick(&mut self, link: &mut Link, tick: u32);
    /// The link rewound to `tick` ahead of a rollback re-simulation:
    /// discard every observation for ticks > `tick`.
    fn on_rewind(&mut self, tick: u32);
}

/// The link plus the bookkeeping the [`getgud::World`] callbacks write,
/// shared between the engine-owned world and the [`Session`] wrapper (the
/// engine owns its world outright, so anything the host must reach lives
/// behind this handle).
struct Shared {
    link: Link,
    /// The tick the live link is parked at (frames of the reference core
    /// since boot). Snapshots are stamped with it so `load` can recognize
    /// a no-op restore.
    live_tick: u32,
    /// Confirmed input rows in player order, tick order, not yet handed
    /// out by [`Session::drain_confirmed`].
    confirmed: Vec<Box<[u32]>>,
    /// Host-installed per-tick observer, if any.
    observer: Option<Box<dyn TickObserver>>,
}

/// A cloneable, cross-thread handle to the live link — the readout seam
/// for hosts that pull from the link off the session's thread (e.g. an
/// audio callback draining a core's sample buffer). Locks the same mutex
/// the engine's per-tick step takes, so access interleaves between engine
/// ticks. [`Session::with_link`]'s contract applies: read out, don't
/// tick/load.
#[derive(Clone)]
pub struct LinkHandle {
    shared: Arc<Mutex<Shared>>,
}

impl LinkHandle {
    /// Run `f` against the live link. See [`Session::with_link`].
    pub fn with_link<R>(&self, f: impl FnOnce(&mut Link) -> R) -> R {
        f(&mut self.shared.lock().unwrap().link)
    }
}

/// A link snapshot stamped with the tick it is poised at.
struct SnapshotAt {
    snap: Snapshot,
    tick: u32,
}

/// The [`getgud::World`] over a [`Link`]: `step` is one lockstep tick,
/// `save`/`load` are whole-link snapshots, prediction is repeat-last.
/// Remote slot `i` is player `i` skipping over `local_player`.
struct LinkWorld {
    shared: Arc<Mutex<Shared>>,
    local_player: usize,
    num_players: usize,
}

impl LinkWorld {
    /// Assemble a player-indexed key row from the engine's (local, remotes)
    /// view.
    fn key_row(&self, local: u32, remotes: &[u32]) -> [u32; MAX_PLAYERS] {
        debug_assert_eq!(remotes.len(), self.num_players - 1);
        let mut keys = [0u32; MAX_PLAYERS];
        keys[self.local_player] = local;
        for (slot, &k) in remotes.iter().enumerate() {
            keys[slot + (slot >= self.local_player) as usize] = k;
        }
        keys
    }
}

impl getgud::World for LinkWorld {
    /// One side's joypad keys for one tick.
    type Input = u32;
    type State = SnapshotAt;
    type Error = mgba::Error;

    fn step(&mut self, local: &u32, remotes: &[u32]) -> Result<(), mgba::Error> {
        let keys = self.key_row(*local, remotes);
        let mut guard = self.shared.lock().unwrap();
        let shared = &mut *guard;
        shared.link.tick(&keys[..self.num_players]);
        shared.live_tick += 1;
        if let Some(observer) = shared.observer.as_mut() {
            observer.on_tick(&mut shared.link, shared.live_tick);
        }
        Ok(())
    }

    fn save(&mut self) -> Result<SnapshotAt, mgba::Error> {
        let mut shared = self.shared.lock().unwrap();
        let tick = shared.live_tick;
        Ok(SnapshotAt {
            snap: shared.link.save()?,
            tick,
        })
    }

    fn load(&mut self, state: &SnapshotAt) -> Result<(), mgba::Error> {
        let mut guard = self.shared.lock().unwrap();
        let shared = &mut *guard;
        // The engine `load`s the settled state before every rollback
        // re-sim; when nothing speculated past it the link is already
        // parked there (and by determinism holds the identical state), so
        // skip the restore and keep steady-state settles forward-only.
        if shared.live_tick == state.tick {
            return Ok(());
        }
        shared.link.load(&state.snap)?;
        shared.live_tick = state.tick;
        if let Some(observer) = shared.observer.as_mut() {
            observer.on_rewind(state.tick);
        }
        Ok(())
    }

    /// Repeat-last: assume a remote keeps holding exactly what they
    /// held (measured best over tango's replay corpus).
    fn predict(&self, last_remote: &u32) -> u32 {
        *last_remote
    }

    fn log(&mut self, local: &u32, remotes: &[u32]) {
        let keys = self.key_row(*local, remotes);
        let mut shared = self.shared.lock().unwrap();
        shared.confirmed.push(keys[..self.num_players].into());
    }
}

/// How many settled-boundary digests to keep for cross-peer checkpoint
/// answering — comfortably past any sane checkpoint interval plus wire
/// latency, at 8 bytes each.
const DIGEST_HISTORY: usize = 600;

pub struct Session {
    inner: getgud::Session<LinkWorld>,
    shared: Arc<Mutex<Shared>>,
    local_player: usize,
    num_players: usize,
    /// Rolling `(tick, digest)` history of settled boundaries observed
    /// after each `advance`, newest last. When remote inputs arrive in
    /// bursts the settled cap can jump several ticks inside one advance;
    /// the skipped boundaries simply aren't answerable
    /// ([`digest_at`](Session::digest_at) returns `None` for them).
    digests: std::collections::VecDeque<(u32, u32)>,
    /// Ticks handed out by [`drain_confirmed`](Session::drain_confirmed).
    drained: u32,
}

impl Session {
    /// Wrap a freshly booted link. `present_delay`: how many ticks behind
    /// the local frontier to present — purely local (the peers need not
    /// agree), trades input latency against prediction depth, adjustable
    /// at runtime via [`set_present_delay`](Session::set_present_delay).
    pub fn new(mut link: Link, local_player: usize, present_delay: u32) -> Result<Self, mgba::Error> {
        let num_players = link.num_players();
        assert!(local_player < num_players);
        // Live play only ever presents the local side, so don't spend the
        // software renderer on the remote cores (frameskip is unserialized
        // and invisible to the simulation — see [`Link::set_frameskip`]).
        // A caller that does want a remote picture can flip it back via
        // [`with_link`](Session::with_link).
        for i in (0..num_players).filter(|&i| i != local_player) {
            link.set_frameskip(i, i32::MAX);
        }
        let initial = SnapshotAt {
            snap: link.save()?,
            tick: 0,
        };
        let shared = Arc::new(Mutex::new(Shared {
            link,
            live_tick: 0,
            confirmed: Vec::new(),
            observer: None,
        }));
        Ok(Session {
            inner: getgud::Session::new(getgud::SessionParams {
                present_delay,
                initial_remotes: vec![0; num_players - 1],
                initial_state: initial,
                world: LinkWorld {
                    shared: shared.clone(),
                    local_player,
                    num_players,
                },
            }),
            shared,
            local_player,
            num_players,
            digests: std::collections::VecDeque::new(),
            drained: 0,
        })
    }

    pub fn local_player(&self) -> usize {
        self.local_player
    }

    /// Number of players (cores) on the link.
    pub fn num_players(&self) -> usize {
        self.num_players
    }

    /// Number of local ticks fed in so far.
    pub fn frontier(&self) -> u32 {
        self.inner.local_frontier()
    }

    pub fn present_delay(&self) -> u32 {
        self.inner.present_delay()
    }

    pub fn set_present_delay(&mut self, present_delay: u32) {
        self.inner.set_present_delay(present_delay);
    }

    /// Install (or clear) the per-tick observer. See [`TickObserver`] for
    /// the speculation/revocation contract. Installing mid-session is
    /// fine — the observer simply starts seeing ticks from here on.
    pub fn set_observer(&mut self, observer: Option<Box<dyn TickObserver>>) {
        self.shared.lock().unwrap().observer = observer;
    }

    /// Borrow the live link (e.g. for video/audio readout). The link is
    /// parked at the newest simulated tick — in steady state that is
    /// exactly the presented tick. Do not tick/load it behind the
    /// session's back; that desyncs the engine's bookkeeping.
    pub fn with_link<R>(&self, f: impl FnOnce(&mut Link) -> R) -> R {
        f(&mut self.shared.lock().unwrap().link)
    }

    /// A [`LinkHandle`] for readout from other threads. The handle keeps
    /// the link alive independently of the session.
    pub fn link_handle(&self) -> LinkHandle {
        LinkHandle {
            shared: self.shared.clone(),
        }
    }

    /// Feed one remote input packet from `player` (any player but the
    /// local one). Packets must arrive here in tick order per player,
    /// exactly once each (dedup/order on the transport side —
    /// [`Outgoing::tick`] is the sequence number).
    pub fn add_remote_input(&mut self, player: usize, keys: u32, tick_advantage: i16) {
        assert!(player < self.num_players && player != self.local_player);
        let slot = player - (player > self.local_player) as usize;
        self.inner.add_remote_input(slot, keys, tick_advantage);
    }

    /// This side's half of the clock-sync handshake — already stamped on
    /// every [`Outgoing`] by [`advance`](Session::advance).
    pub fn local_tick_advantage(&self) -> i16 {
        self.inner.local_tick_advantage()
    }

    /// Clock-sync skew: how far this peer runs ahead of the remote it
    /// most leads, positive = we lead and should slow down. Read it
    /// BEFORE [`advance`](Session::advance) (afterward the just-enqueued
    /// local input biases it up by one) and feed it to a
    /// [`Throttler`](crate::throttler::Throttler).
    pub fn skew(&self) -> i32 {
        self.inner.skew()
    }

    /// Signed distance of the presented frame from the speculation
    /// boundary — the [`Throttler`](crate::throttler::Throttler)'s
    /// engagement gate.
    pub fn speculation_balance(&self) -> i32 {
        self.inner.speculation_balance()
    }

    /// Local inputs buffered but not yet matched by every remote's input —
    /// the stall-guard signal: when this outruns what the transport's
    /// redundancy window can carry, stop advancing.
    pub fn local_queue_length(&self) -> usize {
        self.inner.local_queue_length()
    }

    /// Ticks [0, confirmed) have real inputs from every side and can
    /// never be rolled back again.
    pub fn confirmed(&self) -> u32 {
        self.inner.local_frontier() - self.inner.local_queue_length() as u32
    }

    /// Sample the local joypad for this frame and advance the session one
    /// tick: newly confirmed inputs settle (promoting correct predictions,
    /// rolling back wrong ones), and speculation extends to the present
    /// target. Returns the packet to send to every peer plus a report.
    pub fn advance(&mut self, local_keys: u32) -> Result<(Outgoing, Report), mgba::Error> {
        let outgoing = Outgoing {
            tick: self.inner.local_frontier(),
            keys: local_keys,
            tick_advantage: self.inner.local_tick_advantage(),
        };

        let presented = self.inner.advance(local_keys)?.tick;

        // Harvest the settled boundary this advance ended on, so a peer's
        // checkpoint for that tick can be answered later.
        let settled = self.inner.settled_state();
        if settled.tick > 0 && self.digests.back().map(|(t, _)| *t) != Some(settled.tick) {
            if self.digests.len() == DIGEST_HISTORY {
                self.digests.pop_front();
            }
            let digest = settled.snap.digest();
            self.digests.push_back((settled.tick, digest));
        }

        let frontier = self.inner.local_frontier();
        Ok((
            outgoing,
            Report {
                rolled_back: self.inner.last_misprediction_depth(),
                frontier,
                confirmed: frontier - self.inner.local_queue_length() as u32,
                presented,
            },
        ))
    }

    /// The newest settled tick with a recorded digest, for cross-peer
    /// desync detection: all peers eventually settle the same tick and
    /// the digests must match bit for bit.
    pub fn checkpoint(&self) -> Option<(u32, u32)> {
        self.digests.back().copied()
    }

    /// Digest of this session's settled state at exactly `tick`, if that
    /// boundary was observed — the receive side of cross-peer desync
    /// checking (compare against a peer's [`checkpoint`](Self::checkpoint)).
    /// `None` means "can't check this one", not "mismatch".
    pub fn digest_at(&self, tick: u32) -> Option<u32> {
        self.digests.iter().find(|(t, _)| *t == tick).map(|(_, d)| *d)
    }

    /// Drain newly-confirmed ticks as (tick, player-indexed keys) — final
    /// input rows in tick order, suitable for a replay sink. Ticks are
    /// 1-based, numbered like [`TickObserver::on_tick`]'s: the input
    /// row that produced simulated tick `t` is stamped `t`, so a
    /// tick's confirmed inputs and its telemetry line up exactly.
    pub fn drain_confirmed(&mut self) -> Vec<(u32, Box<[u32]>)> {
        let mut shared = self.shared.lock().unwrap();
        let out = shared
            .confirmed
            .drain(..)
            .enumerate()
            .map(|(i, keys)| (self.drained + i as u32 + 1, keys))
            .collect::<Vec<_>>();
        self.drained += out.len() as u32;
        out
    }
}

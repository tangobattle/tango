//! The rollback loop, once, for every engine.
//!
//! Both peers run the *same* pair of consoles, and the only true inputs
//! are the two players' pads: everything on the wire between the
//! consoles is derived deterministically. So each frame a peer feeds its
//! own input in, predicts the remote's, and simulates ahead; when a
//! prediction turns out wrong it restores the last settled snapshot and
//! re-simulates.
//!
//! None of that reasoning is about a console. It needs a pair that
//! ticks, snapshots and restores — which is exactly [`Backend`] — so it
//! lives here rather than being written once per emulator.

use std::sync::{Arc, Mutex};

use crate::backend::Backend;

/// What the caller forwards to the peer after an
/// [`advance`](Match::advance).
#[derive(Clone, Copy, Debug)]
pub struct Outgoing<Input> {
    pub tick: u32,
    pub input: Input,
    /// How far ahead of the peer this side is running, for the clock
    /// governor on the other end.
    pub tick_advantage: i16,
}

/// What one [`advance`](Match::advance) did.
#[derive(Clone, Copy, Debug, Default)]
pub struct Report {
    /// Ticks simulated (settles plus speculation).
    pub ticks: u32,
    /// Depth of the rollback this advance performed, 0 if none.
    pub rollback_depth: u32,
}

/// A snapshot tagged with the tick it was taken at, so a load that
/// targets where the pair already sits can skip the restore.
struct SnapshotAt<B: Backend> {
    snapshot: B::Snapshot,
    tick: u32,
}

/// The [`getgud::World`] over a linked pair.
struct World<B: Backend> {
    /// Shared with the [`Match`], which hands it out for video, audio
    /// and RAM readout while the session simulates.
    link: Arc<Mutex<B::Link>>,
    live_tick: u32,
    local_player: usize,
    /// Snapshots the engine has retired, kept for their allocations —
    /// a rollback session retires one nearly every tick and they run to
    /// megabytes.
    pool: Vec<B::Snapshot>,
}

impl<B: Backend> getgud::World for World<B> {
    type Input = B::Input;
    type State = SnapshotAt<B>;
    type Error = B::Error;

    fn step(&mut self, local: &B::Input, remotes: &[B::Input]) -> Result<(), B::Error> {
        let mut inputs = [*local; 2];
        inputs[1 - self.local_player] = remotes[0];
        inputs[self.local_player] = *local;
        B::tick(&mut self.link.lock().unwrap(), inputs);
        self.live_tick += 1;
        Ok(())
    }

    fn save(&mut self) -> Result<SnapshotAt<B>, B::Error> {
        Ok(SnapshotAt {
            snapshot: B::snapshot(&mut self.link.lock().unwrap(), self.pool.pop())?,
            tick: self.live_tick,
        })
    }

    fn load(&mut self, state: &SnapshotAt<B>) -> Result<(), B::Error> {
        // The engine loads the settled state before every re-simulation;
        // when nothing speculated past it the pair is already parked
        // there and — by determinism — holds exactly this state.
        if self.live_tick == state.tick {
            return Ok(());
        }
        B::restore(&mut self.link.lock().unwrap(), &state.snapshot)?;
        self.live_tick = state.tick;
        Ok(())
    }

    fn recycle(&mut self, state: SnapshotAt<B>) {
        // Two deep covers the settled state plus the one replacing it.
        if self.pool.len() < 2 {
            self.pool.push(state.snapshot);
        }
    }

    /// Repeat-last: a held button is far likelier to persist than to
    /// change on any given frame.
    fn predict(&self, last_remote: &B::Input) -> B::Input {
        *last_remote
    }
}

/// A running two-player rollback match on any engine.
pub struct Match<B: Backend> {
    inner: getgud::Session<World<B>>,
    link: Arc<Mutex<B::Link>>,
    local_player: usize,
}

impl<B: Backend> Match<B> {
    /// Start a match over an already-booted pair.
    pub fn new(link: B::Link, local_player: usize, present_delay: u32) -> Result<Self, B::Error> {
        assert!(local_player < 2);
        let link = Arc::new(Mutex::new(link));
        let mut world = World::<B> {
            link: link.clone(),
            live_tick: 0,
            local_player,
            pool: Vec::new(),
        };
        let initial_state = {
            use getgud::World as _;
            world.save()?
        };
        Ok(Match {
            inner: getgud::Session::new(getgud::SessionParams {
                present_delay,
                initial_remotes: vec![Default::default()],
                initial_state,
                world,
            }),
            link,
            local_player,
        })
    }

    /// Advance one frame: settle what the peer's arrivals confirm,
    /// rolling back on a misprediction, then speculate to the present
    /// target.
    pub fn advance(&mut self, local: B::Input) -> Result<(Outgoing<B::Input>, Report), B::Error> {
        let before = self.inner.local_frontier();
        let frame = self.inner.advance(local)?;
        let tick = frame.tick;
        Ok((
            Outgoing {
                tick,
                input: local,
                tick_advantage: self.inner.local_tick_advantage(),
            },
            Report {
                ticks: self.inner.local_frontier().saturating_sub(before),
                rollback_depth: self.inner.last_misprediction_depth(),
            },
        ))
    }

    /// Feed one remote input packet, in tick order.
    pub fn add_remote_input(&mut self, input: B::Input, tick_advantage: i16) {
        self.inner.add_remote_input(0, input, tick_advantage);
    }

    /// Run `f` against the live pair — video, audio and RAM readout.
    pub fn with_link<R>(&self, f: impl FnOnce(&mut B::Link) -> R) -> R {
        f(&mut self.link.lock().unwrap())
    }

    pub fn local_player(&self) -> usize {
        self.local_player
    }

    pub fn skew(&self) -> i32 {
        self.inner.skew()
    }

    pub fn speculation_balance(&self) -> i32 {
        self.inner.speculation_balance()
    }

    pub fn local_queue_length(&self) -> usize {
        self.inner.local_queue_length()
    }

    pub fn matchable(&self) -> usize {
        self.inner.matchable()
    }

    pub fn present_delay(&self) -> u32 {
        self.inner.present_delay()
    }

    pub fn set_present_delay(&mut self, present_delay: u32) {
        self.inner.set_present_delay(present_delay);
    }
}

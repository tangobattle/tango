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

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
    /// Which seats the host displays — the local one, until something
    /// like training turns the remote back on. Shared with the
    /// [`Match`] so that can happen mid-session.
    visible: Arc<[AtomicBool; 2]>,
    /// First tick of the current advance whose frame could reach the
    /// screen. Ticks below it are rollback re-simulation: nobody ever
    /// sees their frames, so nobody draws them.
    render_from: Arc<AtomicU32>,
}

impl<B: Backend> getgud::World for World<B> {
    type Input = B::Input;
    type State = SnapshotAt<B>;
    type Error = B::Error;

    fn step(&mut self, local: &B::Input, remotes: &[B::Input]) -> Result<(), B::Error> {
        let mut inputs = [*local; 2];
        inputs[1 - self.local_player] = remotes[0];
        inputs[self.local_player] = *local;
        let mut link = self.link.lock().unwrap();
        let render = self.live_tick + 1 >= self.render_from.load(Ordering::Relaxed);
        for player in 0..2 {
            B::set_render(&mut link, player, render && self.visible[player].load(Ordering::Relaxed));
        }
        B::tick(&mut link, inputs);
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
    visible: Arc<[AtomicBool; 2]>,
    render_from: Arc<AtomicU32>,
}

impl<B: Backend> Match<B> {
    /// Start a match over an already-booted pair.
    pub fn new(link: B::Link, local_player: usize, present_delay: u32) -> Result<Self, B::Error> {
        assert!(local_player < 2);
        let link = Arc::new(Mutex::new(link));
        let visible = Arc::new([AtomicBool::new(local_player == 0), AtomicBool::new(local_player == 1)]);
        let render_from = Arc::new(AtomicU32::new(0));
        let mut world = World::<B> {
            link: link.clone(),
            live_tick: 0,
            local_player,
            pool: Vec::new(),
            visible: visible.clone(),
            render_from: render_from.clone(),
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
            visible,
            render_from,
        })
    }

    /// Advance one frame: settle what the peer's arrivals confirm,
    /// rolling back on a misprediction, then speculate to the present
    /// target.
    pub fn advance(&mut self, local: B::Input) -> Result<(Outgoing<B::Input>, Report), B::Error> {
        let before = self.inner.local_frontier();
        // The simulation only ever reaches the present target — the
        // frontier is the *input* frontier, `present_delay` ticks
        // ahead of it. Everything this advance re-simulates below the
        // target is rollback replay whose frames nobody sees;
        // rendering resumes at the target so the frame the host
        // presents is drawn.
        self.render_from
            .store(before.saturating_sub(self.inner.present_delay()), Ordering::Relaxed);
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

    /// This match's local console audio.
    pub fn audio(&self) -> Box<dyn crate::AudioDrain> {
        self.seat_audio(std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(
            self.local_player,
        )))
    }

    /// Audio for whichever seat `seat` currently names.
    pub fn seat_audio(
        &self,
        seat: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> Box<dyn crate::AudioDrain> {
        B::audio(self.link.clone(), seat)
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


impl<B: Backend> crate::RunningMatch for Match<B> {
    fn advance(&mut self, local: crate::HostInput) -> Result<(u32, crate::HostInput, i16), crate::Error> {
        let (outgoing, _report) = Match::advance(self, B::input_of(local))
            .map_err(|e| crate::Error::Backend(Box::new(e)))?;
        Ok((outgoing.tick, B::host_of(outgoing.input), outgoing.tick_advantage))
    }

    fn add_remote_input(&mut self, input: crate::HostInput, tick_advantage: i16) {
        Match::add_remote_input(self, B::input_of(input), tick_advantage);
    }

    fn frame(&mut self) -> Option<Vec<u8>> {
        let player = self.local_player();
        self.with_link(|link| B::frame(link, player))
    }

    fn skew(&self) -> i32 {
        Match::skew(self)
    }

    fn speculation_balance(&self) -> i32 {
        Match::speculation_balance(self)
    }

    fn local_queue_length(&self) -> usize {
        Match::local_queue_length(self)
    }

    fn present_delay(&self) -> u32 {
        Match::present_delay(self)
    }

    fn set_present_delay(&mut self, present_delay: u32) {
        Match::set_present_delay(self, present_delay);
    }

    fn matchable(&self) -> usize {
        Match::matchable(self)
    }

    fn local_player(&self) -> usize {
        Match::local_player(self)
    }

    fn audio(&self) -> Option<Box<dyn crate::AudioDrain>> {
        Some(Match::audio(self))
    }

    fn seat_audio(
        &self,
        seat: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> Option<Box<dyn crate::AudioDrain>> {
        Some(Match::seat_audio(self, seat))
    }

    fn seat_frame(&mut self, player: usize) -> Option<Vec<u8>> {
        self.with_link(|link| B::frame(link, player))
    }

    fn render_seats(&mut self) {
        for seat in &*self.visible {
            seat.store(true, Ordering::Relaxed);
        }
        self.with_link(|link| {
            for player in 0..2 {
                B::set_render(link, player, true);
            }
        });
    }
}

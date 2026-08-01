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
//! ticks, snapshots and restores — which is exactly [`Link`] — so it
//! lives here rather than being written once per emulator. [`Match`] is
//! the whole host-facing surface: a `Game` registration's backend hands
//! one back, and the host drives it without ever learning which
//! emulator is underneath.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::link::{Link, Snapshot};
use crate::HostInput;

/// A snapshot tagged with the tick it was taken at, so a load that
/// targets where the pair already sits can skip the restore.
struct SnapshotAt {
    snapshot: Snapshot,
    tick: u32,
    /// The link's audio mark at save time, so a load knows how much its
    /// speculation produced past this point. Not part of the snapshot
    /// itself: audio is playback state, and no savestate carries it.
    audio_mark: [u64; 2],
}

/// The [`getgud::World`] over a linked pair.
struct World {
    /// Shared with the [`Match`], which hands it out for video, audio
    /// and RAM readout while the session simulates.
    link: Arc<Mutex<dyn Link>>,
    live_tick: u32,
    local_player: usize,
    /// Snapshots the engine has retired, kept for their allocations —
    /// a rollback session retires one nearly every tick and they run to
    /// megabytes.
    pool: Vec<Snapshot>,
    /// Which seats the host displays — the local one, until something
    /// like training turns the remote back on. Shared with the
    /// [`Match`] so that can happen mid-session.
    visible: Arc<[AtomicBool; 2]>,
    /// First tick of the current advance whose frame could reach the
    /// screen. Ticks below it are rollback re-simulation: nobody ever
    /// sees their frames, so nobody draws them.
    render_from: Arc<AtomicU32>,
    /// Confirmed input rows in player order, tick order, not yet handed
    /// out by [`Match::drain_confirmed`]. Shared because the engine
    /// owns its world outright.
    confirmed: Arc<Mutex<Vec<[HostInput; 2]>>>,
}

impl getgud::World for World {
    type Input = HostInput;
    type State = SnapshotAt;
    type Error = crate::Error;

    fn step(&mut self, local: &HostInput, remotes: &[HostInput]) -> Result<(), crate::Error> {
        let mut inputs = [*local; 2];
        inputs[1 - self.local_player] = remotes[0];
        inputs[self.local_player] = *local;
        let mut link = self.link.lock().unwrap();
        let render = self.live_tick + 1 >= self.render_from.load(Ordering::Relaxed);
        for player in 0..2 {
            link.side(player)
                .set_render(render && self.visible[player].load(Ordering::Relaxed));
        }
        link.tick(inputs);
        self.live_tick += 1;
        Ok(())
    }

    fn save(&mut self) -> Result<SnapshotAt, crate::Error> {
        let mut link = self.link.lock().unwrap();
        Ok(SnapshotAt {
            audio_mark: link.audio_mark(),
            snapshot: link.snapshot(self.pool.pop())?,
            tick: self.live_tick,
        })
    }

    fn load(&mut self, state: &SnapshotAt) -> Result<(), crate::Error> {
        // The engine loads the settled state before every re-simulation;
        // when nothing speculated past it the pair is already parked
        // there and — by determinism — holds exactly this state.
        if self.live_tick == state.tick {
            return Ok(());
        }
        let mut link = self.link.lock().unwrap();
        link.restore(&state.snapshot)?;
        // The restore does not reach the audio the speculation voiced,
        // so that comes back by hand — otherwise the re-simulation
        // queues the same span a second time.
        link.revoke_audio(state.audio_mark);
        self.live_tick = state.tick;
        Ok(())
    }

    fn recycle(&mut self, state: SnapshotAt) {
        // Two deep covers the settled state plus the one replacing it.
        if self.pool.len() < 2 {
            self.pool.push(state.snapshot);
        }
    }

    /// Repeat-last: a held button is far likelier to persist than to
    /// change on any given frame.
    fn predict(&self, last_remote: &HostInput) -> HostInput {
        *last_remote
    }

    fn log(&mut self, local: &HostInput, remotes: &[HostInput]) {
        let mut row = [*local; 2];
        row[1 - self.local_player] = remotes[0];
        row[self.local_player] = *local;
        self.confirmed.lock().unwrap().push(row);
    }
}

/// A running two-player rollback match on any engine — the unified
/// session surface a host drives, one virtual call per tick into the
/// [`Link`] underneath.
pub struct Match {
    inner: getgud::Session<World>,
    link: Arc<Mutex<dyn Link>>,
    local_player: usize,
    visible: Arc<[AtomicBool; 2]>,
    render_from: Arc<AtomicU32>,
    /// Confirmed rows the world's `log` callback has recorded, shared
    /// with it (the engine owns its world outright).
    confirmed: Arc<Mutex<Vec<[HostInput; 2]>>>,
    /// Ticks handed out by [`drain_confirmed`](Match::drain_confirmed).
    drained: u32,
    /// How deep the last [`advance`](Match::advance) rolled back, kept
    /// because that is the only moment it is knowable and a host reads
    /// it on its own schedule.
    last_rollback_depth: u32,
    /// The telemetry this match publishes, when its engine reads any —
    /// installed by the backend that wired the link's pollers up.
    telemetry: Option<crate::telemetry::TelemetryHandle>,
}

impl Match {
    /// Start a match over an already-booted, already-primed pair.
    pub fn new<L: Link>(link: L, local_player: usize, present_delay: u32) -> Result<Self, crate::Error> {
        assert!(local_player < 2);
        let link: Arc<Mutex<dyn Link>> = Arc::new(Mutex::new(link));
        let visible = Arc::new([AtomicBool::new(local_player == 0), AtomicBool::new(local_player == 1)]);
        let render_from = Arc::new(AtomicU32::new(0));
        let confirmed = Arc::new(Mutex::new(Vec::new()));
        let mut world = World {
            link: link.clone(),
            live_tick: 0,
            local_player,
            pool: Vec::new(),
            visible: visible.clone(),
            render_from: render_from.clone(),
            confirmed: confirmed.clone(),
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
            confirmed,
            drained: 0,
            last_rollback_depth: 0,
            telemetry: None,
        })
    }

    /// Install the telemetry handle this match publishes. Factory-side:
    /// the pollers themselves live inside the engine's link, wired up
    /// before the match started; this is just where a host finds the
    /// shared store.
    pub fn set_telemetry(&mut self, telemetry: crate::telemetry::TelemetryHandle) {
        self.telemetry = Some(telemetry);
    }

    /// Advance one frame with the local console's input: settle what
    /// the peer's arrivals confirm, rolling back on a misprediction,
    /// then speculate to the present target. Returns the tick this
    /// input belongs to (a sequence number for the transport), the
    /// input as the engine actually applied it — sanitized through the
    /// link, so both peers feed their consoles identical values — and
    /// this side's tick advantage for the peer's clock sync.
    pub fn advance(&mut self, local: HostInput) -> Result<(u32, HostInput, i16), crate::Error> {
        let local = self.link.lock().unwrap().sanitize(local);
        // Both halves of the outgoing packet are read before the
        // advance enqueues this tick's local input: the tick is this
        // input's own index, and the advantage must match the skew the
        // peer will read against it (afterward the just-enqueued input
        // biases it up by one).
        let tick = self.inner.local_frontier();
        let tick_advantage = self.inner.local_tick_advantage();
        // The simulation only ever reaches the present target — the
        // frontier is the *input* frontier, `present_delay` ticks
        // ahead of it. Everything this advance re-simulates below the
        // target is rollback replay whose frames nobody sees;
        // rendering resumes at the target so the frame the host
        // presents is drawn.
        self.render_from
            .store(tick.saturating_sub(self.inner.present_delay()), Ordering::Relaxed);
        self.inner.advance(local)?;
        self.last_rollback_depth = self.inner.last_misprediction_depth();
        Ok((tick, local, tick_advantage))
    }

    /// Feed one remote input packet, in tick order. Sanitized on the
    /// way in exactly as local input is, so a peer whose packet says
    /// more than the console can express still lands both simulations
    /// on the same value.
    pub fn add_remote_input(&mut self, input: HostInput, tick_advantage: i16) {
        let input = self.link.lock().unwrap().sanitize(input);
        self.inner.add_remote_input(0, input, tick_advantage);
    }

    /// The local console's display, RGBA8 in the backend's
    /// [`screen_layout`](crate::Backend::screen_layout) order.
    pub fn frame(&mut self) -> Option<Vec<u8>> {
        let player = self.local_player;
        self.with_link(|link| link.side(player).frame())
    }

    /// One seat's display, RGBA8, for a host showing more than the
    /// local side. `None` unless [`render_seats`](Self::render_seats)
    /// asked for that seat to be drawn.
    pub fn seat_frame(&mut self, player: usize) -> Option<Vec<u8>> {
        self.with_link(|link| link.side(player).frame())
    }

    /// Draw every seat, not just the local one.
    pub fn render_seats(&mut self) {
        for seat in &*self.visible {
            seat.store(true, Ordering::Relaxed);
        }
        self.with_link(|link| {
            for player in 0..2 {
                link.side(player).set_render(true);
            }
        });
    }

    /// A handle onto whichever seat `seat` currently names, for a host
    /// reading a console off the simulation thread — its audio, mainly.
    /// `seat` is read per call, so a host that moves the sound between
    /// seats (training's side swap) does so without whatever it built
    /// on this being rebuilt under it.
    pub fn side_source(&self, seat: Arc<std::sync::atomic::AtomicUsize>) -> Box<dyn crate::SideSource> {
        Box::new(LinkSeat {
            link: self.link.clone(),
            player: seat,
        })
    }

    /// The telemetry this match publishes, if its engine reads any.
    pub fn telemetry(&self) -> Option<&crate::telemetry::TelemetryHandle> {
        self.telemetry.as_ref()
    }

    /// Confirmed `(tick, [p0, p1])` input rows in order, for the
    /// replay sink. Ticks are 1-based: the row that produced simulated
    /// tick `t` is stamped `t`, so a tick's confirmed inputs and its
    /// telemetry line up exactly.
    pub fn drain_confirmed(&mut self) -> Vec<(u32, [HostInput; 2])> {
        let mut confirmed = self.confirmed.lock().unwrap();
        let out = confirmed
            .drain(..)
            .enumerate()
            .map(|(i, row)| (self.drained + i as u32 + 1, row))
            .collect::<Vec<_>>();
        self.drained += out.len() as u32;
        out
    }

    /// Ticks below this can never be rolled back again — what
    /// telemetry may safely be folded up to.
    pub fn confirmed(&self) -> u32 {
        self.inner.local_frontier() - self.inner.local_queue_length() as u32
    }

    /// Run `f` against the live pair — video, audio and RAM readout.
    /// The pair is parked at the newest simulated tick. Do not tick or
    /// restore it behind the session's back.
    pub fn with_link<R>(&self, f: impl FnOnce(&mut dyn Link) -> R) -> R {
        f(&mut *self.link.lock().unwrap())
    }

    pub fn local_player(&self) -> usize {
        self.local_player
    }

    /// Clock-sync skew for the host's throttler; read before
    /// [`advance`](Self::advance).
    pub fn skew(&self) -> i32 {
        self.inner.skew()
    }

    /// How far speculation currently runs past what is settled, for the
    /// same governor.
    pub fn speculation_balance(&self) -> i32 {
        self.inner.speculation_balance()
    }

    /// Local inputs waiting on the peer. A full queue is what stalls a
    /// session when the peer goes quiet.
    pub fn local_queue_length(&self) -> usize {
        self.inner.local_queue_length()
    }

    /// Ticks the next [`advance`](Self::advance) could settle from
    /// input already buffered — nonzero means advancing drains the
    /// queue rather than only growing it.
    pub fn matchable(&self) -> usize {
        self.inner.matchable()
    }

    /// Ticks the host presents behind the simulation frontier.
    pub fn present_delay(&self) -> u32 {
        self.inner.present_delay()
    }

    pub fn set_present_delay(&mut self, present_delay: u32) {
        self.inner.set_present_delay(present_delay);
    }

    /// How deep the last advance's rollback went, 0 if none.
    pub fn last_rollback_depth(&self) -> u32 {
        self.last_rollback_depth
    }
}

/// One seat of the match's pair, as [`Match::side_source`] hands it
/// out: the same shared link the session ticks, reached under the same
/// lock.
struct LinkSeat {
    link: Arc<Mutex<dyn Link>>,
    player: Arc<std::sync::atomic::AtomicUsize>,
}

impl crate::SideSource for LinkSeat {
    fn with_side(&self, f: &mut dyn FnMut(&mut dyn crate::Side)) {
        let mut link = self.link.lock().unwrap();
        f(&mut *link.side(self.player.load(Ordering::Relaxed)));
    }

    fn try_side(&self, f: &mut dyn FnMut(&mut dyn crate::Side)) -> bool {
        match self.link.try_lock() {
            Ok(mut link) => {
                f(&mut *link.side(self.player.load(Ordering::Relaxed)));
                true
            }
            Err(_) => false,
        }
    }
}

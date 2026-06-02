# getgud

A small, engine-agnostic **rollback netcode** core for two participants (a *local*
player and one *remote* peer). `getgud` owns the hard parts of deterministic
lockstep-with-prediction — input matching, confirmed-state checkpointing,
speculative re-simulation, and time synchronization — and leaves the game
itself entirely to you through a handful of traits.

It does **not** do any networking, threading, rendering, or timekeeping. You
feed it local inputs and the remote inputs that arrive off your transport; it
tells you which world state to draw this frame and the clock skew to throttle
against to stay in sync with the peer.

> Status: `0.1.0`, early. The crate is extracted from a larger workspace and
> inherits `[lints] workspace = true`, so it only builds inside that workspace
> today. There are no tests yet.

---

## The problem it solves

In a real-time game played over the network, each client only knows its **own**
inputs immediately; the peer's inputs arrive a few frames late. You have two bad
choices and one good one:

- **Wait** for the remote input every frame → the game stutters with latency.
- **Guess** the remote input, simulate ahead, and **correct** when the truth
  arrives → smooth gameplay, at the cost of occasionally re-simulating a few
  frames. This is *rollback*, and it's what `getgud` implements.

The core idea: keep a **confirmed checkpoint** built only from inputs both sides
agree on, and every displayed frame is a *throwaway* re-simulation from that
checkpoint forward — predicting the remote's inputs for the frames it hasn't
sent yet. When the real remote inputs arrive, they fold into the checkpoint and
the next frame re-predicts from the new, more-advanced truth.

---

## Mental model

Everything is indexed by an integer **tick** (one simulation step).

| Term | Meaning |
| --- | --- |
| **frontier** | The local wall-clock tick counter. Bumped once per frame via `advance_frontier()`. The local side is "here." |
| **frame delay** | How many ticks the display lags the frontier, `target = frontier - frame_delay`. A small input-buffer that trades responsiveness for fewer rollbacks. |
| **commit frontier** | How many input *pairs* (local **and** remote both known) have been matched so far. Confirmed history can never extend past this. |
| **settled snapshot** | The authoritative checkpoint: world state built purely from confirmed pairs, never from predictions. |
| **speculative tail** | The throwaway re-sim from the checkpoint up to the display target, using predicted remote inputs. This is what the player sees; it's recomputed every frame and never kept. |
| **frame advantage** | How many ticks ahead the local side is versus the remote inputs it has received. Drives time-sync. |

The invariant that makes it safe: **the checkpoint only ever absorbs confirmed
inputs.** Predictions live only in the speculative tail, which is discarded and
rebuilt each frame, so a wrong guess can never corrupt authoritative state — it
just produces one slightly-off displayed frame that self-corrects next tick.

---

## Architecture

```
        local input                          remote packet
            │                                      │
            ▼                                      ▼
   Session::advance(presenter, in)      Session::add_remote_input(in, adv)
            │                                      │
            ▼                                      ▼
        ┌──────────────────── Queue ────────────────────┐
        │   local_queue                 remote_queue     │
        └───────────────────┬────────────────────────────┘
                            │  matched (local, remote)
                            ▼
                      settle_backlog
                            │
                            ▼
        Simulator::simulate(.., speculative = false)  ── authoritative
                            │
                            ▼
                     settled_snapshot ───────► CommitObserver::on_commit  (e.g. replay)
                            │
              checkpoint    │   + peeked local inputs ahead of remote
                            ▼
        Simulator::simulate(.., speculative = true)  ◄── Predictor::predict(remote)
                            │                              (throwaway)
                            ▼
                  Presenter::present(state, tick, skew)
```

`Queue`, `settle_backlog`, and `settled_snapshot` are internal to `Session`. The
four boxes you implement are the trait objects on the edges: `Simulator`,
`Predictor`, `Presenter`, and (optionally) `CommitObserver`, all parameterized by
your `World`.

---

## What you implement

### `World` — your game's type contract

```rust
pub trait World: Sized + 'static {
    type Input: Clone + Send + 'static;   // one participant's input for one tick
    type State: Send + 'static;           // a complete, restorable world state
    type Error: Send + 'static;           // your simulation error type
}
```

`State` must be a *complete* snapshot — the simulator restores from it, so it has
to capture everything the simulation reads. `Input` is **one** player's input for
**one** tick; the session pairs the local and remote ones.

### `Simulator` — advance the world

```rust
pub trait Simulator<W: World>: Send {
    fn simulate(
        &mut self,
        base: &Snapshot<W>,            // start here (base.tick)
        inputs: Vec<(W::Input, W::Input)>,   // local+remote per tick
        speculative: bool,             // true = throwaway tail, false = authoritative commit
    ) -> Result<SimResult<W>, W::Error>;
}
```

The contract — read this carefully, it's the subtle one:

- Apply the **first `inputs.len() - 1`** pairs, advancing the state one tick each.
- The **last** pair is *peeked* at the capture tick: it is the input sampled
  **at** the resulting snapshot's tick, not yet integrated. Use it if your
  snapshot needs the about-to-be-applied input baked in; otherwise just hold it.
- Return a `Snapshot` whose `tick == base.tick + inputs.len() - 1`.
- The session re-supplies that same final pair as `inputs[0]` of the next
  segment, so **do not commit it twice.**
- `speculative` lets you skip work that only matters for confirmed state (audio,
  particles, anything observer-visible) on throwaway tail re-sims.

`SimResult { snapshot, commit_before }`: set `commit_before = Some(end_tick)`
once the world reaches a terminal state (round over) at `end_tick`; the session
then stops reporting committed inputs at or past that tick (so replays aren't
recorded into the few ticks the sim may overshoot the end by). Leave it `None`
while the world is live.

### `Predictor` — guess the remote input

```rust
pub trait Predictor<W: World>: Send + Sync {
    fn predict(&self, last_remote: &W::Input) -> W::Input;
}
```

Called to fill in remote inputs for the speculative tail. The classic, hard-to-
beat strategy is *"the remote will keep doing what it last did"* — just clone
`last_remote`. The session holds the prediction constant across the whole tail.

### `Presenter` — receive the frame to draw

```rust
pub trait Presenter<W: World> {
    fn present(&mut self, state: &W::State, tick: u32, skew: i32);
}
```

`present` hands you the world state to render this frame, the tick it
represents, and `skew` — the raw time-sync signal in frames
(`local_frame_advantage - remote_frame_advantage`). A positive skew means you're
running ahead and should slow down to let the peer catch up; the engine doesn't
act on it, so feed it to your own throttle and turn that into a frame-rate
target. (See *Time synchronization*.)

### `CommitObserver` — optional, observe confirmed history

```rust
pub trait CommitObserver<W: World>: Send {
    fn on_commit(&mut self, tick: u32, pair: &(W::Input, W::Input));
}
```

Fires once per input pair as it becomes confirmed, in tick order — the natural
hook for replay recording, rollback metrics, or desync hashing. Only confirmed
pairs are reported, never predictions.

---

## Driving a session

```rust
use std::sync::Arc;
use getgud::{Session, SessionParams};

// 1. Construct.
let mut session = Session::new(SessionParams {
    frame_delay: 2,                       // display 2 ticks behind the frontier
    initial_remote,                       // seed for the first prediction
    simulator: Box::new(MySimulator::new()),
    predictor: Arc::new(RepeatLastPredictor),
    observer: Some(Box::new(MyReplayRecorder::new())),
});

// 2. Seed the first confirmed state (tick 0) before advancing.
session.set_first_settled_state(initial_world_state);

// 3. Off your transport, whenever a remote input arrives:
//    `frame_advantage` is the peer's reported lead, used for time-sync.
session.add_remote_input(remote_input, frame_advantage);

// 4. Once per rendered frame:
session.advance_frontier();                       // bump the wall clock
session.advance(&mut presenter, local_input)?;    // matches, settles, predicts, presents
```

Inside `advance`, the session: appends the local input; matches any newly
pairable local+remote inputs onto the commit chain; settles the checkpoint
forward as far as confirmed inputs allow; if the display target is still ahead
of the checkpoint, runs a speculative tail with predicted remotes and presents
that; otherwise presents the checkpoint directly. It hands the presenter the
current time-sync skew along with the state.

Send the local frame advantage you report to your peer with
`session.local_frame_advantage()` so *their* throttle can do the same.

### Inspecting the session

| Method | Returns |
| --- | --- |
| `frontier()` / `presented_tick()` | Wall-clock tick / tick actually drawn last frame |
| `frame_delay()` / `set_frame_delay(n)` | Read / live-adjust the display lag |
| `has_committed_state()` | Whether the first settled state has been set |
| `local_queue_length()` / `remote_queue_length()` | Pending inputs each side |
| `speculative_depth()` | Local frames currently running on predicted remotes |
| `local_frame_advantage()` | Local lead, to send to the peer |
| `last_remote_frame_advantage()` | Last lead the peer reported |

---

## Time synchronization

Two clients drifting apart desync rollback. The fix is to slow down whichever
side is **ahead**. Each frame the engine computes

```
skew = local_frame_advantage - last_remote_frame_advantage
```

and hands it to your `Presenter::present`. The engine itself takes no action on
it — *you* decide how to throttle. A simple, effective policy (what the host
this crate was extracted from uses) is an **asymmetric EMA**: ramp a slowdown in
slowly (τ ≈ 5 s at 60 Hz) but back it off quickly (τ ≈ 0.5 s), clamped to
`[0, 30]` fps below your base rate. The asymmetry means the leading client eases
off the gas gently and recovers full speed promptly once the peer catches up —
avoiding oscillation.

---

## Why "throwaway" re-simulation, every frame?

Classic GGPO keeps the predicted state and only rolls back when a prediction is
proven wrong. `getgud` instead rebuilds the displayed tail from the confirmed
checkpoint *every* frame. It's simpler to reason about — there is exactly one
authoritative state and one disposable view of it, and a wrong prediction has no
special "rollback" path; it's just a tail that gets recomputed next frame like
every other. The cost is re-simulating the speculative depth each frame, which
is bounded by how far local input runs ahead of confirmed remote input
(`speculative_depth()`), typically a handful of ticks.

---

## License

Not yet specified.

# getgud

An engine-agnostic **rollback netcode** core for two participants — a local player and one remote peer.

`getgud` owns input matching, confirmed-state checkpointing, speculative re-simulation, and time synchronization. It does **no** networking, threading, rendering, or timekeeping. You feed it your local inputs and the remote inputs that arrive off your transport, and it tells you:

- which world state to draw this tick, and
- the clock skew to throttle against to stay in sync with the peer.

Everything game-specific lives behind a handful of traits; the engine treats your world state as an opaque, restorable blob.

## The model

Everything is indexed by an integer **tick** (one simulation step).

The session holds an authoritative *settled* checkpoint built purely from confirmed input pairs. Each displayed tick is a *throwaway* re-simulation from that checkpoint forward, predicting the remote's not-yet-received inputs:

```
settled checkpoint            speculative tail (disposable)
[ confirmed pairs ] ─────────► [ predicted remotes ] ──► displayed state
        ▲                                                     ▲
   folds in confirmed inputs                          recomputed every tick
```

Confirmed inputs fold into the checkpoint; predictions live only in the disposable tail. A wrong guess can never corrupt authoritative state — the tail is thrown away and rebuilt each tick, and the checkpoint only ever absorbs inputs that have been confirmed by both sides.

The display lags the local frontier by a configurable `present_delay` (a small input buffer): larger values mean fewer rollbacks but more input latency.

## What you supply

You parameterize the engine over a [`World`] — a marker type wiring three associated types to your game:

| Associated type | Meaning | Bounds |
| --- | --- | --- |
| `Input` | one participant's input for one tick | `Clone + Send` |
| `State` | a complete, **restorable** world state | `Send` |
| `Error` | what a simulation step can fail with | `Send` |

> `State` must capture *everything* the simulation reads — anything omitted will desync on rollback.

Then you provide three behaviors:

- **`Simulator`** — advances the world by a list of `(local, remote)` input pairs. Called for both authoritative commits and throwaway tails; a `speculative: bool` flag lets you skip work that only matters for confirmed state (audio, particles, observer-visible side effects).
- **`Predictor`** — guesses a remote input from the last confirmed one. Cloning the last remote ("the peer keeps doing what it last did") is the usual, hard-to-beat strategy.
- **`CommitObserver`** *(optional)* — fired once per confirmed input pair, in tick order. The natural place for replay recording, rollback metrics, or desync hashing. Predictions are never reported — only confirmed history.

### The Simulator contract

`simulate(base, committed, peeked, speculative)` advances the world from `base` by every pair in `committed`, then samples `peeked` at the resulting tick without integrating it:

- Apply **all** of `committed`, advancing one tick per pair.
- Return a snapshot whose `tick == base.tick + committed.len()`.
- `peeked` is the input sampled *at* that snapshot tick. A simulator whose state is a clean inter-tick value can ignore it — the session re-supplies it as `committed[0]` of the next call, integrated there exactly once. It exists for engines that must bake the boundary tick's input into an opaque snapshot up front (e.g. priming an input register a resume will read).

`SimResult::commit_before` lets the simulator report a terminal tick (e.g. a round ending); the session then stops reporting committed inputs at or past that tick, so replays aren't recorded into the few ticks a simulator may overshoot the end by.

## Driving a session

1. Construct with [`Session::new`], passing the tick-0 world state as `SessionParams::initial_state`. The settled checkpoint is seeded immediately — there is no separate "seed me later" step.
2. Call [`advance(local_input)`](Session::advance) **once per rendered tick**. This call *is* the per-tick wall clock — it advances the frontier itself. It returns a [`Frame`]:
   - `frame.tick` — the simulation tick `state` represents (read it; don't recompute — it's clamped early in a match and when `present_delay` changes live).
   - `frame.state` — the world state to draw (a borrow valid only until the next `advance`; don't retain it).
   - `frame.skew` — the time-sync skew in ticks to feed your own throttle (see below).
3. Feed remote inputs in as they arrive with [`add_remote_input(input, tick_advantage)`](Session::add_remote_input).

### Time synchronization

Each tick, send the peer your `local_tick_advantage()` — how far your frontier leads the remote inputs you've received. The peer reports its own advantage back with each remote input (the `tick_advantage` argument to `add_remote_input`).

`Frame::skew` is `local_tick_advantage - last_remote_tick_advantage`. Both advantages carry the symmetric network delay, so their difference isolates the real clock skew. **Positive means you're running ahead and should ease off.** Feed it to your throttle (e.g. lower your frame-rate target); the engine itself never sleeps or slows.

## Sketch

```rust
struct MyGame;
impl getgud::World for MyGame {
    type Input = Buttons;
    type State = GameState;
    type Error = anyhow::Error;
}

// impl getgud::Simulator<MyGame>, Predictor<MyGame>, [CommitObserver<MyGame>] ...

let mut session = getgud::Session::new(getgud::SessionParams {
    present_delay: 2,
    initial_remote: Buttons::default(),
    initial_state: initial_game_state,
    simulator: Box::new(my_simulator),
    predictor: Arc::new(my_predictor),
    observer: Some(Box::new(my_replay_recorder)),
});

// off the network, whenever a packet lands:
session.add_remote_input(remote_buttons, peer_reported_advantage);

// once per rendered tick:
send_to_peer(local_buttons, session.local_tick_advantage());
let frame = session.advance(local_buttons)?;
draw(frame.state);
throttle.apply(frame.skew);
```

For a complete real-world adapter — driving an mGBA core over Game Boy Advance link-cable battles, deriving the opponent's packets via a co-simulated shadow for settles and a per-game predictor for speculative tails — see the sibling `tango-pvp` crate (`battle::world` and `battle::round`).

## Inspecting state

`Session` exposes read-only counters useful for diagnostics and flow control:

- `frontier()` — the local wall-clock tick counter.
- `present_delay()` / `set_present_delay()` — read or adjust the display lag live.
- `local_queue_length()` / `remote_queue_length()` — unmatched inputs on each side.
- `speculative_depth()` — local ticks currently running against a predicted remote.
- `local_tick_advantage()` / `last_remote_tick_advantage()` — the two halves of the skew.

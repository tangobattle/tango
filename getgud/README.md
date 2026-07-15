# getgud

A small, dependency-free **rollback netcode** core for deterministic games with
one local player and any number of remote peers, in Rust.

It handles the hard part of peer-to-peer netcode: confirming the input rows every
peer agrees on, predicting the remote inputs that haven't arrived yet, correcting
those predictions once the real inputs land, and producing a clock-sync signal so
the peers keep their simulations aligned. The crate contains no game logic —
you bring the simulation.

## API

You implement one trait — `World` — on the type that owns your live simulation,
then drive a `Session` once per tick.

### `World`

A `World` is a *live* simulation parked at one tick, which the session can step,
snapshot, and restore. It names the three types the crate is generic over
(`Input`, `State`, `Error`) and supplies five methods:

| Member                            | Responsibility                                                                                       |
|-----------------------------------|------------------------------------------------------------------------------------------------------|
| `Input` / `State` / `Error`       | Your per-tick input, snapshot, and error types. Use `Infallible` for `Error` if stepping can't fail. |
| `step(&local, &remotes)`          | Step the live simulation one tick from where it's parked, applying the input row (`remotes` indexed by remote slot). **Must be deterministic.** |
| `save() -> State`                 | Snapshot the live simulation at the tick it's parked at. The session keeps snapshots to present, to promote a correct prediction without re-simulating, and to reload on rollback. |
| `load(&State)`                    | Restore the live simulation to a snapshot, parking it at that tick. Called to rewind before re-simulating a mispredicted tail. |
| `predict(&last_remote) -> Input`  | Guess a remote's next input from its last confirmed one (e.g. "repeat the last input"). Applied per remote slot, only where the real input hasn't arrived. |
| `log(&local, &remotes)`           | Receive each confirmed input row, in tick order — for replays, spectating, or desync checks. Leave the body empty to ignore it. |

`step` doesn't return a snapshot: the snapshot comes from `save`, called only
when the session actually needs to keep that tick. That split lets a rollback
re-`step` through a corrected tail and `save` just its final state, instead of
snapshotting every intermediate tick.

`Input` must be `Clone + PartialEq` (predictions are compared against the real
inputs to decide promote-vs-rollback); `State` and `Error` must be `Send +
'static`.

Determinism is the one hard requirement: the same snapshot loaded and stepped by
the same input row must always produce the same next state. Rollback depends on
it.

## Operation

Each peer constructs one `Session` from
`SessionParams { present_delay, initial_remotes, initial_state, world }` (the
length of `initial_remotes` fixes the remote count). The session runs for as
long as the host drives it — there is no end-of-round signal; tear it down when
the match is over. The per-tick loop:

```text
loop each tick:
    while a remote packet arrived:
        session.add_remote_input(slot, remote_input, their_tick_advantage)
    adjust_clock(session.skew());        // stall a frame when running ahead
    let frame = session.advance(local_input)?;
    render(frame.state);                 // also available: frame.tick, frame.local, frame.remotes
```

`advance` enqueues the local input, confirms every tick all peers now agree on,
advances the displayed state, and returns a `Frame { tick, state, local, remotes }`.
The clock-sync hint is read *separately* via `session.skew()` — and read it
*before* `advance`, which enqueues this tick's local input.

### Key terms

- **Local frontier** — the newest local tick; `advance` bumps it by one.
- **Present delay** — how many ticks behind the frontier you display
  (`target = frontier − present_delay`). Larger = less prediction but more input
  latency; smaller = snappier but speculates further. Tunable at runtime via
  `set_present_delay`.
- **Settled state** — the authoritative state, folded only from confirmed input
  rows (every player's input present). Rows are handed to `log` in tick order as
  they settle.
- **Speculation** — when the display target runs past the last confirmed tick, the
  session steps forward from the settled state with real local inputs and, per
  remote slot, the real input where it has already arrived (a tick can be
  unconfirmed because a *different* remote is still missing) or a
  `predict`-supplied one where it hasn't, `save`-ing each speculative snapshot
  into a rolling buffer.
- **Promote vs. rollback** — when the outstanding real inputs arrive, the session
  compares them against what it speculated with. The matching prefix is *promoted*
  to settled with no re-simulation (those snapshots are already byte-exact); only
  from the first misprediction on does it `load` and re-`step` the corrected
  inputs. `last_misprediction_depth()` reports how deep the last rollback went.
- **Skew** — clock sync. Each peer reports how far its local input leads the
  furthest-behind remote input it has received (`local_tick_advantage`), ships
  that to every peer alongside each input, and gets each peer's value back through
  `add_remote_input`. `skew()` is the worst case over the remotes of the
  difference; positive means you're ahead of someone — stall a frame to converge.

## Prediction vs. delay

Whether the session speculates depends on how far confirmed input has progressed
relative to the displayed tick. Both diagrams share `frontier` 9 and
`present_delay` 3 (so `target` 6), differing only in how much remote input has
arrived.

**Prediction regime** — confirmed input lags the present, so `target` sits past
the last confirmed tick and the session speculates the gap:

```text
 tick  0   1   2   3   4   5   6   7   8   9
       ●───●───●───●───○───○───○───◌───◌───◌
                   │           │           │
                   │           │           └─ frontier (newest local tick)
                   │           └─ target = frontier − present_delay
                   │                (the frame you render)
                   └─ last tick confirmed by every peer (settled)

   ●  confirmed  — real local + real remotes, folded into settled state
   ○  speculated — real local + arrived-or-predicted remotes, simulated up to
                   the target (later promoted if the predictions held, else
                   rolled back)
   ◌  buffered   — local input entered, still past the target, not yet simulated
```

**Delay regime** — confirmed input has caught up, so `target` is at or behind the
last confirmed tick and the rendered frame is already confirmed; no prediction
runs. A large enough `present_delay` (or low latency) keeps you here:

```text
 tick  0   1   2   3   4   5   6   7   8   9
       ●───●───●───●───●───●───●───●───◌───◌
                               │   │       │
                               │   │       └─ frontier (newest local tick)
                               │   └─ last confirmed tick
                               └─ target — confirmed frame you render

   ●  confirmed — real local + real remotes (settled state)
   ◌  buffered  — local input entered, awaiting a remote
```

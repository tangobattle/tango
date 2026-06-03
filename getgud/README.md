# getgud

A small, dependency-free **rollback netcode** core for two-player deterministic
games, in Rust.

It handles the hard part of peer-to-peer netcode: confirming inputs, predicting
the ones that haven't arrived, correcting mispredictions, and keeping the two
peers' clocks in sync.

## API

The crate is generic over a `World` you define.

| Trait                 | Responsibility                                                  |
|-----------------------|----------------------------------------------------------------|
| `World`               | Names your `Input`, `State`, and `Error` types.                |
| `Simulator`           | Advances `State` by applying input pairs — **deterministically**. |
| `Predictor`           | Guesses the remote player's next input from their last one.     |
| `Logger` *(optional)* | Receives confirmed input pairs (replays, spectators, desync checks). Use `NullLogger` to skip. |

Determinism is the one hard requirement: identical inputs on identical state must
yield identical state: rollback depends on it.

## Operation

Each peer runs a `Session`. Every tick you feed it the local input and any remote
inputs that have arrived; it returns a `Frame` to render plus a `skew` for clock
sync. Key terms:

- **Frontier** — the newest local tick (`Session::advance` advances it).
- **Present delay** — how many ticks behind the frontier you display. Larger =
  less prediction, more latency; smaller = snappier, more speculation. Tunable at
  runtime.
- **Settled state** — authoritative state built only from confirmed
  `(local, remote)` pairs; confirmed inputs are handed to the `Logger`.
- **Speculative tail** — when the presented tick runs past confirmed input, the
  session simulates forward with *predicted* remote inputs. It is rebuilt from the
  settled state each tick, so mispredictions self-correct — no manual rollback.
- **Skew** — each peer reports how far its frontier leads the remote input it has
  received; the difference is the `skew` in every `Frame`. Positive means you're
  ahead — stall a frame to converge.

Whether the session predicts depends on how far confirmed input has progressed
relative to the presented tick. Both diagrams share `frontier` 9 and
`present_delay` 3 (so `target` 6), differing only in how much remote input has
arrived.

**Prediction regime** — confirmed input lags the present, so `target` sits past
the settled cap and the session speculates the gap:

```text
 tick  0   1   2   3   4   5   6   7   8   9
       ●───●───●───●───○───○───○───◌───◌───◌
                   │           │           │
                   │           │           └─ frontier (newest local tick)
                   │           └─ target = frontier - present_delay
                   │                (the frame you render)
                   └─ settled cap (last tick confirmed by both)

   ●  confirmed  — real local + real remote, folded into settled state
   ○  speculated — real local + predicted remote (rebuilt every tick)
   ◌  buffered   — local input entered, not yet presented (= present_delay)
```

**Delay regime** — confirmed input has caught up, so `target` is at or behind the
settled cap and the rendered frame is already confirmed; no `Predictor` runs. A
large enough `present_delay` (or low latency) keeps you here:

```text
 tick  0   1   2   3   4   5   6   7   8   9
       ●───●───●───●───●───●───●───●───◌───◌
                               │   │       │
                               │   │       └─ frontier (newest local tick)
                               │   └─ last confirmed tick
                               └─ target (= settled cap) — confirmed frame you render

   ●  confirmed — real local + real remote (settled state)
   ◌  buffered  — local input entered, not yet presented
```

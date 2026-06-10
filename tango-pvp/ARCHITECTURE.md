# tango-pvp architecture

tango-pvp is the rollback-netplay engine for the Mega Man Battle Network
games: it runs the live game, co-simulates the opponent, re-simulates ticks
for rollback, and records/plays back replays. This document is the map; the
module docs hold the details.

## Glossary

| Term | Meaning |
| --- | --- |
| **trap** | A callback registered on a ROM program-counter address. When an emulated core reaches that PC, the closure runs. All engine↔game interaction happens through traps — control is inverted, the game drives us. |
| **primary** | The live, visible core the player plays on. Its traps talk to the `Match`/`Round`. |
| **shadow** | A second, headless core running the *opponent's* ROM + save locally. It answers "what link packet would the opponent's game have produced at tick T?" so the engine never waits on the network for packets — only for joyflags. |
| **stepper** | The headless re-sim core. In live play it advances one tick per `Stepper::step` for the rollback engine; in replay mode it plays recorded rounds from boot. |
| **munger** | A game's RAM/register poking layer ("munges" memory). Each game has its own, driven by its offsets table. |
| **offsets** | Per-ROM-version tables of code addresses (trap PCs) and EWRAM addresses (munger targets). Pure data. |
| **offerer** | The peer that initiated the connection. Used only to break symmetry (player-index pick, per-side RNG draws). |
| **joyflags** | The GBA's 10-bit pad state. The only thing that crosses the wire per frame. |
| **packet** | The 16-byte link-cable exchange blob the games trade each tick. Never sent over the network — always derived by co-simulating the shadow. |
| **commit / first commit** | The synchronization point at the start of each round where both sides' cores are in a known identical state: RNG is seeded, tick anchored to 0, and a snapshot taken. Everything rollback-related is relative to this. |
| **settle / speculate** | getgud (the generic rollback engine) settles ticks with confirmed remote inputs and speculates ahead with predicted ones; a misprediction rolls back to the settled state and re-simulates. |
| **golden suite** | `tests/golden/`: committed replays + determinism fingerprints, replayed end-to-end against real ROMs. The safety net for everything on the replay path. |

## The three drive loops

Each emulated core has a loop that runs it and a set of traps that talk back
to engine state. The engine state each loop shares with its traps is an
explicit state machine, not loose flags:

```
LIVE PVP
                          mgba emulator thread
  ┌──────────────────────────────────────────────────────────────┐
  │ primary core ──trap──► Match / Round                         │
  │   main_read_joyflags:                                        │
  │     first fire: seed RNG, record_first_commit ───────────────┼──► Shadow.advance_until_first_committed_state
  │     every fire: Round.add_local_input_and_fastforward        │
  │        ├─ send local joyflags ──────────────► network        │
  │        ├─ getgud Session.advance                             │
  │        │    └─ MgbaWorld.step ──► Stepper.step (re-sim core) │
  │        │         └─ stepper trap ──► RemotePacketSource ─────┼──► Shadow.apply_input (co-sim core)
  │        └─ load chosen state into primary, throttle fps       │
  │   round_set_ending / round_ending_entry:                     │
  │     Match.end_round ─────────────────────────────────────────┼──► Shadow.advance_until_round_end
  └──────────────────────────────────────────────────────────────┘
                          tokio runtime
  ┌──────────────────────────────────────────────────────────────┐
  │ Match.run: receive remote Input/EndOfRound, tag with round   │
  │ index, hold until the Round is ready, attach to its queue    │
  └──────────────────────────────────────────────────────────────┘

REPLAY (playback / export / eval / golden)
  ┌──────────────────────────────────────────────────────────────┐
  │ playback core ──stepper traps──► stepper::State (PlaybackPhase)
  │     per tick: pop recorded input pair, resolve remote packet ┼──► Shadow.apply_input
  │ prefetch std::thread: second core racing ahead, pushing      │
  │     ReplaySnapshots (stepper checkpoint + both core states)  │
  └──────────────────────────────────────────────────────────────┘
```

Key inversion to keep in mind: **drive loops poll, traps transition.** A
drive method like `Shadow::apply_input` queues a request into the state
machine, runs `core.run_loop()`, and polls for the completion transition;
the per-game traps perform the transitions as the game executes.

## The state machines

- **`shadow::round::Exchange`** (`Idle → Queued → (taken) → Applied`) — one
  link exchange through the shadow core, from `apply_input` queueing a tick's
  input to the per-game traps consuming it and buffering the next remote
  packet. The shadow's mutable state (round, RNG, the `input_applied`
  completion wakeup) lives behind one lock; the trap error channel sits
  outside it so traps can report errors while holding `&mut Round` borrows.
  `pending_remote_packet` is a one-tick pipeline register: the packet
  returned for tick T was buffered during tick T−1's run.

- **`stepper::state::PlaybackPhase`** (`AwaitingRoundStart →
  AwaitingFirstCommit → InRound → RoundEnding → RoundEnded → {next round |
  Finished}`) — replay playback's per-round lifecycle. Shadow advances fire
  on the edges (which absorbs BN1/BN2's double round-ending fires).
  `Finished` is the "replay exhausted but the game marches on" terminal.

- **`stepper::state::Mode`** (`Replay(ReplayExtras)` /
  `Fastforward(FastforwardExtras)`) — the stepper's two jobs. Replay carries
  the phase machine above; Fastforward carries the capture boundary
  (`capture_tick`) and its own small ending tracker. The shared simulation
  core (current tick, input window, link-exchange bookkeeping, outcome,
  error channel) lives once in `InnerState`.

- **`battle::round::RoundStage`** (`Armed → Running`) — a live round exists
  from `round_start_ret`, but the rollback engine can only be built at first
  commit (it must be seeded with that state).

- **`hooks::MatchHandle`** — the slot primary traps reach the `Match`
  through. Traps `get()` a clone of the `Arc` under a momentary read lock
  and hold no lock for their body. The slot has two faces, enforced by
  visibility: `get()`/`is_set()` are crate-private and yield a `TrapMatch`
  view (rng, round state, round lifecycle, cancel — nothing else), so the
  host can't reach trap API; the host's face is `set`/`clear`/
  `round_metrics()`, and `Match`'s remaining `pub` surface is host
  lifecycle only (`new`/`run`/`cancel`/`cancelled`/`finish_replay`).
  `Round` is entirely crate-internal.

## Determinism invariants

- The replay on-disk format (`TOOT`, version 0x1B) is frozen.
- RNG draw order is sacred: both peers (and replay playback) must consume
  the shared RNG stream in exactly the same sequence. `Match::new`'s
  player-index pick burns one bool draw; `State::new` and
  `Shadow::new_for_replay` replicate it.
- Cross-trap pipeline registers are verified, not assumed: the stepper's
  `LocalPacket` carries a send-count (`output_pairs.len()`, *not* the tick —
  BN3 decouples them) and the shadow's `RemotePacket` carries a target tick;
  consumers check both and surface mismatches as errors.
- Boundary parking is byte-exact: drive loops return with the core halted at
  a `main_read_joyflags` (`end_run_loop`), so a snapshot taken afterward
  equals one taken inside the trap.

## Per-game code

Each game (bn1–bn6, exe45) keeps its **own complete trap set** under
`src/game/<game>/` — `offsets` (addresses), `munger` (RAM pokes), `common` /
`primary` / `shadow` / `stepper` (trap registrations), `rng`. The games are
deliberately *not* unified: their battle loops genuinely differ (BN1/2:
single RNG, two round-ending entry points, combined send hooks; BN3: dual
RNG, three send sites, multi-fire jump table; BN4+: dual RNG, chip-select
custom screens, consolidated dispatch; BN4 additionally has `pizzazz.rs`).
The engine accommodates every game's firing order — see the transition
notes on each state machine — rather than forcing games into one shape.

## Error and panic policy

- **Trap context panics abort the process** (the closures run inside
  `extern "C"`); they are reserved for determinism tripwires — the
  tick-mismatch checks that mean the simulation has already diverged and no
  recovery is meaningful.
- Everything else trap-reachable reports instead of panicking: stepper and
  shadow traps push into their state's **error channel** (`set_anyhow_error`
  — `apply_shadow_input` does this internally and returns `None`), which the
  drive loops poll and propagate; primary traps **log + `Match::cancel()`**,
  written down once as the `Match::*_or_cancel` wrappers.

## Test coverage map

| Path | Coverage |
| --- | --- |
| Replay decode, stepper playback, shadow co-sim, per-game stepper/shadow/common traps, mungers, RNG | **Golden suite** (`cargo test -p tango-pvp --test golden`, needs `TANGO_TEST_ROMS_DIR` pointing at `*.gba` ROMs). Fingerprints every shadow packet + final RAM + round outcomes per replay. Treat a fingerprint change as a bug in your change — never re-bless to make a refactor pass. |
| Replay seek/checkpoint (`restore_replay_checkpoint`, prefetch) | **Manual only** — scrub the replay viewer backward across a round boundary and forward past the prefetch frontier. |
| Live PvP (primary traps, `battle::*`, netcode) | **Manual only** — play real matches. Minimum smoke: bn1 (combined send hook, double end-round), bn3 (three send sites), bn6 (tick tripwires); through a round end and a rematch, plus one high-latency run to exercise rollback. |

# tango-match architecture

tango-match is the deterministic match engine for the Mega Man Battle
Network games: both games run locally as a pair of mgba cores linked
through mgba's lockstep SIO driver, and that pair is the rollback unit.
The games speak their *real* link protocol over the emulated cable — no
handshake skips, no packet munging, no shadow co-simulation — so the
only game-specific code is data-side (priming and RAM-poll telemetry),
and it lives outside this crate. This document is the map; the module
docs hold the details.

The crate is deliberately runtime-free: no networking, no tokio, no
threads of its own, no audio output. Hosts (tango-session) drive it in
real time — `advance` once per frame — and everything in here is a
synchronous, reproducible function of its inputs.

## Glossary

| Term | Meaning |
| --- | --- |
| **pair** | The two linked cores ([`mgba_rollback::Link`]). Core `i` runs player `i`'s ROM + save **on both peers** — the pair is symmetric, so both sides simulate the identical match. (mgba-rollback links go to four players; every game tango supports is a two-player link battle, so this engine is two-player throughout.) |
| **session** | `mgba_rollback::session::Session`: the rollback engine over the pair. Settles ticks with confirmed remote inputs, speculates ahead on predicted ones, rolls back and re-simulates on misprediction (getgud underneath). |
| **priming** | Walking a freshly booted pair to its link battle: PC-sited traps at known menu-code anchors poke control state so the games' own boot → comm-menu → battle flow runs itself, pads idle throughout, real link exchanges included. Ends when both games' own battle-start code fires (`PrimedLatch`). |
| **trap** | A callback on a ROM program-counter address (`Link::set_traps`). Used only for priming and for the round-lifecycle anchors — never for netcode. |
| **joyflags** | The GBA's 10-bit pad state. The only thing that crosses the wire (and the only thing replays record) per tick. |
| **confirmed** | Ticks `[0, confirmed)` are settled against real remote input and can never be rolled back. Everything downstream that must be final (replay records, stats, round events) keys off this boundary. |
| **present delay** | How many ticks behind the local input frontier the displayed frame trails. Purely local (each side picks its own); trades displayed-rollback visibility against input latency. |
| **skew / throttler** | Clock-sync: `skew()` reads how far this peer leads; the re-exported `Throttler` tells the host how much fps to shave so the leading side slows instead of the trailing side starving. |
| **telemetry** | Per-tick RAM polls (HP, custom screen, chips) plus trap-driven lifecycle events (round start, match end), recorded eagerly for speculative ticks and truncated again on rollback — what stands is exactly what the current timeline simulated. |

## Layer map

```
mgba (emulator)
  └── mgba-rollback (the pair: lockstep SIO driver, savestates, Session)
        + getgud (generic rollback)
              └── tango-match (this crate: Match, playback, telemetry,
                  analysis, replay format)
                    ▲ implemented by: tango-gamesupport-<game> crates
                    │   (GameSupport: primer traps + telemetry pollers)
                    └ driven by: tango-session (drive threads, netcode,
                        audio routing, replay/stats IO scheduling)
```

## Modules

- **`lib.rs`** — the per-game contract: [`GameSupport`] (primer traps +
  a `CorePoller` per core + patched-ROM-dependent chip/buster
  semantics), `PrimeConfig` (match type, RNG seed derivation, BGM
  silence), `PrimedLatch`. Re-exports the host-facing mgba-rollback
  surface (`Link`, `LinkHandle`, `TickObserver`, `Throttler`).

- **`engine`** — [`Match`]: boots the pair, primes it (bounded by
  `MAX_PRIME_TICKS`), deepens + clears the audio buffers (host-side
  only; sample buffers aren't in savestates), then runs the rollback
  session with the telemetry observer attached. The host calls
  `advance(local_keys)` per frame and gets the outgoing input packet +
  a report; `add_remote_input` feeds the peer's packets in tick order;
  `drain_confirmed` hands the replay sink its final input pairs;
  `checkpoint`/`digest_at` expose settled-state digests for cross-peer
  desync detection; `with_pair`/`pair_handle`/`local_video_buffer` are
  the video/audio readout paths.

- **`playback`** — linear re-simulation of recorded matches:
  [`Playback`] (boot + prime + feed the stream), whole-pair
  [`Snapshot`]s with both framebuffers, the sparse `SnapshotStore`
  (keyframe per `KEYFRAME_INTERVAL`), the dense `RewindRing`
  (`REWIND_FRAMES` behind the playhead, so single-frame back-steps land
  on exact snapshots), the `SeekController` + `run_seek_worker` chase
  (newest target supersedes mid-flight), and `run_prefetch` (a second
  pair racing ahead for keyframes, round marks, and optionally the
  stats analysis). The host owns all the threads; this module provides
  the work they do.

- **`telemetry`** — the observation layer over the pair. Battle values
  are polled from EWRAM after every simulated tick (each core answers
  for its own player where a game only knows its local side); round
  lifecycle is trap-driven off the games' own battle-start/match-end
  code paths (match-end anchors on *both* cores — on a one-sided
  decline only the decliner's game exits). Both kinds revoke cleanly
  under rollback because re-simulation re-fires them identically.

- **`analysis`** — `StatsBuilder`/`MatchStats`: one aggregation path
  for live matches (the session folds each confirmed batch as it
  plays) and offline re-analysis (`analyze` re-simulates a replay on a
  headless pair). The `.stats` sidecar codec lives here
  (`FORMAT_VERSION`, currently v8; readers reject other versions and
  recompute).

- **`replay`** — the on-disk format: `TOOT`, schema [`VERSION`] 0x1C —
  boot configuration + one continuous run of confirmed `[p0, p1]` pair
  ticks with inline round-start markers. Playback is "reboot, re-prime,
  feed the stream verbatim". Trap-engine recordings (0x1B and older)
  are rejected: the engine that played them is gone.

- **`battle`** — `RoundSample`, the per-tick stats sample the
  gamesupport pollers report and the analysis fold consumes.

- **`input`** — `Input` (what replays store) and `JOYFLAGS_MASK`.

## Determinism invariants

- **Both peers build bit-identical pairs.** Same ROMs/saves/RTC/seed in
  the same core order (core 0 always runs player 0's game), and priming
  is a pure function of emulation state + `PrimeConfig` — so both peers
  reach the same pre-session state, and the session starts from
  identical snapshots on both sides.
- **The cart RTC is pinned** to the negotiated match clock on every
  pair (live, playback, analysis) — RTC-reading games (exe45) stay
  deterministic, and replays record the same value as `ts`.
- **RNG reseed is derived, not drawn**: `PrimeConfig::core_rng_seed`
  computes each core's per-stream seeds from the shared match seed —
  identical on both peers, distinct between cores, never zero.
- **Priming never skips the link handshake.** The comm-menu bring-up
  states are where the real handshake happens; jumping them lands in
  the games' "communication failed" path. Pokes are data-side only,
  pads stay idle, and every menu cursor is at its deterministic init
  position.
- **Traps re-fire identically under rollback re-simulation**, so
  telemetry (samples *and* lifecycle events) can be recorded eagerly
  for speculative ticks and truncated on rewind; everything at or
  below `confirmed` is final.
- **The replay stream is the whole match.** No per-round state in the
  format — rounds are re-derived from telemetry (the prefetch pass) or
  the inline markers. Any tick is reachable from the nearest snapshot
  at or before it.
- **Audio bring-up cannot perturb simulation**: sample buffers aren't
  part of savestates. (It's still done identically on both cores so
  the pairs stay configured bit-identically.)

## Error policy

Construction and stepping return `anyhow::Result` — a wedged priming
walk trips `MAX_PRIME_TICKS` instead of hanging, a failed advance
surfaces to the host, which decides teardown policy (tango-session
cancels the match). There are no determinism tripwire panics left in
this crate: cross-peer divergence is *detected* (settled digests via
`checkpoint`/`digest_at`), not asserted.

## Test coverage map

There is no in-crate test suite (the trap-era golden suite is gone).
Verification is recipe-based:

| Path | Coverage |
| --- | --- |
| Live engine, priming, per-game support | Real-game runs via the gamesupport example harnesses (`cargo run -p tango-gamesupport-<game> --example pvp_netplay`, plus the per-game `pvp_probe`/`pvp_ko_probe` probes). Minimum smoke across families: one of bn1–3 (silent-walk priming) and one of bn4+ — through a round end and match end. |
| Playback, seek, snapshots | Manual — scrub the replay viewer backward across a round boundary and forward past the prefetch frontier. |
| Analysis / stats fold | Recompute a known replay's sidecar and compare against the Replays tab's chart; `examples/replay_inspect.rs` dumps a recording. |
| Rollback prediction quality | `examples/predictor-eval.rs` measures rollback counts across a replay corpus — rerun it before changing the input predictor. |
| Cross-peer desync | Runtime detection via settled digests (`checkpoint`/`digest_at`), surfaced by the host. |

# mgba-siolink

Experimental generic rollback netplay over emulated SIO (link cable), built on
[mgba-rs](https://github.com/tangobattle/mgba-rs).

Instead of per-game traps that replace a game's link protocol with memory-level
input exchange, both GBAs run locally as a *pair* connected through mgba's
lockstep SIO driver, and the pair is the rollback unit: the only true inputs
are the two joypads, everything on the wire is derived deterministically. A
netplay session runs the same `Pair` on both peers, feeds confirmed local +
predicted remote keys into `tick`, and restores a `Snapshot` to re-simulate
when a prediction turns out wrong.

- `Pair` — two cores interleaved cooperatively on one thread, snapshotted and
  restored as a unit
- `session` — rollback session over [getgud] (tango's rollback engine):
  repeat-last prediction, speculative snapshots promoted or rolled back as
  confirmations land, a purely local present delay (nothing negotiated),
  tick-advantage clock sync, periodic desync checkpoints
- `throttler` — tango's time-sync throttler (verbatim copy): feeds on the
  session's skew and speculation balance, the leading peer sheds fps until
  the clocks realign
- `replay` — input replay format with deterministic re-simulation
- `testrom` — built-in SIO ping-pong ROM, so tests need no game ROMs

Requires the `mgba-sio-rollback` branch of mgba-rs, which carries the SIO
driver state in the savestate blob, and a sibling checkout of the [tango]
repo for `getgud` (a path dependency — tango's private gamesupport submodule
rules out a git dependency).

[getgud]: https://github.com/tangobattle/tango/tree/main/getgud
[tango]: https://github.com/tangobattle/tango

## Netplay demo

`examples/netplay_demo.rs` is a minimal multiplayer emulator frontend over
tango's real transport stack (tango-rtc, tango-signaling, rennet — the dev
dependencies also take `rennet` from the sibling tango checkout).

```sh
# direct, no server (host listens on a UDP port):
cargo run --release --example netplay_demo -- --host 35835 game.gba --save p1.sav
cargo run --release --example netplay_demo -- --connect 127.0.0.1:35835 game.gba --save p2.sav

# or matched by link code through a signaling server:
cargo run --release --example netplay_demo -- --session some-code game.gba
```

## License

MPL-2.0

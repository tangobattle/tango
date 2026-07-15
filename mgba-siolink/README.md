# mgba-siolink

Experimental generic rollback netplay over emulated SIO (link cable), built on
[mgba-rs](https://github.com/tangobattle/mgba-rs).

Instead of per-game traps that replace a game's link protocol with memory-level
input exchange, all of the GBAs on the cable (two to four) run locally as a
*link* connected through mgba's lockstep SIO driver, and the link is the
rollback unit: the only true inputs are the joypads, everything on the wire is
derived deterministically. A netplay session runs the same `Link` on every
peer, feeds confirmed local + predicted remote keys into `tick`, and restores a
`Snapshot` to re-simulate when a prediction turns out wrong.

- `Link` — two to four cores interleaved cooperatively on one thread,
  snapshotted and restored as a unit
- `session` — rollback session over [getgud] (tango's rollback engine):
  repeat-last prediction per remote, speculative snapshots promoted or rolled
  back as confirmations land, a purely local present delay (nothing
  negotiated), tick-advantage clock sync, periodic desync checkpoints
- `throttler` — tango's time-sync throttler (verbatim copy): feeds on the
  session's skew and speculation balance, the leading peer sheds fps until
  the clocks realign
- `replay` — two-sided input replay format with deterministic re-simulation
  (tango itself is two-player only)
- `testrom` — built-in SIO ping-pong ROM that runs at any player count, so
  tests need no game ROMs

`examples/link_bench.rs` benchmarks tick/snapshot/rollback throughput over the
test ROM or a real game.

[getgud]: https://github.com/tangobattle/tango/tree/main/getgud

## License

MPL-2.0

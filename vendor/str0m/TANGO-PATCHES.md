# Tango patches to str0m

This is a vendored copy of [`str0m` 0.21.0](https://crates.io/crates/str0m)
(MIT OR Apache-2.0, license included), applied over the crates.io version via
`[patch.crates-io]` in the workspace root. Stripped of `tests/`, `examples/`,
`docs/`, `logo/` and the matching manifest sections plus dev-dependencies
(we never build str0m's own tests); `src/` is otherwise byte-identical to the
release except for the marked hunk below. To upgrade: copy the new release
over this directory, re-strip, re-apply.

## The diff against upstream 0.21.0

- `src/sctp/mod.rs`, inbound `DcepOpen` handling
  (`StreamEntryState::AwaitConfig`): call `entry.configure_reliability()`
  after storing the parsed channel config — exactly what the out-of-band
  `open_stream` path in the same file does, and what Pion (this code's
  ancestor) does at the same point. Upstream stores the config for the
  channel *registry* but never applies it to the sctp-proto `Stream`, so the
  **DCEP-receiving side of every in-band channel sends with the stream
  defaults: ordered + fully reliable**, whatever the channel was declared as.
  On an "unordered, unreliable" channel, one lost datagram then
  head-of-line-blocks every later message from that side for a
  retransmission round trip.

  For tango this meant one side of every match (the direct-link host; on the
  matchmaking path whichever peer's offer lost the glare) streamed its
  in-match frames ordered+reliable. Measured with the synthetic skew A/B
  (`tango/src/net/direct_rtc.rs::skew_ab`, 10% loss / 60ms RTT): skew std
  4.6–10.1 and swings to ±46 ticks vs libdatachannel's 0.6–0.9 / ±4. With
  this hunk: std 0.58–0.82, swings ≤ 5 — at parity with (slightly better
  than) libdatachannel under identical conditions.

Upstreaming this is worthwhile; it's a one-line correctness fix.

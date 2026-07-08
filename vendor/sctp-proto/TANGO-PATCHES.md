# Tango patches to sctp-proto

This is a vendored copy of [`sctp-proto` 0.10.1](https://crates.io/crates/sctp-proto)
(MIT OR Apache-2.0, licenses included), applied over the crates.io version via
`[patch.crates-io]` in the workspace root. It exists to carry a small
retransmission-tuning diff that str0m provides no configuration surface for
(str0m builds its SCTP `TransportConfig` internally in
`src/sctp/snap.rs::webrtc_transport_config()` from these crate defaults).

## Why

sctp-proto ships RFC 4960's timer defaults, which are tuned for bulk transfer:
RTO initial 3s / min 1s / max 60s, delayed-SACK 200ms. On a low-latency data
channel, any loss that fast retransmit can't catch (sparse traffic — no
following packets to generate the 3 duplicate SACKs — or the tail of a burst)
is recovered by RTO, i.e. a **>= 1s stall per loss, with exponential backoff**.
Measured with `tango-rtc/tests/loss.rs` at 15% loss: a 1 Hz reliable stream
(tango's lobby ping shape) saw p50 = 1.8s, max = 7.8s message latency.

libdatachannel (the stack tango-rtc replaced) tunes usrsctp for exactly this
workload, and this diff mirrors those values.

## The diff against upstream 0.10.1

Everything else is byte-identical to the crates.io release. To upgrade: copy
the new release over this directory and re-apply the two hunks (both are
marked with `TANGO PATCH` comments).

- `src/config.rs`: `RTO_INITIAL` 3000 → **1000**, `RTO_MIN` 1000 → **200**,
  `RTO_MAX` 60000 → **10000** (milliseconds). These consts are the defaults
  for `TransportConfig`, which is where str0m picks them up.
- `src/association/timer.rs`: `ACK_INTERVAL` 200 → **20** (ms). The delayed
  SACK interval; 200ms inflates the sender's smoothed RTT (and therefore its
  RTO) on low-rate streams. Gap-triggered SACKs were already immediate
  upstream; this only affects in-order traffic.

The right long-term fix is upstream plumbing (str0m `RtcConfig` →
`TransportConfig`), at which point this vendored copy goes away and the
values move to `tango-rtc`.

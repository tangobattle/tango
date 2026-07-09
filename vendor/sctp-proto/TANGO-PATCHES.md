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
the new release over this directory and re-apply the hunks (all marked with
`TANGO PATCH` comments).

- `src/config.rs`: `RTO_INITIAL` 3000 → **1000**, `RTO_MIN` 1000 → **200**,
  `RTO_MAX` 60000 → **10000** (milliseconds). These consts are the defaults
  for `TransportConfig`, which is where str0m picks them up.
- `src/association/timer.rs`: `ACK_INTERVAL` 200 → **20** (ms). The delayed
  SACK interval; 200ms inflates the sender's smoothed RTT (and therefore its
  RTO) on low-rate streams. Gap-triggered SACKs were already immediate
  upstream; this only affects in-order traffic.
- `src/association/mod.rs` (`check_partial_reliability_status`): the
  PR-SCTP Rexmit abandonment predicate `nsent >= reliability_value` →
  **`nsent > reliability_value + 1`** (saturating). `reliability_value`
  counts allowed *retransmissions* while `nsent` counts total sends, and
  the check also runs at **first send** — so upstream marks every chunk of
  a `max_retransmits: 0` stream abandoned the moment it is first
  transmitted (`1 >= 0`). The sender then advances its FORWARD-TSN point
  over data that is still in flight on every SACK arrival, and whenever a
  FORWARD-TSN beats the data to the receiver, the receiver discards the
  arriving data as a duplicate below its cumulative point. Measured with
  `tango-rtc/tests/loss.rs` (`wobble_rtt60`): **25–40% of unreliable
  messages silently dropped at 60ms RTT with zero packet loss**, in an
  alternating pattern locked to the SACK-every-2nd-packet cadence — felt
  in tango as constant in-match input holes and a swinging time-sync skew.
  With the fix: 100% delivery at 0% loss regardless of RTT.
- `src/association/mod.rs` (fast-retransmit + T3 retransmit sites): a chunk
  whose retransmission budget just ran out is **abandoned without the final
  retransmit** upstream/Pion would still send. That "bonus" retransmit
  delivered stale frames 150–400ms late under loss — traffic
  `max_retransmits: 0` callers asked never to exist (usrsctp never sends
  it), and noise for anything measuring delivery timing. With both PR-SCTP
  hunks: at 10% loss / 60ms RTT (± 10ms jitter), delivery ≈ 1−loss and the
  frontier-advance latency is p99 ≤ 48ms with a worst stall of ~60ms —
  the redundancy-recovery bound; the app-level (rennet) window is the
  recovery layer, exactly as with usrsctp.

The right long-term fix is upstream plumbing (str0m `RtcConfig` →
`TransportConfig`), at which point this vendored copy goes away and the
values move to `tango-rtc`.

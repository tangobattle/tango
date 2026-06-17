//! Per-peer netplay networking, split into planes that mirror each other:
//!
//! * [`control`] — the reliable lobby/handshake channel: the `Packet`
//!   [`protocol`](control::protocol), the `Sender` / `Receiver` byte transport,
//!   and the version `negotiate` handshake.
//! * [`data`] — the live in-match channel: the loss-tolerant `wire` protocol
//!   and reliability state machines, plus the `InMatchTx` / `PvpSender` /
//!   `PvpReceiver` adapters used by the battle loop. Runs over a separate
//!   **unreliable** data channel.
//!
//! [`channel`] owns the data-channel specs (labels / stream ids / reliability)
//! and the adapter that splits a WebRTC `DataChannel` into a `Sender` /
//! `Receiver`. [`direct_rtc`] is the signaling-free direct transport.
//!
//! The control plane's transport types and the `protocol` module are re-exported
//! at the root so callers can keep saying `crate::net::Sender`,
//! `crate::net::protocol`, etc. without knowing the layering.

pub mod channel;
pub mod control;
pub mod data;
pub mod direct_rtc;

pub use control::protocol;
pub use control::{negotiate, NegotiationError, PacketSink, PacketStream, Receiver, Sender};
pub use data::{InMatchTx, PvpReceiver, PvpSender};

/// Default UDP port for the signaling-free direct local-play transport
/// (link-code commands `/host` and `/connect`; see
/// [`direct_rtc`]). `24680` reads as a memorable even-step sequence and
/// steers clear of every well-known service in the ephemeral range —
/// easy to type, easy to recite over voice chat, unlikely to clash with
/// anything already listening locally.
pub const DEFAULT_LOCAL_PORT: u16 = 24680;

/// How often the lobby + match loops fire a ping. Latency is computed
/// from the matching Pong; absent pongs after a few intervals signal
/// a dropped peer.
pub const PING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// Median-of-window latency tracker. Identical to the legacy
/// `tango/src/stats.rs::LatencyCounter` — used by the PvP loop
/// to report ping in the running match.
#[derive(Clone)]
pub struct LatencyCounter {
    marks: std::collections::VecDeque<std::time::Duration>,
    window_size: usize,
}

impl LatencyCounter {
    pub fn new(window_size: usize) -> Self {
        Self {
            marks: std::collections::VecDeque::with_capacity(window_size),
            window_size,
        }
    }

    pub fn mark(&mut self, d: std::time::Duration) {
        while self.marks.len() >= self.window_size {
            self.marks.pop_front();
        }
        self.marks.push_back(d);
    }

    pub fn median(&self) -> std::time::Duration {
        if self.marks.is_empty() {
            return std::time::Duration::ZERO;
        }
        let mut marks = self.marks.iter().collect::<Vec<_>>();
        let (_, v, _) = marks.select_nth_unstable(self.marks.len() / 2);
        **v
    }

    /// Most recent (raw) ping mark — the latest single measurement, with no
    /// smoothing. `None` before the first `mark` (so callers can tell "no
    /// reading yet" from a genuine 0 ms ping). Feeds the live latency readout,
    /// where the median's lag would hide a real spike; [`median`](Self::median)
    /// stays the source for the frame-delay suggestion, which wants it smoothed.
    pub fn latest(&self) -> Option<std::time::Duration> {
        self.marks.back().copied()
    }
}

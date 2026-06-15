//! Per-peer netplay networking, split into three layers:
//!
//! * [`transport`] — the byte pipe: the `PacketSink` / `PacketStream` traits,
//!   the concrete WebRTC / TCP / UDP implementations, and the `Sender` /
//!   `Receiver` raw-message pair.
//! * [`control`] — the reliable lobby/handshake `Packet` protocol: message
//!   types, the version `negotiate` handshake, and the typed `Packet` send
//!   helpers. Runs over the reliable, ordered channel.
//! * [`data`] — the live in-match protocol: the `wire` frame codec, the
//!   `stream` reliability state machines, and the `InMatchTx` / `PvpSender` /
//!   `PvpReceiver` adapters. Runs over the unreliable in-match channel.
//!
//! [`LatencyCounter`] and [`PING_INTERVAL`] are shared by the control and data
//! planes, so they live at the root. The previous flat paths
//! (`crate::net::Sender`, `crate::net::protocol`, `crate::net::tcp`, …) are
//! preserved as re-exports below, so callers don't need to know the layering.

pub mod control;
pub mod data;
pub mod transport;

pub use control::{negotiate, protocol, NegotiationError};
pub use data::{InMatchTx, PvpReceiver, PvpSender};
pub use transport::{datachannel, tcp, udp, DEFAULT_LOCAL_PORT, Receiver, Sender};

/// How often the lobby + match loops fire a ping. Latency is computed
/// from the matching Pong; absent pongs after a few intervals signal
/// a dropped peer. Shared by the control plane's lobby loop and the data
/// plane's in-match receiver.
pub const PING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// Median-of-window latency tracker. Identical to the legacy
/// `tango/src/stats.rs::LatencyCounter` — used by the lobby ping line and the
/// PvP loop to report ping in the running match.
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

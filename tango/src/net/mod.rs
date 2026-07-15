//! Per-peer netplay networking, split into planes that mirror each other:
//!
//! * [`control`] — the reliable lobby/handshake channel: the `Packet`
//!   [`protocol`](control::protocol), the `Sender` / `Receiver` pair that frames
//!   those typed `Packet`s, and the version `negotiate` handshake.
//! * [`data`] — the live in-match channel: a raw-bytes [`Sender`](data::Sender) /
//!   [`Receiver`](data::Receiver) transport carrying tango's concrete `protocol`
//!   `Element`s (via the loss-tolerant frame codec + reliability state machines
//!   in the [`rennet`] crate), plus the `InMatchTx` / `PvpSender` / `PvpReceiver`
//!   adapters the battle loop drives over it. Runs over a separate **unreliable**
//!   data channel.
//!
//! Both planes build their `Sender` / `Receiver` on the shared [`PacketSink`] /
//! [`PacketStream`] byte-pipe defined here — a message-boundary-preserving
//! datagram transport, agnostic to whether the underlying channel is
//! reliable/ordered. [`channel`] owns the data-channel specs (labels / stream
//! ids / reliability) and the adapters that split a WebRTC `DataChannel` into
//! either pair ([`channel::control_pair`] for control, [`channel::data_pair`]
//! for data). [`direct_rtc`] is the signaling-free direct transport.
//!
//! The control plane's `Sender` / `Receiver` and the `protocol` module are
//! re-exported at the root so callers can keep saying `crate::net::Sender`,
//! `crate::net::protocol`, etc.; the data plane's same-named transport types stay
//! under `crate::net::data` to keep the two straight.

pub mod channel;
pub mod control;
pub mod data;
pub mod direct_rtc;
pub mod link;

pub use control::protocol;
pub use control::{negotiate, NegotiationError, Receiver, Sender};
pub use data::{InMatchTx, PvpReceiver, PvpSender};

/// One half of a peer connection's send side: the byte-pipe both planes' typed
/// transports build on. Carries discrete byte messages, preserving message
/// boundaries — each `send` lands as exactly one `recv` on the peer, a WebRTC
/// DataChannel's native contract. A stream-oriented impl would have to add its
/// own length-prefix framing to match. Reliability and ordering are the concrete
/// channel's properties — the control channel is reliable + ordered, the
/// in-match channel unreliable + unordered — not this trait's; it only promises
/// boundaries.
#[async_trait::async_trait]
pub trait PacketSink: Send + Sync {
    async fn send(&mut self, bytes: &[u8]) -> std::io::Result<()>;
}

/// One half of a peer connection's receive side. See [`PacketSink`] for the
/// contract on message boundaries. A clean stream close is reported as
/// `io::ErrorKind::UnexpectedEof`.
#[async_trait::async_trait]
pub trait PacketStream: Send + Sync {
    async fn recv(&mut self) -> std::io::Result<Vec<u8>>;
}

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

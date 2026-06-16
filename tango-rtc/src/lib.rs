//! WebRTC data-channel transport for Tango, built on [`str0m`].
//!
//! This replaces the old libdatachannel-based stack with a pure-Rust one:
//! str0m provides ICE/DTLS/SCTP as a sans-IO state machine, and this crate
//! supplies the I/O around it — UDP sockets, candidate gathering (host +
//! STUN server-reflexive + TURN relay) and a Tokio driver task — while
//! keeping the async, channel-based shape the rest of Tango is written
//! against: peer-connection events arrive on an `mpsc::Receiver`, and
//! data-channel sends/receives are `await`-able.
//!
//! Tango does non-trickle ICE: gathering runs to completion first, the full
//! SDP (candidates included) is shipped over signaling, and the connection
//! proceeds from there. A connection carries one or more in-band (DCEP) data
//! channels, declared up front: [`PeerConnection::new`] takes a slice of
//! [`ChannelConfig`]s and returns the connection together with one
//! [`DataChannel`] per spec, in the same order. The connection drives the
//! signaling exchange and owns the transport: keep it alive for as long as any
//! channel (or its [`split`](DataChannel::split) halves) is in use — dropping
//! it hangs up and every channel goes dead.

mod driver;
mod gather;

use std::sync::{Arc, Mutex};

/// Which way a session description points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdpType {
    Offer,
    Answer,
}

/// A full SDP session description, as raw SDP text.
#[derive(Debug, Clone)]
pub struct SessionDescription {
    pub sdp_type: SdpType,
    pub sdp: String,
}

/// Peer connection lifecycle, mirroring the subset of the old
/// libdatachannel states that Tango observes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Disconnected,
    Failed,
    Closed,
}

/// ICE candidate gathering progress. Tango waits for `Complete` before
/// reading the local description off the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatheringState {
    InProgress,
    Complete,
}

/// Restrict which candidates are used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportPolicy {
    /// Direct (host/server-reflexive) and relayed candidates.
    #[default]
    All,
    /// TURN relay only.
    Relay,
}

/// A STUN or TURN server, in the same shape the signaling server hands out:
/// `stun:`/`turn:` URLs plus optional long-term credentials.
#[derive(Debug, Clone, Default)]
pub struct IceServer {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

/// Peer connection configuration.
#[derive(Debug, Clone, Default)]
pub struct RtcConfig {
    pub ice_servers: Vec<IceServer>,
    pub ice_transport_policy: TransportPolicy,
    /// Also gather host candidates on loopback interfaces. Off by default;
    /// lets tests connect two peers within a single machine.
    pub include_loopback: bool,
}

/// Which end of a direct ([`PeerConnection::new_direct`]) connection this is.
/// The link code is the host's `addr:port`; there's no signaling server, so the
/// two ends agree everything else (ICE creds, roles) from fixed constants.
#[derive(Debug, Clone)]
pub enum DirectRole {
    /// Listen on `port` (the only thing that needs port-forwarding). The host
    /// learns the dialer's address peer-reflexively from its first ICE check.
    Host { port: u16 },
    /// Dial the host at `remote` (its `addr:port`).
    Connect { remote: std::net::SocketAddr },
}

/// Per-channel config + reliability, re-exported from str0m so callers don't
/// depend on str0m directly. Build one [`ChannelConfig`] per channel — set
/// `label`, `ordered` and `reliability` and leave the rest at
/// `..Default::default()`. In particular **leave `negotiated` unset**: both paths
/// negotiate channels in-band (DCEP) and demux them by `label`, so each label has
/// to be unique within the slice. Hand the slice to [`PeerConnection::new`] /
/// [`new_direct`](PeerConnection::new_direct); the returned [`DataChannel`]s line
/// up with it by index.
pub use str0m::channel::{ChannelConfig, Reliability};

/// Whether a channel may drop a message rather than deliver it — anything but
/// fully [`Reliable`](Reliability::Reliable). A lossy channel's caller tolerates
/// loss, so the driver may shed under backpressure instead of blocking the
/// sender (see [`DataChannelSender::send`] and the driver's outbox).
pub(crate) fn is_lossy(reliability: Reliability) -> bool {
    !matches!(reliability, Reliability::Reliable)
}

/// Events forwarded from the driver task onto the peer-connection's event
/// channel.
#[derive(Debug)]
pub enum PeerConnectionEvent {
    ConnectionStateChange(ConnectionState),
    GatheringStateChange(GatheringState),
}

/// Why the transport died, when it died on its own (deliberate teardown
/// reports `Closed`, not a failure). Surfaced to the consumer boxed inside
/// the `std::io::Error` that `DataChannel::send` returns.
#[derive(Debug, Clone)]
enum Failure {
    /// The str0m state machine errored while being fed or polled. `Arc`
    /// because this travels through a watch channel ([`DataChannelStatus`]
    /// is `Clone`) and `RtcError` isn't.
    Rtc(Arc<str0m::RtcError>),
    /// The `Rtc` instance reported itself dead without an explicit close.
    Died,
    /// ICE sat in `Disconnected` past the grace period.
    IceDisconnected,
    /// No connection within the post-signaling-exchange timeout.
    ConnectTimeout,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Rtc(e) => write!(f, "rtc: {}", e),
            Failure::Died => write!(f, "connection died"),
            Failure::IceDisconnected => write!(f, "ice disconnected"),
            Failure::ConnectTimeout => write!(f, "timed out establishing connection"),
        }
    }
}

impl std::error::Error for Failure {}

/// The lifecycle of the data channel as observed by the send side.
#[derive(Debug, Clone)]
enum DataChannelStatus {
    Pending,
    Open,
    Closed,
    Error(Failure),
}

/// State shared between the user-facing [`PeerConnection`] (sync methods)
/// and the driver task. All str0m access happens under this lock; the
/// driver re-polls after every mutation (via [`Shared::notify`]).
struct Inner {
    rtc: str0m::Rtc,
    /// One entry per requested channel, in the order they were passed to
    /// [`PeerConnection::new`] / [`new_direct`](PeerConnection::new_direct). The
    /// driver binds each entry's `id` by `label` when str0m reports the channel
    /// open — whether we opened it (signaling offerer / direct dialer) or the
    /// remote did (signaling answerer / direct host).
    channels: Vec<ChannelState>,
    pending_offer: Option<str0m::change::SdpPendingOffer>,
    local_desc: Option<SessionDescription>,
    remote_desc: Option<SessionDescription>,
    gathering_complete: bool,
    /// Local candidates accepted by the ICE agent, for selected-pair
    /// reporting.
    local_candidates: Vec<str0m::Candidate>,
    /// Remote candidates parsed out of the remote SDP, likewise.
    remote_candidates: Vec<str0m::Candidate>,
    /// (source, destination) of the most recent non-STUN (i.e. DTLS)
    /// transmit: str0m only sends application traffic over the nominated
    /// pair, so this is the selected path.
    current_path: Option<(std::net::SocketAddr, std::net::SocketAddr)>,
    /// The driver's transmit routes, shared here once gathering is done so
    /// [`PeerConnection`]'s `Drop` can hang up without the driver task.
    routes: Option<Arc<std::collections::HashMap<std::net::SocketAddr, driver::Route>>>,
}

/// Per-channel binding state held under [`Inner`]'s lock: the spec we declared
/// it with, plus the str0m channel id once it's open.
struct ChannelState {
    config: ChannelConfig,
    /// `Some` once the channel is open and bound (matched by `config.label`).
    id: Option<str0m::channel::ChannelId>,
}

struct Shared {
    inner: Mutex<Inner>,
    notify: tokio::sync::Notify,
}

impl Shared {
    /// Once gathering is complete — and no exchange has happened yet —
    /// produce the local offer, declaring every data channel. Called by the
    /// driver when gathering finishes.
    fn maybe_make_offer(inner: &mut Inner) {
        if !inner.gathering_complete || inner.local_desc.is_some() || inner.remote_desc.is_some() {
            return;
        }
        // In-band (DCEP) for the signaling path: the caller leaves `negotiated`
        // unset, so str0m picks the stream id and carries the reliability over
        // DCEP.
        let configs: Vec<_> = inner.channels.iter().map(|c| c.config.clone()).collect();
        let mut api = inner.rtc.sdp_api();
        for config in configs {
            api.add_channel_with_config(config);
        }
        let Some((offer, pending)) = api.apply() else {
            return;
        };
        inner.pending_offer = Some(pending);
        inner.local_desc = Some(SessionDescription {
            sdp_type: SdpType::Offer,
            sdp: offer.to_sdp_string(),
        });
    }
}

fn other_err<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

/// The connection half: it drives the SDP exchange, reports on the ICE
/// outcome, and owns the transport, while the [`DataChannel`]s created
/// alongside it carry the data. Keep it alive for as long as any channel is
/// in use.
pub struct PeerConnection {
    shared: Arc<Shared>,
    /// Dropping this (i.e. dropping the `PeerConnection`) tears down the
    /// driver task and with it the whole transport.
    _shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl PeerConnection {
    /// Create a connection and its data channels, and spawn the driver task
    /// on the current Tokio runtime. Candidate gathering starts immediately;
    /// `GatheringStateChange(Complete)` on the event channel signals that
    /// [`PeerConnection::local_description`] is ready.
    ///
    /// Channels are negotiated in-band (DCEP): whichever side's offer survives
    /// the exchange opens them and the other side binds to the incoming ones —
    /// so both peers must construct this with the same `channels` (matched by
    /// label), before the SDP exchange. The returned [`DataChannel`]s line up
    /// with `channels` by index.
    pub fn new(
        config: RtcConfig,
        channels: &[ChannelConfig],
    ) -> Result<(Self, Vec<DataChannel>, tokio::sync::mpsc::Receiver<PeerConnectionEvent>), std::io::Error> {
        let rtc = str0m::Rtc::builder().build(std::time::Instant::now());
        Self::spawn_connection(rtc, config, channels, driver::Setup::Signaled)
    }

    /// Create a connection over the **direct** (link-code) path: no signaling
    /// server. Both peers configure ICE/DTLS/SCTP locally from fixed shared
    /// constants and the host's `addr:port` (see [`DirectRole`]) — DTLS
    /// fingerprint verification is off, so the trust model is "address =
    /// identity". Channels are negotiated in-band (DCEP), just like the signaling
    /// path — the dialer opens them and the host binds each by label; the returned
    /// [`DataChannel`]s line up with `channels` by index. Drives the connection to
    /// `Connected` with no offer/answer exchange.
    pub fn new_direct(
        config: RtcConfig,
        channels: &[ChannelConfig],
        role: DirectRole,
    ) -> Result<(Self, Vec<DataChannel>, tokio::sync::mpsc::Receiver<PeerConnectionEvent>), std::io::Error> {
        let rtc = str0m::Rtc::builder()
            .set_fingerprint_verification(false)
            .build(std::time::Instant::now());
        Self::spawn_connection(rtc, config, channels, driver::Setup::Direct(role))
    }

    /// Shared construction for [`new`](Self::new) / [`new_direct`](Self::new_direct):
    /// build the shared state + per-channel pipelines and spawn the driver. The
    /// `rtc` is pre-built (the two paths differ only in its config) and `setup`
    /// selects gather-and-signal vs. direct configuration.
    fn spawn_connection(
        rtc: str0m::Rtc,
        config: RtcConfig,
        channels: &[ChannelConfig],
        setup: driver::Setup,
    ) -> Result<(Self, Vec<DataChannel>, tokio::sync::mpsc::Receiver<PeerConnectionEvent>), std::io::Error> {
        let runtime = tokio::runtime::Handle::try_current().map_err(other_err)?;

        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                rtc,
                channels: channels
                    .iter()
                    .map(|config| ChannelState {
                        config: config.clone(),
                        id: None,
                    })
                    .collect(),
                pending_offer: None,
                local_desc: None,
                remote_desc: None,
                gathering_complete: false,
                local_candidates: vec![],
                remote_candidates: vec![],
                current_path: None,
                routes: None,
            }),
            notify: tokio::sync::Notify::new(),
        });

        let (event_tx, event_rx) = tokio::sync::mpsc::channel(32);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        // One independent set of pipelines per channel: the driver gets the
        // I/O halves, the caller gets the `DataChannel`s. Keeping them separate
        // means a full reliable channel can't back up the unreliable one.
        let mut data_channels = Vec::with_capacity(channels.len());
        let mut driver_channels = Vec::with_capacity(channels.len());
        for config in channels {
            let (outgoing_tx, outgoing_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
            // Inbound messages must never block the driver — a consumer that's
            // slow to read (e.g. sending a burst before it starts receiving)
            // would otherwise stall the transport itself: no SACKs, no ICE
            // keepalive answers, dead connection. Messages are tiny; let them
            // queue.
            let (message_tx, message_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
            let (status_tx, status_rx) = tokio::sync::watch::channel(DataChannelStatus::Pending);
            data_channels.push(DataChannel {
                sender: DataChannelSender {
                    outgoing_tx,
                    status_rx,
                    lossy: is_lossy(config.reliability),
                },
                receiver: DataChannelReceiver { message_rx },
            });
            driver_channels.push(driver::ChannelIo {
                status_tx,
                message_tx: Some(message_tx),
                outgoing_rx,
            });
        }

        runtime.spawn(driver::run(driver::Driver {
            shared: shared.clone(),
            config,
            event_tx,
            channels: driver_channels,
            shutdown_rx,
            setup,
        }));

        Ok((
            PeerConnection {
                shared,
                _shutdown_tx: shutdown_tx,
            },
            data_channels,
            event_rx,
        ))
    }

    /// Apply the remote description. An `Offer` implicitly rolls back our
    /// own pending offer (the "polite peer" path) and produces the answer,
    /// which is then available via [`PeerConnection::local_description`];
    /// an `Answer` completes our own offer.
    pub fn set_remote_description(&mut self, desc: SessionDescription) -> Result<(), std::io::Error> {
        let mut inner = self.shared.inner.lock().unwrap();

        // Re-base str0m's clock to the present before we init DTLS. While we
        // waited in the lobby the driver held str0m's clock still (it has
        // nothing to do without a remote, and ticking it would spend the DTLS
        // handshake's ~40s connect budget against the wait). Accepting the
        // offer/answer below inits DTLS and arms that timeout relative to the
        // clock, so step it forward to now — that hands the handshake its full
        // budget from the real exchange, and lets the driver resume on real time
        // without a backwards jump. (str0m ignores a timeout that would move its
        // clock backwards, so this is a no-op if the driver already advanced it.)
        let _ = inner.rtc.handle_input(str0m::Input::Timeout(std::time::Instant::now()));

        match desc.sdp_type {
            SdpType::Offer => {
                let offer = str0m::change::SdpOffer::from_sdp_string(&desc.sdp).map_err(other_err)?;
                let answer = inner.rtc.sdp_api().accept_offer(offer).map_err(other_err)?;
                // accept_offer rolled back our pending offer (and with it our
                // locally-declared channels; the remote's in-band channels
                // replace them, and the driver re-binds each by label).
                inner.pending_offer = None;
                inner.local_desc = Some(SessionDescription {
                    sdp_type: SdpType::Answer,
                    sdp: answer.to_sdp_string(),
                });
            }
            SdpType::Answer => {
                let answer = str0m::change::SdpAnswer::from_sdp_string(&desc.sdp).map_err(other_err)?;
                let pending = inner
                    .pending_offer
                    .take()
                    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no pending offer"))?;
                inner.rtc.sdp_api().accept_answer(pending, answer).map_err(other_err)?;
            }
        }
        inner.remote_candidates = parse_sdp_candidates(&desc.sdp);
        inner.remote_desc = Some(desc);
        drop(inner);
        self.shared.notify.notify_one();
        Ok(())
    }

    pub fn local_description(&self) -> Option<SessionDescription> {
        self.shared.inner.lock().unwrap().local_desc.clone()
    }

    pub fn remote_description(&self) -> Option<SessionDescription> {
        self.shared.inner.lock().unwrap().remote_desc.clone()
    }

    /// The selected ICE candidate pair as raw candidate strings,
    /// `(local, remote)` — e.g. for telling a relayed (TURN)
    /// connection from a direct one (`typ relay`). Errors until
    /// the agent has picked a pair.
    pub fn selected_candidate_pair(&self) -> Result<(String, String), std::io::Error> {
        let inner = self.shared.inner.lock().unwrap();
        let (source, destination) = inner
            .current_path
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::WouldBlock, "no nominated pair yet"))?;

        // The transmit source is the candidate base: the relay address for
        // relayed candidates, the local socket for host ones. (A
        // server-reflexive path shows up under its host candidate, which
        // is fine — relay detection is what matters here.)
        let local = inner
            .local_candidates
            .iter()
            .find(|c| c.kind() == str0m::CandidateKind::Relayed && c.addr() == source)
            .or_else(|| inner.local_candidates.iter().find(|c| c.addr() == source))
            .map(|c| c.to_sdp_string())
            .unwrap_or_else(|| synthesize_candidate(source, "host"));

        // A destination that doesn't appear in the remote SDP is
        // peer-reflexive (e.g. their NAT mapping).
        let remote = inner
            .remote_candidates
            .iter()
            .find(|c| c.addr() == destination)
            .map(|c| c.to_sdp_string())
            .unwrap_or_else(|| synthesize_candidate(destination, "prflx"));

        Ok((local, remote))
    }
}

impl Drop for PeerConnection {
    /// Hang up synchronously: DTLS close_notify, straight onto the wire.
    ///
    /// The driver task has its own graceful close on shutdown, but it only
    /// runs if the task gets polled again — on process exit the runtime is
    /// torn down by *dropping* its tasks, so anything that must reach the
    /// remote has to go out inline here, before `_shutdown_tx` drops. The
    /// remote turns close_notify into a prompt EOF instead of sitting out
    /// its disconnect grace. Best effort: one unacknowledged datagram.
    fn drop(&mut self) {
        let mut inner = self.shared.inner.lock().unwrap();
        if !inner.rtc.is_alive() {
            return;
        }
        // No routes means gathering never finished: nothing to hang up.
        let Some(routes) = inner.routes.clone() else {
            return;
        };
        if let Err(e) = inner.rtc.close() {
            log::debug!("rtc.close: {}", e);
            return;
        }
        // Drain the close_notify out, as in the driver's graceful close;
        // close() flips the instance to not-alive once fully polled.
        loop {
            let mut sent_any = false;
            loop {
                match inner.rtc.poll_output() {
                    Ok(str0m::Output::Transmit(t)) => {
                        driver::send_transmit_sync(&routes, t.source, t.destination, &t.contents);
                        sent_any = true;
                    }
                    Ok(str0m::Output::Event(_)) => {}
                    Ok(str0m::Output::Timeout(_)) | Err(_) => break,
                }
            }
            if !sent_any || !inner.rtc.is_alive() {
                break;
            }
        }
    }
}

/// Pull the `a=candidate:` lines out of an SDP blob.
fn parse_sdp_candidates(sdp: &str) -> Vec<str0m::Candidate> {
    sdp.lines()
        .filter_map(|line| {
            let line = line.trim();
            let attr = line.strip_prefix("a=")?;
            if !attr.starts_with("candidate:") {
                return None;
            }
            str0m::Candidate::from_sdp_string(attr).ok()
        })
        .collect()
}

/// Fallback candidate string for addresses we can't match to a known
/// candidate; only its address and `typ` matter to consumers.
fn synthesize_candidate(addr: std::net::SocketAddr, typ: &str) -> String {
    format!("candidate:0 1 UDP 0 {} {} typ {}", addr.ip(), addr.port(), typ)
}

/// The data half of a connection, created by [`PeerConnection::new`]:
/// `await`-able sends and receives, splittable into its two halves. The
/// [`PeerConnection`] owns the transport — the channel goes dead if it
/// isn't kept alive alongside.
pub struct DataChannel {
    sender: DataChannelSender,
    receiver: DataChannelReceiver,
}

impl DataChannel {
    pub async fn send(&mut self, msg: &[u8]) -> Result<(), std::io::Error> {
        self.sender.send(msg).await
    }

    pub async fn receive(&mut self) -> Option<Vec<u8>> {
        self.receiver.receive().await
    }

    pub fn split(self) -> (DataChannelSender, DataChannelReceiver) {
        (self.sender, self.receiver)
    }
}

pub struct DataChannelSender {
    outgoing_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    status_rx: tokio::sync::watch::Receiver<DataChannelStatus>,
    /// The channel tolerates loss (unreliable reliability mode), so a send may
    /// drop rather than block when the pipeline is full — matching the QUIC
    /// datagram path. Reliable channels keep blocking backpressure.
    lossy: bool,
}

impl DataChannelSender {
    pub async fn send(&mut self, msg: &[u8]) -> Result<(), std::io::Error> {
        // Block on the first send until the channel leaves `Pending` (opens or
        // dies); once it's `Open` this returns immediately on every later send.
        let status = match self
            .status_rx
            .wait_for(|s| !matches!(s, DataChannelStatus::Pending))
            .await
        {
            Ok(s) => s.clone(),
            // The status sender is gone, i.e. the driver shut down.
            Err(_) => DataChannelStatus::Closed,
        };

        match status {
            DataChannelStatus::Open => {}
            DataChannelStatus::Error(err) => return Err(std::io::Error::other(err)),
            DataChannelStatus::Closed => {
                return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected"))
            }
            DataChannelStatus::Pending => unreachable!("wait_for guarantees we left Pending"),
        }

        if self.lossy {
            // Never block a lossy channel's sender: under a full pipeline (a
            // congested SCTP send buffer backing up through the bounded channel)
            // drop this message rather than stalling the caller. The in-match
            // protocol recovers the loss from its own redundancy window, and the
            // retransmit heartbeat keeps flowing instead of being held behind a
            // backpressured `send` — the same shed-don't-stall behaviour the QUIC
            // datagram path gets for free.
            use tokio::sync::mpsc::error::TrySendError;
            return match self.outgoing_tx.try_send(msg.to_vec()) {
                Ok(()) => Ok(()),
                Err(TrySendError::Full(_)) => Ok(()),
                Err(TrySendError::Closed(_)) => {
                    Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected"))
                }
            };
        }

        self.outgoing_tx
            .send(msg.to_vec())
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected"))
    }

    pub fn unsplit(self, receiver: DataChannelReceiver) -> DataChannel {
        DataChannel { sender: self, receiver }
    }
}

pub struct DataChannelReceiver {
    message_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
}

impl DataChannelReceiver {
    pub async fn receive(&mut self) -> Option<Vec<u8>> {
        self.message_rx.recv().await
    }

    pub fn unsplit(self, tx: DataChannelSender) -> DataChannel {
        tx.unsplit(self)
    }
}

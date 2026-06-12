//! WebRTC data-channel transport for Tango, built on [`str0m`].
//!
//! This replaces the old libdatachannel-based stack with a pure-Rust one:
//! str0m provides ICE/DTLS/SCTP as a sans-IO state machine, and this crate
//! supplies the I/O around it — UDP sockets, candidate gathering (host +
//! STUN server-reflexive + TURN relay) and a Tokio driver task — while
//! keeping the async, channel-based shape the rest of Tango is written
//! against: connection events arrive on an `mpsc::Receiver`, and
//! data-channel sends/receives are `await`-able.
//!
//! Tango does non-trickle ICE: gathering runs to completion first, the full
//! SDP (candidates included) is shipped over signaling, and the connection
//! proceeds from there. A connection carries exactly one data channel, so
//! connection and channel are a single type here: [`DataChannel::new`]
//! starts the transport, the SDP methods on the same value drive the
//! signaling exchange, and afterwards it (or its
//! [`split`](DataChannel::split) halves) is the channel. The transport
//! stays up as long as either half is alive, and hangs up when the last
//! one is dropped.

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
/// reading the local description off the channel.
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

/// Events forwarded from the driver task onto the channel's event channel.
#[derive(Debug)]
pub enum ConnectionEvent {
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

/// State shared between the user-facing [`DataChannel`] (sync methods)
/// and the driver task. All str0m access happens under this lock; the
/// driver re-polls after every mutation (via [`Shared::notify`]).
struct Inner {
    rtc: str0m::Rtc,
    /// Label for the single data channel.
    channel_label: String,
    /// Bound when str0m reports the channel open (either our own in-band
    /// channel on the offering side, or the remote-opened one after we
    /// answered).
    channel_id: Option<str0m::channel::ChannelId>,
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
    /// [`Transport`]'s `Drop` can hang up without the driver task.
    routes: Option<Arc<std::collections::HashMap<std::net::SocketAddr, driver::Route>>>,
}

struct Shared {
    inner: Mutex<Inner>,
    notify: tokio::sync::Notify,
}

impl Shared {
    /// Once gathering is complete — and no exchange has happened yet —
    /// produce the local offer, declaring our data channel. Called by the
    /// driver when gathering finishes.
    fn maybe_make_offer(inner: &mut Inner) {
        if !inner.gathering_complete || inner.local_desc.is_some() || inner.remote_desc.is_some() {
            return;
        }
        let mut api = inner.rtc.sdp_api();
        api.add_channel(inner.channel_label.clone());
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

/// The engine behind the channel halves: each half holds an `Arc` of this,
/// so the transport lives until the last half is dropped.
struct Transport {
    shared: Arc<Shared>,
    /// Dropping this (i.e. dropping the whole `Transport`) tears down the
    /// driver task.
    _shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl Drop for Transport {
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

fn other_err<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

/// A peer-to-peer data channel and the connection that carries it, as one
/// value: construct it, run the SDP exchange through it, then send/receive
/// on it (or on its [`split`](DataChannel::split) halves).
pub struct DataChannel {
    sender: DataChannelSender,
    receiver: DataChannelReceiver,
}

impl DataChannel {
    /// Create the transport and spawn its driver task on the current Tokio
    /// runtime. Candidate gathering starts immediately;
    /// `GatheringStateChange(Complete)` on the event channel signals that
    /// [`DataChannel::local_description`] is ready.
    ///
    /// The channel is negotiated in-band (DCEP): whichever side's offer
    /// survives the exchange opens it, and the other side binds to the
    /// incoming channel — so both peers construct this with the same
    /// `label`, before the SDP exchange.
    pub fn new(
        config: RtcConfig,
        label: &str,
    ) -> Result<(Self, tokio::sync::mpsc::Receiver<ConnectionEvent>), std::io::Error> {
        let runtime = tokio::runtime::Handle::try_current().map_err(other_err)?;

        let rtc = str0m::Rtc::builder().build(std::time::Instant::now());

        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                rtc,
                channel_label: label.to_owned(),
                channel_id: None,
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
        let (outgoing_tx, outgoing_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
        // Inbound messages must never block the driver — a consumer that's
        // slow to read (e.g. sending a burst before it starts receiving)
        // would otherwise stall the transport itself: no SACKs, no ICE
        // keepalive answers, dead connection. Messages are tiny; let them
        // queue.
        let (message_tx, message_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let (status_tx, status_rx) = tokio::sync::watch::channel(DataChannelStatus::Pending);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        runtime.spawn(driver::run(driver::Driver {
            shared: shared.clone(),
            config,
            event_tx,
            status_tx,
            message_tx: Some(message_tx),
            outgoing_rx,
            shutdown_rx,
        }));

        Ok((
            DataChannel {
                sender: DataChannelSender {
                    outgoing_tx,
                    status_rx,
                    transport: Transport {
                        shared,
                        _shutdown_tx: shutdown_tx,
                    },
                },
                receiver: DataChannelReceiver { message_rx },
            },
            event_rx,
        ))
    }

    fn shared(&self) -> &Shared {
        &self.sender.transport.shared
    }

    /// Apply the remote description. An `Offer` implicitly rolls back our
    /// own pending offer (the "polite peer" path) and produces the answer,
    /// which is then available via [`DataChannel::local_description`];
    /// an `Answer` completes our own offer.
    pub fn set_remote_description(&mut self, desc: SessionDescription) -> Result<(), std::io::Error> {
        let shared = self.shared();
        let mut inner = shared.inner.lock().unwrap();
        match desc.sdp_type {
            SdpType::Offer => {
                let offer = str0m::change::SdpOffer::from_sdp_string(&desc.sdp).map_err(other_err)?;
                let answer = inner.rtc.sdp_api().accept_offer(offer).map_err(other_err)?;
                // accept_offer rolled back our pending offer (and with it
                // our locally-declared channel; the remote's in-band
                // channel replaces it).
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
        shared.notify.notify_one();
        Ok(())
    }

    pub fn local_description(&self) -> Option<SessionDescription> {
        self.shared().inner.lock().unwrap().local_desc.clone()
    }

    pub fn remote_description(&self) -> Option<SessionDescription> {
        self.shared().inner.lock().unwrap().remote_desc.clone()
    }

    /// The selected ICE candidate pair as raw candidate strings,
    /// `(local, remote)` — e.g. for telling a relayed (TURN)
    /// connection from a direct one (`typ relay`). Errors until
    /// the agent has picked a pair.
    pub fn selected_candidate_pair(&self) -> Result<(String, String), std::io::Error> {
        let inner = self.shared().inner.lock().unwrap();
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

pub struct DataChannelSender {
    outgoing_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    status_rx: tokio::sync::watch::Receiver<DataChannelStatus>,
    transport: Transport,
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

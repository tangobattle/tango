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
//! proceeds from there. One data channel per peer connection.

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

/// Events forwarded from the driver task onto the peer-connection's event
/// channel.
#[derive(Debug)]
pub enum PeerConnectionEvent {
    ConnectionStateChange(ConnectionState),
    GatheringStateChange(GatheringState),
}

/// The lifecycle of the data channel as observed by the send side.
#[derive(Debug, Clone)]
enum DataChannelStatus {
    Pending,
    Open,
    Closed,
    Error(String),
}

/// State shared between the user-facing [`PeerConnection`] (sync methods)
/// and the driver task. All str0m access happens under this lock; the
/// driver re-polls after every mutation (via [`Shared::notify`]).
struct Inner {
    rtc: str0m::Rtc,
    /// Label for the single data channel, once requested.
    channel_label: Option<String>,
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
}

struct Shared {
    inner: Mutex<Inner>,
    notify: tokio::sync::Notify,
}

impl Shared {
    /// Once gathering is complete and a channel has been requested — and
    /// no exchange has happened yet — produce the local offer. Called from
    /// whichever of the two preconditions is satisfied last.
    fn maybe_make_offer(inner: &mut Inner) {
        if !inner.gathering_complete || inner.local_desc.is_some() || inner.remote_desc.is_some() {
            return;
        }
        let Some(label) = inner.channel_label.clone() else {
            return;
        };
        let mut api = inner.rtc.sdp_api();
        api.add_channel(label);
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

/// Endpoints handed to the single [`DataChannel`] when it's created. Built
/// up-front in [`PeerConnection::new`] so the driver task can hold the
/// other halves from the start.
struct ChannelSlot {
    outgoing_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    status_rx: tokio::sync::watch::Receiver<DataChannelStatus>,
    message_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
}

pub struct PeerConnection {
    shared: Arc<Shared>,
    channel_slot: Option<ChannelSlot>,
    /// Dropping this (i.e. dropping the `PeerConnection`) tears down the
    /// driver task and with it the whole transport — same contract as the
    /// old wrapper, where the peer connection owned the channel's
    /// transport.
    _shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

fn other_err<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

impl PeerConnection {
    /// Create a peer connection and spawn its driver task on the current
    /// Tokio runtime. Candidate gathering starts immediately;
    /// `GatheringStateChange(Complete)` on the event channel signals that
    /// [`PeerConnection::local_description`] is ready.
    pub fn new(config: RtcConfig) -> Result<(Self, tokio::sync::mpsc::Receiver<PeerConnectionEvent>), std::io::Error> {
        let runtime = tokio::runtime::Handle::try_current().map_err(other_err)?;

        let rtc = str0m::Rtc::builder().build(std::time::Instant::now());

        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                rtc,
                channel_label: None,
                channel_id: None,
                pending_offer: None,
                local_desc: None,
                remote_desc: None,
                gathering_complete: false,
                local_candidates: vec![],
                remote_candidates: vec![],
                current_path: None,
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
            PeerConnection {
                shared,
                channel_slot: Some(ChannelSlot {
                    outgoing_tx,
                    status_rx,
                    message_rx,
                }),
                _shutdown_tx: shutdown_tx,
            },
            event_rx,
        ))
    }

    /// Create the connection's data channel. The channel is negotiated
    /// in-band (DCEP): whichever side's offer survives the exchange opens
    /// it, and the other side's handle binds to it on arrival — so both
    /// peers call this exactly once, before the SDP exchange.
    pub fn create_data_channel(&mut self, label: &str) -> Result<DataChannel, std::io::Error> {
        let slot = self.channel_slot.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Unsupported, "only one data channel is supported")
        })?;

        {
            let mut inner = self.shared.inner.lock().unwrap();
            inner.channel_label = Some(label.to_owned());
            Shared::maybe_make_offer(&mut inner);
        }
        self.shared.notify.notify_one();

        Ok(DataChannel {
            sender: DataChannelSender {
                outgoing_tx: slot.outgoing_tx,
                status_rx: slot.status_rx,
            },
            receiver: DataChannelReceiver {
                message_rx: slot.message_rx,
            },
        })
    }

    /// Apply the remote description. An `Offer` implicitly rolls back our
    /// own pending offer (the "polite peer" path) and produces the answer,
    /// which is then available via [`PeerConnection::local_description`];
    /// an `Answer` completes our own offer.
    pub fn set_remote_description(&mut self, desc: SessionDescription) -> Result<(), std::io::Error> {
        let mut inner = self.shared.inner.lock().unwrap();
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

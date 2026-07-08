//! Pure-Rust WebRTC data-channel transport for Tango.
//!
//! [`str0m`] provides ICE/DTLS/SCTP as a sans-IO state machine; this crate
//! supplies the I/O around it — UDP sockets, trickle ICE gathering (host
//! candidates, STUN server-reflexive via the `stun` crate, TURN relays via the
//! `turn` crate) and a Tokio driver task — while keeping the async,
//! channel-based shape the rest of Tango is written against: peer-connection
//! events arrive on an `mpsc::Receiver`, and data-channel sends/receives are
//! `await`-able.
//!
//! Two bring-up paths:
//!
//! * [`PeerConnection::new`] — the signaling (matchmaking) path, with trickle
//!   ICE: the local offer is ready synchronously (read it off
//!   [`local_description`](PeerConnection::local_description) right away),
//!   candidates stream out as [`PeerConnectionEvent::IceCandidate`] events as
//!   gathering finds them, and the peer's trickled candidates are fed back in
//!   with [`add_remote_candidate`](PeerConnection::add_remote_candidate).
//! * [`PeerConnection::new_direct`] — the signaling-free direct (link-code)
//!   path: both ends configure ICE/DTLS/SCTP from fixed shared constants plus
//!   the host's `addr:port`, and no descriptions or candidates are exchanged
//!   at all.
//!
//! Either way, a connection carries one or more data channels, declared up
//! front: the constructors take a slice of [`ChannelConfig`]s and return one
//! [`DataChannel`] per config, in the same order. Channels are negotiated
//! in-band (DCEP) and matched up by `label` — both peers must construct their
//! connection with the same configs. The [`PeerConnection`] owns the
//! transport: keep it alive for as long as any channel (or its
//! [`split`](DataChannel::split) halves) is in use — dropping it hangs up and
//! every channel goes dead.

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

/// Peer connection lifecycle, as coarse states the consumer can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    /// ICE lost contact with the peer. The driver holds this state for a
    /// grace period before giving the connection up as [`Failed`](Self::Failed).
    Disconnected,
    Failed,
    Closed,
}

/// A STUN or TURN server, in the same shape the signaling server hands out:
/// `stun:`/`turn:` URLs plus optional long-term credentials.
#[derive(Debug, Clone, Default)]
pub struct IceServer {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

/// Restrict which candidates are gathered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportPolicy {
    /// Direct (host/server-reflexive) and relayed candidates.
    #[default]
    All,
    /// TURN relay only.
    Relay,
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

/// One data channel's shape. Both peers must declare the same set (matched by
/// `label`, which therefore has to be unique within the slice); whichever side
/// ends up opening them carries these settings over DCEP and the other side
/// binds each incoming channel to its declaration by label.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub label: String,
    /// In-order delivery. An unordered channel delivers messages as they
    /// arrive.
    pub ordered: bool,
    /// Retransmit until delivered. An unreliable channel never retransmits —
    /// a lost datagram is simply gone — and its send side sheds instead of
    /// blocking when the transport backs up (see [`DataChannelSender::send`]).
    pub reliable: bool,
}

impl ChannelConfig {
    fn to_str0m(&self) -> str0m::channel::ChannelConfig {
        str0m::channel::ChannelConfig {
            label: self.label.clone(),
            ordered: self.ordered,
            reliability: if self.reliable {
                str0m::channel::Reliability::Reliable
            } else {
                str0m::channel::Reliability::MaxRetransmits { retransmits: 0 }
            },
            // In-band DCEP, never out-of-band `negotiated` stream ids: with
            // both peers offering (the signaling path's glare) a pre-agreed
            // stream id would have to survive one side's rollback, whereas
            // DCEP just lets whoever's offer wins open the channels and the
            // other side bind them by label.
            negotiated: None,
            protocol: String::new(),
        }
    }
}

/// Which end of a direct ([`PeerConnection::new_direct`]) connection this is.
/// The link code is the host's `addr:port`; there's no signaling exchange, so
/// everything else (ICE credentials, DTLS/SCTP roles) comes from fixed
/// constants both ends already agree on.
#[derive(Debug, Clone)]
pub enum DirectRole {
    /// Listen on `port` (the only thing that needs port-forwarding). The host
    /// learns the dialer's address peer-reflexively from its first ICE check.
    Host { port: u16 },
    /// Dial the host at `remote` (its `addr:port`).
    Connect { remote: std::net::SocketAddr },
}

/// Events forwarded from the driver task onto the peer-connection's event
/// channel.
#[derive(Debug)]
pub enum PeerConnectionEvent {
    ConnectionStateChange(ConnectionState),
    /// A freshly gathered local candidate, as an RFC 5245 candidate attribute
    /// value (`candidate:...`), ready to trickle to the peer over signaling.
    IceCandidate(String),
}

/// Why the transport died, when it died on its own (deliberate teardown
/// reports `Closed`, not a failure). Reaches the consumer boxed inside the
/// `std::io::Error` that [`DataChannel::send`] returns.
#[derive(Debug, Clone)]
enum Failure {
    /// The str0m state machine errored while being fed or polled. `Arc`
    /// because this travels through a watch channel ([`ChannelStatus`] is
    /// `Clone`) and `RtcError` isn't.
    Rtc(Arc<str0m::RtcError>),
    /// The `Rtc` instance reported itself dead without an explicit close.
    Died,
    /// ICE sat in `Disconnected` past the grace period.
    IceDisconnected,
    /// No connection within the post-exchange timeout.
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

/// The lifecycle of one data channel as observed by its send side.
#[derive(Debug, Clone)]
enum ChannelStatus {
    Pending,
    Open,
    Closed,
    Error(Failure),
}

/// One declared channel's binding state: the config it was declared with,
/// plus the str0m channel id once it's open (matched by label).
struct ChannelState {
    config: ChannelConfig,
    id: Option<str0m::channel::ChannelId>,
}

/// State shared between the user-facing [`PeerConnection`] (sync methods) and
/// the driver task. All str0m access happens under this lock; the driver
/// re-polls after every mutation (via [`Shared::notify`]).
struct Inner {
    rtc: str0m::Rtc,
    /// One entry per requested channel, in constructor order.
    channels: Vec<ChannelState>,
    /// Our un-answered offer, from construction until the exchange resolves
    /// it — completed by an incoming `Answer`, or discarded (str0m treats it
    /// as rolled back) when an incoming `Offer` makes us the answering side.
    pending_offer: Option<str0m::change::SdpPendingOffer>,
    local_desc: Option<SessionDescription>,
    remote_desc: Option<SessionDescription>,
    /// Local candidates the ICE agent accepted, for selected-pair reporting.
    local_candidates: Vec<str0m::Candidate>,
    /// Remote candidates — trickled in or embedded in the remote SDP —
    /// likewise.
    remote_candidates: Vec<str0m::Candidate>,
    /// Where to send a transmit, keyed by its source address: a host socket,
    /// or a TURN allocation for relayed candidates. Grows as gathering finds
    /// them.
    routes: std::collections::HashMap<std::net::SocketAddr, driver::Route>,
    /// (source, destination) of the most recent non-STUN (i.e. DTLS)
    /// transmit: str0m only sends application traffic over the nominated
    /// pair, so this is the selected path.
    current_path: Option<(std::net::SocketAddr, std::net::SocketAddr)>,
}

struct Shared {
    inner: Mutex<Inner>,
    notify: tokio::sync::Notify,
}

fn other_err<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

/// The connection half: it owns the transport, drives the SDP/candidate
/// exchange on the signaling path, and reports on the ICE outcome, while the
/// [`DataChannel`]s created alongside it carry the data. Keep it alive for as
/// long as any channel is in use; dropping it hangs up (DTLS close_notify).
pub struct PeerConnection {
    shared: Arc<Shared>,
    /// Dropping this (i.e. dropping the `PeerConnection`) tears down the
    /// driver task and with it the whole transport.
    _shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl PeerConnection {
    /// Create a connection and its data channels for the signaling path, and
    /// spawn the driver task on the current Tokio runtime.
    ///
    /// The local offer (declaring every channel) is built synchronously:
    /// [`local_description`](Self::local_description) is `Some` on return, so
    /// the caller can ship it immediately. Candidate gathering starts in the
    /// background and trickles [`PeerConnectionEvent::IceCandidate`] events as
    /// it goes.
    ///
    /// Both peers construct this the same way — each holding its own offer —
    /// and the signaling exchange breaks the tie: the side that receives the
    /// peer's `Offer` answers it (its own offer is rolled back), the side that
    /// receives an `Answer` completes its offer. See
    /// [`set_remote_description`](Self::set_remote_description).
    pub fn new(
        config: RtcConfig,
        channels: &[ChannelConfig],
    ) -> Result<(Self, Vec<DataChannel>, tokio::sync::mpsc::Receiver<PeerConnectionEvent>), std::io::Error> {
        if channels.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "at least one channel is required",
            ));
        }
        let mut rtc = str0m::Rtc::builder().build(std::time::Instant::now());

        // Declare every channel in our offer. If the exchange rolls this
        // offer back (we turn out to be the answering side), the declarations
        // go with it and the peer's identically-labelled DCEP opens replace
        // them.
        let mut api = rtc.sdp_api();
        for config in channels {
            api.add_channel_with_config(config.to_str0m());
        }
        let (offer, pending) = api
            .apply()
            .expect("adding a channel is a change, so apply() produces an offer");
        let local_desc = SessionDescription {
            sdp_type: SdpType::Offer,
            sdp: offer.to_sdp_string(),
        };

        Self::spawn(rtc, config, channels, Some(pending), Some(local_desc), driver::Setup::Signaled)
    }

    /// Create a connection over the **direct** (link-code) path: no signaling
    /// at all. Both peers configure ICE/DTLS/SCTP locally from fixed shared
    /// constants and the host's `addr:port` (see [`DirectRole`]) — DTLS
    /// fingerprint verification is off, so the trust model is "address =
    /// identity". The dialer opens the channels over DCEP and the host binds
    /// each by label. Drives itself to `Connected` with no exchange;
    /// [`local_description`](Self::local_description) stays `None`.
    pub fn new_direct(
        config: RtcConfig,
        channels: &[ChannelConfig],
        role: DirectRole,
    ) -> Result<(Self, Vec<DataChannel>, tokio::sync::mpsc::Receiver<PeerConnectionEvent>), std::io::Error> {
        let rtc = str0m::Rtc::builder()
            .set_fingerprint_verification(false)
            .build(std::time::Instant::now());
        Self::spawn(rtc, config, channels, None, None, driver::Setup::Direct(role))
    }

    /// Shared tail of [`new`](Self::new) / [`new_direct`](Self::new_direct):
    /// build the shared state and per-channel pipelines, then spawn the driver.
    fn spawn(
        rtc: str0m::Rtc,
        config: RtcConfig,
        channels: &[ChannelConfig],
        pending_offer: Option<str0m::change::SdpPendingOffer>,
        local_desc: Option<SessionDescription>,
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
                pending_offer,
                local_desc,
                remote_desc: None,
                local_candidates: vec![],
                remote_candidates: vec![],
                routes: Default::default(),
                current_path: None,
            }),
            notify: tokio::sync::Notify::new(),
        });

        let (event_tx, event_rx) = tokio::sync::mpsc::channel(64);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        // One independent pipeline per channel — the driver gets the I/O
        // halves, the caller gets the `DataChannel`s — so a backed-up
        // reliable channel can't clog the unreliable one.
        let mut data_channels = Vec::with_capacity(channels.len());
        let mut channel_ios = Vec::with_capacity(channels.len());
        for config in channels {
            let (outgoing_tx, outgoing_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
            // Inbound is unbounded: delivery must never block the driver — a
            // consumer that's slow to read would otherwise stall the transport
            // itself (no SACKs, no ICE keepalives, dead connection). Messages
            // are tiny; let them queue.
            let (message_tx, message_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
            let (status_tx, status_rx) = tokio::sync::watch::channel(ChannelStatus::Pending);
            data_channels.push(DataChannel {
                sender: DataChannelSender {
                    outgoing_tx,
                    status_rx,
                    lossy: !config.reliable,
                },
                receiver: DataChannelReceiver { message_rx },
            });
            channel_ios.push(driver::ChannelIo {
                status_tx,
                message_tx: Some(message_tx),
                outgoing_rx,
            });
        }

        runtime.spawn(driver::run(driver::Driver {
            shared: shared.clone(),
            config,
            event_tx,
            channels: channel_ios,
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

    /// Apply the remote description. An `Offer` makes us the answering side:
    /// our own pending offer is rolled back (with its channel declarations —
    /// the peer's replace them) and the answer is generated, available via
    /// [`local_description`](Self::local_description). An `Answer` completes
    /// our offer.
    pub fn set_remote_description(&mut self, desc: SessionDescription) -> Result<(), std::io::Error> {
        let mut inner = self.shared.inner.lock().unwrap();

        // Re-base str0m's clock to the present before the exchange inits
        // DTLS. While we waited for the peer the driver held str0m's clock
        // still (ticking it would spend the DTLS handshake's connect budget
        // against the wait — a long lobby sit would "time out" a connection
        // that never got to start). Accepting the offer/answer arms that
        // timeout relative to the clock, so hand it the real present first.
        // (str0m ignores a timeout that would move its clock backwards, so
        // this is a no-op if the driver already advanced it.)
        let _ = inner.rtc.handle_input(str0m::Input::Timeout(std::time::Instant::now()));

        match desc.sdp_type {
            SdpType::Offer => {
                let offer = str0m::change::SdpOffer::from_sdp_string(&desc.sdp).map_err(other_err)?;
                let answer = inner.rtc.sdp_api().accept_offer(offer).map_err(other_err)?;
                // str0m treats our pending offer as rolled back; drop the
                // stale completion token.
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
        // Candidates embedded in the SDP (a non-trickling peer, or ones that
        // made it in before the description was sent) are applied by str0m
        // itself; pull them out for selected-pair reporting only.
        let embedded = parse_sdp_candidates(&desc.sdp);
        inner.remote_candidates.extend(embedded);
        inner.remote_desc = Some(desc);
        drop(inner);
        self.shared.notify.notify_one();
        Ok(())
    }

    /// Feed one of the peer's trickled ICE candidates, as an RFC 5245
    /// candidate attribute (with or without the leading `a=`).
    pub fn add_remote_candidate(&mut self, candidate: &str) -> Result<(), std::io::Error> {
        let payload = candidate.trim().trim_start_matches("a=");
        let candidate = str0m::Candidate::from_sdp_string(payload).map_err(other_err)?;
        let mut inner = self.shared.inner.lock().unwrap();
        inner.rtc.add_remote_candidate(candidate.clone());
        inner.remote_candidates.push(candidate);
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
    /// `(local, remote)` — e.g. for telling a relayed (TURN) connection from
    /// a direct one (`typ relay`). Errors until the agent has picked a pair.
    pub fn selected_candidate_pair(&self) -> Result<(String, String), std::io::Error> {
        let inner = self.shared.inner.lock().unwrap();
        let (source, destination) = inner
            .current_path
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::WouldBlock, "no nominated pair yet"))?;

        // The transmit source is the candidate base: the relay address for
        // relayed candidates, the local socket for host ones. (A
        // server-reflexive path shows up under its host candidate, which is
        // fine — relay detection is what matters here.)
        let local = inner
            .local_candidates
            .iter()
            .find(|c| c.kind() == str0m::CandidateKind::Relayed && c.addr() == source)
            .or_else(|| inner.local_candidates.iter().find(|c| c.addr() == source))
            .map(|c| c.to_sdp_string())
            .unwrap_or_else(|| synthesize_candidate(source, "host"));

        // A destination that doesn't appear among the remote's candidates is
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
    /// This can't be left to the driver task — on process exit the runtime is
    /// torn down by *dropping* its tasks, so the driver may never be polled
    /// again; anything that must reach the remote has to go out inline here,
    /// before `_shutdown_tx` drops. The remote turns close_notify into a
    /// prompt EOF instead of sitting out its disconnect grace. Best effort:
    /// one unacknowledged datagram.
    fn drop(&mut self) {
        let mut inner = self.shared.inner.lock().unwrap();
        if !inner.rtc.is_alive() {
            return;
        }
        if let Err(e) = inner.rtc.close() {
            log::debug!("rtc.close: {}", e);
            return;
        }
        // Drain the close_notify out; close() flips the instance to
        // not-alive once fully polled.
        loop {
            let mut sent_any = false;
            loop {
                match inner.rtc.poll_output() {
                    Ok(str0m::Output::Transmit(t)) => {
                        let route = inner.routes.get(&t.source).cloned();
                        driver::send_transmit_sync(route, t.source, t.destination, &t.contents);
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
            let attr = line.trim().strip_prefix("a=")?;
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

/// The data half of a connection: `await`-able sends and receives, splittable
/// into its two halves. The [`PeerConnection`] owns the transport — the
/// channel goes dead if it isn't kept alive alongside.
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
    status_rx: tokio::sync::watch::Receiver<ChannelStatus>,
    /// The channel tolerates loss (`reliable: false`), so a send may drop
    /// rather than block when the pipeline is full. Reliable channels keep
    /// blocking backpressure.
    lossy: bool,
}

impl DataChannelSender {
    pub async fn send(&mut self, msg: &[u8]) -> Result<(), std::io::Error> {
        // Block on the first send until the channel leaves `Pending` (opens
        // or dies); once it's `Open` this returns immediately on every later
        // send.
        let status = match self
            .status_rx
            .wait_for(|s| !matches!(s, ChannelStatus::Pending))
            .await
        {
            Ok(s) => s.clone(),
            // The status sender is gone, i.e. the driver shut down.
            Err(_) => ChannelStatus::Closed,
        };

        match status {
            ChannelStatus::Open => {}
            ChannelStatus::Error(err) => return Err(std::io::Error::other(err)),
            ChannelStatus::Closed => {
                return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected"))
            }
            ChannelStatus::Pending => unreachable!("wait_for guarantees we left Pending"),
        }

        if self.lossy {
            // Never block a lossy channel's sender: under a full pipeline (a
            // congested SCTP send buffer backing up through the bounded
            // channel) drop this message rather than stalling the caller —
            // the in-match protocol's redundancy window recovers the loss,
            // and the send cadence keeps flowing instead of being held
            // behind backpressure.
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
    /// Receive one message; `None` is EOF (the channel closed or the
    /// connection went away).
    pub async fn receive(&mut self) -> Option<Vec<u8>> {
        self.message_rx.recv().await
    }

    pub fn unsplit(self, tx: DataChannelSender) -> DataChannel {
        tx.unsplit(self)
    }
}

//! The per-connection driver task: gathers candidates, then pumps the
//! str0m state machine — socket I/O in, transmits out, events onto the
//! wrapper's channels — until the peer connection is dropped or the
//! connection dies.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{gather, ConnectionState, DataChannelStatus, Failure, GatheringState, Inner, PeerConnectionEvent, Shared};

/// How long ICE may sit in `Disconnected` before we give the connection up
/// for dead. str0m's ICE never reaches a terminal failed state on its own.
const DISCONNECT_GRACE: Duration = Duration::from_secs(10);
/// How long after the SDP exchange we wait for the first connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Big enough for any UDP datagram a WebRTC stack will produce.
const MAX_DATAGRAM: usize = 2048;

/// The driver side of one data channel's pipelines, handed over from
/// [`crate::PeerConnection::new`]. There is one per requested channel, in the
/// same order as [`Inner::channels`].
pub(crate) struct ChannelIo {
    pub status_tx: tokio::sync::watch::Sender<DataChannelStatus>,
    pub message_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    pub outgoing_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
}

pub(crate) struct Driver {
    pub shared: Arc<Shared>,
    pub config: crate::RtcConfig,
    pub event_tx: tokio::sync::mpsc::Sender<PeerConnectionEvent>,
    pub channels: Vec<ChannelIo>,
    pub shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    pub setup: Setup,
}

/// How the connection is brought up before the shared steady-state loop.
pub(crate) enum Setup {
    /// Matchmaking path: gather ICE candidates, declare an SDP offer, and let
    /// the signaling exchange (`set_remote_description`) drive ICE/DTLS.
    Signaled,
    /// Direct link-code path: no signaling server. Configure str0m's ICE/DTLS/
    /// SCTP and channels directly (str0m `direct_api`) from fixed shared
    /// constants + the host's `addr:port`. See [`direct_setup`].
    Direct(crate::DirectRole),
}

/// Where to send a transmit, keyed by its source address: a host socket,
/// or a TURN allocation for relayed candidates.
pub(crate) enum Route {
    Socket(Arc<tokio::net::UdpSocket>),
    Relay(Arc<dyn webrtc_util::Conn + Send + Sync>),
}

/// A datagram from a reader task: (remote, local-candidate-base, payload,
/// receive time).
type Incoming = (SocketAddr, SocketAddr, Vec<u8>, Instant);

enum Exit {
    /// Deliberate teardown: the peer connection was dropped.
    Closed,
    /// The connection died underneath us.
    Failed(Failure),
}

/// The driver's per-channel loop state: the I/O ends from [`ChannelIo`] plus
/// the outbox/done bookkeeping the main loop keeps for each channel.
struct ChannelLoop {
    status_tx: tokio::sync::watch::Sender<DataChannelStatus>,
    /// Moved out of [`ChannelIo`]; dropping it on close is an immediate EOF
    /// for the matching `receive()`, even though the driver keeps running.
    message_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    outgoing_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    outbox: Outbox,
    /// The `DataChannelSender` was dropped; stop pulling from `outgoing_rx`.
    outgoing_done: bool,
}

impl ChannelLoop {
    fn new(io: ChannelIo) -> Self {
        ChannelLoop {
            status_tx: io.status_tx,
            message_tx: io.message_tx,
            outgoing_rx: io.outgoing_rx,
            outbox: Outbox::default(),
            outgoing_done: false,
        }
    }
}

pub(crate) async fn run(mut driver: Driver) {
    let gathered = match std::mem::replace(&mut driver.setup, Setup::Signaled) {
        // Matchmaking: gather, declare the offer, signal.
        Setup::Signaled => {
            let _ = driver
                .event_tx
                .send(PeerConnectionEvent::GatheringStateChange(GatheringState::InProgress))
                .await;

            let gathered = tokio::select! {
                gathered = gather::gather(&driver.config) => gathered,
                // Dropped before gathering even finished: nothing to unwind.
                _ = &mut driver.shutdown_rx => {
                    for ch in &driver.channels {
                        ch.status_tx.send_replace(DataChannelStatus::Closed);
                    }
                    return;
                }
            };

            {
                let mut inner = driver.shared.inner.lock().unwrap();
                for candidate in &gathered.candidates {
                    if inner.rtc.add_local_candidate(candidate.clone()).is_some() {
                        inner.local_candidates.push(candidate.clone());
                    }
                }
                inner.gathering_complete = true;
                Shared::maybe_make_offer(&mut inner);
            }
            let _ = driver
                .event_tx
                .send(PeerConnectionEvent::GatheringStateChange(GatheringState::Complete))
                .await;
            gathered
        }
        // Direct link-code: configure str0m directly, no signaling/offer.
        Setup::Direct(role) => match direct_setup(&driver, role).await {
            Ok(gathered) => gathered,
            Err(e) => {
                log::warn!("direct connection setup failed: {}", e);
                for ch in &driver.channels {
                    ch.status_tx.send_replace(DataChannelStatus::Closed);
                }
                let _ = driver
                    .event_tx
                    .send(PeerConnectionEvent::ConnectionStateChange(ConnectionState::Failed))
                    .await;
                return;
            }
        },
    };

    // Reader tasks: one per socket/relay, all funneling into net_rx. They
    // end when their socket dies or when we drop net_rx at function exit.
    let (net_tx, mut net_rx) = tokio::sync::mpsc::channel::<Incoming>(256);
    let mut readers = vec![];
    let mut routes: HashMap<SocketAddr, Route> = HashMap::new();

    for socket in &gathered.sockets {
        let Ok(local) = socket.local_addr() else {
            continue;
        };
        routes.insert(local, Route::Socket(socket.clone()));
        let socket = socket.clone();
        let net_tx = net_tx.clone();
        readers.push(tokio::spawn(async move {
            let mut buf = vec![0u8; MAX_DATAGRAM];
            loop {
                let Ok((n, from)) = socket.recv_from(&mut buf).await else {
                    break;
                };
                if net_tx
                    .send((from, local, buf[..n].to_vec(), Instant::now()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
    for relay in &gathered.relays {
        routes.insert(relay.addr, Route::Relay(relay.conn.clone()));
        let conn = relay.conn.clone();
        let local = relay.addr;
        let net_tx = net_tx.clone();
        readers.push(tokio::spawn(async move {
            let mut buf = vec![0u8; MAX_DATAGRAM];
            loop {
                let Ok((n, from)) = conn.recv_from(&mut buf).await else {
                    break;
                };
                if net_tx
                    .send((from, local, buf[..n].to_vec(), Instant::now()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
    drop(net_tx);

    // Share the route map with `PeerConnection`'s `Drop`, which uses it to
    // get a close_notify out synchronously when this task will never be
    // polled again (process exit tears the runtime down by dropping tasks).
    let routes = Arc::new(routes);
    driver.shared.inner.lock().unwrap().routes = Some(routes.clone());

    // Take the channel I/O ends out of the driver for the loop's lifetime.
    let mut channels: Vec<ChannelLoop> = driver.channels.drain(..).map(ChannelLoop::new).collect();

    let exit = main_loop(&mut driver, &routes, &mut net_rx, &mut channels).await;

    // Teardown.
    for reader in &readers {
        reader.abort();
    }
    for relay in &gathered.relays {
        let _ = relay.client.close().await;
    }
    {
        // Inert from here on: no more output is produced, and stray API
        // calls fail like they would against a closed connection.
        let mut inner = driver.shared.inner.lock().unwrap();
        inner.rtc.disconnect();
    }

    let (status, event) = match exit {
        Exit::Closed => (DataChannelStatus::Closed, ConnectionState::Closed),
        Exit::Failed(reason) => (DataChannelStatus::Error(reason), ConnectionState::Failed),
    };
    for ch in &channels {
        ch.status_tx.send_if_modified(|s| {
            // Don't clobber a more specific terminal status (e.g. Closed from
            // the remote's channel close) with a generic one.
            if matches!(s, DataChannelStatus::Pending | DataChannelStatus::Open) {
                *s = status.clone();
                true
            } else {
                false
            }
        });
    }
    let _ = driver
        .event_tx
        .send(PeerConnectionEvent::ConnectionStateChange(event))
        .await;
    // `channels` (and every `message_tx`) drops here, signaling EOF to each
    // `receive()`.
}

/// Direct (link-code) connection setup: no signaling. Both peers configure
/// str0m's ICE/DTLS/SCTP and the negotiated channels directly from fixed shared
/// constants plus the host's `addr:port`. The dialer is ICE controlling + DTLS/
/// SCTP active; the host is the ICE-lite controlled responder + passive and
/// learns the dialer peer-reflexively from its first check. Mirrors str0m's own
/// `tests/data-channel-direct.rs`. Returns the bound sockets for the shared
/// loop (no STUN/TURN relays on this path).
async fn direct_setup(driver: &Driver, role: crate::DirectRole) -> std::io::Result<gather::Gathered> {
    let io_other = |e: str0m::RtcError| std::io::Error::other(e.to_string());

    // Role sets the listen port (host = the known forwarded `port`, dialer =
    // ephemeral), the dialer's target candidate, and the ICE/DTLS/SCTP roles.
    let (port, remote_candidate, controlling) = match role {
        crate::DirectRole::Host { port } => (port, None, false),
        crate::DirectRole::Connect { remote } => {
            let candidate = str0m::Candidate::host(remote, "udp").map_err(|e| std::io::Error::other(e.to_string()))?;
            (0, Some(candidate), true)
        }
    };

    // One concrete host candidate per usable interface — str0m rejects an
    // unspecified-IP candidate, so we can't bind `0.0.0.0`. (The host's are all
    // on the known `port`; the dialer's get ephemeral ports.) The host learns
    // the dialer peer-reflexively, so it needs no remote candidate.
    //
    // Always include loopback here, regardless of `config.include_loopback`:
    // `/connect 127.0.0.1` (two instances on one machine) is a legitimate direct
    // target, and a loopback candidate is harmless for a real cross-machine
    // connection — a check from `127.0.0.1` to a routable remote just fails
    // silently while the real-interface candidate wins the pair.
    let mut sockets = vec![];
    let mut local_candidates = vec![];
    for ip in gather::local_ips(true) {
        let socket = match tokio::net::UdpSocket::bind(SocketAddr::new(ip, port)).await {
            Ok(socket) => Arc::new(socket),
            Err(e) => {
                log::debug!("direct: bind {}:{}: {}", ip, port, e);
                continue;
            }
        };
        if let Ok(local) = socket.local_addr() {
            if let Ok(candidate) = str0m::Candidate::host(local, "udp") {
                local_candidates.push(candidate);
            }
        }
        sockets.push(socket);
    }
    if sockets.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!("could not bind any direct socket (port {})", port),
        ));
    }

    let mut inner = driver.shared.inner.lock().unwrap();

    // Negotiated channels: both ends agree the stream ids (0, 1, …) up front, so
    // they open with no DCEP/SDP exchange. Collect before borrowing the api.
    let configs: Vec<str0m::channel::ChannelConfig> = inner
        .channels
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let mut config = c.config.clone();
            config.negotiated = Some(i as u16);
            config
        })
        .collect();

    {
        let mut direct = inner.rtc.direct_api();
        let (local_creds, remote_creds) = if controlling {
            (client_ice_creds(), host_ice_creds())
        } else {
            (host_ice_creds(), client_ice_creds())
        };
        direct.set_local_ice_credentials(local_creds);
        direct.set_remote_ice_credentials(remote_creds);
        // Address = identity: fingerprint verification is off (set in
        // `new_direct`), but str0m still needs *a* remote fingerprint set before
        // DTLS — hand it a throwaway.
        direct.set_remote_fingerprint(placeholder_fingerprint());
        direct.set_ice_controlling(controlling);
        if !controlling {
            // The host is the passive ICE-lite responder: it never sends checks,
            // only answers the dialer's — and learns the dialer's address
            // peer-reflexively from them.
            direct.set_ice_lite(true);
        }
        direct.start_dtls(controlling).map_err(io_other)?;
        direct.start_sctp(controlling);
        for config in configs {
            direct.create_data_channel(config);
        }
    }

    for candidate in &local_candidates {
        if inner.rtc.add_local_candidate(candidate.clone()).is_some() {
            inner.local_candidates.push(candidate.clone());
        }
    }
    if let Some(remote) = remote_candidate {
        inner.rtc.add_remote_candidate(remote.clone());
        inner.remote_candidates.push(remote);
    }
    inner.gathering_complete = true;
    // There's no SDP exchange on this path, but the main loop's lobby-wait clock
    // hold and its connect-timeout both key off `remote_desc`: a direct
    // connection is never "waiting in the lobby" and should arm its connect
    // timeout immediately, so plant a sentinel so it reads as "exchange done".
    inner.remote_desc = Some(crate::SessionDescription {
        sdp_type: crate::SdpType::Answer,
        sdp: String::new(),
    });

    Ok(gather::Gathered {
        sockets,
        candidates: local_candidates,
        relays: vec![],
    })
}

/// Fixed ICE credentials for the host end of a direct connection — both peers
/// hard-code the same two sets (host + dialer) so there's no exchange. Trust is
/// "whoever answered on that address" (see [`crate::PeerConnection::new_direct`]).
fn host_ice_creds() -> str0m::IceCreds {
    str0m::IceCreds {
        ufrag: "tangodirecthost".to_owned(),
        pass: "tango-direct-host-ice-credential".to_owned(),
    }
}

/// Fixed ICE credentials for the dialing end. See [`host_ice_creds`].
fn client_ice_creds() -> str0m::IceCreds {
    str0m::IceCreds {
        ufrag: "tangodirectpeer".to_owned(),
        pass: "tango-direct-peer-ice-credential".to_owned(),
    }
}

/// Throwaway remote DTLS fingerprint. `new_direct` turns fingerprint
/// verification off (the dialer can't know the host's per-session cert), but
/// str0m still needs the field populated before DTLS starts.
fn placeholder_fingerprint() -> str0m::config::Fingerprint {
    str0m::config::Fingerprint {
        hash_func: "sha-256".to_owned(),
        bytes: vec![0u8; 32],
    }
}

/// Feed one datagram into str0m.
fn feed(inner: &mut Inner, (source, destination, buf, at): Incoming) {
    let contents: str0m::net::DatagramRecv = match buf.as_slice().try_into() {
        Ok(contents) => contents,
        // Unrecognized packet type (not STUN/DTLS/RTP); drop.
        Err(_) => return,
    };
    if let Err(e) = inner.rtc.handle_input(str0m::Input::Receive(
        at,
        str0m::net::Receive {
            proto: str0m::net::Protocol::Udp,
            source,
            destination,
            contents,
        },
    )) {
        log::debug!("rtc.handle_input: {}", e);
    }
}

/// Advance str0m's clock to `now` ([`feed`]'s timeout sibling).
fn step_time(shared: &Shared, now: Instant) -> Result<(), Failure> {
    let mut inner = shared.inner.lock().unwrap();
    inner
        .rtc
        .handle_input(str0m::Input::Timeout(now))
        .map_err(|e| Failure::Rtc(Arc::new(e)))
}

async fn send_transmit(
    routes: &HashMap<SocketAddr, Route>,
    source: SocketAddr,
    destination: SocketAddr,
    payload: &[u8],
) {
    let Some(route) = routes.get(&source) else {
        log::debug!("no route for transmit source {}", source);
        return;
    };
    let result = match route {
        Route::Socket(socket) => socket.send_to(payload, destination).await.map_err(|e| e.to_string()),
        Route::Relay(conn) => conn.send_to(payload, destination).await.map_err(|e| e.to_string()),
    };
    if let Err(e) = result {
        // UDP send errors (e.g. unreachable) are routine during ICE; the
        // agent's own retries/timeouts deal with them.
        log::debug!("send_to {} -> {}: {}", source, destination, e);
    }
}

/// Non-blocking, runtime-independent variant of [`send_transmit`], for
/// [`crate::PeerConnection`]'s `Drop`. Host sockets use `try_send_to`
/// (a plain non-blocking syscall); relayed sends get the one poll an
/// established TURN channel needs (ChannelData wrap + non-blocking UDP
/// write) and are abandoned if they'd wait. Best effort by nature —
/// the remote's disconnect grace remains the fallback.
pub(crate) fn send_transmit_sync(
    routes: &HashMap<SocketAddr, Route>,
    source: SocketAddr,
    destination: SocketAddr,
    payload: &[u8],
) {
    let Some(route) = routes.get(&source) else {
        log::debug!("no route for transmit source {}", source);
        return;
    };
    let result = match route {
        Route::Socket(socket) => socket.try_send_to(payload, destination).map(|_| ()).map_err(|e| e.to_string()),
        Route::Relay(conn) => {
            let mut fut = conn.send_to(payload, destination);
            let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
            match fut.as_mut().poll(&mut cx) {
                std::task::Poll::Ready(r) => r.map(|_| ()).map_err(|e| e.to_string()),
                std::task::Poll::Pending => Err("would block".to_owned()),
            }
        }
    };
    if let Err(e) = result {
        log::debug!("sync send_to {} -> {}: {}", source, destination, e);
    }
}

/// The connection-liveness timers: the ICE disconnect grace and the
/// post-exchange connect timeout. Both are deadline-based failures, and
/// both the "has one expired?" check and the "when to wake up" computation
/// read off the same [`Liveness::deadlines`].
#[derive(Default)]
struct Liveness {
    /// `Event::Connected` has fired; disarms the connect timeout.
    connected: bool,
    /// When ICE entered `Disconnected`, if it's still there.
    disconnected_since: Option<Instant>,
    /// When the SDP exchange completed; arms the connect timeout.
    exchange_done_at: Option<Instant>,
}

impl Liveness {
    fn deadlines(&self) -> [Option<(Instant, Failure)>; 2] {
        [
            self.disconnected_since
                .map(|since| (since + DISCONNECT_GRACE, Failure::IceDisconnected)),
            if self.connected { None } else { self.exchange_done_at }
                .map(|at| (at + CONNECT_TIMEOUT, Failure::ConnectTimeout)),
        ]
    }

    /// The failure whose deadline has passed, if any.
    fn expired(&self, now: Instant) -> Option<Failure> {
        self.deadlines()
            .into_iter()
            .flatten()
            .find_map(|(at, failure)| (at <= now).then_some(failure))
    }

    /// The earliest pending deadline, to sleep until.
    fn next_deadline(&self) -> Option<Instant> {
        self.deadlines().into_iter().flatten().map(|(at, _)| at).min()
    }
}

/// Everything one full drain of the state machine wants done, collected
/// under the lock so [`dispatch`] can do the async parts (socket sends,
/// channel sends) without holding it. Channel-scoped fields carry the channel
/// index (into [`Inner::channels`] / the loop's `channels`).
#[derive(Default)]
struct Drained {
    transmits: Vec<(SocketAddr, SocketAddr, Vec<u8>)>,
    /// (channel index, message) for messages received on a bound channel.
    messages: Vec<(usize, Vec<u8>)>,
    /// Connection state changes, in the order they happened.
    states: Vec<ConnectionState>,
    /// Indices of channels that opened this drain.
    opened: Vec<usize>,
    /// Indices of channels that closed this drain.
    closed: Vec<usize>,
    /// The remote sent DTLS close_notify: it hung up.
    remote_closed: bool,
}

/// Drain the state machine: poll until str0m has nothing left but a
/// timeout, which is returned alongside the collected work. Connection
/// events also update `liveness`'s timers on the way through.
fn drain_rtc(inner: &mut Inner, liveness: &mut Liveness) -> Result<(Drained, Instant), Failure> {
    let mut drained = Drained::default();
    loop {
        let event = match inner.rtc.poll_output() {
            Ok(str0m::Output::Timeout(deadline)) => return Ok((drained, deadline)),
            Ok(str0m::Output::Transmit(t)) => {
                // Anything that isn't STUN (first byte < 20) is DTLS
                // application traffic, which str0m only sends over the
                // nominated pair — i.e. the selected path.
                if t.contents.first().is_some_and(|b| *b >= 20) {
                    inner.current_path = Some((t.source, t.destination));
                }
                drained.transmits.push((t.source, t.destination, t.contents.to_vec()));
                continue;
            }
            Ok(str0m::Output::Event(event)) => event,
            Err(e) => return Err(Failure::Rtc(Arc::new(e))),
        };

        if !matches!(event, str0m::Event::ChannelData(_)) {
            log::debug!("rtc event: {:?}", event);
        }
        match event {
            str0m::Event::Connected => {
                liveness.connected = true;
                liveness.disconnected_since = None;
                drained.states.push(ConnectionState::Connected);
            }
            str0m::Event::IceConnectionStateChange(state) => {
                use str0m::IceConnectionState::*;
                match state {
                    Checking => drained.states.push(ConnectionState::Connecting),
                    Connected | Completed => liveness.disconnected_since = None,
                    Disconnected => {
                        liveness.disconnected_since = Some(Instant::now());
                        drained.states.push(ConnectionState::Disconnected);
                    }
                    _ => {}
                }
            }
            // Bind the open channel to its spec by label — works both for our
            // own in-band channel on the offering side and the remote-declared
            // one after we answered.
            str0m::Event::ChannelOpen(id, label) => {
                if let Some(idx) = inner.channels.iter().position(|c| c.config.label == label) {
                    inner.channels[idx].id = Some(id);
                    drained.opened.push(idx);
                } else {
                    log::warn!("channel opened with unknown label {:?}", label);
                }
            }
            str0m::Event::ChannelData(data) => {
                if let Some(idx) = inner.channels.iter().position(|c| c.id == Some(data.id)) {
                    drained.messages.push((idx, data.data));
                }
            }
            // The remote sent DTLS close_notify: it hung up. Every channel dies.
            str0m::Event::Closed => {
                for ch in &mut inner.channels {
                    ch.id = None;
                }
                drained.remote_closed = true;
            }
            str0m::Event::ChannelClose(id) => {
                if let Some(idx) = inner.channels.iter().position(|c| c.id == Some(id)) {
                    inner.channels[idx].id = None;
                    drained.closed.push(idx);
                }
            }
            _ => {}
        }
    }
}

/// The handoff between a channel's `outgoing_rx` (async, pulled by the select)
/// and SCTP's send buffer (sans-IO, refusable try-writes): one message, parked
/// here until SCTP takes it.
///
/// There is no event for "send buffer freed" or "channel opened" to await,
/// so [`Outbox::flush`] runs on every loop pass and the retry rides
/// whatever woke the loop — usually the very SACK datagram that freed the
/// space. While a message is parked the select stops pulling from that
/// channel's `outgoing_rx` ([`Outbox::has_room`]), so a full SCTP buffer backs
/// up through the bounded channel and blocks that channel's `DataChannel::send`:
/// per-channel backpressure, with exactly one message of slack.
#[derive(Default)]
struct Outbox {
    parked: Option<Vec<u8>>,
}

impl Outbox {
    /// Move the parked message, if any, into channel `idx`'s SCTP stream.
    /// Not-writable-*yet* (channel not open, send buffer full) keeps it parked
    /// for the next pass; a vanished channel discards it.
    fn flush(&mut self, inner: &mut Inner, idx: usize) {
        let Some(message) = self.parked.take() else {
            return;
        };
        let Some(id) = inner.channels[idx].id else {
            // Channel not open yet.
            self.parked = Some(message);
            return;
        };
        let lossy = crate::is_lossy(inner.channels[idx].config.reliability);
        let Some(mut channel) = inner.rtc.channel(id) else {
            // Channel is gone; the message has nowhere to go.
            return;
        };
        // Keep a lossy channel's SCTP send queue shallow: once more than a
        // couple of frames are already waiting (cwnd-blocked), drop this one
        // rather than deepening the queue. Left unchecked the buffer can grow to
        // str0m's 128 KB cap — seconds of stale inputs that flush out in a
        // post-congestion burst and poison the rollback time-sync. The in-match
        // redundancy window + retransmit heartbeat recover the dropped frame;
        // `message.len()` tracks the current frame size, so a fat recovery frame
        // still gets a slot while a tiny steady-state frame sheds the instant the
        // link backs up. This is the shed-don't-stall behaviour the QUIC datagram
        // path has for free.
        //
        // Floored at LOSSY_SHED_FLOOR_BYTES so the smallest messages aren't
        // starved: a 3-byte ping/pong probe (which has no redundancy to recover
        // it) would otherwise gate at `2 * 3 = 6` bytes and shed the instant any
        // input frame sat in the buffer — so ping measurement never lands a
        // sample. The floor is a handful of frames' worth and still negligible
        // against the 128 KB cap, so the "don't bank seconds of stale inputs"
        // intent holds; fat recovery frames still get the larger `2 * len`.
        const LOSSY_SHED_FLOOR_BYTES: usize = 256;
        if lossy && channel.buffered_amount() > (2 * message.len()).max(LOSSY_SHED_FLOOR_BYTES) {
            return;
        }
        match channel.write(true, &message) {
            Ok(true) => {}
            // SCTP send buffer is full. A reliable channel parks and retries
            // (backpressure); a lossy one drops, same as the shallow-queue gate.
            Ok(false) if !lossy => self.parked = Some(message),
            Ok(false) => {}
            Err(e) => log::warn!("channel write failed: {}", e),
        }
    }

    fn has_room(&self) -> bool {
        self.parked.is_none()
    }

    fn park(&mut self, message: Vec<u8>) {
        debug_assert!(self.parked.is_none(), "outbox already occupied");
        self.parked = Some(message);
    }
}

/// Deliver everything a drain produced: transmits onto the wire, messages
/// and channel status onto each data-channel's halves, state changes onto the
/// event channel.
async fn dispatch(
    event_tx: &tokio::sync::mpsc::Sender<PeerConnectionEvent>,
    channels: &mut [ChannelLoop],
    routes: &HashMap<SocketAddr, Route>,
    drained: Drained,
) {
    for (source, destination, payload) in drained.transmits {
        send_transmit(routes, source, destination, &payload).await;
    }
    for idx in drained.opened {
        channels[idx].status_tx.send_replace(DataChannelStatus::Open);
    }
    for (idx, message) in drained.messages {
        if let Some(tx) = channels[idx].message_tx.as_ref() {
            // Unbounded and non-blocking by design: delivery may never stall
            // the driver, or the transport itself (SACKs, ICE keepalives) dies
            // with it. Err means that channel's receiver is gone — fine, the
            // other channels are independent.
            let _ = tx.send(message);
        }
    }
    for idx in drained.closed {
        channels[idx].status_tx.send_replace(DataChannelStatus::Closed);
        // EOF for that channel's `receive()`.
        channels[idx].message_tx = None;
    }
    for state in drained.states {
        let _ = event_tx
            .send(PeerConnectionEvent::ConnectionStateChange(state))
            .await;
    }
}

/// Wait for the next outgoing message on any channel that can currently accept
/// one (open slot in its `Outbox`, sender still alive). Resolves to the channel
/// index and the message (`None` = that channel's `DataChannelSender` was
/// dropped). Pends forever when no channel can accept right now — the loop's
/// other wakeups (an incoming SACK frees SCTP, emptying an `Outbox`) bring us
/// back around.
async fn recv_outgoing(channels: &mut [ChannelLoop]) -> (usize, Option<Vec<u8>>) {
    let futs: Vec<_> = channels
        .iter_mut()
        .enumerate()
        .filter(|(_, ch)| !ch.outgoing_done && ch.outbox.has_room())
        .map(|(i, ch)| {
            Box::pin(async move { (i, ch.outgoing_rx.recv().await) })
                as std::pin::Pin<Box<dyn std::future::Future<Output = (usize, Option<Vec<u8>>)> + Send>>
        })
        .collect();
    if futs.is_empty() {
        std::future::pending().await
    } else {
        futures_util::future::select_all(futs).await.0
    }
}

/// The driver's steady state: flush each channel's outbox into SCTP, drain
/// str0m, dispatch what it produced, check liveness, then sleep until the next
/// wakeup source — datagram, outgoing message, API call, timeout or shutdown —
/// and go again.
async fn main_loop(
    driver: &mut Driver,
    routes: &HashMap<SocketAddr, Route>,
    net_rx: &mut tokio::sync::mpsc::Receiver<Incoming>,
    channels: &mut [ChannelLoop],
) -> Exit {
    let mut net_done = false;
    let mut liveness = Liveness::default();

    loop {
        let waiting;
        let (drained, rtc_deadline) = {
            let mut inner = driver.shared.inner.lock().unwrap();
            waiting = inner.remote_desc.is_none();
            // Flush before draining, so a parked message rides this same pass
            // onto the wire. (Flushing after the drain would leave it sitting
            // in SCTP's send buffer until the next — possibly unrelated,
            // possibly hundreds of ms away — wakeup, which shows up directly
            // as latency jitter.)
            for (idx, ch) in channels.iter_mut().enumerate() {
                ch.outbox.flush(&mut inner, idx);
            }
            match drain_rtc(&mut inner, &mut liveness) {
                Ok(drained) => drained,
                Err(failure) => return Exit::Failed(failure),
            }
        };

        let remote_closed = drained.remote_closed;
        dispatch(&driver.event_tx, channels, routes, drained).await;
        if remote_closed {
            // The teardown in `run` delivers the Closed status/EOF/event.
            return Exit::Closed;
        }

        // Liveness.
        {
            let inner = driver.shared.inner.lock().unwrap();
            if !inner.rtc.is_alive() {
                return Exit::Failed(Failure::Died);
            }
            // The connect timeout runs from the end of the SDP exchange.
            if liveness.exchange_done_at.is_none() && inner.remote_desc.is_some() {
                liveness.exchange_done_at = Some(Instant::now());
            }
        }
        if let Some(failure) = liveness.expired(Instant::now()) {
            return Exit::Failed(failure);
        }

        // Until the peer shows up, str0m has nothing to do but tick the DTLS
        // handshake it optimistically starts at offer time — whose ~40s connect
        // timeout would otherwise drain against the lobby wait. So while there's
        // no remote description we hold str0m's clock still: don't advance time,
        // wake only on a real event (the remote arriving, a datagram,
        // shutdown). `PeerConnection::set_remote_description` re-bases the clock
        // to the present before it inits DTLS, so the handshake gets its full
        // budget from the real exchange.
        let wake = if waiting {
            None
        } else {
            Some(
                liveness
                    .next_deadline()
                    .map_or(rtc_deadline, |deadline| deadline.min(rtc_deadline)),
            )
        };

        if let Some(wake) = wake {
            if wake <= Instant::now() {
                if let Err(failure) = step_time(&driver.shared, Instant::now()) {
                    return Exit::Failed(failure);
                }
                continue;
            }
        }

        tokio::select! {
            _ = sleep_until_opt(wake) => {
                if let Err(failure) = step_time(&driver.shared, Instant::now()) {
                    return Exit::Failed(failure);
                }
            }
            incoming = net_rx.recv(), if !net_done => {
                match incoming {
                    // One datagram per pass: str0m wants poll_output fully
                    // drained after every handle_input, so no batching.
                    Some(incoming) => {
                        let mut inner = driver.shared.inner.lock().unwrap();
                        feed(&mut inner, incoming);
                    }
                    // All sockets are gone (e.g. no usable interfaces).
                    // The connect timeout reports the failure if ICE never
                    // makes it.
                    None => net_done = true,
                }
            }
            (idx, outgoing) = recv_outgoing(channels) => {
                match outgoing {
                    Some(message) => channels[idx].outbox.park(message),
                    // That channel's DataChannelSender was dropped.
                    None => channels[idx].outgoing_done = true,
                }
            }
            // An API call (offer/answer applied, channel created) changed
            // the state machine; re-poll it.
            _ = driver.shared.notify.notified() => {}
            _ = &mut driver.shutdown_rx => {
                graceful_close(driver, routes).await;
                return Exit::Closed;
            }
        }
    }
}

/// Sleep until `wake` (real time), or forever if `None` — used while we hold
/// str0m's clock and only want to wake on a real event.
async fn sleep_until_opt(wake: Option<Instant>) {
    match wake {
        Some(wake) => tokio::time::sleep_until(tokio::time::Instant::from_std(wake)).await,
        None => std::future::pending().await,
    }
}

/// On deliberate teardown, tell the remote we're hanging up — a DTLS
/// close_notify it turns into a prompt EOF (`Event::Closed`), rather than
/// waiting out its disconnect grace — and flush it onto the wire. Best
/// effort: it's a single unacknowledged datagram, and the disconnect grace
/// remains the remote's fallback.
async fn graceful_close(driver: &Driver, routes: &HashMap<SocketAddr, Route>) {
    {
        let mut inner = driver.shared.inner.lock().unwrap();
        if !inner.rtc.is_alive() {
            return;
        }
        if let Err(e) = inner.rtc.close() {
            log::debug!("rtc.close: {}", e);
            return;
        }
    }

    // Drain the close_notify out. close() flips the instance to not-alive
    // once everything pending has been polled.
    loop {
        let mut transmits: Vec<(SocketAddr, SocketAddr, Vec<u8>)> = vec![];
        {
            let mut inner = driver.shared.inner.lock().unwrap();
            loop {
                match inner.rtc.poll_output() {
                    Ok(str0m::Output::Transmit(t)) => {
                        transmits.push((t.source, t.destination, t.contents.to_vec()));
                    }
                    Ok(str0m::Output::Event(_)) => {}
                    Ok(str0m::Output::Timeout(_)) | Err(_) => break,
                }
            }
        }
        log::debug!("graceful close: flushing {} transmit(s)", transmits.len());
        if transmits.is_empty() {
            break;
        }
        for (source, destination, payload) in transmits {
            send_transmit(routes, source, destination, &payload).await;
        }
        if !driver.shared.inner.lock().unwrap().rtc.is_alive() {
            break;
        }
    }
}

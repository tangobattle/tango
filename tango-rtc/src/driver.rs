//! The per-connection driver task: registers sockets/candidates as gathering
//! finds them, then pumps the str0m state machine — datagrams in, transmits
//! out, events onto the wrapper's channels — until the peer connection is
//! dropped or the connection dies.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{gather, ChannelStatus, ConnectionState, Failure, Inner, PeerConnectionEvent, Shared};

/// How long ICE may sit in `Disconnected` before the connection is given up
/// for dead. str0m's ICE never reaches a terminal failed state on its own.
const DISCONNECT_GRACE: Duration = Duration::from_secs(10);
/// How long the connection attempt may run once both ends are committed (the
/// SDP exchange completed; or, direct: the dial started / the host heard the
/// first packet) before it's declared failed.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Big enough for any UDP datagram a WebRTC stack will produce.
const MAX_DATAGRAM: usize = 2048;

/// How the connection is brought up before the shared steady-state loop.
pub(crate) enum Setup {
    /// Signaling path: the offer was already made at construction; gathering
    /// trickles candidates and the exchange arrives via
    /// [`crate::PeerConnection::set_remote_description`].
    Signaled,
    /// Direct link-code path: no signaling. Configure str0m's ICE/DTLS/SCTP
    /// directly (str0m `direct_api`) from fixed shared constants + the host's
    /// `addr:port`. See [`direct_setup`].
    Direct(crate::DirectRole),
}

/// Where to send a transmit, keyed (in [`Inner::routes`]) by its source
/// address: a host socket, or a TURN allocation for relayed candidates.
#[derive(Clone)]
pub(crate) enum Route {
    Socket(Arc<tokio::net::UdpSocket>),
    Relay(Arc<dyn webrtc_util::Conn + Send + Sync>),
}

/// The driver side of one data channel's pipelines, handed over from
/// [`crate::PeerConnection`]'s constructors. One per requested channel, in
/// the same order as [`Inner::channels`].
pub(crate) struct ChannelIo {
    pub status_tx: tokio::sync::watch::Sender<ChannelStatus>,
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

/// A datagram from a reader task: (remote, local-candidate-base, payload,
/// receive time).
type Incoming = (SocketAddr, SocketAddr, Vec<u8>, Instant);

/// What must happen before str0m's clock is allowed to run (and, with it,
/// the connect timeout armed). Until then the driver never advances time on
/// its own: str0m would only tick the DTLS handshake it starts eagerly, whose
/// connect budget would drain against a wait that can last however long the
/// peer takes to show up.
#[derive(PartialEq, Eq)]
enum ClockGate {
    /// Signaling path: wait for the SDP exchange
    /// (`set_remote_description` re-bases the clock when it happens).
    RemoteDescription,
    /// Direct host: wait for the dialer's first datagram (feeding it advances
    /// the clock to its receive time, which is exactly the re-base we want).
    FirstPacket,
    /// Free-running (direct dialer, or the gate above has lifted).
    Open,
}

enum Exit {
    /// Deliberate teardown: the peer connection was dropped.
    Closed,
    /// The connection died underneath us.
    Failed(Failure),
}

pub(crate) async fn run(mut driver: Driver) {
    // Reader tasks all funnel into net_rx; new ones are spawned as gathering
    // produces sockets/relays. The driver keeps `net_tx` for the spawning, so
    // `net_rx` never yields `None`.
    let (net_tx, mut net_rx) = tokio::sync::mpsc::channel::<Incoming>(256);
    let mut readers: Vec<tokio::task::JoinHandle<()>> = vec![];
    // TURN clients must stay alive for their allocations to keep refreshing.
    let mut relays: Vec<turn::client::Client> = vec![];

    let (mut found_rx, clock_gate) = match std::mem::replace(&mut driver.setup, Setup::Signaled) {
        Setup::Signaled => (gather::spawn(driver.config.clone()), ClockGate::RemoteDescription),
        Setup::Direct(role) => {
            let gate = match &role {
                crate::DirectRole::Host { .. } => ClockGate::FirstPacket,
                // The dialer drives the ICE checks; its clock must run from
                // the start, and an unreachable host should fail the dial
                // within the connect timeout.
                crate::DirectRole::Connect { .. } => ClockGate::Open,
            };
            match direct_setup(&driver, role, &net_tx, &mut readers).await {
                Ok(()) => {
                    // Nothing to gather: a closed channel that reports
                    // "done" immediately.
                    let (tx, rx) = tokio::sync::mpsc::channel(1);
                    drop(tx);
                    (rx, gate)
                }
                Err(e) => {
                    log::warn!("direct connection setup failed: {}", e);
                    for ch in &driver.channels {
                        ch.status_tx.send_replace(ChannelStatus::Closed);
                    }
                    let _ = driver
                        .event_tx
                        .send(PeerConnectionEvent::ConnectionStateChange(ConnectionState::Failed))
                        .await;
                    return;
                }
            }
        }
    };

    let mut state = LoopState {
        clock_gate,
        liveness: Liveness::default(),
        gathering: true,
        channels: driver
            .channels
            .drain(..)
            .map(|io| ChannelLoop {
                status_tx: io.status_tx,
                message_tx: io.message_tx,
                outgoing_rx: io.outgoing_rx,
                parked: None,
                outgoing_done: false,
            })
            .collect(),
    };
    // The direct dialer is committed from the start; everyone else arms the
    // connect timeout when their gate lifts.
    if state.clock_gate == ClockGate::Open {
        state.liveness.committed_at = Some(Instant::now());
    }

    let exit = main_loop(
        &driver.shared,
        &driver.event_tx,
        &mut driver.shutdown_rx,
        &mut state,
        &mut net_rx,
        &net_tx,
        &mut found_rx,
        &mut readers,
        &mut relays,
    )
    .await;

    // Teardown. (On a deliberate close the DTLS close_notify already went out
    // inline in `PeerConnection::drop`, before our shutdown signal fired.)
    for reader in &readers {
        reader.abort();
    }
    for relay in &relays {
        let _ = relay.close().await;
    }
    {
        // Inert from here on: no more output is produced, and stray API calls
        // fail like they would against a closed connection.
        let mut inner = driver.shared.inner.lock().unwrap();
        inner.rtc.disconnect();
    }

    let (status, event) = match exit {
        Exit::Closed => (ChannelStatus::Closed, ConnectionState::Closed),
        Exit::Failed(reason) => (ChannelStatus::Error(reason), ConnectionState::Failed),
    };
    for ch in &state.channels {
        ch.status_tx.send_if_modified(|s| {
            // Don't clobber a more specific terminal status (e.g. Closed from
            // the remote's channel close) with a generic one.
            if matches!(s, ChannelStatus::Pending | ChannelStatus::Open) {
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
    // `state.channels` (and every `message_tx`) drops here, signaling EOF to
    // each `receive()`.
}

/// The driver's loop state: liveness timers plus each channel's I/O ends and
/// parked outbound message.
struct LoopState {
    clock_gate: ClockGate,
    liveness: Liveness,
    /// Gathering still running (`found_rx` not yet exhausted).
    gathering: bool,
    channels: Vec<ChannelLoop>,
}

struct ChannelLoop {
    status_tx: tokio::sync::watch::Sender<ChannelStatus>,
    /// Dropping it (channel close) is an immediate EOF for the matching
    /// `receive()`, even while the driver keeps running.
    message_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    outgoing_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    /// The handoff between `outgoing_rx` (async, pulled by the select) and
    /// SCTP's send buffer (sans-IO, refusable try-writes): one message,
    /// parked here until SCTP takes it. There is no "buffer freed" event to
    /// await — the flush retries on every loop pass, riding whatever woke the
    /// loop (usually the very SACK datagram that freed the space). While a
    /// message is parked, a reliable channel stops pulling from `outgoing_rx`
    /// (backpressure, one message of slack); a lossy channel keeps pulling
    /// and overwrites the slot, newest wins — the fresher frame's redundancy
    /// window re-carries the dropped one's inputs, so nothing stale banks up.
    parked: Option<Vec<u8>>,
    /// The `DataChannelSender` was dropped; stop pulling from `outgoing_rx`.
    outgoing_done: bool,
}

impl ChannelLoop {
    /// May this channel accept another message off its `outgoing_rx`?
    fn can_pull(&self, lossy: bool) -> bool {
        !self.outgoing_done && (lossy || self.parked.is_none())
    }

    /// Move the parked message, if any, into the channel's SCTP stream.
    /// Not-writable-*yet* (channel not open, send buffer full) keeps it
    /// parked for the next pass; a vanished channel discards it.
    fn flush(&mut self, inner: &mut Inner, idx: usize) {
        let Some(message) = self.parked.take() else {
            return;
        };
        let Some(id) = inner.channels[idx].id else {
            // Channel not open yet.
            self.parked = Some(message);
            return;
        };
        let Some(mut channel) = inner.rtc.channel(id) else {
            // Channel is gone; the message has nowhere to go.
            return;
        };
        match channel.write(true, &message) {
            Ok(true) => {}
            Ok(false) => self.parked = Some(message),
            Err(e) => log::warn!("channel write failed: {}", e),
        }
    }
}

/// The connection-liveness timers: the ICE disconnect grace and the connect
/// timeout. Both are deadline-based failures; the "has one expired?" check
/// and the "when to wake up" computation read the same deadlines.
#[derive(Default)]
struct Liveness {
    /// `Event::Connected` has fired; disarms the connect timeout.
    connected: bool,
    /// When ICE entered `Disconnected`, if it's still there.
    disconnected_since: Option<Instant>,
    /// When both ends became committed to connecting (exchange done / dial
    /// started / first packet heard); arms the connect timeout.
    committed_at: Option<Instant>,
}

impl Liveness {
    fn deadlines(&self) -> [Option<(Instant, Failure)>; 2] {
        [
            self.disconnected_since
                .map(|since| (since + DISCONNECT_GRACE, Failure::IceDisconnected)),
            if self.connected { None } else { self.committed_at }
                .map(|at| (at + CONNECT_TIMEOUT, Failure::ConnectTimeout)),
        ]
    }

    fn expired(&self, now: Instant) -> Option<Failure> {
        self.deadlines()
            .into_iter()
            .flatten()
            .find_map(|(at, failure)| (at <= now).then_some(failure))
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.deadlines().into_iter().flatten().map(|(at, _)| at).min()
    }
}

/// Everything one full drain of the state machine wants done, collected under
/// the lock so [`dispatch`] can do the async parts (socket sends, channel
/// sends) without holding it. Channel-scoped fields carry the channel index
/// (into [`Inner::channels`] / [`LoopState::channels`]).
#[derive(Default)]
struct Drained {
    /// (route, destination, payload) — the route already resolved from the
    /// transmit's source address while the lock was held.
    transmits: Vec<(Option<Route>, SocketAddr, Vec<u8>)>,
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

/// Drain the state machine: poll until str0m has nothing left but a timeout,
/// which is returned alongside the collected work. Connection events also
/// update `liveness`'s timers on the way through.
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
                let route = inner.routes.get(&t.source).cloned();
                drained.transmits.push((route, t.destination, t.contents.to_vec()));
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
            // Bind the opened channel to its declaration by label — ours if
            // our offer won (or we're the direct dialer), the remote's
            // otherwise.
            str0m::Event::ChannelOpen(id, label) => {
                if let Some(idx) = inner
                    .channels
                    .iter()
                    .position(|c| c.id.is_none() && c.config.label == label)
                {
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
            str0m::Event::ChannelClose(id) => {
                if let Some(idx) = inner.channels.iter().position(|c| c.id == Some(id)) {
                    inner.channels[idx].id = None;
                    drained.closed.push(idx);
                }
            }
            // The remote sent DTLS close_notify: it hung up. Every channel
            // dies with the connection.
            str0m::Event::Closed => {
                for ch in &mut inner.channels {
                    ch.id = None;
                }
                drained.remote_closed = true;
            }
            _ => {}
        }
    }
}

/// Deliver everything a drain produced: transmits onto the wire, messages and
/// channel status onto each data-channel's halves, state changes onto the
/// event channel.
async fn dispatch(
    event_tx: &tokio::sync::mpsc::Sender<PeerConnectionEvent>,
    channels: &mut [ChannelLoop],
    drained: Drained,
) {
    for (route, destination, payload) in drained.transmits {
        send_transmit(route, destination, &payload).await;
    }
    for idx in drained.opened {
        channels[idx].status_tx.send_replace(ChannelStatus::Open);
    }
    for (idx, message) in drained.messages {
        if let Some(tx) = channels[idx].message_tx.as_ref() {
            // Unbounded and non-blocking by design: delivery may never stall
            // the driver, or the transport itself (SACKs, ICE keepalives)
            // dies with it. Err means that channel's receiver is gone — fine,
            // the other channels are independent.
            let _ = tx.send(message);
        }
    }
    for idx in drained.closed {
        channels[idx].status_tx.send_replace(ChannelStatus::Closed);
        // EOF for that channel's `receive()`.
        channels[idx].message_tx = None;
    }
    for state in drained.states {
        let _ = event_tx
            .send(PeerConnectionEvent::ConnectionStateChange(state))
            .await;
    }
}

/// The driver's steady state: flush each channel's parked message into SCTP,
/// drain str0m, dispatch what it produced, check liveness, then sleep until
/// the next wakeup source — datagram, gathered candidate, outgoing message,
/// API call, timeout or shutdown — and go again.
#[allow(clippy::too_many_arguments)]
async fn main_loop(
    shared: &Arc<Shared>,
    event_tx: &tokio::sync::mpsc::Sender<PeerConnectionEvent>,
    shutdown_rx: &mut tokio::sync::oneshot::Receiver<()>,
    state: &mut LoopState,
    net_rx: &mut tokio::sync::mpsc::Receiver<Incoming>,
    net_tx: &tokio::sync::mpsc::Sender<Incoming>,
    found_rx: &mut tokio::sync::mpsc::Receiver<gather::Found>,
    readers: &mut Vec<tokio::task::JoinHandle<()>>,
    relays: &mut Vec<turn::client::Client>,
) -> Exit {
    // Immutable per-channel loss tolerance, read once (the configs live under
    // the lock, but this must be consultable without it).
    let lossy: Vec<bool> = {
        let inner = shared.inner.lock().unwrap();
        inner.channels.iter().map(|c| !c.config.reliable).collect()
    };

    loop {
        let waiting;
        let (drained, rtc_deadline) = {
            let mut inner = shared.inner.lock().unwrap();
            waiting = match state.clock_gate {
                ClockGate::RemoteDescription => inner.remote_desc.is_none(),
                ClockGate::FirstPacket => true, // lifted on first datagram below
                ClockGate::Open => false,
            };
            // Flush before draining, so a parked message rides this same pass
            // onto the wire rather than sitting in SCTP's buffer until the
            // next (possibly unrelated, possibly far-off) wakeup — that gap
            // shows up directly as latency jitter.
            for (idx, ch) in state.channels.iter_mut().enumerate() {
                ch.flush(&mut inner, idx);
            }
            match drain_rtc(&mut inner, &mut state.liveness) {
                Ok(ok) => ok,
                Err(failure) => return Exit::Failed(failure),
            }
        };

        let remote_closed = drained.remote_closed;
        dispatch(event_tx, &mut state.channels, drained).await;
        if remote_closed {
            // The teardown in `run` delivers the Closed status/EOF/event.
            return Exit::Closed;
        }

        // Liveness.
        {
            let inner = shared.inner.lock().unwrap();
            if !inner.rtc.is_alive() {
                return Exit::Failed(Failure::Died);
            }
            // The signaling path becomes committed when the exchange lands.
            if state.liveness.committed_at.is_none() && inner.remote_desc.is_some() {
                state.liveness.committed_at = Some(Instant::now());
            }
        }
        if let Some(failure) = state.liveness.expired(Instant::now()) {
            return Exit::Failed(failure);
        }

        // While the clock gate holds we never advance time on our own: wake
        // only on a real event (the peer arriving, a datagram, an API call,
        // shutdown). See [`ClockGate`].
        let wake = if waiting {
            None
        } else {
            Some(
                state
                    .liveness
                    .next_deadline()
                    .map_or(rtc_deadline, |deadline| deadline.min(rtc_deadline)),
            )
        };
        if let Some(wake) = wake {
            if wake <= Instant::now() {
                if let Err(failure) = step_time(shared, Instant::now()) {
                    return Exit::Failed(failure);
                }
                continue;
            }
        }

        tokio::select! {
            _ = sleep_until_opt(wake) => {
                if let Err(failure) = step_time(shared, Instant::now()) {
                    return Exit::Failed(failure);
                }
            }
            incoming = net_rx.recv() => {
                // One datagram per pass: str0m wants poll_output fully
                // drained after every handle_input, so no batching. `net_tx`
                // lives in this frame, so recv() can't yield `None`.
                if let Some(incoming) = incoming {
                    let recognized = {
                        let mut inner = shared.inner.lock().unwrap();
                        feed(&mut inner, incoming)
                    };
                    if recognized && state.clock_gate == ClockGate::FirstPacket {
                        // The dialer showed up: let the clock run (feeding
                        // the datagram advanced it to the receive time) and
                        // start the connect countdown. Gated on `recognized`
                        // so a stray scan datagram hitting the host's
                        // forwarded port can't arm the timeout against a
                        // host that's still legitimately waiting.
                        state.clock_gate = ClockGate::Open;
                        state.liveness.committed_at = Some(Instant::now());
                    }
                }
            }
            found = found_rx.recv(), if state.gathering => {
                match found {
                    Some(found) => register_found(shared, event_tx, found, net_tx, readers, relays).await,
                    None => {
                        state.gathering = false;
                        let inner = shared.inner.lock().unwrap();
                        log::info!(
                            "gathering complete; {} local candidate(s): [{}]",
                            inner.local_candidates.len(),
                            inner
                                .local_candidates
                                .iter()
                                .map(|c| c.to_sdp_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                }
            }
            (idx, outgoing) = recv_outgoing(&mut state.channels, &lossy) => {
                match outgoing {
                    // Lossy overwrite is deliberate: newest wins.
                    Some(message) => state.channels[idx].parked = Some(message),
                    None => state.channels[idx].outgoing_done = true,
                }
            }
            // An API call (exchange applied, candidate added) changed the
            // state machine; re-poll it.
            _ = shared.notify.notified() => {}
            _ = &mut *shutdown_rx => {
                // The close_notify already went out inline in
                // `PeerConnection::drop`.
                return Exit::Closed;
            }
        }
    }
}

/// Wait for the next outgoing message on any channel that can currently
/// accept one. Resolves to the channel index and the message (`None` = that
/// channel's `DataChannelSender` was dropped). Pends forever when no channel
/// can accept right now — the loop's other wakeups (an incoming SACK frees
/// SCTP, emptying a parked slot) bring us back around.
async fn recv_outgoing(channels: &mut [ChannelLoop], lossy: &[bool]) -> (usize, Option<Vec<u8>>) {
    let futs: Vec<_> = channels
        .iter_mut()
        .enumerate()
        .filter(|(idx, ch)| ch.can_pull(lossy[*idx]))
        .map(|(idx, ch)| {
            Box::pin(async move { (idx, ch.outgoing_rx.recv().await) })
                as std::pin::Pin<Box<dyn std::future::Future<Output = (usize, Option<Vec<u8>>)> + Send>>
        })
        .collect();
    if futs.is_empty() {
        std::future::pending().await
    } else {
        futures::future::select_all(futs).await.0
    }
}

/// Sleep until `wake` (real time), or forever if `None` — used while the
/// clock gate holds and only a real event should wake us.
async fn sleep_until_opt(wake: Option<Instant>) {
    match wake {
        Some(wake) => tokio::time::sleep_until(tokio::time::Instant::from_std(wake)).await,
        None => std::future::pending().await,
    }
}

/// Feed one datagram into str0m. Returns whether it was a recognized WebRTC
/// packet type (as opposed to arbitrary noise on the socket).
fn feed(inner: &mut Inner, (source, destination, buf, at): Incoming) -> bool {
    let contents: str0m::net::DatagramRecv = match buf.as_slice().try_into() {
        Ok(contents) => contents,
        // Unrecognized packet type (not STUN/DTLS/RTP); drop.
        Err(_) => return false,
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
    true
}

/// Advance str0m's clock to `now` ([`feed`]'s timeout sibling).
fn step_time(shared: &Shared, now: Instant) -> Result<(), Failure> {
    let mut inner = shared.inner.lock().unwrap();
    inner
        .rtc
        .handle_input(str0m::Input::Timeout(now))
        .map_err(|e| Failure::Rtc(Arc::new(e)))
}

/// Register one gathering find: route + reader task for anything that carries
/// traffic, candidate into the ICE agent, and — if the agent accepted it — an
/// `IceCandidate` event out to the consumer to trickle to the peer.
async fn register_found(
    shared: &Arc<Shared>,
    event_tx: &tokio::sync::mpsc::Sender<PeerConnectionEvent>,
    found: gather::Found,
    net_tx: &tokio::sync::mpsc::Sender<Incoming>,
    readers: &mut Vec<tokio::task::JoinHandle<()>>,
    relays: &mut Vec<turn::client::Client>,
) {
    let mut candidates = vec![];
    match found {
        gather::Found::Socket { socket, host, srflx } => {
            let Ok(local) = socket.local_addr() else { return };
            {
                let mut inner = shared.inner.lock().unwrap();
                inner.routes.insert(local, Route::Socket(socket.clone()));
            }
            readers.push(spawn_socket_reader(socket, local, net_tx.clone()));
            candidates.push(host);
            candidates.extend(srflx);
        }
        gather::Found::Relay {
            client,
            conn,
            addr,
            candidate,
        } => {
            {
                let mut inner = shared.inner.lock().unwrap();
                inner.routes.insert(addr, Route::Relay(conn.clone()));
            }
            readers.push(spawn_relay_reader(conn, addr, net_tx.clone()));
            relays.push(client);
            candidates.push(candidate);
        }
    }

    for candidate in candidates {
        let accepted = {
            let mut inner = shared.inner.lock().unwrap();
            if inner.rtc.add_local_candidate(candidate.clone()).is_some() {
                inner.local_candidates.push(candidate.clone());
                true
            } else {
                false
            }
        };
        if accepted {
            let _ = event_tx
                .send(PeerConnectionEvent::IceCandidate(candidate.to_sdp_string()))
                .await;
        }
    }
    shared.notify.notify_one();
}

fn spawn_socket_reader(
    socket: Arc<tokio::net::UdpSocket>,
    local: SocketAddr,
    net_tx: tokio::sync::mpsc::Sender<Incoming>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = vec![0u8; MAX_DATAGRAM];
        loop {
            let Ok((n, from)) = socket.recv_from(&mut buf).await else {
                break;
            };
            if net_tx.send((from, local, buf[..n].to_vec(), Instant::now())).await.is_err() {
                break;
            }
        }
    })
}

fn spawn_relay_reader(
    conn: Arc<dyn webrtc_util::Conn + Send + Sync>,
    local: SocketAddr,
    net_tx: tokio::sync::mpsc::Sender<Incoming>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = vec![0u8; MAX_DATAGRAM];
        loop {
            let Ok((n, from)) = conn.recv_from(&mut buf).await else {
                break;
            };
            if net_tx.send((from, local, buf[..n].to_vec(), Instant::now())).await.is_err() {
                break;
            }
        }
    })
}

async fn send_transmit(route: Option<Route>, destination: SocketAddr, payload: &[u8]) {
    let Some(route) = route else {
        log::debug!("no route for transmit to {}", destination);
        return;
    };
    let result = match route {
        Route::Socket(socket) => socket.send_to(payload, destination).await.map(|_| ()).map_err(|e| e.to_string()),
        Route::Relay(conn) => conn.send_to(payload, destination).await.map(|_| ()).map_err(|e| e.to_string()),
    };
    if let Err(e) = result {
        // UDP send errors (e.g. unreachable) are routine during ICE; the
        // agent's own retries/timeouts deal with them.
        log::debug!("send_to {}: {}", destination, e);
    }
}

/// Non-blocking, runtime-independent variant of [`send_transmit`], for
/// [`crate::PeerConnection`]'s `Drop`. Host sockets use `try_send_to` (a
/// plain non-blocking syscall); relayed sends get the one poll an established
/// TURN channel needs (ChannelData wrap + non-blocking UDP write) and are
/// abandoned if they'd wait. Best effort by nature — the remote's disconnect
/// grace remains the fallback.
pub(crate) fn send_transmit_sync(route: Option<Route>, source: SocketAddr, destination: SocketAddr, payload: &[u8]) {
    let Some(route) = route else {
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

/// Direct (link-code) connection setup: no signaling. Both peers configure
/// str0m's ICE/DTLS/SCTP directly from fixed shared constants plus the host's
/// `addr:port`. The dialer is ICE controlling + DTLS/SCTP active; the host is
/// the ICE-lite controlled responder + passive, and learns the dialer
/// peer-reflexively from its first check. Channels are opened over DCEP by
/// the dialer (the SCTP client); the host declares none and binds each
/// incoming one by label.
async fn direct_setup(
    driver: &Driver,
    role: crate::DirectRole,
    net_tx: &tokio::sync::mpsc::Sender<Incoming>,
    readers: &mut Vec<tokio::task::JoinHandle<()>>,
) -> std::io::Result<()> {
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
    // unspecified-IP candidate, so we can't just bind `0.0.0.0`. (The host's
    // are all on the known `port`; the dialer's get ephemeral ports.)
    //
    // Loopback is always included here, regardless of `include_loopback`:
    // `/connect 127.0.0.1` (two instances on one machine) is a legitimate
    // direct target, and a loopback candidate is harmless for a real
    // cross-machine connection — a check from `127.0.0.1` to a routable
    // remote just fails silently while the real-interface candidate wins.
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

    // The dialer (the SCTP client) opens every channel over DCEP; the host
    // creates nothing and binds the incoming opens by label. Collect before
    // borrowing the api.
    let configs: Vec<str0m::channel::ChannelConfig> = if controlling {
        inner.channels.iter().map(|c| c.config.to_str0m()).collect()
    } else {
        vec![]
    };

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
        // `new_direct`), but str0m still needs *a* remote fingerprint set
        // before DTLS — hand it a throwaway.
        direct.set_remote_fingerprint(placeholder_fingerprint());
        direct.set_ice_controlling(controlling);
        if !controlling {
            // The host is the passive ICE-lite responder: it never sends
            // checks, only answers the dialer's — and learns the dialer's
            // address peer-reflexively from them.
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

    for socket in sockets {
        let Ok(local) = socket.local_addr() else { continue };
        inner.routes.insert(local, Route::Socket(socket.clone()));
        readers.push(spawn_socket_reader(socket, local, net_tx.clone()));
    }

    Ok(())
}

/// Fixed ICE credentials for the host end of a direct connection — both peers
/// hard-code the same two sets (host + dialer) so there's no exchange. Trust
/// is "whoever answered on that address" (see
/// [`crate::PeerConnection::new_direct`]).
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

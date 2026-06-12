//! The per-connection driver task: gathers candidates, then pumps the
//! str0m state machine — socket I/O in, transmits out, events onto the
//! wrapper's channels — until the `PeerConnection` is dropped or the
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

pub(crate) struct Driver {
    pub shared: Arc<Shared>,
    pub config: crate::RtcConfig,
    pub event_tx: tokio::sync::mpsc::Sender<PeerConnectionEvent>,
    pub status_tx: tokio::sync::watch::Sender<DataChannelStatus>,
    pub message_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    pub outgoing_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    pub shutdown_rx: tokio::sync::oneshot::Receiver<()>,
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
    /// Deliberate teardown: the `PeerConnection` was dropped.
    Closed,
    /// The connection died underneath us.
    Failed(Failure),
}

pub(crate) async fn run(mut driver: Driver) {
    let _ = driver
        .event_tx
        .send(PeerConnectionEvent::GatheringStateChange(GatheringState::InProgress))
        .await;

    let gathered = tokio::select! {
        gathered = gather::gather(&driver.config) => gathered,
        // Dropped before gathering even finished: nothing to unwind.
        _ = &mut driver.shutdown_rx => {
            driver.status_tx.send_replace(DataChannelStatus::Closed);
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

    // Share the route map with `PeerConnection::drop`, which uses it to get
    // a close_notify out synchronously when this task will never be polled
    // again (process exit tears the runtime down by dropping tasks).
    let routes = Arc::new(routes);
    driver.shared.inner.lock().unwrap().routes = Some(routes.clone());

    let exit = main_loop(&mut driver, &routes, &mut net_rx).await;

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
    driver.status_tx.send_if_modified(|s| {
        // Don't clobber a more specific terminal status (e.g. Closed from
        // the remote's channel close) with a generic one.
        if matches!(s, DataChannelStatus::Pending | DataChannelStatus::Open) {
            *s = status;
            true
        } else {
            false
        }
    });
    let _ = driver
        .event_tx
        .send(PeerConnectionEvent::ConnectionStateChange(event))
        .await;
    // driver.message_tx drops here, signaling EOF to `receive()`.
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

async fn main_loop(
    driver: &mut Driver,
    routes: &HashMap<SocketAddr, Route>,
    net_rx: &mut tokio::sync::mpsc::Receiver<Incoming>,
) -> Exit {
    // The one queued outgoing message str0m's SCTP buffer didn't take yet.
    // While occupied we don't pull more from outgoing_rx, so backpressure
    // propagates to `DataChannel::send`.
    let mut parked_out: Option<Vec<u8>> = None;
    let mut outgoing_done = false;
    let mut net_done = false;
    // Moved (not cloned!) out of the driver so that dropping it on channel
    // close is an immediate EOF for `receive()`, even though the driver —
    // whose lifetime is the PeerConnection's — keeps running.
    let mut message_tx = driver.message_tx.take();
    let mut disconnected_since: Option<Instant> = None;
    let mut exchange_done_at: Option<Instant> = None;
    let mut connected = false;

    loop {
        // Hand a parked outgoing message to SCTP first, so the drain just
        // below carries it onto the wire in this same pass. (Writing after
        // the drain would leave it sitting in SCTP's send buffer until the
        // next — possibly unrelated, possibly hundreds of ms away — wakeup,
        // which shows up directly as latency jitter.)
        if let Some(message) = parked_out.take() {
            let mut inner = driver.shared.inner.lock().unwrap();
            if let Some(id) = inner.channel_id {
                if let Some(mut channel) = inner.rtc.channel(id) {
                    match channel.write(true, &message) {
                        Ok(true) => {}
                        // SCTP send buffer is full; retry on the next pass.
                        Ok(false) => parked_out = Some(message),
                        Err(e) => {
                            log::warn!("channel write failed: {}", e);
                        }
                    }
                } else {
                    // Channel is gone; the message has nowhere to go.
                }
            } else {
                parked_out = Some(message);
            }
        }

        // Drain the state machine: collect everything it wants done under
        // one short lock, then do the async parts (socket sends, channel
        // sends) without holding it.
        let mut transmits: Vec<(SocketAddr, SocketAddr, Vec<u8>)> = vec![];
        let mut messages: Vec<Vec<u8>> = vec![];
        let mut events: Vec<ConnectionState> = vec![];
        let mut channel_opened = false;
        let mut channel_closed = false;
        let mut remote_closed = false;

        let deadline = {
            let mut inner = driver.shared.inner.lock().unwrap();
            loop {
                match inner.rtc.poll_output() {
                    Ok(str0m::Output::Timeout(t)) => break t,
                    Ok(str0m::Output::Transmit(t)) => {
                        // Anything that isn't STUN (first byte < 20) is
                        // DTLS application traffic, which str0m only sends
                        // over the nominated pair — i.e. the selected path.
                        if t.contents.first().is_some_and(|b| *b >= 20) {
                            inner.current_path = Some((t.source, t.destination));
                        }
                        transmits.push((t.source, t.destination, t.contents.to_vec()));
                    }
                    Ok(str0m::Output::Event(event)) => {
                        if !matches!(event, str0m::Event::ChannelData(_)) {
                            log::debug!("rtc event: {:?}", event);
                        }
                        match event {
                            str0m::Event::Connected => {
                                connected = true;
                                disconnected_since = None;
                                events.push(ConnectionState::Connected);
                            }
                            str0m::Event::IceConnectionStateChange(state) => {
                                use str0m::IceConnectionState::*;
                                match state {
                                    Checking => events.push(ConnectionState::Connecting),
                                    Connected | Completed => disconnected_since = None,
                                    Disconnected => {
                                        disconnected_since = Some(Instant::now());
                                        events.push(ConnectionState::Disconnected);
                                    }
                                    _ => {}
                                }
                            }
                            str0m::Event::ChannelOpen(id, _label) => {
                                inner.channel_id = Some(id);
                                channel_opened = true;
                            }
                            str0m::Event::ChannelData(data) => {
                                if Some(data.id) == inner.channel_id {
                                    messages.push(data.data);
                                }
                            }
                            // The remote sent DTLS close_notify: it hung up.
                            str0m::Event::Closed => {
                                inner.channel_id = None;
                                remote_closed = true;
                            }
                            str0m::Event::ChannelClose(id) => {
                                if Some(id) == inner.channel_id {
                                    inner.channel_id = None;
                                    channel_closed = true;
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        return Exit::Failed(Failure::Rtc(Arc::new(e)));
                    }
                }
            }
        };

        for (source, destination, payload) in transmits {
            send_transmit(routes, source, destination, &payload).await;
        }

        if channel_opened {
            driver.status_tx.send_replace(DataChannelStatus::Open);
        }
        if let Some(tx) = &message_tx {
            for message in messages {
                // Unbounded and non-blocking by design: delivery may never
                // stall the driver, or the transport itself (SACKs, ICE
                // keepalives) dies with it. Err means the receiver is gone.
                if tx.send(message).is_err() {
                    break;
                }
            }
        }
        if channel_closed {
            driver.status_tx.send_replace(DataChannelStatus::Closed);
            // EOF for `receive()`.
            message_tx = None;
        }
        for state in events {
            let _ = driver
                .event_tx
                .send(PeerConnectionEvent::ConnectionStateChange(state))
                .await;
        }
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
            if exchange_done_at.is_none() && inner.remote_desc.is_some() {
                exchange_done_at = Some(Instant::now());
            }
        }
        if let Some(since) = disconnected_since {
            if since.elapsed() >= DISCONNECT_GRACE {
                return Exit::Failed(Failure::IceDisconnected);
            }
        }
        if !connected {
            if let Some(at) = exchange_done_at {
                if at.elapsed() >= CONNECT_TIMEOUT {
                    return Exit::Failed(Failure::ConnectTimeout);
                }
            }
        }

        // Sleep until str0m's next timeout or one of our own deadlines,
        // whichever wakeup source fires first.
        let mut wake = deadline;
        if let Some(since) = disconnected_since {
            wake = wake.min(since + DISCONNECT_GRACE);
        }
        if !connected {
            if let Some(at) = exchange_done_at {
                wake = wake.min(at + CONNECT_TIMEOUT);
            }
        }

        let now = Instant::now();
        if wake <= now {
            let mut inner = driver.shared.inner.lock().unwrap();
            if let Err(e) = inner.rtc.handle_input(str0m::Input::Timeout(now)) {
                return Exit::Failed(Failure::Rtc(Arc::new(e)));
            }
            continue;
        }

        tokio::select! {
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(wake)) => {
                let mut inner = driver.shared.inner.lock().unwrap();
                if let Err(e) = inner.rtc.handle_input(str0m::Input::Timeout(Instant::now())) {
                    return Exit::Failed(Failure::Rtc(Arc::new(e)));
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
            outgoing = driver.outgoing_rx.recv(), if !outgoing_done && parked_out.is_none() => {
                match outgoing {
                    Some(message) => parked_out = Some(message),
                    // The DataChannelSender was dropped; no more sends.
                    None => outgoing_done = true,
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

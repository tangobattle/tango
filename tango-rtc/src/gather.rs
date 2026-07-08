//! Trickle ICE candidate gathering: host sockets on every usable interface,
//! server-reflexive addresses via STUN binding requests (the `stun` crate),
//! and relayed addresses via TURN allocations (the `turn` crate).
//!
//! str0m deliberately does no gathering of its own (sans-IO), so this is the
//! half the old libdatachannel stack did in C. Finds are streamed to the
//! driver as they happen — one [`Found`] per socket or relay — and the stream
//! closing means gathering is done. Every network operation is time-bounded,
//! so that's guaranteed to happen.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use webrtc_util::Conn as _;

/// Per-attempt STUN binding-response wait; two attempts per server.
const STUN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1500);
/// Cap on the whole STUN phase of one socket.
const STUN_OVERALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);
/// Cap on one TURN allocation (resolve + auth + allocate).
const TURN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// One gathering find, streamed to the driver as it happens.
pub(crate) enum Found {
    /// A bound local socket, its host candidate, and — when a STUN server
    /// answered on it — the server-reflexive candidate based on it. They
    /// arrive together (rather than the host candidate first) because the
    /// STUN probe must own the socket's read side; once this is sent, the
    /// driver's reader task takes over and everything the socket receives is
    /// ICE traffic.
    Socket {
        socket: Arc<tokio::net::UdpSocket>,
        host: str0m::Candidate,
        srflx: Option<str0m::Candidate>,
    },
    /// A live TURN allocation. `client` must stay alive for the allocation to
    /// keep refreshing; `conn` sends/receives the relayed traffic; `addr` is
    /// the allocated (relayed) address — the candidate's address and str0m's
    /// transmit "source" for it.
    Relay {
        client: turn::client::Client,
        conn: Arc<dyn webrtc_util::Conn + Send + Sync>,
        addr: SocketAddr,
        candidate: str0m::Candidate,
    },
}

/// Start gathering for `config`, streaming finds on the returned channel.
/// The channel closing signals that every (time-bounded) probe has finished.
pub(crate) fn spawn(config: crate::RtcConfig) -> tokio::sync::mpsc::Receiver<Found> {
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    tokio::spawn(run(config, tx));
    rx
}

async fn run(config: crate::RtcConfig, tx: tokio::sync::mpsc::Sender<Found>) {
    let (stun_servers, turn_servers) = parse_ice_servers(&config.ice_servers);

    if config.ice_transport_policy == crate::TransportPolicy::All {
        // Resolve the STUN servers once; every interface probes the same set.
        let mut stun_addrs = vec![];
        for server in &stun_servers {
            match tokio::net::lookup_host(server).await {
                Ok(addrs) => stun_addrs.extend(addrs),
                Err(e) => log::warn!("failed to resolve stun server {}: {}", server, e),
            }
        }

        for ip in local_ips(config.include_loopback) {
            let stun_addrs = stun_addrs.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Some(found) = gather_interface(ip, &stun_addrs).await {
                    let _ = tx.send(found).await;
                }
            });
        }
    }

    for server in turn_servers {
        let tx = tx.clone();
        tokio::spawn(async move {
            match tokio::time::timeout(TURN_TIMEOUT, allocate_relay(&server)).await {
                Ok(Ok(found)) => {
                    let _ = tx.send(found).await;
                }
                Ok(Err(e)) => log::warn!("TURN allocation on {} failed: {}", server.addr, e),
                Err(_) => log::warn!("TURN allocation on {} timed out", server.addr),
            }
        });
    }

    // Our own `tx` drops here; the last live clone (in whichever probe task
    // finishes last) closes the channel.
}

pub(crate) struct TurnServer {
    /// `host:port`.
    addr: String,
    username: String,
    credential: String,
}

/// Split `stun:`/`turn:` URLs into STUN servers and TURN servers (`host:port`
/// each). TURN-over-TCP and TURN-over-TLS are not supported and are skipped,
/// matching the old stack, which also only did TURN/UDP.
fn parse_ice_servers(servers: &[crate::IceServer]) -> (Vec<String>, Vec<TurnServer>) {
    let mut stun = vec![];
    let mut turn = vec![];

    for server in servers {
        for url in &server.urls {
            let Some((scheme, rest)) = url.split_once(':') else {
                log::warn!("malformed ice server url: {}", url);
                continue;
            };
            let (host_port, query) = match rest.split_once('?') {
                Some((hp, q)) => (hp, Some(q)),
                None => (rest, None),
            };
            // Bare hostname → default STUN/TURN port.
            let host_port = if host_port.contains(':') {
                host_port.to_owned()
            } else {
                format!("{}:3478", host_port)
            };

            match scheme {
                "stun" | "stuns" => stun.push(host_port),
                "turn" => {
                    if query == Some("transport=tcp") {
                        log::info!("skipping TURN over TCP server: {}", url);
                        continue;
                    }
                    let (Some(username), Some(credential)) = (&server.username, &server.credential) else {
                        log::warn!("TURN server without credentials: {}", url);
                        continue;
                    };
                    turn.push(TurnServer {
                        addr: host_port,
                        username: username.clone(),
                        credential: credential.clone(),
                    });
                }
                "turns" => {
                    log::info!("skipping TURN over TLS server: {}", url);
                }
                _ => {
                    log::warn!("unknown ice server scheme: {}", url);
                }
            }
        }
    }

    (stun, turn)
}

/// Local unicast addresses worth gathering host candidates on.
pub(crate) fn local_ips(include_loopback: bool) -> Vec<IpAddr> {
    let ifaces = match if_addrs::get_if_addrs() {
        Ok(ifaces) => ifaces,
        Err(e) => {
            log::warn!("failed to enumerate interfaces: {}", e);
            return vec![];
        }
    };

    let mut ips = std::collections::BTreeSet::new();
    for iface in ifaces {
        let ip = iface.ip();
        if ip.is_unspecified() || ip.is_multicast() {
            continue;
        }
        if iface.is_loopback() && !include_loopback {
            continue;
        }
        match ip {
            IpAddr::V4(v4) => {
                if v4.is_link_local() || v4.is_broadcast() {
                    continue;
                }
            }
            IpAddr::V6(v6) => {
                // Link-local (fe80::/10) addresses need scope ids; skip.
                if (v6.segments()[0] & 0xffc0) == 0xfe80 {
                    continue;
                }
            }
        }
        ips.insert(ip);
    }
    ips.into_iter().collect()
}

/// Bind one interface's socket and probe its server-reflexive address.
async fn gather_interface(ip: IpAddr, stun_addrs: &[SocketAddr]) -> Option<Found> {
    let socket = match tokio::net::UdpSocket::bind(SocketAddr::new(ip, 0)).await {
        Ok(socket) => socket,
        Err(e) => {
            log::debug!("failed to bind {}: {}", ip, e);
            return None;
        }
    };
    let local = socket.local_addr().ok()?;
    let host = match str0m::Candidate::host(local, "udp") {
        Ok(candidate) => candidate,
        Err(e) => {
            log::debug!("not a usable host candidate: {}: {}", local, e);
            return None;
        }
    };

    let srflx = if stun_addrs.is_empty() {
        None
    } else {
        tokio::time::timeout(STUN_OVERALL_TIMEOUT, gather_srflx(&socket, stun_addrs))
            .await
            .ok()
            .flatten()
            .and_then(|mapped| match str0m::Candidate::server_reflexive(mapped, local, "udp") {
                Ok(candidate) => Some(candidate),
                Err(e) => {
                    log::debug!("bad srflx candidate {}: {}", mapped, e);
                    None
                }
            })
    };

    Some(Found::Socket {
        socket: Arc::new(socket),
        host,
        srflx,
    })
}

/// Discover the server-reflexive address for one host socket: try each
/// resolved STUN server of a matching address family until one answers.
async fn gather_srflx(socket: &tokio::net::UdpSocket, stun_addrs: &[SocketAddr]) -> Option<SocketAddr> {
    let local = socket.local_addr().ok()?;
    for server in stun_addrs {
        if server.is_ipv4() != local.is_ipv4() {
            continue;
        }
        if let Some(mapped) = stun_binding(socket, *server).await {
            return Some(mapped);
        }
    }
    None
}

/// One STUN binding round trip on `socket`: our mapped (server-reflexive)
/// address as the server saw it, or `None` on timeout/parse failure.
async fn stun_binding(socket: &tokio::net::UdpSocket, server: SocketAddr) -> Option<SocketAddr> {
    let mut request = stun::message::Message::new();
    request
        .build(&[
            Box::new(stun::agent::TransactionId::new()),
            Box::new(stun::message::BINDING_REQUEST),
        ])
        .ok()?;
    let transaction_id = request.transaction_id;

    for _attempt in 0..2 {
        if socket.send_to(&request.raw, server).await.is_err() {
            return None;
        }

        let deadline = tokio::time::Instant::now() + STUN_TIMEOUT;
        let mut buf = vec![0u8; 1500];
        loop {
            let recv = tokio::time::timeout_at(deadline, socket.recv_from(&mut buf)).await;
            let (n, from) = match recv {
                Err(_timeout) => break,
                Ok(Err(_)) => return None,
                Ok(Ok(r)) => r,
            };
            if from != server {
                continue;
            }

            let mut response = stun::message::Message::new();
            response.raw = buf[..n].to_vec();
            if response.decode().is_err() || response.transaction_id != transaction_id {
                continue;
            }

            let mut mapped = stun::xoraddr::XorMappedAddress::default();
            if stun::message::Getter::get_from(&mut mapped, &response).is_err() {
                return None;
            }
            return Some(SocketAddr::new(mapped.ip, mapped.port));
        }
    }

    None
}

/// Set up a TURN client + allocation against one server.
async fn allocate_relay(server: &TurnServer) -> Result<Found, Box<dyn std::error::Error + Send + Sync>> {
    // Resolve up-front to pick the right socket family.
    let resolved = tokio::net::lookup_host(&server.addr)
        .await?
        .next()
        .ok_or("no addresses resolved")?;
    let bind_addr: SocketAddr = if resolved.is_ipv4() {
        "0.0.0.0:0".parse().unwrap()
    } else {
        "[::]:0".parse().unwrap()
    };
    let socket = tokio::net::UdpSocket::bind(bind_addr).await?;
    let local = socket.local_addr()?;

    let client = turn::client::Client::new(turn::client::ClientConfig {
        stun_serv_addr: server.addr.clone(),
        turn_serv_addr: server.addr.clone(),
        username: server.username.clone(),
        password: server.credential.clone(),
        // The realm comes back in the server's 401 challenge.
        realm: String::new(),
        software: String::new(),
        rto_in_ms: 0,
        conn: Arc::new(socket),
        vnet: None,
    })
    .await?;
    client.listen().await?;

    let conn = client.allocate().await?;
    let addr = conn.local_addr()?;

    let candidate = str0m::Candidate::relayed(addr, local, "udp").map_err(|e| e.to_string())?;

    Ok(Found::Relay {
        client,
        conn: Arc::new(conn),
        addr,
        candidate,
    })
}

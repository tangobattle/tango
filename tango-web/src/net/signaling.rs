//! The matchmaking signaling choreography, browser flavor — the same
//! message flow as the desktop's `tango-signaling` client over
//! [`super::ws`] + [`super::webrtc`]:
//!
//! 1. dial `wss://…?session_id=<code>` (browsers can't set the
//!    `X-Tango-Protocol-Version` header the desktop also sends; the
//!    version rides in `Start.protocol_version`, which the server
//!    checks too);
//! 2. read the server's `Hello{ice_servers}`, build the peer
//!    connection with tango's two negotiated channels, install a local
//!    offer, and send `Start{protocol_version, offer_sdp,
//!    connection_id}`;
//! 3. wait for the peer: whoever receives an `Offer` is the *polite*
//!    side (roll back the un-answered local offer, apply theirs,
//!    answer); whoever receives an `Answer` is *impolite* (apply it).
//!    A websocket drop before the peer commits is transparently
//!    re-dialed with a fresh offer under the same `connection_id`;
//! 4. trickle ICE both ways until the connection comes up, then close
//!    the socket.
//!
//! No mTLS client identity on the web: browsers cannot present a
//! client certificate, so web peers are anonymous — the fingerprint
//! fields are optional throughout the protocol and the reconnect id
//! falls back accordingly.

use futures::{FutureExt, StreamExt};
use prost::Message as _;
use tango_signaling::proto::signaling::{packet, Packet};

use super::webrtc;
use super::ws::SignalSocket;

const PING_INTERVAL_MS: u32 = 15_000;
const READ_TIMEOUT_MS: u32 = 30_000;
const MIN_RECONNECT_BACKOFF_MS: u32 = 500;
const MAX_RECONNECT_BACKOFF_MS: u32 = 8_000;

/// The successful outcome: the live channels + connection, and the
/// identity material the reconnect rendezvous derives from.
pub struct Connected {
    pub pc: webrtc::PeerConnection,
    pub control_tx: webrtc::ChannelSender,
    pub control_rx: webrtc::ChannelReceiver,
    pub in_match_tx: webrtc::ChannelSender,
    pub in_match_rx: webrtc::ChannelReceiver,
    pub control_open: futures::channel::oneshot::Receiver<()>,
    /// Whether this side ended up the offerer (impolite side). Feeds
    /// the match-clock pick and the player-index draw.
    pub is_offerer: bool,
    pub local_dtls_fingerprint: Vec<u8>,
    pub peer_dtls_fingerprint: Vec<u8>,
    /// The peer's server-attested mTLS client-certificate fingerprint
    /// (empty for anonymous peers — including every web peer).
    #[allow(dead_code)] // recorded into replay metadata (M4)
    pub peer_client_cert_fingerprint: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("signaling abort: {0:?}")]
    ServerAbort(packet::abort::Reason),
    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Error::Other(format!("{e:#}"))
    }
}

fn send_packet(ws: &SignalSocket, which: packet::Which) -> anyhow::Result<()> {
    ws.send(&Packet { which: Some(which) }.encode_to_vec())
}

async fn sleep_ms(ms: u32) {
    crate::compat::sleep_ms(ms).await;
}

/// `encodeURIComponent`, target-neutral: percent-encode everything
/// outside JS's unreserved set.
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// One `establish` round: dial, read Hello, build the peer connection,
/// send Start. Re-run wholesale on a transparent pre-exchange redial so
/// every attempt gets fresh ICE credentials and a brand-new offer.
async fn establish(
    endpoint: &str,
    session_id: &str,
    use_relay: Option<bool>,
    connection_id: &[u8; 16],
) -> Result<(SignalSocket, webrtc::PeerParts), Error> {
    let url = format!("{endpoint}/?session_id={}", encode_uri_component(session_id));
    let mut ws = SignalSocket::connect(&url).await?;

    // The server speaks first: Hello with the ICE set (or an Abort).
    let hello = loop {
        let Some(raw) = ws.next().await else {
            return Err(Error::Other("signaling closed before Hello".into()));
        };
        let p = Packet::decode(raw.as_slice()).map_err(|e| Error::Other(format!("bad signaling packet: {e}")))?;
        match p.which {
            Some(packet::Which::Hello(hello)) => break hello,
            Some(packet::Which::Abort(abort)) => {
                return Err(Error::ServerAbort(
                    packet::abort::Reason::try_from(abort.reason).unwrap_or_default(),
                ));
            }
            Some(packet::Which::Ping(_)) | None => continue,
            Some(other) => {
                return Err(Error::Other(format!("unexpected pre-Hello packet: {other:?}")));
            }
        }
    };
    log::info!("signaling hello: {} ice server(s)", hello.ice_servers.len());

    let parts = webrtc::new(&hello.ice_servers, use_relay)?;
    let offer_sdp = parts.pc.create_offer().await?;
    send_packet(
        &ws,
        packet::Which::Start(packet::Start {
            protocol_version: tango_net_protocol::PROTOCOL_VERSION,
            offer_sdp,
            connection_id: connection_id.to_vec(),
        }),
    )?;
    Ok((ws, parts))
}

enum ExchangeOutcome {
    /// The peer committed (their Offer answered / their Answer applied).
    Exchanged {
        is_offerer: bool,
        peer_client_cert_fingerprint: Vec<u8>,
    },
    /// The socket died before the peer said anything — safe to redial.
    Dropped(String),
}

/// Pump the socket until the peer starts the SDP exchange. Local ICE
/// candidates gathered meanwhile buffer in `pending` (the peer can't
/// take them before it has our SDP).
async fn wait_for_exchange(
    ws: &mut SignalSocket,
    parts: &mut webrtc::PeerParts,
    pending: &mut Vec<String>,
) -> Result<ExchangeOutcome, Error> {
    let mut ping_elapsed = 0u32;
    loop {
        // A bounded wait: peer-connection events (candidates), the next
        // socket frame, or a timeout slice that drives the keepalive.
        let step = futures::select! {
            ev = parts.events.next() => Step::Peer(ev),
            raw = ws.next().fuse() => Step::Socket(raw),
            _ = sleep_ms(PING_INTERVAL_MS).fuse() => Step::PingDue,
        };
        match step {
            Step::Peer(Some(webrtc::PeerEvent::Candidate(c))) => {
                pending.push(c);
                continue;
            }
            Step::Peer(Some(webrtc::PeerEvent::Connected)) => continue,
            Step::Peer(Some(webrtc::PeerEvent::Failed)) | Step::Peer(None) => {
                return Err(Error::Other("peer connection failed during signaling".into()));
            }
            Step::PingDue => {
                ping_elapsed += PING_INTERVAL_MS;
                if ping_elapsed >= READ_TIMEOUT_MS + PING_INTERVAL_MS {
                    return Ok(ExchangeOutcome::Dropped("signaling read timeout".into()));
                }
                if send_packet(ws, packet::Which::Ping(packet::Ping {})).is_err() {
                    return Ok(ExchangeOutcome::Dropped("keepalive send failed".into()));
                }
                continue;
            }
            Step::Socket(None) => {
                return Ok(ExchangeOutcome::Dropped("signaling socket closed".into()));
            }
            Step::Socket(Some(raw)) => {
                ping_elapsed = 0;
                let p =
                    Packet::decode(raw.as_slice()).map_err(|e| Error::Other(format!("bad signaling packet: {e}")))?;
                match p.which {
                    Some(packet::Which::Ping(_)) | None => continue,
                    Some(packet::Which::Abort(abort)) => {
                        return Err(Error::ServerAbort(
                            packet::abort::Reason::try_from(abort.reason).unwrap_or_default(),
                        ));
                    }
                    Some(packet::Which::Offer(offer)) => {
                        // The polite side: their offer wins; ours rolls
                        // back. From here the peer has committed — any
                        // failure is fatal, never a redial.
                        log::info!("signaling: received offer (we are the polite side)");
                        let answer_sdp = parts.pc.rollback_and_accept_offer(&offer.sdp).await?;
                        send_packet(
                            ws,
                            packet::Which::Answer(packet::Answer {
                                sdp: answer_sdp,
                                // Server-filled on relay; nothing we
                                // set here survives.
                                client_cert_fingerprint_sha256: vec![],
                            }),
                        )?;
                        return Ok(ExchangeOutcome::Exchanged {
                            is_offerer: false,
                            peer_client_cert_fingerprint: offer.client_cert_fingerprint_sha256,
                        });
                    }
                    Some(packet::Which::Answer(answer)) => {
                        log::info!("signaling: received answer (we are the offerer)");
                        parts.pc.accept_answer(&answer.sdp).await?;
                        return Ok(ExchangeOutcome::Exchanged {
                            is_offerer: true,
                            peer_client_cert_fingerprint: answer.client_cert_fingerprint_sha256,
                        });
                    }
                    Some(other) => {
                        return Err(Error::Other(format!("unexpected signaling packet: {other:?}")));
                    }
                }
            }
        }
    }
}

enum Step {
    Peer(Option<webrtc::PeerEvent>),
    Socket(Option<Vec<u8>>),
    PingDue,
}

/// Bring a peer connection up end to end through the matchmaking
/// server. Resolves once the WebRTC connection is live (the channel
/// `onopen` barriers may land moments later).
pub async fn connect(endpoint: &str, session_id: &str, use_relay: Option<bool>) -> Result<Connected, Error> {
    // A stable id for this logical connection attempt, sent with every
    // Start. It survives transparent redials, so the server replaces
    // our stale offer instead of mistaking the new socket for the
    // answering peer.
    let connection_id: [u8; 16] = rand::random();

    let (mut ws, mut parts) = establish(endpoint, session_id, use_relay, &connection_id).await?;

    // Wait for the peer, transparently redialing while nothing is
    // committed.
    let mut pending: Vec<String> = Vec::new();
    let (is_offerer, peer_client_cert_fingerprint) = loop {
        match wait_for_exchange(&mut ws, &mut parts, &mut pending).await? {
            ExchangeOutcome::Exchanged {
                is_offerer,
                peer_client_cert_fingerprint,
            } => break (is_offerer, peer_client_cert_fingerprint),
            ExchangeOutcome::Dropped(reason) => {
                log::warn!("signaling dropped before the peer committed ({reason}); redialing");
                let mut backoff = MIN_RECONNECT_BACKOFF_MS;
                loop {
                    match establish(endpoint, session_id, use_relay, &connection_id).await {
                        Ok((w, p)) => {
                            ws = w;
                            parts = p;
                            // Fresh peer connection → stale candidates.
                            pending.clear();
                            break;
                        }
                        Err(Error::ServerAbort(r)) => return Err(Error::ServerAbort(r)),
                        Err(Error::Other(e)) => {
                            log::warn!("signaling redial failed ({e}); retrying in {backoff}ms");
                            sleep_ms(backoff).await;
                            backoff = (backoff * 2).min(MAX_RECONNECT_BACKOFF_MS);
                        }
                    }
                }
            }
        }
    };

    // Trickle phase: flush what we buffered, then pump until the
    // connection itself comes up.
    for candidate in pending.drain(..) {
        let _ = send_packet(&ws, packet::Which::IceCandidate(packet::IceCandidate { candidate }));
    }
    loop {
        let step = futures::select! {
            ev = parts.events.next() => Step::Peer(ev),
            raw = ws.next().fuse() => Step::Socket(raw),
            _ = sleep_ms(PING_INTERVAL_MS).fuse() => Step::PingDue,
        };
        match step {
            Step::Peer(Some(webrtc::PeerEvent::Candidate(c))) => {
                let _ = send_packet(&ws, packet::Which::IceCandidate(packet::IceCandidate { candidate: c }));
            }
            Step::Peer(Some(webrtc::PeerEvent::Connected)) => break,
            Step::Peer(Some(webrtc::PeerEvent::Failed)) | Step::Peer(None) => {
                return Err(Error::Other("peer connection failed".into()));
            }
            Step::PingDue => {
                // Socket death here is non-fatal: the exchanged
                // candidates usually suffice.
                let _ = send_packet(&ws, packet::Which::Ping(packet::Ping {}));
            }
            Step::Socket(None) => {
                // Keep waiting on the connection state alone.
                continue;
            }
            Step::Socket(Some(raw)) => {
                if let Ok(Packet {
                    which: Some(packet::Which::IceCandidate(c)),
                }) = Packet::decode(raw.as_slice())
                {
                    let _ = parts.pc.add_remote_candidate(&c.candidate).await;
                }
            }
        }
    }

    // Connected — done with signaling.
    ws.close();

    let local_dtls_fingerprint = parts.pc.local_dtls_fingerprint();
    let peer_dtls_fingerprint = parts.pc.peer_dtls_fingerprint();
    Ok(Connected {
        pc: parts.pc,
        control_tx: parts.control_tx,
        control_rx: parts.control_rx,
        in_match_tx: parts.in_match_tx,
        in_match_rx: parts.in_match_rx,
        control_open: parts.control_open,
        is_offerer,
        local_dtls_fingerprint,
        peer_dtls_fingerprint,
        peer_client_cert_fingerprint,
    })
}

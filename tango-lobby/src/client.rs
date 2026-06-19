//! The long-lived lobby connection: dial (mTLS) → `Join` → `Welcome`, then a
//! duplex stream of [`Event`]s in and commands ([`Lobby`] methods) out, with
//! app-level keepalive pings and transparent reconnect-and-rejoin.

use std::sync::Arc;

use futures_util::{SinkExt, TryStreamExt};
use prost::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::Connector;

use crate::friend_code::FriendCode;
use crate::proto::lobby as pb;
use pb::{client_message, server_message};

/// The websocket stream `connect_async_tls_with_config` returns.
type LobbyStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// How long to wait for any traffic before treating the socket as dead. The
/// server is expected to reply to our pings, so a healthy idle connection reads
/// at least every [`PING_INTERVAL`].
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const PING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

const MIN_RECONNECT_BACKOFF: std::time::Duration = std::time::Duration::from_millis(500);
const MAX_RECONNECT_BACKOFF: std::time::Duration = std::time::Duration::from_secs(8);

/// The caller's persistent client identity, presented as a TLS client
/// certificate (mTLS) so the server can derive a stable friend code. Both
/// fields are DER.
#[derive(Clone)]
pub struct ClientIdentity {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
}

impl std::fmt::Debug for ClientIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientIdentity")
            .field("cert_der_len", &self.cert_der.len())
            .field("key_der_len", &self.key_der.len())
            .finish()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("tungstenite: {0:?}")]
    Tungstenite(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("rustls: {0:?}")]
    Rustls(#[from] rustls::Error),

    #[error("io: {0:?}")]
    Io(#[from] std::io::Error),

    #[error("prost decode error: {0:?}")]
    ProstDecode(#[from] prost::DecodeError),

    #[error("expected a Welcome as the first server message")]
    UnexpectedMessage,

    #[error("invalid websocket frame")]
    InvalidMessage,
}

/// Build a rustls `ClientConfig` trusting the webpki roots and presenting
/// `identity` as the client certificate. Behind an `Arc` so it clones cheaply
/// into a fresh `Connector` on every transparent reconnect.
fn build_tls_config(identity: &ClientIdentity) -> Result<Arc<rustls::ClientConfig>, Error> {
    let mut root_store = rustls::RootCertStore::empty();
    root_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
        rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
            ta.subject,
            ta.spki,
            ta.name_constraints,
        )
    }));
    let config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_store)
        .with_single_cert(
            vec![rustls::Certificate(identity.cert_der.clone())],
            rustls::PrivateKey(identity.key_der.clone()),
        )?;
    Ok(Arc::new(config))
}

/// Whether an error is a transport hiccup a reconnect might paper over, versus
/// a definitive protocol-level problem.
fn is_transient(e: &Error) -> bool {
    use tokio_tungstenite::tungstenite::Error as Ws;
    match e {
        Error::Io(_) => true,
        Error::Tungstenite(ws) => matches!(
            ws,
            Ws::ConnectionClosed | Ws::AlreadyClosed | Ws::Io(_) | Ws::Protocol(_) | Ws::Tls(_)
        ),
        _ => false,
    }
}

/// The presence status a client sets on itself, mirroring `Status` in the
/// proto. Exactly one of these holds at a time.
#[derive(Clone, Debug)]
pub enum Status {
    /// Visible and idle.
    Online,
    /// Hidden — indistinguishable from offline to everyone else.
    Invisible,
    /// Visible and in a match; the proposal renders as `now_playing`.
    InMatch(pb::MatchProposal),
}

impl Status {
    fn to_proto(&self) -> pb::Status {
        let state = match self {
            Status::Online => pb::status::State::Online(pb::status::Online {}),
            Status::Invisible => pb::status::State::Invisible(pb::status::Invisible {}),
            Status::InMatch(proposal) => pb::status::State::NowPlaying(proposal.clone()),
        };
        pb::Status { state: Some(state) }
    }
}

/// A visible roster member, with the friend code decoded.
#[derive(Clone, Debug)]
pub struct RosterEntry {
    pub friend_code: FriendCode,
    /// Present iff the player is in a match.
    pub now_playing: Option<pb::MatchProposal>,
}

/// The reply to our `Join`: our server-assigned friend code and the current
/// roster snapshot.
#[derive(Clone, Debug)]
pub struct Welcome {
    pub your_friend_code: FriendCode,
    pub roster: Vec<RosterEntry>,
    pub protocol_version: u32,
}

/// A decoded inbound server event. Friend codes are decoded; `commitment` stays
/// opaque bytes (the lobby never inspects it). Challenges are keyed by peer
/// (at most one pending per peer), so there's no challenge id.
#[derive(Clone, Debug)]
pub enum Event {
    RosterUpsert(RosterEntry),
    RosterLeave(FriendCode),
    ChallengeIncoming {
        peer: FriendCode,
        proposal: pb::MatchProposal,
        commitment: Vec<u8>,
    },
    ChallengeAccepted {
        peer: FriendCode,
        proposal: pb::MatchProposal,
        commitment: Vec<u8>,
        ice_servers: Vec<pb::IceServer>,
    },
    ChallengeConfirmed {
        peer: FriendCode,
        ice_servers: Vec<pb::IceServer>,
    },
    ChallengeDeclined {
        peer: FriendCode,
    },
    ChallengeWithdrawn {
        peer: FriendCode,
    },
    RtcOffer {
        peer: FriendCode,
        sdp: String,
    },
    RtcAnswer {
        peer: FriendCode,
        sdp: String,
    },
    /// A newer connection for our identity displaced us; the driver stops.
    Displaced,
    /// The socket dropped and we're transparently reconnecting.
    Reconnecting,
    /// A reconnect succeeded and re-joined; carries the fresh snapshot. Treat it
    /// like a `Welcome`: replace the local roster wholesale.
    Resynced {
        your_friend_code: FriendCode,
        roster: Vec<RosterEntry>,
    },
}

/// Handle for sending commands to the lobby. Cheap to clone; all methods are
/// fire-and-forget (a dead connection silently drops them — observe liveness
/// via the [`Event`] stream instead).
#[derive(Clone, Debug)]
pub struct Lobby {
    tx: tokio::sync::mpsc::UnboundedSender<pb::ClientMessage>,
    your_friend_code: FriendCode,
}

impl Lobby {
    /// This client's own friend code, as assigned by the server (derived from
    /// the mTLS certificate fingerprint and returned in `Welcome`).
    pub fn friend_code(&self) -> FriendCode {
        self.your_friend_code
    }

    fn send(&self, which: client_message::Which) {
        let _ = self.tx.send(pb::ClientMessage { which: Some(which) });
    }

    pub fn set_status(&self, status: Status) {
        self.send(client_message::Which::SetStatus(status.to_proto()));
    }

    pub fn challenge(&self, peer: &FriendCode, proposal: pb::MatchProposal, commitment: Vec<u8>) {
        self.send(client_message::Which::Challenge(client_message::Challenge {
            peer_friend_code: peer.to_vec(),
            proposal: Some(proposal),
            commitment,
        }));
    }

    pub fn accept(&self, peer: &FriendCode, proposal: pb::MatchProposal, commitment: Vec<u8>) {
        self.send(client_message::Which::ChallengeAccept(
            client_message::ChallengeAccept {
                peer_friend_code: peer.to_vec(),
                proposal: Some(proposal),
                commitment,
            },
        ));
    }

    pub fn decline(&self, peer: &FriendCode) {
        self.send(client_message::Which::ChallengeDecline(
            client_message::ChallengeDecline {
                peer_friend_code: peer.to_vec(),
            },
        ));
    }

    pub fn cancel(&self, peer: &FriendCode) {
        self.send(client_message::Which::ChallengeCancel(
            client_message::ChallengeCancel {
                peer_friend_code: peer.to_vec(),
            },
        ));
    }

    pub fn rtc_offer(&self, peer: &FriendCode, sdp: String) {
        self.send(client_message::Which::RtcOffer(client_message::RtcOffer {
            peer_friend_code: peer.to_vec(),
            sdp,
        }));
    }

    pub fn rtc_answer(&self, peer: &FriendCode, sdp: String) {
        self.send(client_message::Which::RtcAnswer(client_message::RtcAnswer {
            peer_friend_code: peer.to_vec(),
            sdp,
        }));
    }
}

fn decode_friend_code(bytes: &[u8]) -> Option<FriendCode> {
    FriendCode::from_bytes(bytes).ok()
}

fn convert_entry(entry: pb::RosterEntry) -> Option<RosterEntry> {
    Some(RosterEntry {
        friend_code: decode_friend_code(&entry.friend_code)?,
        now_playing: entry.now_playing,
    })
}

fn decode_welcome(msg: pb::ServerMessage) -> Option<Welcome> {
    let server_message::Which::Welcome(w) = msg.which? else {
        return None;
    };
    Some(Welcome {
        your_friend_code: decode_friend_code(&w.your_friend_code)?,
        roster: w.roster.into_iter().filter_map(convert_entry).collect(),
        protocol_version: w.protocol_version,
    })
}

/// Map an inbound server oneof to an [`Event`]. `Welcome`/`Pong`/`Displaced`
/// are handled by the driver, not here. Returns `None` on a malformed payload
/// (e.g. an undecodable friend code), which the driver simply skips.
fn decode_event(which: server_message::Which) -> Option<Event> {
    use server_message::Which as W;
    Some(match which {
        W::RosterUpsert(u) => Event::RosterUpsert(convert_entry(u.entry?)?),
        W::RosterLeave(l) => Event::RosterLeave(decode_friend_code(&l.friend_code)?),
        W::ChallengeIncoming(c) => Event::ChallengeIncoming {
            peer: decode_friend_code(&c.peer_friend_code)?,
            proposal: c.proposal?,
            commitment: c.commitment,
        },
        W::ChallengeAccepted(c) => Event::ChallengeAccepted {
            peer: decode_friend_code(&c.peer_friend_code)?,
            proposal: c.proposal?,
            commitment: c.commitment,
            ice_servers: c.ice_servers,
        },
        W::ChallengeConfirmed(c) => Event::ChallengeConfirmed {
            peer: decode_friend_code(&c.peer_friend_code)?,
            ice_servers: c.ice_servers,
        },
        W::ChallengeDeclined(c) => Event::ChallengeDeclined {
            peer: decode_friend_code(&c.peer_friend_code)?,
        },
        W::ChallengeWithdrawn(c) => Event::ChallengeWithdrawn {
            peer: decode_friend_code(&c.peer_friend_code)?,
        },
        W::RtcOffer(c) => Event::RtcOffer {
            peer: decode_friend_code(&c.peer_friend_code)?,
            sdp: c.sdp,
        },
        W::RtcAnswer(c) => Event::RtcAnswer {
            peer: decode_friend_code(&c.peer_friend_code)?,
            sdp: c.sdp,
        },
        W::Welcome(_) | W::Pong(_) | W::Displaced(_) => return None,
    })
}

/// Dial, send `Join` with `status`, and read until the `Welcome`. Re-run on
/// every transparent reconnect, so each attempt re-presents the identity and
/// gets a fresh roster snapshot.
async fn establish(
    addr: &str,
    tls_config: &Arc<rustls::ClientConfig>,
    protocol_version: u32,
    status: &pb::Status,
) -> Result<(LobbyStream, Welcome), Error> {
    let mut req = addr.into_client_request()?;
    req.headers_mut().append(
        "User-Agent",
        tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&format!(
            "tango-lobby/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .expect("valid header value"),
    );

    let connector = Some(Connector::Rustls(tls_config.clone()));
    let (mut stream, _) =
        tokio_tungstenite::connect_async_tls_with_config(req, None, connector).await?;

    let join = pb::ClientMessage {
        which: Some(client_message::Which::Join(client_message::Join {
            protocol_version,
            status: Some(status.clone()),
        })),
    };
    stream
        .send(WsMessage::Binary(join.encode_to_vec()))
        .await?;

    loop {
        let raw = match tokio::time::timeout(READ_TIMEOUT, stream.try_next()).await {
            Err(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "timed out waiting for welcome",
                )
                .into())
            }
            Ok(Err(e)) => return Err(e.into()),
            Ok(Ok(None)) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "stream ended before welcome",
                )
                .into())
            }
            Ok(Ok(Some(raw))) => raw,
        };

        match raw {
            WsMessage::Binary(d) => {
                let msg = pb::ServerMessage::decode(d.as_slice())?;
                return match decode_welcome(msg) {
                    Some(welcome) => Ok((stream, welcome)),
                    None => Err(Error::UnexpectedMessage),
                };
            }
            WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
            WsMessage::Close(_) => {
                return Err(tokio_tungstenite::tungstenite::Error::ConnectionClosed.into())
            }
            _ => return Err(Error::InvalidMessage),
        }
    }
}

/// How a single pumping session ended.
enum PumpEnd {
    /// The handle was dropped, or the server displaced us — stop for good.
    Closed,
    /// A transport hiccup — the driver should reconnect.
    Transient(Error),
    /// A protocol-level failure — stop for good.
    Fatal(Error),
}

/// Pump one live websocket: forward outbound commands, answer pings, and emit
/// inbound events, until it ends one of [`PumpEnd`]'s ways.
async fn pump(
    stream: &mut LobbyStream,
    cmd_rx: &mut tokio::sync::mpsc::UnboundedReceiver<pb::ClientMessage>,
    event_tx: &tokio::sync::mpsc::UnboundedSender<Event>,
    last_status: &mut pb::Status,
) -> PumpEnd {
    let mut ping = tokio::time::interval_at(
        tokio::time::Instant::now() + PING_INTERVAL,
        PING_INTERVAL,
    );
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                let Some(msg) = cmd else {
                    // Every handle dropped: close cleanly and stop.
                    let _ = stream.close(None).await;
                    return PumpEnd::Closed;
                };
                // Remember the latest status so a reconnect can re-Join with it.
                if let Some(client_message::Which::SetStatus(s)) = &msg.which {
                    *last_status = s.clone();
                }
                if let Err(e) = stream.send(WsMessage::Binary(msg.encode_to_vec())).await {
                    return PumpEnd::Transient(e.into());
                }
            }
            _ = ping.tick() => {
                let ping_msg = pb::ClientMessage {
                    which: Some(client_message::Which::Ping(client_message::Ping {})),
                };
                if let Err(e) = stream.send(WsMessage::Binary(ping_msg.encode_to_vec())).await {
                    return PumpEnd::Transient(e.into());
                }
            }
            res = tokio::time::timeout(READ_TIMEOUT, stream.try_next()) => {
                let raw = match res {
                    Err(_) => return PumpEnd::Transient(
                        std::io::Error::new(std::io::ErrorKind::TimedOut, "read timed out").into(),
                    ),
                    Ok(Err(e)) => {
                        let e: Error = e.into();
                        return if is_transient(&e) { PumpEnd::Transient(e) } else { PumpEnd::Fatal(e) };
                    }
                    Ok(Ok(None)) => return PumpEnd::Transient(
                        std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "stream ended").into(),
                    ),
                    Ok(Ok(Some(raw))) => raw,
                };

                match raw {
                    WsMessage::Binary(d) => {
                        let msg = match pb::ServerMessage::decode(d.as_slice()) {
                            Ok(msg) => msg,
                            Err(e) => return PumpEnd::Fatal(e.into()),
                        };
                        match msg.which {
                            Some(server_message::Which::Displaced(_)) => {
                                let _ = event_tx.send(Event::Displaced);
                                let _ = stream.close(None).await;
                                return PumpEnd::Closed;
                            }
                            // Pong is keepalive; a mid-stream Welcome is unexpected
                            // but harmless — ignore both.
                            Some(server_message::Which::Pong(_))
                            | Some(server_message::Which::Welcome(_))
                            | None => {}
                            Some(other) => {
                                if let Some(ev) = decode_event(other) {
                                    if event_tx.send(ev).is_err() {
                                        return PumpEnd::Closed;
                                    }
                                }
                            }
                        }
                    }
                    WsMessage::Ping(_) | WsMessage::Pong(_) => {}
                    WsMessage::Close(_) => {
                        return PumpEnd::Transient(
                            tokio_tungstenite::tungstenite::Error::ConnectionClosed.into(),
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Own the connection for its whole life: pump it, and on a transient drop
/// reconnect (with backoff), re-Join with the last known status, and emit a
/// `Resynced` snapshot before resuming.
async fn drive(
    mut stream: LobbyStream,
    addr: String,
    tls_config: Arc<rustls::ClientConfig>,
    protocol_version: u32,
    mut last_status: pb::Status,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<pb::ClientMessage>,
    event_tx: tokio::sync::mpsc::UnboundedSender<Event>,
) {
    loop {
        match pump(&mut stream, &mut cmd_rx, &event_tx, &mut last_status).await {
            PumpEnd::Closed => return,
            PumpEnd::Fatal(e) => {
                log::error!("lobby connection failed: {e}");
                return;
            }
            PumpEnd::Transient(e) => {
                log::warn!("lobby connection dropped ({e}); reconnecting");
                if event_tx.send(Event::Reconnecting).is_err() {
                    return;
                }
                let mut backoff = MIN_RECONNECT_BACKOFF;
                loop {
                    match establish(&addr, &tls_config, protocol_version, &last_status).await
                    {
                        Ok((s, welcome)) => {
                            stream = s;
                            log::info!("lobby reconnected as {}", welcome.your_friend_code);
                            if event_tx
                                .send(Event::Resynced {
                                    your_friend_code: welcome.your_friend_code,
                                    roster: welcome.roster,
                                })
                                .is_err()
                            {
                                return;
                            }
                            break;
                        }
                        Err(e) if is_transient(&e) => {
                            log::warn!("lobby reconnect failed ({e}); retrying in {backoff:?}");
                            tokio::time::sleep(backoff).await;
                            backoff = (backoff * 2).min(MAX_RECONNECT_BACKOFF);
                        }
                        Err(e) => {
                            log::error!("lobby reconnect rejected: {e}");
                            return;
                        }
                    }
                }
            }
        }
    }
}

/// Connect to the lobby, join with `status`, and start driving the connection
/// on a background task. Returns the [`Lobby`] handle, the initial [`Welcome`],
/// and the inbound [`Event`] stream.
///
/// The initial dial surfaces failures to the caller (so "couldn't reach the
/// lobby" is reported promptly); transparent reconnects only kick in afterward.
pub async fn connect(
    addr: &str,
    identity: ClientIdentity,
    protocol_version: u32,
    status: Status,
) -> Result<(Lobby, Welcome, tokio::sync::mpsc::UnboundedReceiver<Event>), Error> {
    // An identity is mandatory: the server derives the friend code from the
    // mTLS certificate and refuses connections that present none.
    let tls_config = build_tls_config(&identity)?;

    let proto_status = status.to_proto();
    let (stream, welcome) = establish(addr, &tls_config, protocol_version, &proto_status).await?;

    // The server assigns our friend code (derived from the mTLS fingerprint) and
    // hands it back in Welcome; the client never computes it itself.
    let your_friend_code = welcome.your_friend_code;

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();

    tokio::spawn(drive(
        stream,
        addr.to_owned(),
        tls_config,
        protocol_version,
        proto_status,
        cmd_rx,
        event_tx,
    ));

    Ok((Lobby { tx: cmd_tx, your_friend_code }, welcome, event_rx))
}

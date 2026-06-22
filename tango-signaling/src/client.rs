use futures_util::SinkExt;
use futures_util::TryStreamExt;
use prost::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

pub type AbortReason = crate::proto::signaling::packet::abort::Reason;

/// The concrete websocket stream `tokio_tungstenite::connect_async` hands back.
type SignalingStream = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The caller's persistent client identity, presented as a TLS client
/// certificate (mTLS) on the signaling websocket so the server can recognize
/// the same install across sessions. Both fields are DER: a single self-signed
/// certificate and its private key. The certificate is only sent if the server
/// asks for one during the TLS handshake (i.e. mTLS is enabled on the
/// endpoint); when it doesn't, the connection proceeds as an ordinary client,
/// so attaching an identity is always safe.
#[derive(Clone)]
pub struct ClientIdentity {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
}

/// Hand-rolled so the private key never lands in a `Debug` dump (the enclosing
/// netplay `Message` derives `Debug` and gets logged) — just the byte lengths.
impl std::fmt::Debug for ClientIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientIdentity")
            .field("cert_der_len", &self.cert_der.len())
            .field("key_der_len", &self.key_der.len())
            .finish()
    }
}

/// Build a rustls `ClientConfig` that trusts the webpki root set (same roots
/// `tokio_tungstenite`'s default connector uses) and presents `identity` as the
/// client certificate. Returned behind an `Arc` so it can be cloned cheaply
/// into a fresh `Connector` on every transparent reconnect.
fn build_tls_config(identity: &ClientIdentity) -> Result<std::sync::Arc<rustls::ClientConfig>, Error> {
    let mut root_store = rustls::RootCertStore::empty();
    root_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
        rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(ta.subject, ta.spki, ta.name_constraints)
    }));
    let config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_store)
        .with_single_cert(
            vec![rustls::Certificate(identity.cert_der.clone())],
            rustls::PrivateKey(identity.key_der.clone()),
        )?;
    Ok(std::sync::Arc::new(config))
}

/// How long to wait for any signaling traffic before treating the websocket as
/// dead. The server echoes our pings, so a healthy idle connection reads at
/// least every `PING_INTERVAL`.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const PING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

/// Backoff bounds for transparent reconnects while we're still waiting for the
/// peer to start the SDP exchange.
const MIN_RECONNECT_BACKOFF: std::time::Duration = std::time::Duration::from_millis(500);
const MAX_RECONNECT_BACKOFF: std::time::Duration = std::time::Duration::from_secs(8);

/// One data channel's `(label, init)`. The caller owns the channel policy
/// (label / stream id / reliability) rather than this crate hardcoding it, and
/// passes every channel the session needs so they're all created together,
/// before the offer. The `init` is cloned per attempt because [`connect`]
/// recreates the channels on every transparent reconnect (and creating one
/// consumes its `init`).
pub type ChannelSpec = (&'static str, datachannel_wrapper::DataChannelInit);

/// Build a fresh peer connection, create every requested channel on it, then
/// generate the offer. Returns immediately with a (candidate-less or partial)
/// local description — ICE candidates are trickled separately as they gather, so
/// the offer ships before gathering finishes. Channels come back in the same
/// order as `channels`.
///
/// Auto-negotiation is disabled and the offer is driven explicitly *after* all
/// channels exist: relying on auto-negotiation here raced the channel creation,
/// because creating the first channel kicks off offer generation + gathering on
/// libdatachannel's own thread, and a second `create_data_channel` landing
/// mid-negotiation made the captured `local_description` intermittently
/// inconsistent. One explicit `set_local_description` after both channels are
/// registered is deterministic (and mirrors the direct transport's bring-up).
async fn create_data_channels(
    rtc_config: datachannel_wrapper::RtcConfig,
    channels: &[ChannelSpec],
) -> Result<
    (
        Vec<datachannel_wrapper::DataChannel>,
        tokio::sync::mpsc::Receiver<datachannel_wrapper::PeerConnectionEvent>,
        datachannel_wrapper::PeerConnection,
    ),
    std::io::Error,
> {
    let (mut peer_conn, event_rx) = datachannel_wrapper::PeerConnection::new(rtc_config)?;

    let dcs = channels
        .iter()
        .map(|(label, init)| peer_conn.create_data_channel(label, init.clone()))
        .collect::<Result<Vec<_>, _>>()?;

    // All channels registered — now drive the single offer that puts them all
    // in the initial association and starts gathering.
    peer_conn.set_local_description(datachannel_wrapper::SdpType::Offer, None)?;

    // Trickle ICE: don't wait for gathering. `local_description()` already holds
    // the offer; candidates flow out of `event_rx` as they're gathered and the
    // caller forwards each as an `IceCandidate` packet.
    Ok((dcs, event_rx, peer_conn))
}

/// Encode and send one signaling `Packet` over the websocket.
async fn send_signal(
    stream: &mut SignalingStream,
    which: crate::proto::signaling::packet::Which,
) -> Result<(), tokio_tungstenite::tungstenite::Error> {
    stream
        .send(tokio_tungstenite::tungstenite::Message::Binary(
            crate::proto::signaling::Packet { which: Some(which) }.encode_to_vec(),
        ))
        .await
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("signaling abort: {0:?}")]
    ServerAbort(AbortReason),

    #[error("tungstenite: {0:?}")]
    Tungstenite(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("rustls: {0:?}")]
    Rustls(#[from] rustls::Error),

    #[error("io: {0:?}")]
    Io(#[from] std::io::Error),

    #[error("prost decode error: {0:?}")]
    ProstDecode(#[from] prost::DecodeError),

    #[error("url parse error: {0:?}")]
    UrlParse(#[from] url::ParseError),

    #[error("http error: {0:?}")]
    Http(#[from] tokio_tungstenite::tungstenite::http::Error),

    #[error("invalid packet")]
    InvalidPacket(tokio_tungstenite::tungstenite::Message),

    #[error("unexpected packet: {0:?}")]
    UnexpectedPacket(crate::proto::signaling::Packet),

    #[error("peer connection unexpectedly disconnected")]
    PeerConnectionDisconnected,

    #[error("peer connection failed")]
    PeerConnectionFailed,

    #[error("peer connection unexpectedly closed")]
    PeerConnectionClosed,
}

/// Whether an error is a transport-level hiccup that a reconnect might paper
/// over (websocket dropped, timed out, reset, EOF) as opposed to a definitive
/// protocol-level rejection (server abort, malformed/unexpected packet, bad
/// SDP). Only the former is worth retrying transparently.
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

/// The successful outcome of [`connect`]: the negotiated data channels and peer
/// connection, plus both ends' DTLS certificate fingerprints (raw SHA-256
/// digest bytes) as observed during the SDP exchange. The fingerprints let the
/// caller bind a later reconnect rendezvous to *this* connection's cryptographic
/// identities — per-connection, high-entropy, never persisted — rather than to a
/// value (like a game RNG seed) that might leak through other channels.
///
/// `local_fingerprint` is parsed from our own offer/answer SDP; `peer_fingerprint`
/// from the remote description libdatachannel verified against the peer's
/// certificate. Either may be empty if it couldn't be parsed — callers must
/// tolerate that.
pub struct Connected {
    pub channels: Vec<datachannel_wrapper::DataChannel>,
    pub peer_conn: datachannel_wrapper::PeerConnection,
    pub local_dtls_fingerprint: Vec<u8>,
    pub peer_dtls_fingerprint: Vec<u8>,
}

pub type Connecting = futures_util::future::BoxFuture<'static, Result<Connected, Error>>;

/// Parse a DTLS certificate fingerprint out of an SDP blob, returning the raw
/// SHA-256 digest bytes. SDP carries it as an `a=fingerprint:sha-256 <hex>`
/// attribute whose value is colon-separated, hex-encoded octets (e.g.
/// `AA:BB:...`). Returns `None` if there's no SHA-256 fingerprint line or it
/// doesn't decode; only `sha-256` is accepted (what libdatachannel emits).
fn parse_dtls_fingerprint(sdp: &str) -> Option<Vec<u8>> {
    for line in sdp.lines() {
        let Some(rest) = line.trim().strip_prefix("a=fingerprint:") else {
            continue;
        };
        let mut parts = rest.splitn(2, ' ');
        let algo = parts.next()?;
        if !algo.eq_ignore_ascii_case("sha-256") {
            continue;
        }
        let Some(hex) = parts.next() else { continue };
        let bytes: Option<Vec<u8>> = hex
            .split(':')
            .map(|octet| u8::from_str_radix(octet.trim(), 16).ok())
            .collect();
        match bytes {
            Some(b) if !b.is_empty() => return Some(b),
            _ => continue,
        }
    }
    None
}

/// Bring up a fresh signaling websocket end to end: connect, read the server's
/// `Hello`, build a new peer connection from the offered ICE servers, and send
/// our `Start`. This is the unit we re-run on a transparent reconnect, so every
/// attempt gets fresh ICE credentials and a brand-new local offer.
async fn establish(
    addr: &str,
    session_id: &str,
    use_relay: Option<bool>,
    protocol_version: u32,
    connection_id: &[u8],
    channels: &[ChannelSpec],
    tls_config: Option<&std::sync::Arc<rustls::ClientConfig>>,
) -> Result<
    (
        SignalingStream,
        Vec<datachannel_wrapper::DataChannel>,
        tokio::sync::mpsc::Receiver<datachannel_wrapper::PeerConnectionEvent>,
        datachannel_wrapper::PeerConnection,
    ),
    Error,
> {
    let mut url = url::Url::parse(addr)?;
    url.set_query(Some(
        &url::form_urlencoded::Serializer::new(String::new())
            .append_pair("session_id", session_id)
            .finish(),
    ));

    let mut req = url.to_string().into_client_request()?;
    req.headers_mut().append(
        "User-Agent",
        tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&format!(
            "tango-signaling/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .map_err(|e| tokio_tungstenite::tungstenite::http::Error::from(e))?,
    );
    req.headers_mut().append(
        "X-Tango-Protocol-Version",
        tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&format!("{:x}", protocol_version))
            .map_err(|e| tokio_tungstenite::tungstenite::http::Error::from(e))?,
    );
    // A `Connector::Rustls` carrying our client certificate, rebuilt per
    // attempt from the shared `ClientConfig` (the `Connector` itself isn't
    // `Clone`, but the `Arc<ClientConfig>` inside it is). With no identity we
    // pass `None`, which lets tungstenite fall back to its default connector.
    let connector = tls_config.map(|c| tokio_tungstenite::Connector::Rustls(c.clone()));
    let mut signaling_stream = match tokio_tungstenite::connect_async_tls_with_config(req, None, connector).await {
        Ok((signaling_stream, _)) => signaling_stream,
        Err(tokio_tungstenite::tungstenite::Error::Http(e)) if e.status() == http::StatusCode::BAD_REQUEST => {
            let abort = crate::proto::signaling::packet::Abort::decode(
                e.body().as_ref().map(|b| b.as_bytes()).unwrap_or_default(),
            )?;
            return Err(Error::ServerAbort(
                AbortReason::try_from(abort.reason).unwrap_or_default(),
            ));
        }
        Err(e) => {
            return Err(e.into());
        }
    };

    let Some(raw) = signaling_stream.try_next().await? else {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "stream ended early").into());
    };

    let packet = if let tokio_tungstenite::tungstenite::Message::Binary(d) = raw {
        crate::proto::signaling::Packet::decode(d.as_slice())?
    } else {
        return Err(Error::InvalidPacket(raw));
    };

    let Some(crate::proto::signaling::packet::Which::Hello(hello)) = packet.which else {
        return Err(Error::UnexpectedPacket(packet));
    };

    log::info!("hello received from signaling stream: {:?}", hello);

    let mut rtc_config = datachannel_wrapper::RtcConfig::new(
        &hello
            .ice_servers
            .into_iter()
            .flat_map(|ice_server| {
                ice_server
                    .urls
                    .into_iter()
                    .flat_map(|url| {
                        let Some(colon_idx) = url.chars().position(|c| c == ':') else {
                            return vec![];
                        };

                        let proto = &url[..colon_idx];
                        let rest = &url[colon_idx + 1..];

                        if (proto == "turn" || proto == "turns") && use_relay == Some(false) {
                            return vec![];
                        }

                        // libdatachannel doesn't support TURN over TCP: in fact, it explodes!
                        if url.chars().skip_while(|c| *c != '?').collect::<String>() == "?transport=tcp" {
                            return vec![];
                        }

                        if let (Some(username), Some(credential)) = (&ice_server.username, &ice_server.credential) {
                            vec![format!(
                                "{}:{}:{}@{}",
                                proto,
                                urlencoding::encode(username),
                                urlencoding::encode(credential),
                                rest
                            )]
                        } else {
                            vec![format!("{}:{}", proto, rest)]
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
    );
    if use_relay == Some(true) {
        rtc_config.ice_transport_policy = datachannel_wrapper::TransportPolicy::Relay;
    }
    rtc_config.disable_auto_negotiation = true;
    let (dcs, event_rx, peer_conn) = create_data_channels(rtc_config, channels).await?;

    signaling_stream
        .send(tokio_tungstenite::tungstenite::Message::Binary(
            crate::proto::signaling::Packet {
                which: Some(crate::proto::signaling::packet::Which::Start(
                    crate::proto::signaling::packet::Start {
                        protocol_version,
                        offer_sdp: peer_conn.local_description().unwrap().sdp.to_string(),
                        connection_id: connection_id.to_vec(),
                    },
                )),
            }
            .encode_to_vec(),
        ))
        .await?;

    Ok((signaling_stream, dcs, event_rx, peer_conn))
}

/// Outcome of waiting on a single signaling websocket for the peer to begin the
/// SDP exchange.
enum WaitOutcome {
    /// We received the peer's `Offer` (and answered it) or `Answer` (and applied
    /// it). The peer has committed to this handshake — `peer_conn` now holds the
    /// remote description and we proceed to the ICE phase.
    Exchanged,
    /// The websocket dropped (closed / reset / timed out / EOF) *before* the peer
    /// sent any SDP. Nothing is committed on either side, so it's safe to throw
    /// this connection away and reconnect from scratch.
    Dropped(Error),
}

/// Pump the signaling websocket, keeping it alive with pings, until either the
/// peer starts the SDP exchange or the connection drops underneath us.
///
/// The key invariant: once the peer has sent an `Offer` or `Answer`, both sides
/// are committed to *this* set of SDPs, so any subsequent failure is fatal and
/// propagates as `Err`. Only failures observed strictly before the peer says
/// anything become `Dropped`, which the caller may transparently reconnect.
async fn wait_for_exchange(
    signaling_stream: &mut SignalingStream,
    event_rx: &mut tokio::sync::mpsc::Receiver<datachannel_wrapper::PeerConnectionEvent>,
    peer_conn: &mut datachannel_wrapper::PeerConnection,
    pending_local_candidates: &mut Vec<String>,
) -> Result<WaitOutcome, Error> {
    let mut ping_interval = tokio::time::interval_at(tokio::time::Instant::now() + PING_INTERVAL, PING_INTERVAL);
    ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        let raw = tokio::select! {
            // Drain our own ICE candidates as they gather, buffering them until
            // the SDP exchange completes (the peer can't accept them before it
            // has our offer/answer); they're flushed right after. No connection
            // state can change before a remote description exists, so anything
            // else is ignored.
            event = event_rx.recv() => {
                if let Some(datachannel_wrapper::PeerConnectionEvent::IceCandidate(c)) = event {
                    pending_local_candidates.push(c.candidate);
                }
                continue;
            }
            _ = ping_interval.tick() => {
                if let Err(e) = signaling_stream
                    .send(tokio_tungstenite::tungstenite::Message::Binary(
                        crate::proto::signaling::Packet {
                            which: Some(crate::proto::signaling::packet::Which::Ping(
                                crate::proto::signaling::packet::Ping {},
                            )),
                        }
                        .encode_to_vec(),
                    ))
                    .await
                {
                    // Couldn't even send a keepalive: the socket is gone.
                    return Ok(WaitOutcome::Dropped(e.into()));
                }
                continue;
            }
            result = tokio::time::timeout(READ_TIMEOUT, signaling_stream.try_next()) => {
                match result {
                    // No traffic at all within the timeout: treat as a dead socket.
                    Err(_elapsed) => {
                        return Ok(WaitOutcome::Dropped(
                            std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out").into(),
                        ));
                    }
                    // Read error off the socket.
                    Ok(Err(e)) => return Ok(WaitOutcome::Dropped(e.into())),
                    // Clean EOF before the peer said anything.
                    Ok(Ok(None)) => {
                        return Ok(WaitOutcome::Dropped(
                            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "stream ended early").into(),
                        ));
                    }
                    Ok(Ok(Some(raw))) => raw,
                }
            }
        };

        let packet = match raw {
            tokio_tungstenite::tungstenite::Message::Binary(d) => {
                crate::proto::signaling::Packet::decode(d.as_slice())?
            }
            tokio_tungstenite::tungstenite::Message::Ping(_) => {
                // Note that upon receiving a ping message, tungstenite cues a pong reply automatically.
                // When you call either read_message, write_message or write_pending next it will try to send that pong out if the underlying connection can take more data.
                // This means you should not respond to ping frames manually.
                continue;
            }
            // The server closed the socket on us before any exchange happened
            // (e.g. it dropped the session). Safe to reconnect.
            tokio_tungstenite::tungstenite::Message::Close(_) => {
                return Ok(WaitOutcome::Dropped(
                    tokio_tungstenite::tungstenite::Error::ConnectionClosed.into(),
                ));
            }
            _ => {
                return Err(Error::InvalidPacket(raw));
            }
        };

        match &packet.which {
            Some(crate::proto::signaling::packet::Which::Ping(_)) => continue,
            Some(crate::proto::signaling::packet::Which::Abort(abort)) => {
                return Err(Error::ServerAbort(
                    AbortReason::try_from(abort.reason).unwrap_or_default(),
                ))
            }
            Some(crate::proto::signaling::packet::Which::Offer(offer)) => {
                log::info!("received an offer, this is the polite side. rolling back our local description and switching to answer");

                // From here on the peer has committed to this offer: any failure
                // is fatal, never a reconnect.
                peer_conn.set_local_description(datachannel_wrapper::SdpType::Rollback, None)?;
                peer_conn.set_remote_description(datachannel_wrapper::SessionDescription {
                    sdp_type: datachannel_wrapper::SdpType::Offer,
                    sdp: offer.sdp.clone(),
                })?;
                // Auto-negotiation is off (see `create_data_channels`), so the
                // answer is generated explicitly rather than implied by applying
                // the remote offer — otherwise `local_description` below would be
                // read before the answer existed.
                peer_conn.set_local_description(datachannel_wrapper::SdpType::Answer, None)?;

                let local_description = peer_conn.local_description().unwrap();
                signaling_stream
                    .send(tokio_tungstenite::tungstenite::Message::Binary(
                        crate::proto::signaling::Packet {
                            which: Some(crate::proto::signaling::packet::Which::Answer(
                                crate::proto::signaling::packet::Answer {
                                    sdp: local_description.sdp.to_string(),
                                },
                            )),
                        }
                        .encode_to_vec(),
                    ))
                    .await?;
                log::info!("sent answer to impolite side");
                return Ok(WaitOutcome::Exchanged);
            }
            Some(crate::proto::signaling::packet::Which::Answer(answer)) => {
                log::info!("received an answer, this is the impolite side");

                peer_conn.set_remote_description(datachannel_wrapper::SessionDescription {
                    sdp_type: datachannel_wrapper::SdpType::Answer,
                    sdp: answer.sdp.clone(),
                })?;
                return Ok(WaitOutcome::Exchanged);
            }
            _ => {
                return Err(Error::UnexpectedPacket(packet));
            }
        }
    }
}

pub async fn connect(
    addr: &str,
    session_id: &str,
    use_relay: Option<bool>,
    protocol_version: u32,
    channels: Vec<ChannelSpec>,
    identity: Option<ClientIdentity>,
) -> Result<Connecting, Error> {
    // A stable id for this logical connection attempt, sent with every `Start`.
    // It survives transparent reconnects, so when our offerer socket drops and
    // we re-dial with a fresh offer, the server recognizes the matching id and
    // replaces our stale offer instead of mistaking the new socket for the
    // answering peer.
    let connection_id: [u8; 16] = rand::random();

    // Build the mTLS client config once: it's identical across every
    // (re)connect, so the parse/validate cost (and any cert error) happens here,
    // surfaced to the caller alongside the initial dial. Cloned per attempt as a
    // cheap `Arc` bump in `establish`.
    let tls_config = match identity.as_ref() {
        Some(id) => Some(build_tls_config(id)?),
        None => None,
    };

    // The initial dial surfaces failures to the caller (so "couldn't reach the
    // matchmaking server" is reported promptly); transparent reconnects only
    // kick in once we've successfully connected at least once.
    let (mut signaling_stream, mut dcs, mut event_rx, mut peer_conn) = establish(
        addr,
        session_id,
        use_relay,
        protocol_version,
        &connection_id,
        &channels,
        tls_config.as_ref(),
    )
    .await?;

    let addr = addr.to_owned();
    let session_id = session_id.to_owned();

    Ok(Box::pin(async move {
        // Local ICE candidates gathered before the peer has our SDP — buffered by
        // `wait_for_exchange`, flushed once the exchange completes.
        let mut pending_local_candidates: Vec<String> = Vec::new();

        // Wait for the peer to start the SDP exchange. As long as the peer hasn't
        // started, a websocket drop is recoverable: tear everything down and dial
        // again with a fresh peer connection / offer.
        loop {
            match wait_for_exchange(
                &mut signaling_stream,
                &mut event_rx,
                &mut peer_conn,
                &mut pending_local_candidates,
            )
            .await?
            {
                WaitOutcome::Exchanged => break,
                WaitOutcome::Dropped(reason) => {
                    log::warn!(
                        "signaling websocket dropped before the peer started exchanging ({reason}); reconnecting transparently"
                    );

                    let mut backoff = MIN_RECONNECT_BACKOFF;
                    loop {
                        match establish(
                            &addr,
                            &session_id,
                            use_relay,
                            protocol_version,
                            &connection_id,
                            &channels,
                            tls_config.as_ref(),
                        )
                        .await
                        {
                            Ok((s, d, e, p)) => {
                                signaling_stream = s;
                                dcs = d;
                                event_rx = e;
                                peer_conn = p;
                                // Fresh peer connection → buffered candidates are
                                // stale; they'll gather anew.
                                pending_local_candidates.clear();
                                log::info!("signaling reconnected; still waiting for the peer");
                                break;
                            }
                            Err(e) if is_transient(&e) => {
                                log::warn!("signaling reconnect attempt failed ({e}); retrying in {backoff:?}");
                                tokio::time::sleep(backoff).await;
                                backoff = (backoff * 2).min(MAX_RECONNECT_BACKOFF);
                            }
                            // A protocol-level rejection won't fix itself on retry.
                            Err(e) => return Err(e),
                        }
                    }
                }
            }
        }

        // Both ends' DTLS fingerprints, parsed from the SDP each side committed
        // to: ours from the local description, the peer's from the remote one
        // libdatachannel just verified against the peer's certificate. The caller
        // pairs them to derive a rendezvous id both ends agree on.
        let local_dtls_fingerprint = peer_conn
            .local_description()
            .and_then(|d| parse_dtls_fingerprint(&d.sdp))
            .unwrap_or_default();
        let peer_dtls_fingerprint = peer_conn
            .remote_description()
            .and_then(|d| parse_dtls_fingerprint(&d.sdp))
            .unwrap_or_default();

        log::debug!(
            "local sdp (type = {:?}): {}",
            peer_conn.local_description().expect("local sdp").sdp_type,
            peer_conn.local_description().expect("local sdp").sdp
        );
        log::debug!(
            "remote sdp (type = {:?}): {}",
            peer_conn.remote_description().expect("remote sdp").sdp_type,
            peer_conn.remote_description().expect("remote sdp").sdp
        );

        // Trickle phase: both peers now hold each other's SDP. Flush the
        // candidates we buffered during the exchange, then keep the websocket
        // open to trickle new local candidates out and apply the peer's, until
        // our connection comes up. Each peer closes its own socket on `Connected`
        // (the server no longer closes them after the answer).
        for candidate in pending_local_candidates.drain(..) {
            let _ = send_signal(
                &mut signaling_stream,
                crate::proto::signaling::packet::Which::IceCandidate(crate::proto::signaling::packet::IceCandidate {
                    candidate,
                }),
            )
            .await;
        }

        let mut ping_interval = tokio::time::interval_at(tokio::time::Instant::now() + PING_INTERVAL, PING_INTERVAL);
        ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let outcome: Result<(), Error> = loop {
            tokio::select! {
                // The peer connection's own events are the authority on when we're
                // up, and the source of the local candidates we trickle out.
                ev = event_rx.recv() => match ev {
                    Some(datachannel_wrapper::PeerConnectionEvent::IceCandidate(c)) => {
                        let _ = send_signal(
                            &mut signaling_stream,
                            crate::proto::signaling::packet::Which::IceCandidate(
                                crate::proto::signaling::packet::IceCandidate { candidate: c.candidate },
                            ),
                        )
                        .await;
                    }
                    Some(datachannel_wrapper::PeerConnectionEvent::ConnectionStateChange(c)) => match c {
                        datachannel_wrapper::ConnectionState::Connected => break Ok(()),
                        datachannel_wrapper::ConnectionState::Disconnected => break Err(Error::PeerConnectionDisconnected),
                        datachannel_wrapper::ConnectionState::Failed => break Err(Error::PeerConnectionFailed),
                        datachannel_wrapper::ConnectionState::Closed => break Err(Error::PeerConnectionClosed),
                        _ => {}
                    },
                    Some(_) => {}
                    None => {
                        break Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "peer connection event stream ended",
                        )
                        .into())
                    }
                },
                // Incoming peer candidates over the websocket — best-effort: if
                // the socket dies here the already-exchanged candidates usually
                // suffice, so a read error isn't fatal; we keep waiting on the
                // connection state above.
                msg = tokio::time::timeout(READ_TIMEOUT, signaling_stream.try_next()) => {
                    if let Ok(Ok(Some(tokio_tungstenite::tungstenite::Message::Binary(d)))) = msg {
                        if let Ok(crate::proto::signaling::Packet {
                            which: Some(crate::proto::signaling::packet::Which::IceCandidate(c)),
                        }) = crate::proto::signaling::Packet::decode(d.as_slice())
                        {
                            let _ = peer_conn
                                .add_remote_candidate(datachannel_wrapper::IceCandidate { candidate: c.candidate });
                        }
                    }
                }
                _ = ping_interval.tick() => {
                    let _ = send_signal(
                        &mut signaling_stream,
                        crate::proto::signaling::packet::Which::Ping(crate::proto::signaling::packet::Ping {}),
                    )
                    .await;
                }
            }
        };

        // Connected or failed — either way we're done with signaling. Closing is
        // best-effort; losing the close race must not fail a healthy bring-up.
        let _ = signaling_stream.close(None).await;
        outcome?;

        Ok(Connected {
            channels: dcs,
            peer_conn,
            local_dtls_fingerprint,
            peer_dtls_fingerprint,
        })
    }))
}

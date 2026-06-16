//! QUIC transport for the direct (link-code) path. A single QUIC connection
//! over one UDP socket carries *both* netplay channels:
//!
//! * a reliable, ordered **bidirectional stream** for the control plane
//!   (lobby handshake + save-state transfer) — see [`super::super::control`];
//! * unreliable **datagrams** for the data plane (the per-frame `wire`
//!   traffic) — see [`super::super::data`].
//!
//! This replaces the old dual transport, where the reliable channel was a
//! length-prefixed TCP connection and the unreliable channel was a sibling UDP
//! socket on an ephemeral port. Folding both into one QUIC connection means the
//! host only has to forward a **single UDP port** ([`super::DEFAULT_LOCAL_PORT`])
//! — no second protocol, no ephemeral port to punch.
//!
//! **Stream framing.** A QUIC bidi stream is a byte stream, not a message
//! stream, so the reliable half re-uses the same 4-byte little-endian length
//! prefix the old TCP adapter used. Datagrams are already message-oriented, so
//! the unreliable half is a straight pass-through like the old UDP adapter.
//!
//! **Trust model.** Unchanged from the old plaintext transport: the address is
//! the identity. The host mints a throwaway self-signed cert each session and
//! the client skips certificate verification entirely. QUIC's mandatory
//! transport encryption rides along for free, so the wire is now encrypted —
//! but we make no authentication claim beyond "whoever answered on that
//! address".

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;

use super::{PacketSink, PacketStream, Receiver, Sender};

/// Maximum framed payload on the reliable stream. Matches [`super::tcp`]'s cap:
/// the biggest control packet is a save-state `Chunk`, well under this, so
/// anything larger is a framing bug or a hostile peer — bail rather than
/// allocate.
const MAX_FRAME_BYTES: u32 = 64 * 1024;

/// ALPN identifier. Both ends advertise exactly this; the QUIC handshake fails
/// fast if some other QUIC speaker dials the port.
const ALPN: &[u8] = b"tango-direct";

/// Idle timeout for the connection. The data plane pings every second over the
/// datagram channel (and we set a keep-alive below), so anything approaching
/// this means the peer is genuinely gone — the reliable receiver's read errors
/// out and the session's disconnect watch fires.
const MAX_IDLE: Duration = Duration::from_secs(20);
/// Keep-alive cadence. Comfortably under [`MAX_IDLE`] so a momentarily silent
/// link (e.g. between rounds) doesn't trip the idle timeout on its own.
const KEEP_ALIVE: Duration = Duration::from_secs(5);

/// The four transport halves a direct connection yields, in the shape
/// [`crate::netplay::NegotiationOutput`] consumes them: a reliable Sender +
/// Receiver for the lobby/handshake, and an unreliable Sender + Receiver for
/// the in-match `wire` datagrams. All four share one underlying QUIC
/// connection.
pub struct DirectChannels {
    pub reliable_sender: Sender,
    pub reliable_receiver: Receiver,
    pub in_match_sender: Sender,
    pub in_match_receiver: Receiver,
}

/// Keep-alive holder shared by all four transport halves. As long as any half
/// is live, the endpoint (which owns the UDP socket + driver) and the
/// connection both stay up — so the connection lives exactly as long as the
/// session that holds its Sender/Receiver pairs, with no separate handle to
/// thread through `NegotiationOutput` (cf. the WebRTC path's `peer_conn`).
struct QuicConn {
    /// Handle to the Tokio runtime the endpoint was built on, captured at
    /// construction (always inside the runtime). Used only by [`Drop`] — see
    /// there for why.
    handle: Option<tokio::runtime::Handle>,
    /// Both `Option` so [`Drop`] can drop them *within* an entered runtime
    /// context via `take()`; `Some` for the entire session otherwise.
    endpoint: Option<quinn::Endpoint>,
    connection: Option<quinn::Connection>,
}

impl QuicConn {
    /// The live connection. Infallible for the session's lifetime — only
    /// [`Drop`] empties it, after which no transport half exists to call this.
    fn connection(&self) -> &quinn::Connection {
        self.connection.as_ref().expect("connection live for the session")
    }
}

impl Drop for QuicConn {
    /// quinn's endpoint owns a Tokio-registered UDP socket, which panics
    /// ("no reactor running") if dropped on a thread with no runtime context.
    /// At app shutdown iced tears down its runtime and then drops the `App`
    /// (and the netplay state holding this `Arc`) on the winit main thread —
    /// exactly that no-context case. Re-entering the owning runtime handle
    /// puts a reactor back in scope so the socket drop takes its graceful
    /// shutdown path instead of panicking. The fields are dropped here, inside
    /// the guard's scope, via `take()` — letting the implicit field-drop run
    /// after `drop` returns would be back outside the guard.
    fn drop(&mut self) {
        let handle = self.handle.take();
        let _guard = handle.as_ref().map(|h| h.enter());
        if let Some(connection) = self.connection.take() {
            // Best-effort polite close so the peer disconnects promptly rather
            // than waiting out the idle timeout; the driver flushes it if the
            // runtime is still turning.
            connection.close(0u32.into(), b"closing");
        }
        self.endpoint.take();
    }
}

fn io_other<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
}

/// Map any stream/connection read failure to `UnexpectedEof`. The
/// [`PacketStream`] contract reports a clean close that way, and for our
/// callers (the lobby `negotiate`, the in-match disconnect watch) the only
/// thing that matters is that a vanished peer surfaces as an error.
fn io_eof<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::UnexpectedEof, e.to_string())
}

// ---------------------------------------------------------------------------
// Reliable channel: a QUIC bidi stream with TCP-style length framing.
// ---------------------------------------------------------------------------

struct ReliableSink {
    send: quinn::SendStream,
    _conn: Arc<QuicConn>,
}

#[async_trait::async_trait]
impl PacketSink for ReliableSink {
    async fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let len = bytes.len();
        if len as u64 > MAX_FRAME_BYTES as u64 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("frame too large: {len} bytes"),
            ));
        }
        self.send.write_all(&(len as u32).to_le_bytes()).await.map_err(io_other)?;
        self.send.write_all(bytes).await.map_err(io_other)?;
        Ok(())
    }
}

struct ReliableStream {
    recv: quinn::RecvStream,
    _conn: Arc<QuicConn>,
}

#[async_trait::async_trait]
impl PacketStream for ReliableStream {
    async fn recv(&mut self) -> std::io::Result<Vec<u8>> {
        let mut len_buf = [0u8; 4];
        self.recv.read_exact(&mut len_buf).await.map_err(io_eof)?;
        let len = u32::from_le_bytes(len_buf);
        if len > MAX_FRAME_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("oversized frame: {len} bytes"),
            ));
        }
        let mut buf = vec![0u8; len as usize];
        self.recv.read_exact(&mut buf).await.map_err(io_eof)?;
        Ok(buf)
    }
}

// ---------------------------------------------------------------------------
// Unreliable channel: QUIC datagrams, one datagram per message.
// ---------------------------------------------------------------------------

struct DatagramSink {
    conn: Arc<QuicConn>,
}

#[async_trait::async_trait]
impl PacketSink for DatagramSink {
    async fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        // `send_datagram` is synchronous (it queues, the driver flushes); a
        // too-large datagram or a closed connection is the only failure.
        self.conn
            .connection()
            .send_datagram(Bytes::copy_from_slice(bytes))
            .map_err(io_other)
    }
}

struct DatagramStream {
    conn: Arc<QuicConn>,
}

#[async_trait::async_trait]
impl PacketStream for DatagramStream {
    async fn recv(&mut self) -> std::io::Result<Vec<u8>> {
        let bytes = self.conn.connection().read_datagram().await.map_err(io_eof)?;
        Ok(bytes.to_vec())
    }
}

/// Assemble the four transport halves around an established connection. The
/// reliable halves own the bidi stream; the unreliable halves talk datagrams
/// over the shared `Connection`. Every half holds an `Arc<QuicConn>`, so the
/// endpoint + connection live until the last of them is dropped.
fn build(endpoint: quinn::Endpoint, connection: quinn::Connection, send: quinn::SendStream, recv: quinn::RecvStream) -> DirectChannels {
    let conn = Arc::new(QuicConn {
        handle: tokio::runtime::Handle::try_current().ok(),
        endpoint: Some(endpoint),
        connection: Some(connection),
    });
    DirectChannels {
        reliable_sender: Sender::new(Box::new(ReliableSink {
            send,
            _conn: conn.clone(),
        })),
        reliable_receiver: Receiver::new(Box::new(ReliableStream {
            recv,
            _conn: conn.clone(),
        })),
        in_match_sender: Sender::new(Box::new(DatagramSink { conn: conn.clone() })),
        in_match_receiver: Receiver::new(Box::new(DatagramStream { conn })),
    }
}

// ---------------------------------------------------------------------------
// Crypto: self-signed cert (host) + skip-verify (client). Address = identity.
// ---------------------------------------------------------------------------

/// Shared QUIC transport tuning for both ends: idle timeout + keep-alive.
/// Datagram support is on by default in quinn; we leave its buffers at their
/// defaults (a `wire` frame is tens of bytes — orders of magnitude under one
/// datagram).
fn transport_config() -> quinn::TransportConfig {
    let mut tc = quinn::TransportConfig::default();
    tc.max_idle_timeout(Some(MAX_IDLE.try_into().expect("idle timeout fits VarInt")));
    tc.keep_alive_interval(Some(KEEP_ALIVE));
    tc
}

/// Build the host's `ServerConfig` around a throwaway self-signed cert.
fn server_config() -> std::io::Result<quinn::ServerConfig> {
    let cert = rcgen::generate_simple_self_signed(vec!["tango".to_string()]).map_err(io_other)?;
    // Route the cert + key through DER bytes so the `pki_types` we hand quinn
    // are quinn's own, regardless of which version rcgen links.
    let cert_der = quinn::rustls::pki_types::CertificateDer::from(cert.cert.der().to_vec());
    let key_der = quinn::rustls::pki_types::PrivateKeyDer::Pkcs8(
        quinn::rustls::pki_types::PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()),
    );

    let provider = Arc::new(quinn::rustls::crypto::ring::default_provider());
    let mut crypto = quinn::rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(io_other)?
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .map_err(io_other)?;
    crypto.alpn_protocols = vec![ALPN.to_vec()];

    let mut server_config =
        quinn::ServerConfig::with_crypto(Arc::new(quinn::crypto::rustls::QuicServerConfig::try_from(crypto).map_err(io_other)?));
    server_config.transport_config(Arc::new(transport_config()));
    Ok(server_config)
}

/// A `ServerCertVerifier` that accepts any certificate — the address is the
/// identity, exactly as the old plaintext transport. Signature *verification*
/// still runs (so the handshake is cryptographically sound against a passive
/// attacker); only the cert-to-identity binding is skipped.
#[derive(Debug)]
struct SkipServerVerification(Arc<quinn::rustls::crypto::CryptoProvider>);

impl SkipServerVerification {
    fn new(provider: Arc<quinn::rustls::crypto::CryptoProvider>) -> Arc<Self> {
        Arc::new(Self(provider))
    }
}

impl quinn::rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &quinn::rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[quinn::rustls::pki_types::CertificateDer<'_>],
        _server_name: &quinn::rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: quinn::rustls::pki_types::UnixTime,
    ) -> Result<quinn::rustls::client::danger::ServerCertVerified, quinn::rustls::Error> {
        Ok(quinn::rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &quinn::rustls::pki_types::CertificateDer<'_>,
        dss: &quinn::rustls::DigitallySignedStruct,
    ) -> Result<quinn::rustls::client::danger::HandshakeSignatureValid, quinn::rustls::Error> {
        quinn::rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &quinn::rustls::pki_types::CertificateDer<'_>,
        dss: &quinn::rustls::DigitallySignedStruct,
    ) -> Result<quinn::rustls::client::danger::HandshakeSignatureValid, quinn::rustls::Error> {
        quinn::rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<quinn::rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Build the client's `ClientConfig` with the skip-verify verifier.
fn client_config() -> std::io::Result<quinn::ClientConfig> {
    let provider = Arc::new(quinn::rustls::crypto::ring::default_provider());
    let mut crypto = quinn::rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(io_other)?
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new(provider))
        .with_no_client_auth();
    crypto.alpn_protocols = vec![ALPN.to_vec()];

    let mut client_config =
        quinn::ClientConfig::new(Arc::new(quinn::crypto::rustls::QuicClientConfig::try_from(crypto).map_err(io_other)?));
    client_config.transport_config(Arc::new(transport_config()));
    Ok(client_config)
}

// ---------------------------------------------------------------------------
// Endpoint setup: host (listen + accept) / connect (dial).
// ---------------------------------------------------------------------------

/// Open a dual-stack UDP listen socket on `port`. One IPv6 socket with
/// `IPV6_V6ONLY` cleared serves both families (IPv4 arrives v4-mapped) —
/// matching the old TCP host's two-listener behaviour with a single socket.
/// Falls back to IPv4-only if v6 is unavailable on the box.
fn bind_listen_socket(port: u16) -> std::io::Result<std::net::UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};

    // quinn drives the socket via `tokio::net::UdpSocket::from_std`, which
    // requires it already be non-blocking — set that before handing it over.
    let try_v6 = || -> std::io::Result<std::net::UdpSocket> {
        let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_only_v6(false)?;
        socket.bind(&SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port).into())?;
        socket.set_nonblocking(true)?;
        Ok(socket.into())
    };
    match try_v6() {
        Ok(socket) => Ok(socket),
        Err(_) => {
            let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
            socket.bind(&SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port).into())?;
            socket.set_nonblocking(true)?;
            Ok(socket.into())
        }
    }
}

/// Host listen endpoints, one per port, kept bound for the process lifetime.
///
/// quinn frees an endpoint's UDP socket only once its driver winds down, and a
/// connection holds that driver open through a mandatory post-close QUIC drain
/// (a few × PTO). So a brand-new `Endpoint` on a just-disconnected port races
/// that drain and fails with `AddrInUse`. Instead of rebinding, we bind each
/// host port exactly once and reuse the endpoint for every session on it —
/// re-hosting is just another `accept()` on the socket that's already open, so
/// there's no rebind to race and the previous connection's drain happens
/// underneath the still-live endpoint.
static HOST_ENDPOINTS: std::sync::Mutex<Option<std::collections::HashMap<u16, quinn::Endpoint>>> =
    std::sync::Mutex::new(None);

/// The reusable listen endpoint for `port`, binding it the first time. The
/// returned handle is cheap to clone; the cache's own clone is what keeps the
/// socket bound across sessions.
fn host_endpoint(port: u16) -> std::io::Result<quinn::Endpoint> {
    let mut guard = HOST_ENDPOINTS.lock().unwrap();
    let endpoints = guard.get_or_insert_with(std::collections::HashMap::new);
    if let Some(endpoint) = endpoints.get(&port) {
        return Ok(endpoint.clone());
    }
    let socket = bind_listen_socket(port)?;
    let runtime = quinn::default_runtime().ok_or_else(|| io_other("no tokio runtime for quinn endpoint"))?;
    let endpoint = quinn::Endpoint::new(quinn::EndpointConfig::default(), Some(server_config()?), socket, runtime)?;
    endpoints.insert(port, endpoint.clone());
    Ok(endpoint)
}

/// Listen on `port` and accept the first peer that completes a QUIC handshake.
/// Reuses the persistent [`host_endpoint`] so re-hosting never rebinds, then
/// blocks until one peer connects and accepts the peer-opened bidi stream that
/// carries the reliable channel.
pub async fn host(port: u16) -> std::io::Result<DirectChannels> {
    let endpoint = host_endpoint(port)?;

    let incoming = endpoint.accept().await.ok_or_else(|| io_eof("endpoint closed before a peer connected"))?;
    let connection = incoming.accept().map_err(io_other)?.await.map_err(io_other)?;
    // The dialer opens the reliable bidi stream (and writes its first
    // `negotiate` Hello on it); we accept it. `accept_bi` resolves once that
    // first write arrives — no deadlock, since the dialer needs nothing from us
    // to send it.
    let (send, recv) = connection.accept_bi().await.map_err(io_other)?;
    Ok(build(endpoint, connection, send, recv))
}

/// Dial `addr` (a `host:port` string; the caller supplies the default port).
/// Like the old TCP connect: resolves the target, opens a QUIC connection,
/// then opens the reliable bidi stream the host accepts.
pub async fn connect(addr: &str) -> std::io::Result<DirectChannels> {
    let target = tokio::net::lookup_host(addr)
        .await?
        .next()
        .ok_or_else(|| io_other(format!("no addresses resolved for {addr}")))?;

    // Bind an ephemeral client socket of the target's family.
    let bind: SocketAddr = if target.is_ipv4() {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    } else {
        (Ipv6Addr::UNSPECIFIED, 0).into()
    };
    let mut endpoint = quinn::Endpoint::client(bind)?;
    endpoint.set_default_client_config(client_config()?);

    // Server name "tango" matches the cert SAN; verification is skipped anyway.
    let connection = endpoint.connect(target, "tango").map_err(io_other)?.await.map_err(io_other)?;
    // Open the reliable bidi stream; the host accepts it. quinn doesn't surface
    // it to the host until we write, which `negotiate`'s first Hello does.
    let (send, recv) = connection.open_bi().await.map_err(io_other)?;
    Ok(build(endpoint, connection, send, recv))
}

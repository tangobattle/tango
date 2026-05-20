//! Length-prefixed TCP adapter for the [`super::PacketSink`] /
//! [`super::PacketStream`] traits. Each on-wire frame is a 4-byte
//! little-endian `u32` payload length followed by exactly that many
//! bytes — the existing `protocol::Packet::serialize()` blob. This
//! recreates the message boundary the DataChannel gives us natively;
//! see the docs on [`super::PacketSink`] for the contract the rest
//! of the netplay stack depends on.
//!
//! `TCP_NODELAY` is enabled on the socket so per-tick `Input`
//! packets (≈6 bytes) aren't coalesced by Nagle into 40 ms batches
//! — that would dwarf the entire input-delay budget the lobby is
//! trying to negotiate.

use super::{PacketSink, PacketStream, Receiver, Sender};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{
    tcp::{OwnedReadHalf, OwnedWriteHalf},
    TcpListener, TcpStream,
};

/// Maximum framed payload size. WebRTC DataChannels are configured
/// for arbitrary-size SCTP messages, but the largest packet the
/// tango protocol ships is a save-state `Chunk` and those are
/// capped well under a megabyte. Anything larger is a framing-bug
/// or a hostile peer; bail rather than allocate.
const MAX_FRAME_BYTES: u32 = 64 * 1024;

struct TcpSink {
    inner: OwnedWriteHalf,
}

#[async_trait::async_trait]
impl PacketSink for TcpSink {
    async fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let len = bytes.len();
        if len as u64 > MAX_FRAME_BYTES as u64 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("frame too large: {len} bytes"),
            ));
        }
        self.inner.write_all(&(len as u32).to_le_bytes()).await?;
        self.inner.write_all(bytes).await?;
        Ok(())
    }
}

struct TcpRecvStream {
    inner: OwnedReadHalf,
}

#[async_trait::async_trait]
impl PacketStream for TcpRecvStream {
    async fn recv(&mut self) -> std::io::Result<Vec<u8>> {
        let mut len_buf = [0u8; 4];
        self.inner.read_exact(&mut len_buf).await?;
        let len = u32::from_le_bytes(len_buf);
        if len > MAX_FRAME_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("oversized frame: {len} bytes"),
            ));
        }
        let mut buf = vec![0u8; len as usize];
        self.inner.read_exact(&mut buf).await?;
        Ok(buf)
    }
}

fn wrap(stream: TcpStream) -> std::io::Result<(Sender, Receiver)> {
    stream.set_nodelay(true)?;
    let (rh, wh) = stream.into_split();
    Ok((
        Sender::new(Box::new(TcpSink { inner: wh })),
        Receiver::new(Box::new(TcpRecvStream { inner: rh })),
    ))
}

/// Bind a TCP listener on the given port across both address
/// families and wait for one peer to connect. Returns the framed
/// Sender + Receiver pair as soon as the connection arrives; both
/// listeners are dropped — i.e. only one peer per `host` call.
///
/// **Cross-platform dual-stack:** we try `[::]:port` first, then
/// `0.0.0.0:port`. On Linux the IPv6 socket is dual-stack by
/// default (`IPV6_V6ONLY=0`) so it already accepts IPv4 connections
/// as v4-mapped addresses, and the v4 bind will fail with
/// `AddrInUse` — which we treat as "already covered, carry on".
/// On Windows/macOS the IPv6 socket is v6-only by default, so the
/// v4 bind succeeds and we `select!` across both `accept()` calls.
/// If only one family is available (rare), we run with just that
/// one; only when both binds fail do we surface the error.
pub async fn host(port: u16) -> std::io::Result<(Sender, Receiver)> {
    let v6 = TcpListener::bind((std::net::Ipv6Addr::UNSPECIFIED, port)).await;
    let v4 = TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port)).await;
    let stream = match (v6, v4) {
        (Ok(v6), Ok(v4)) => tokio::select! {
            r = v6.accept() => r?.0,
            r = v4.accept() => r?.0,
        },
        (Ok(v6), Err(_)) => v6.accept().await?.0,
        (Err(_), Ok(v4)) => v4.accept().await?.0,
        (Err(e), Err(_)) => return Err(e),
    };
    wrap(stream)
}

/// Open a TCP connection to the given `host:port`. The caller is
/// responsible for substituting [`super::DEFAULT_LOCAL_PORT`] when
/// the user omitted the port.
pub async fn connect(addr: &str) -> std::io::Result<(Sender, Receiver)> {
    let stream = TcpStream::connect(addr).await?;
    wrap(stream)
}

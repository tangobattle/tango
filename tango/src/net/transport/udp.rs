//! UDP datagram adapter for the unreliable in-match channel on the direct
//! (link-code-less) path. It pairs with the TCP lobby/negotiate channel: TCP
//! carries the reliable handshake + lobby packets, this carries the per-frame
//! [`super::wire`] datagrams.
//!
//! Each UDP datagram is exactly one message, so — unlike [`super::tcp`] — no
//! length framing is needed. The socket is `connect`ed to the peer, so plain
//! `send`/`recv` suffice and stray datagrams from other sources are dropped by
//! the kernel. Trust model matches the old plaintext TCP transport: no
//! encryption, the address is the identity.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use tokio::net::UdpSocket;

use super::{PacketSink, PacketStream, Receiver, Sender};

/// Largest in-match datagram we'll accept. `wire` frames are tens of bytes
/// even with a wide redundancy window; this is generous slack and stays well
/// under the path MTU so datagrams never fragment.
const MAX_DATAGRAM: usize = 2048;

struct UdpSink {
    socket: Arc<UdpSocket>,
}

#[async_trait::async_trait]
impl PacketSink for UdpSink {
    async fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.socket.send(bytes).await?;
        Ok(())
    }
}

struct UdpStream {
    socket: Arc<UdpSocket>,
}

#[async_trait::async_trait]
impl PacketStream for UdpStream {
    async fn recv(&mut self) -> std::io::Result<Vec<u8>> {
        let mut buf = [0u8; MAX_DATAGRAM];
        let n = self.socket.recv(&mut buf).await?;
        Ok(buf[..n].to_vec())
    }
}

/// Wrap a connected UDP socket as a transport-agnostic Sender + Receiver pair.
/// Both halves share the socket (`tokio::net::UdpSocket` send/recv take `&self`).
pub fn pair(socket: UdpSocket) -> (Sender, Receiver) {
    let socket = Arc::new(socket);
    (
        Sender::new(Box::new(UdpSink { socket: socket.clone() })),
        Receiver::new(Box::new(UdpStream { socket })),
    )
}

/// Bind an ephemeral UDP socket of `peer_ip`'s family and `connect` it to the
/// peer. The peer's UDP port is discovered by swapping local ports over the
/// already-established reliable channel (`tcp_sender` / `tcp_receiver`) — this
/// runs right after `negotiate`, before the lobby loop takes the channel, so
/// the two 2-byte raw frames can't collide with lobby `Packet` traffic.
pub async fn connect_via(
    peer_ip: IpAddr,
    tcp_sender: &mut Sender,
    tcp_receiver: &mut Receiver,
) -> std::io::Result<UdpSocket> {
    let bind: SocketAddr = if peer_ip.is_ipv4() {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    } else {
        (Ipv6Addr::UNSPECIFIED, 0).into()
    };
    let socket = UdpSocket::bind(bind).await?;
    let local_port = socket.local_addr()?.port();

    // Full-duplex over TCP: both sides send-then-recv, so no deadlock.
    tcp_sender.send_raw(&local_port.to_le_bytes()).await?;
    let peer_port_bytes = tcp_receiver.recv_raw().await?;
    let peer_port = u16::from_le_bytes(
        peer_port_bytes
            .as_slice()
            .try_into()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "bad udp port frame"))?,
    );

    socket.connect(SocketAddr::new(peer_ip, peer_port)).await?;
    Ok(socket)
}

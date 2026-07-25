//! The post-negotiate lobby: the detached background loop that owns the
//! reliable receiver (pings, settings/commit/chunk/start-match dispatch).
//!
//! The loop emits its observations down an unbounded channel; bridging
//! that channel into a host's event loop is the host's business (see
//! [`State::take_lobby_events`](crate::State::take_lobby_events)), which
//! is the only part of this module that ever knew about a UI toolkit.

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::Message;

/// Lobby background loop: pings every second, reads incoming
/// packets, responds to Ping with Pong, measures Pong RTT. Any
/// other packet kind for now is logged and ignored. Exits
/// cleanly when the cancel token fires; emits `PeerDisconnected`
/// on a clean channel close, `Failed` on a transport error.
///
/// `tx` is an unbounded sender so sends are non-blocking — that's
/// important, because the only awaits in this loop are inside
/// `select!` arm heads (`cancel.cancelled()`, `ping_timer.tick()`,
/// `receiver.receive()`). If sends could block, a stuck consumer
/// would prevent the cancel arm from being re-polled and the
/// task could hang past `cancel.cancel()`.
pub(super) async fn run_lobby_loop(
    mut receiver: tango_session::net::Receiver,
    sender: Arc<tokio::sync::Mutex<tango_session::net::Sender>>,
    tx: futures::channel::mpsc::UnboundedSender<Message>,
    cancel: CancellationToken,
) -> tango_session::net::Receiver {
    let mut ping_timer = tokio::time::interval(tango_session::net::PING_INTERVAL);
    // First interval tick fires immediately by default; skip so
    // we don't ping before the peer is ready.
    ping_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return receiver,
            _ = ping_timer.tick() => {
                let now_short = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u16;
                if let Err(e) = sender.lock().await.send_ping(now_short).await {
                    log::warn!("lobby: send_ping failed: {e}");
                    let _ = tx.unbounded_send(Message::Failed(crate::Error::Other(format!("ping: {e}"))));
                    return receiver;
                }
            }
            packet = receiver.receive() => {
                match packet {
                    Ok(tango_net_protocol::control::Packet::Ping(p)) => {
                        if let Err(e) = sender.lock().await.send_pong(p.ts).await {
                            log::warn!("lobby: send_pong failed: {e}");
                            let _ = tx.unbounded_send(Message::Failed(crate::Error::Other(format!("pong: {e}"))));
                            return receiver;
                        }
                    }
                    Ok(tango_net_protocol::control::Packet::Pong(p)) => {
                        let now_short = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u16;
                        let dt = now_short.wrapping_sub(p.ts);
                        let _ = tx.unbounded_send(Message::PingMeasured(std::time::Duration::from_millis(dt as u64)));
                    }
                    Ok(tango_net_protocol::control::Packet::Settings(s)) => {
                        let _ = tx.unbounded_send(Message::RemoteSettings(Box::new(s)));
                    }
                    Ok(tango_net_protocol::control::Packet::Commit(c)) => {
                        let _ = tx.unbounded_send(Message::RemoteCommit(c.commitment));
                    }
                    Ok(tango_net_protocol::control::Packet::Uncommit(_)) => {
                        let _ = tx.unbounded_send(Message::RemoteUncommit);
                    }
                    Ok(tango_net_protocol::control::Packet::ChunkStart(c)) => {
                        let _ = tx.unbounded_send(Message::RemoteChunkStart(c.len));
                    }
                    Ok(tango_net_protocol::control::Packet::Chunk(c)) => {
                        let _ = tx.unbounded_send(Message::RemoteChunk(c.chunk));
                    }
                    Ok(tango_net_protocol::control::Packet::StartMatch(_)) => {
                        let _ = tx.unbounded_send(Message::RemoteStartMatch);
                    }
                    Ok(other) => {
                        // Hello (already handled in negotiate) and
                        // Input (only after StartMatch — round 6)
                        // land here today. Logged + ignored so they
                        // don't kill the lobby connection.
                        log::debug!("lobby: ignoring {:?}", std::mem::discriminant(&other));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        log::info!("lobby: peer disconnected (channel closed)");
                        let _ = tx.unbounded_send(Message::PeerDisconnected);
                        return receiver;
                    }
                    Err(e) => {
                        log::warn!("lobby: receive failed: {e}");
                        let _ = tx.unbounded_send(Message::Failed(crate::Error::Other(format!("recv: {e}"))));
                        return receiver;
                    }
                }
            }
        }
    }
}

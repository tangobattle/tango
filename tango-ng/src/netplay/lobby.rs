//! The post-negotiate lobby: the detached background loop that owns
//! the reliable receiver (pings, settings/commit/chunk/start-match
//! dispatch).
//!
//! Copied from `tango/src/netplay/lobby.rs`, transformed: the iced
//! subscription bridge (`subscription` / `LobbyTag` /
//! `build_lobby_stream`) is deleted — the loop's events go straight
//! into the main-loop `std::sync::mpsc` channel as
//! `crate::Event::Netplay`, drained by the 16ms timer. The loop body
//! is otherwise verbatim.

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::Message;

/// Lobby background loop: pings every second, reads incoming
/// packets, responds to Ping with Pong, measures Pong RTT. Any
/// other packet kind for now is logged and ignored. Exits
/// cleanly when the cancel token fires; emits `PeerDisconnected`
/// on a clean channel close, `Failed` on a transport error.
///
/// `tx` is a std mpsc sender, so sends are synchronous and
/// non-blocking (the channel is unbounded) — that's important,
/// because the only awaits in this loop are inside `select!` arm
/// heads (`cancel.cancelled()`, `ping_timer.tick()`,
/// `receiver.receive()`). If sends could block, a stuck consumer
/// would prevent the cancel arm from being re-polled and the
/// task could hang past `cancel.cancel()`.
pub(super) async fn run_lobby_loop(
    mut receiver: crate::net::Receiver,
    sender: Arc<tokio::sync::Mutex<crate::net::Sender>>,
    tx: std::sync::mpsc::Sender<crate::Event>,
    cancel: CancellationToken,
) -> crate::net::Receiver {
    let send = |msg: Message| {
        let _ = tx.send(crate::Event::Netplay(msg));
    };
    let mut ping_timer = tokio::time::interval(crate::net::PING_INTERVAL);
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
                    send(Message::Failed(format!("ping: {e}")));
                    return receiver;
                }
            }
            packet = receiver.receive() => {
                match packet {
                    Ok(crate::net::protocol::Packet::Ping(p)) => {
                        if let Err(e) = sender.lock().await.send_pong(p.ts).await {
                            log::warn!("lobby: send_pong failed: {e}");
                            send(Message::Failed(format!("pong: {e}")));
                            return receiver;
                        }
                    }
                    Ok(crate::net::protocol::Packet::Pong(p)) => {
                        let now_short = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u16;
                        let dt = now_short.wrapping_sub(p.ts);
                        send(Message::PingMeasured(std::time::Duration::from_millis(dt as u64)));
                    }
                    Ok(crate::net::protocol::Packet::Settings(s)) => {
                        send(Message::RemoteSettings(Box::new(s)));
                    }
                    Ok(crate::net::protocol::Packet::Commit(c)) => {
                        send(Message::RemoteCommit(c.commitment));
                    }
                    Ok(crate::net::protocol::Packet::Uncommit(_)) => {
                        send(Message::RemoteUncommit);
                    }
                    Ok(crate::net::protocol::Packet::Chunk(c)) => {
                        send(Message::RemoteChunk(c.chunk));
                    }
                    Ok(crate::net::protocol::Packet::StartMatch(_)) => {
                        send(Message::RemoteStartMatch);
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
                        send(Message::PeerDisconnected);
                        return receiver;
                    }
                    Err(e) => {
                        log::warn!("lobby: receive failed: {e}");
                        send(Message::Failed(format!("recv: {e}")));
                        return receiver;
                    }
                }
            }
        }
    }
}

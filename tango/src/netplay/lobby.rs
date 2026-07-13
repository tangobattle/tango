//! The post-negotiate lobby: the detached background loop that owns
//! the reliable receiver (pings, settings/commit/chunk/start-match
//! dispatch) and the iced subscription bridge that ferries its
//! events into the update loop.

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::{Message, Phase, State};

/// Subscription that forwards messages from the detached lobby
/// task to the iced event loop. Re-keyed on `session_id` so a
/// fresh Connect tears the previous bridge down; short-circuits
/// to empty when we're not in the lobby phase. The actual loop
/// runs on a `tokio::spawn` task owned by [`NegotiationDone`],
/// so dropping this subscription cannot abort the loop or strand
/// the data-channel receiver.
pub fn subscription(state: &State) -> iced::Subscription<Message> {
    if !matches!(state.phase, Phase::Lobby { .. }) {
        return iced::Subscription::none();
    }
    iced::Subscription::run_with(
        LobbyTag {
            session_id: state.session_id,
            event_rx_slot: state.lobby_event_rx_slot.clone(),
        },
        build_lobby_stream,
    )
}

/// Identity + payload for the lobby subscription. iced 0.14
/// hashes this to decide whether to keep the existing stream or
/// tear it down + restart: only `session_id` is mixed into the
/// hash, so the Arc can change freely without re-keying.
struct LobbyTag {
    session_id: u64,
    event_rx_slot: Arc<std::sync::Mutex<Option<futures::channel::mpsc::UnboundedReceiver<Message>>>>,
}

impl std::hash::Hash for LobbyTag {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        // Tag string + session id only. cancel_and_renew bumps
        // session_id so a fresh Connect re-keys the subscription.
        "netplay-lobby".hash(h);
        self.session_id.hash(h);
    }
}

/// Body of the lobby subscription. Pulled out as a free `fn`
/// because iced 0.14's `run_with` takes a function pointer, not
/// a closure, so the only state available is what comes in
/// through the [`LobbyTag`] argument. Just a passthrough that
/// drains the per-session event channel — owns no transport
/// state, so dropping it is harmless.
fn build_lobby_stream(tag: &LobbyTag) -> impl futures::Stream<Item = Message> {
    use futures::StreamExt;
    let rx = tag.event_rx_slot.lock().unwrap().take();
    match rx {
        Some(rx) => rx.left_stream(),
        // Re-key polled an already-consumed slot. Empty stream
        // until a new session repopulates lobby_event_rx_slot
        // (which only happens behind a fresh session_id, i.e.
        // a re-keyed Subscription anyway).
        None => futures::stream::empty().right_stream(),
    }
}

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
    mut receiver: crate::net::Receiver,
    sender: Arc<tokio::sync::Mutex<crate::net::Sender>>,
    tx: futures::channel::mpsc::UnboundedSender<Message>,
    cancel: CancellationToken,
) -> crate::net::Receiver {
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
                    let _ = tx.unbounded_send(Message::Failed(super::Error::Other(format!("ping: {e}"))));
                    return receiver;
                }
            }
            packet = receiver.receive() => {
                match packet {
                    Ok(crate::net::protocol::Packet::Ping(p)) => {
                        if let Err(e) = sender.lock().await.send_pong(p.ts).await {
                            log::warn!("lobby: send_pong failed: {e}");
                            let _ = tx.unbounded_send(Message::Failed(super::Error::Other(format!("pong: {e}"))));
                            return receiver;
                        }
                    }
                    Ok(crate::net::protocol::Packet::Pong(p)) => {
                        let now_short = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u16;
                        let dt = now_short.wrapping_sub(p.ts);
                        let _ = tx.unbounded_send(Message::PingMeasured(std::time::Duration::from_millis(dt as u64)));
                    }
                    Ok(crate::net::protocol::Packet::Settings(s)) => {
                        let _ = tx.unbounded_send(Message::RemoteSettings(Box::new(s)));
                    }
                    Ok(crate::net::protocol::Packet::Commit(c)) => {
                        let _ = tx.unbounded_send(Message::RemoteCommit(c.commitment));
                    }
                    Ok(crate::net::protocol::Packet::Uncommit(_)) => {
                        let _ = tx.unbounded_send(Message::RemoteUncommit);
                    }
                    Ok(crate::net::protocol::Packet::Chunk(c)) => {
                        let _ = tx.unbounded_send(Message::RemoteChunk(c.chunk));
                    }
                    Ok(crate::net::protocol::Packet::StartMatch(_)) => {
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
                        let _ = tx.unbounded_send(Message::Failed(super::Error::Other(format!("recv: {e}"))));
                        return receiver;
                    }
                }
            }
        }
    }
}

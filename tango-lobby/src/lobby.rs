//! The post-negotiate lobby's transport pump: the background task that
//! owns the reliable channel once a connection is up.
//!
//! Everything crossing the wire crosses it here. Inbound packets become
//! [`Inbound`] items on the session's progress channel, which
//! [`State::apply`](crate::State::apply) folds into the state machine;
//! outbound sends arrive as [`Command`]s the state machine pushes. That
//! split is what lets the state machine itself stay synchronous: it
//! decides and queues, the pump awaits.
//!
//! The pump is detached from whatever the host built to drain the
//! progress channel, so an incidental drop of that bridge can't abort it
//! mid-`.await` and strand the data-channel receiver. It exits only when
//! the cancellation token fires, and hands the receiver back down a
//! oneshot for the PvP handoff to take.

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::{Error, Inbound, Progress};

/// A wire send the state machine wants performed. Queued rather than
/// awaited so the handlers that decide on a send stay synchronous.
pub(crate) enum Command {
    Settings(Box<tango_net_protocol::control::Settings>),
    Commit([u8; 16]),
    Uncommit,
    /// Our zstd'd `NegotiatedState`. The pump announces the total length
    /// and streams it in `REVEAL_CHUNK_SIZE` pieces.
    Reveal(Vec<u8>),
    StartMatch,
}

/// Lobby pump: pings every second, answers Ping with Pong, measures Pong
/// RTT, forwards every other packet inward, and performs queued commands.
/// Exits cleanly when the cancel token fires, reporting
/// [`Inbound::PeerDisconnected`] on a clean channel close and
/// [`Inbound::Failed`] on a transport error.
///
/// `progress` is unbounded so reporting never blocks — important, because
/// the only awaits here are in `select!` arm heads. If a report could
/// block, a stuck consumer would keep the cancel arm from being re-polled
/// and the task could hang past `cancel.cancel()`.
pub(crate) async fn run_pump(
    mut receiver: tango_session::net::Receiver,
    sender: Arc<tokio::sync::Mutex<tango_session::net::Sender>>,
    mut commands: futures::channel::mpsc::UnboundedReceiver<Command>,
    progress: Progress,
    cancel: CancellationToken,
) -> tango_session::net::Receiver {
    use futures::StreamExt as _;

    // First tick after a full period, so we don't ping before the peer
    // has finished coming up.
    let mut ping_timer = tango_session::platform::Ticker::every(tango_session::net::PING_INTERVAL);
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return receiver,
            _ = ping_timer.tick() => {
                if let Err(e) = sender.lock().await.send_ping(now_short()).await {
                    progress.fail("ping", e);
                    return receiver;
                }
            }
            cmd = commands.next() => {
                // The state machine dropped its sender — the session is
                // over; wait for the cancel that follows it.
                let Some(cmd) = cmd else { continue };
                if let Err(e) = perform(cmd, &sender, &progress, &cancel).await {
                    progress.send(Inbound::Failed(e));
                    return receiver;
                }
            }
            packet = receiver.receive() => {
                use tango_net_protocol::control::Packet;
                match packet {
                    Ok(Packet::Ping(p)) => {
                        if let Err(e) = sender.lock().await.send_pong(p.ts).await {
                            progress.fail("pong", e);
                            return receiver;
                        }
                    }
                    Ok(Packet::Pong(p)) => {
                        let dt = now_short().wrapping_sub(p.ts);
                        progress.send(Inbound::Ping(std::time::Duration::from_millis(dt as u64)));
                    }
                    Ok(Packet::Settings(s)) => progress.send(Inbound::RemoteSettings(Box::new(s))),
                    Ok(Packet::Commit(c)) => progress.send(Inbound::RemoteCommit(c.commitment)),
                    Ok(Packet::Uncommit(_)) => progress.send(Inbound::RemoteUncommit),
                    Ok(Packet::ChunkStart(c)) => progress.send(Inbound::RemoteChunkStart(c.len)),
                    Ok(Packet::Chunk(c)) => progress.send(Inbound::RemoteChunk(c.chunk)),
                    Ok(Packet::StartMatch(_)) => progress.send(Inbound::RemoteStartMatch),
                    Ok(other) => {
                        // Hello (already handled in negotiate) and Input
                        // (only after StartMatch — round 6) land here
                        // today. Logged + ignored so they don't kill the
                        // lobby connection.
                        log::debug!("lobby: ignoring {:?}", std::mem::discriminant(&other));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        log::info!("lobby: peer disconnected (channel closed)");
                        progress.send(Inbound::PeerDisconnected);
                        return receiver;
                    }
                    Err(e) => {
                        progress.fail("recv", e);
                        return receiver;
                    }
                }
            }
        }
    }
}

/// Put one queued command on the wire. The reveal is the only one that
/// isn't a single packet: it announces its total length first, because
/// the receiving side counts arriving bytes against that rather than
/// watching for an end-of-stream sentinel.
async fn perform(
    cmd: Command,
    sender: &Arc<tokio::sync::Mutex<tango_session::net::Sender>>,
    progress: &Progress,
    cancel: &CancellationToken,
) -> Result<(), Error> {
    match cmd {
        Command::Settings(s) => wire("send_settings", sender.lock().await.send_settings(*s).await),
        Command::Commit(c) => wire("send_commit", sender.lock().await.send_commit(c).await),
        Command::Uncommit => wire("send_uncommit", sender.lock().await.send_uncommit().await),
        Command::StartMatch => wire("send_start_match", sender.lock().await.send_start_match().await),
        Command::Reveal(compressed) => {
            // Streamed off the pump so a multi-megabyte save doesn't stall
            // ping/pong or inbound packet handling behind it. Failures come
            // back through `progress` like any other wire error.
            let sender = sender.clone();
            let progress = progress.clone();
            let cancel = cancel.clone();
            tango_session::platform::spawn(async move {
                let stream = async {
                    wire(
                        "send_chunk_start",
                        sender.lock().await.send_chunk_start(compressed.len() as u64).await,
                    )?;
                    // bincode-framed Packet caps at 64 KB; 32 KB payload
                    // leaves room for the discriminant + length prefix.
                    // Protocol-visible, so the size lives with the codec.
                    for chunk in compressed.chunks(tango_net_protocol::control::REVEAL_CHUNK_SIZE) {
                        wire("send_chunk", sender.lock().await.send_chunk(chunk.to_vec()).await)?;
                    }
                    Ok(())
                };
                let result = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return,
                    r = stream => r,
                };
                if let Err(e) = result {
                    progress.send(Inbound::Failed(e));
                }
            });
            Ok(())
        }
    }
}

fn wire(what: &str, result: std::io::Result<()>) -> Result<(), Error> {
    result.map_err(|e| Error::Other(format!("{what}: {e}")))
}

/// Wall clock truncated to 16 bits of milliseconds — the ping timestamp
/// the protocol carries. Wrapping subtraction recovers the delta, which
/// is why only the low bits need to agree.
fn now_short() -> u16 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u16
}

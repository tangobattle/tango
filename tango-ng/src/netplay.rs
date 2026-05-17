//! Netplay state + connection lifecycle.
//!
//! Phase transitions:
//! `Idle → Connecting → Negotiating → Lobby` (any → `Failed` on
//! error; any → `Idle` on user Disconnect). A live [`CancellationToken`]
//! kept on the State aborts the in-flight async task on Disconnect /
//! re-Connect — without it the orphaned future would keep racing the
//! new one and clobber state when it eventually resolved. Each
//! Message handler verifies `phase` before applying so late results
//! from a cancelled task no-op cleanly.
//!
//! The lobby background loop (post-negotiate) lives in an iced
//! `Subscription::run_with_id`-keyed by `session_id`, so a fresh
//! Connect tears the previous subscription down by changing the id.
//! The loop pings every second + reads packets — anything other
//! than Ping/Pong/Settings ends the session with `Failed`.

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub mod compat;

/// Same protocol version the legacy app speaks (`tango/src/net/protocol.rs`).
/// Bumped in lockstep with the signaling server's allowlist; keep
/// this in sync or the server rejects the handshake with
/// `AbortReason::ProtocolVersionTooOld` / `TooNew`.
pub const PROTOCOL_VERSION: u32 = 0x3a;

/// Where the lifecycle is right now. Drives the Play tab's status
/// bar + the Cancel button's visibility.
#[derive(Clone, Debug)]
pub enum Phase {
    /// No connection attempt in flight.
    Idle,
    /// Signaling websocket open; waiting for the WebRTC handshake.
    Connecting { link_code: String },
    /// Data channel up; exchanging Hello packets / verifying both
    /// peers speak the same `protocol::VERSION`.
    Negotiating { link_code: String },
    /// Both peers agreed on the protocol. Lobby loop is running in
    /// the background; settings exchange + match start come next.
    Lobby { link_code: String },
    /// Last attempt failed. Stays here until the user starts a new
    /// connection or clears the field.
    Failed { error: String },
}

impl Default for Phase {
    fn default() -> Self {
        Self::Idle
    }
}

pub struct State {
    pub phase: Phase,
    /// Live connection objects, when post-negotiate. Cleared on
    /// Disconnect / Failed / on the next Connect.
    conn: Option<ConnectionHandles>,
    /// Cancellation token shared with every in-flight async task
    /// (signaling, negotiate, lobby loop). `Disconnect` calls
    /// `cancel()`, which makes the running task short-circuit via
    /// the `tokio::select!` arms below; the late result Message
    /// then no-ops because `phase` no longer matches.
    cancel: CancellationToken,
    /// Monotonic counter for the iced subscription key. Bumped on
    /// every Connect so the prior lobby subscription is torn down
    /// even if the user reconnects within the same Phase::Lobby.
    session_id: u64,
    /// Receiver handed off to the lobby subscription on first poll.
    /// Stored as a once-take Arc<Mutex<Option<_>>> so the closure
    /// can take it without needing &mut access to the State.
    pending_receiver: Arc<parking_lot::Mutex<Option<crate::net::Receiver>>>,
    /// Lobby-only state — what each side has advertised so far.
    /// `local` is what we sent; `remote` is what came in over the
    /// Settings packet. Both being `Some` means the lobby pane
    /// can render the symmetric "you vs them" view.
    pub lobby: LobbyState,
}

#[derive(Default, Clone)]
pub struct LobbyState {
    pub local: Option<crate::net::protocol::Settings>,
    pub remote: Option<crate::net::protocol::Settings>,
    /// Most recent measured round-trip ping. None before the first
    /// Pong; updated by `PingMeasured` from the lobby loop.
    pub latency: Option<std::time::Duration>,
    /// User-picked match type (mode + subtype). Defaults to (0, 0)
    /// = Single. Local-only UI state; gets folded into Settings
    /// on send.
    pub match_type: (u8, u8),
    /// User-picked input delay frames. Higher = smoother on flaky
    /// connections, more responsive on good ones. Range 0..=10;
    /// default 3 matches the legacy app.
    pub input_delay: u8,
}

impl Default for State {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            conn: None,
            cancel: CancellationToken::new(),
            session_id: 0,
            pending_receiver: Arc::new(parking_lot::Mutex::new(None)),
            lobby: LobbyState {
                input_delay: 3,
                ..LobbyState::default()
            },
        }
    }
}

/// Handles we hang onto for the duration of a connected session:
/// the Sender (locked behind a tokio Mutex because the lobby loop
/// + the eventual battle loop share it), and the peer-connection
/// itself so the underlying RTC stays up.
struct ConnectionHandles {
    sender: Arc<tokio::sync::Mutex<crate::net::Sender>>,
    _peer_conn: datachannel_wrapper::PeerConnection,
}

/// Messages the netplay subsystem emits + accepts. App routes
/// these via `Message::Netplay(_)`.
#[derive(Debug, Clone)]
pub enum Message {
    /// User pressed Play with a link code. Kicks off the async
    /// connect task.
    Connect { link_code: String, endpoint: String },
    /// Tear down the active / pending connection. Cancels the
    /// running async task; drops the connection handles.
    Disconnect,
    /// Internal: the signaling + WebRTC handshake resolved. We then
    /// kick off the protocol negotiate task before lifecycle moves
    /// out of Connecting.
    SignalingDone(Slot<ConnectionPayload>),
    /// Internal: protocol negotiate succeeded. Receiver is parked
    /// in the slot for the lobby subscription to take.
    NegotiationDone(Slot<NegotiationOutput>),
    /// Internal: any step (signaling, datachannel, negotiate, or
    /// lobby loop) failed. Includes the user-readable error
    /// message.
    Failed(String),
    /// Internal: lobby loop noticed the peer disconnected (data
    /// channel closed cleanly without a Failed-worthy error).
    /// We end the session quietly back at Idle.
    PeerDisconnected,
    /// Internal: lobby loop measured a round-trip ping. Drives the
    /// latency indicator on the lobby pane.
    PingMeasured(std::time::Duration),
    /// User has reached the lobby and we have the data needed to
    /// build a Settings packet — send it over the wire. App
    /// dispatches this exactly once per Lobby entry.
    SendLocalSettings(Box<crate::net::protocol::Settings>),
    /// Internal: lobby loop saw a Settings packet from the peer.
    RemoteSettings(Box<crate::net::protocol::Settings>),
    /// Internal: best-effort ack that our Settings made it onto the
    /// wire. No-op message; just bumps the state-changed counter
    /// so iced re-renders.
    LocalSettingsSent,
    /// User changed the match-type pick. Lobby state updates and
    /// the App resends the Settings packet.
    SetMatchType((u8, u8)),
    /// User dragged the input-delay slider. Same resend flow.
    SetInputDelay(u8),
}

/// Single-take Arc<Mutex<Option<T>>> we use to pass non-Clone /
/// non-Sync payloads through iced's `Task::perform` boundary. The
/// runtime needs `Message: Clone + Send`, and DataChannel /
/// PeerConnection aren't Clone — this wrapper papers over that by
/// taking the inner once on receipt and going None afterwards.
pub type Slot<T> = Arc<parking_lot::Mutex<Option<T>>>;

pub struct ConnectionPayload {
    pub dc: datachannel_wrapper::DataChannel,
    pub peer_conn: datachannel_wrapper::PeerConnection,
}

impl std::fmt::Debug for ConnectionPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ConnectionPayload { .. }")
    }
}

/// Output of the negotiate task — the post-handshake sender /
/// receiver (the lobby + match loops own them from here) and the
/// peer-conn handle they need to stay alive against.
pub struct NegotiationOutput {
    pub sender: Arc<tokio::sync::Mutex<crate::net::Sender>>,
    pub receiver: crate::net::Receiver,
    pub peer_conn: datachannel_wrapper::PeerConnection,
}

impl std::fmt::Debug for NegotiationOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NegotiationOutput { .. }")
    }
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        !matches!(self.phase, Phase::Idle | Phase::Failed { .. })
    }

    /// Reset the cancellation token + bump session_id. Called from
    /// every transition that starts or stops async work so the
    /// background tasks notice and the subscription rekeys.
    fn cancel_and_renew(&mut self) {
        self.cancel.cancel();
        self.cancel = CancellationToken::new();
        self.session_id = self.session_id.wrapping_add(1);
        *self.pending_receiver.lock() = None;
        self.conn = None;
        self.lobby = LobbyState::default();
    }

    /// Apply a Message. Returns the iced Task to schedule for any
    /// async follow-up.
    pub fn update(&mut self, msg: Message) -> iced::Task<Message> {
        match msg {
            Message::Connect { link_code, endpoint } => {
                self.cancel_and_renew();
                self.phase = Phase::Connecting {
                    link_code: link_code.clone(),
                };
                let cancel = self.cancel.clone();
                iced::Task::perform(
                    run_connect(endpoint, link_code, cancel),
                    map_connect_result,
                )
            }
            Message::SignalingDone(slot_rx) => {
                let link_code = match &self.phase {
                    Phase::Connecting { link_code } => link_code.clone(),
                    // Cancelled / superseded — late delivery, ignore.
                    _ => return iced::Task::none(),
                };
                let Some(payload) = slot_rx.lock().take() else {
                    return iced::Task::none();
                };
                self.phase = Phase::Negotiating { link_code };
                let cancel = self.cancel.clone();
                iced::Task::perform(run_negotiate(payload, cancel), map_negotiate_result)
            }
            Message::NegotiationDone(slot_rx) => {
                let link_code = match &self.phase {
                    Phase::Negotiating { link_code } => link_code.clone(),
                    _ => return iced::Task::none(),
                };
                let Some(out) = slot_rx.lock().take() else {
                    return iced::Task::none();
                };
                self.conn = Some(ConnectionHandles {
                    sender: out.sender,
                    _peer_conn: out.peer_conn,
                });
                // Park the receiver for the lobby subscription to
                // pick up on its first poll.
                *self.pending_receiver.lock() = Some(out.receiver);
                self.phase = Phase::Lobby { link_code };
                iced::Task::none()
            }
            Message::PingMeasured(dur) => {
                self.lobby.latency = Some(dur);
                iced::Task::none()
            }
            Message::SendLocalSettings(settings) => {
                // Only meaningful in Lobby phase; ignore late
                // arrivals after a Disconnect/Failed.
                if !matches!(self.phase, Phase::Lobby { .. }) {
                    return iced::Task::none();
                }
                // Dedupe — `make_local_settings()` re-runs on
                // every Play / Netplay handler dispatch and most
                // of those don't actually change anything that
                // crosses the wire.
                if self.lobby.local.as_ref() == Some(&*settings) {
                    return iced::Task::none();
                }
                let Some(sender) = self.conn.as_ref().map(|c| c.sender.clone()) else {
                    return iced::Task::none();
                };
                self.lobby.local = Some(*settings.clone());
                iced::Task::perform(
                    async move {
                        sender
                            .lock()
                            .await
                            .send_settings(*settings)
                            .await
                            .map_err(|e| format!("send_settings: {e}"))
                    },
                    |r| match r {
                        Ok(()) => Message::LocalSettingsSent,
                        Err(e) => Message::Failed(e),
                    },
                )
            }
            Message::LocalSettingsSent => iced::Task::none(),
            Message::RemoteSettings(settings) => {
                self.lobby.remote = Some(*settings);
                iced::Task::none()
            }
            Message::SetMatchType(mt) => {
                self.lobby.match_type = mt;
                iced::Task::none()
            }
            Message::SetInputDelay(d) => {
                self.lobby.input_delay = d.min(10);
                iced::Task::none()
            }
            Message::Failed(e) => {
                self.cancel_and_renew();
                self.phase = Phase::Failed { error: e };
                iced::Task::none()
            }
            Message::PeerDisconnected => {
                // Clean remote-side close (data channel went None
                // without a Failed-worthy error). Quietly return
                // to Idle.
                self.cancel_and_renew();
                self.phase = Phase::Idle;
                iced::Task::none()
            }
            Message::Disconnect => {
                self.cancel_and_renew();
                self.phase = Phase::Idle;
                iced::Task::none()
            }
        }
    }
}

/// Subscription that runs the lobby background loop while we're in
/// Phase::Lobby. Re-keyed on `session_id` so a fresh Connect tears
/// the previous loop down, and short-circuits to empty when we're
/// not in the lobby phase.
pub fn subscription(state: &State) -> iced::Subscription<Message> {
    if !matches!(state.phase, Phase::Lobby { .. }) {
        return iced::Subscription::none();
    }
    let Some(handles) = state.conn.as_ref() else {
        return iced::Subscription::none();
    };
    let sender = handles.sender.clone();
    let pending = state.pending_receiver.clone();
    let cancel = state.cancel.clone();
    iced::Subscription::run_with_id(
        ("netplay-lobby", state.session_id),
        iced::stream::channel(16, move |tx| async move {
            let Some(receiver) = pending.lock().take() else {
                return;
            };
            run_lobby_loop(receiver, sender, tx, cancel).await;
        }),
    )
}

fn slot<T>(payload: T) -> Slot<T> {
    Arc::new(parking_lot::Mutex::new(Some(payload)))
}

fn map_connect_result(result: Result<ConnectionPayload, String>) -> Message {
    match result {
        Ok(payload) => Message::SignalingDone(slot(payload)),
        Err(e) => Message::Failed(e),
    }
}

fn map_negotiate_result(result: Result<NegotiationOutput, String>) -> Message {
    match result {
        Ok(out) => Message::NegotiationDone(slot(out)),
        Err(e) => Message::Failed(e),
    }
}

/// Run the two-stage signaling handshake (`connect()` websocket +
/// the WebRTC ICE exchange) until the data channel is open. Aborts
/// cleanly if the cancellation token fires.
async fn run_connect(
    endpoint: String,
    link_code: String,
    cancel: CancellationToken,
) -> Result<ConnectionPayload, String> {
    let work = async {
        let connecting = tango_signaling::connect(
            &endpoint,
            &link_code,
            // None = let the server pick: STUN by default, TURN
            // when peers can't reach each other directly.
            None,
            PROTOCOL_VERSION,
        )
        .await
        .map_err(|e| format!("signaling: {e}"))?;
        let (dc, peer_conn) = connecting.await.map_err(|e| format!("webrtc: {e}"))?;
        Ok::<_, String>(ConnectionPayload { dc, peer_conn })
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err("cancelled".to_string()),
        out = work => out,
    }
}

/// Split the data channel + run `protocol::negotiate`. Aborts on
/// cancel.
async fn run_negotiate(
    payload: ConnectionPayload,
    cancel: CancellationToken,
) -> Result<NegotiationOutput, String> {
    let ConnectionPayload { dc, peer_conn } = payload;
    let (dc_tx, dc_rx) = dc.split();
    let mut sender = crate::net::Sender::new(dc_tx);
    let mut receiver = crate::net::Receiver::new(dc_rx);
    let work = crate::net::negotiate(&mut sender, &mut receiver);
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err("cancelled".to_string()),
        result = work => {
            result.map_err(|e| format!("negotiate: {e}"))?;
            Ok(NegotiationOutput {
                sender: Arc::new(tokio::sync::Mutex::new(sender)),
                receiver,
                peer_conn,
            })
        }
    }
}

/// Lobby background loop: pings every second, reads incoming
/// packets, responds to Ping with Pong, measures Pong RTT. Any
/// other packet kind for now is logged and ignored (Settings /
/// Commit / Chunk wiring lands in the next round). Exits cleanly
/// when the cancel token fires; emits `PeerDisconnected` on a
/// clean channel close, `Failed` on a transport error.
async fn run_lobby_loop(
    mut receiver: crate::net::Receiver,
    sender: Arc<tokio::sync::Mutex<crate::net::Sender>>,
    mut tx: futures::channel::mpsc::Sender<Message>,
    cancel: CancellationToken,
) {
    use futures::SinkExt;
    let mut ping_timer = tokio::time::interval(crate::net::PING_INTERVAL);
    // First interval tick fires immediately by default; skip so
    // we don't ping before the peer is ready.
    ping_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_ping_sent: Option<std::time::SystemTime> = None;
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            _ = ping_timer.tick() => {
                let ts = std::time::SystemTime::now();
                last_ping_sent = Some(ts);
                if let Err(e) = sender.lock().await.send_ping(ts).await {
                    log::warn!("lobby: send_ping failed: {e}");
                    let _ = tx.send(Message::Failed(format!("ping: {e}"))).await;
                    return;
                }
            }
            packet = receiver.receive() => {
                match packet {
                    Ok(crate::net::protocol::Packet::Ping(p)) => {
                        if let Err(e) = sender.lock().await.send_pong(p.ts).await {
                            log::warn!("lobby: send_pong failed: {e}");
                            let _ = tx.send(Message::Failed(format!("pong: {e}"))).await;
                            return;
                        }
                    }
                    Ok(crate::net::protocol::Packet::Pong(p)) => {
                        if let Ok(dt) = std::time::SystemTime::now().duration_since(p.ts) {
                            let _ = tx.send(Message::PingMeasured(dt)).await;
                        }
                        let _ = last_ping_sent.take();
                    }
                    Ok(crate::net::protocol::Packet::Settings(s)) => {
                        let _ = tx.send(Message::RemoteSettings(Box::new(s))).await;
                    }
                    Ok(other) => {
                        // Commit / Chunk / StartMatch / Input land
                        // in the next netplay round; ignore for now
                        // so they don't kill the lobby connection.
                        log::debug!("lobby: ignoring {:?}", std::mem::discriminant(&other));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        log::info!("lobby: peer disconnected (channel closed)");
                        let _ = tx.send(Message::PeerDisconnected).await;
                        return;
                    }
                    Err(e) => {
                        log::warn!("lobby: receive failed: {e}");
                        let _ = tx.send(Message::Failed(format!("recv: {e}"))).await;
                        return;
                    }
                }
            }
        }
    }
}

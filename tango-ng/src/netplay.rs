//! Netplay state + connection lifecycle. Round-1 scope: wire up
//! `tango_signaling::connect` so Play with a non-empty link code
//! reaches the matchmaking server, establishes the WebRTC data
//! channel, and lands in a "connected" placeholder state. Protocol
//! negotiation, lobby settings exchange, and the live battle loop
//! arrive in subsequent rounds.

use std::sync::Arc;

/// Same protocol version the legacy app speaks (`tango/src/net/protocol.rs`).
/// Bumped in lockstep with the signaling server's allowlist; keep
/// this in sync or the server rejects the handshake with
/// `AbortReason::ProtocolVersionTooOld` / `TooNew`.
pub const PROTOCOL_VERSION: u32 = 0x3b;

/// Where the lifecycle is right now. Drives the Play tab's status
/// bar + the Cancel button's visibility.
#[derive(Clone, Debug)]
pub enum Phase {
    /// No connection attempt in flight.
    Idle,
    /// Signaling websocket open; waiting for the WebRTC handshake.
    Connecting { link_code: String },
    /// Data channel up. Future rounds will move from here into
    /// settings exchange + lobby + match.
    Connected { link_code: String },
    /// Last attempt failed. Stays here until the user starts a new
    /// connection or clears the field.
    Failed { error: String },
}

impl Default for Phase {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Default)]
pub struct State {
    pub phase: Phase,
    /// Live connection objects, when [`Phase::Connected`]. Cleared
    /// on Disconnect or on the next Connect.
    conn: Option<ConnectionHandles>,
}

/// Handles we hang onto for the duration of a connected session.
/// Dropping them tears down the data channel + the underlying RTC
/// peer connection.
struct ConnectionHandles {
    _dc: datachannel_wrapper::DataChannel,
    _peer_conn: datachannel_wrapper::PeerConnection,
}

/// Messages the netplay subsystem emits + accepts. App routes
/// these via `Message::Netplay(_)`.
#[derive(Debug, Clone)]
pub enum Message {
    /// User pressed Play with a link code. Kicks off the async
    /// connect task.
    Connect { link_code: String, endpoint: String },
    /// Tear down the active / pending connection.
    Disconnect,
    /// Internal: the async connect task resolved.
    Connected(Arc<parking_lot::Mutex<Option<ConnectionPayload>>>),
    /// Internal: the async connect task failed.
    Failed(String),
}

/// One-shot bundle of the data-channel + peer-conn returned by the
/// connect task. Wrapped in an `Arc<Mutex<Option<…>>>` so it can be
/// `Clone + Send` for iced's Task machinery; we take() it once on
/// receipt and the inner Option goes None.
pub struct ConnectionPayload {
    pub dc: datachannel_wrapper::DataChannel,
    pub peer_conn: datachannel_wrapper::PeerConnection,
}

impl std::fmt::Debug for ConnectionPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ConnectionPayload { .. }")
    }
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        !matches!(self.phase, Phase::Idle | Phase::Failed { .. })
    }

    /// Apply a Message. Returns a Task for any async work
    /// (currently just the initial Connect → signaling handshake).
    pub fn update(&mut self, msg: Message) -> iced::Task<Message> {
        match msg {
            Message::Connect { link_code, endpoint } => {
                self.conn = None;
                self.phase = Phase::Connecting { link_code: link_code.clone() };
                iced::Task::perform(
                    async move { run_connect(endpoint, link_code).await },
                    |result| match result {
                        Ok(payload) => Message::Connected(Arc::new(parking_lot::Mutex::new(
                            Some(payload),
                        ))),
                        Err(e) => Message::Failed(e),
                    },
                )
            }
            Message::Connected(slot) => {
                if let Some(payload) = slot.lock().take() {
                    let link_code = match &self.phase {
                        Phase::Connecting { link_code } => link_code.clone(),
                        _ => String::new(),
                    };
                    self.conn = Some(ConnectionHandles {
                        _dc: payload.dc,
                        _peer_conn: payload.peer_conn,
                    });
                    self.phase = Phase::Connected { link_code };
                }
                iced::Task::none()
            }
            Message::Failed(e) => {
                self.conn = None;
                self.phase = Phase::Failed { error: e };
                iced::Task::none()
            }
            Message::Disconnect => {
                self.conn = None;
                self.phase = Phase::Idle;
                iced::Task::none()
            }
        }
    }
}

/// Run the two-stage signaling handshake: open the websocket
/// (`connect()`), then drive the WebRTC ICE exchange (`Connecting`
/// future) until the data channel is open. Returns a `String` error
/// on failure so the message can stay `Clone`.
async fn run_connect(endpoint: String, link_code: String) -> Result<ConnectionPayload, String> {
    let connecting = tango_signaling::connect(
        &endpoint,
        &link_code,
        // `use_relay = None` lets the server pick — STUN by default,
        // TURN if the peers can't reach each other directly.
        None,
        PROTOCOL_VERSION,
    )
    .await
    .map_err(|e| format!("signaling: {e}"))?;
    let (dc, peer_conn) = connecting.await.map_err(|e| format!("webrtc: {e}"))?;
    Ok(ConnectionPayload { dc, peer_conn })
}

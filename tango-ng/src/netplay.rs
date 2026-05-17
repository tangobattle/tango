//! Netplay state + connection lifecycle.
//!
//! Round-1 scope wired `tango_signaling::connect` so Play with a link
//! code lights up the WebRTC data channel. Round 2 splits the channel
//! into Sender + Receiver and runs the Hello-exchange `negotiate()`
//! handshake from `crate::net` — after which the lifecycle lands in
//! the Lobby phase. Settings exchange + the live battle loop come in
//! subsequent rounds.
//!
//! Phase transitions:
//! `Idle → Connecting → Negotiating → Lobby` (or `→ Failed`).

use std::sync::Arc;

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
    /// Both peers agreed on the protocol. Ready for settings
    /// exchange (round 3) and then match start.
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

#[derive(Default)]
pub struct State {
    pub phase: Phase,
    /// Live connection objects, when post-Connecting. Cleared on
    /// Disconnect / Failed / on the next Connect.
    conn: Option<ConnectionHandles>,
}

/// Handles we hang onto for the duration of a connected session:
/// the Sender (locked behind a tokio Mutex because future rounds
/// will share it with the lobby ping loop + the battle loop), and
/// the peer-connection itself so the underlying RTC stays up.
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
    /// Tear down the active / pending connection.
    Disconnect,
    /// Internal: the signaling + WebRTC handshake resolved. We then
    /// kick off the protocol negotiate task before lifecycle moves
    /// out of Connecting.
    SignalingDone(Slot<ConnectionPayload>),
    /// Internal: protocol negotiate succeeded. Receiver is parked
    /// in the slot for the lobby / match loops to pick up later.
    NegotiationDone(Slot<NegotiationOutput>),
    /// Internal: any step (signaling, datachannel, negotiate) failed.
    Failed(String),
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

/// Output of the negotiate task — the post-handshake sender / receiver
/// (the lobby + match loops own them from here) and the peer-conn
/// handle they need to stay alive against.
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

    /// Apply a Message. Returns the iced Task to schedule for any
    /// async follow-up — currently the signaling handshake and the
    /// post-connect `negotiate()`.
    pub fn update(&mut self, msg: Message) -> iced::Task<Message> {
        match msg {
            Message::Connect { link_code, endpoint } => {
                self.conn = None;
                self.phase = Phase::Connecting {
                    link_code: link_code.clone(),
                };
                iced::Task::perform(
                    async move { run_connect(endpoint, link_code).await },
                    |result| match result {
                        Ok(payload) => Message::SignalingDone(slot(payload)),
                        Err(e) => Message::Failed(e),
                    },
                )
            }
            Message::SignalingDone(slot_rx) => {
                let link_code = match &self.phase {
                    Phase::Connecting { link_code } => link_code.clone(),
                    _ => return iced::Task::none(),
                };
                let Some(payload) = slot_rx.lock().take() else {
                    return iced::Task::none();
                };
                self.phase = Phase::Negotiating { link_code };
                // Split the channel + run protocol::negotiate on a
                // tokio task. Sender stays in the slot bundle so we
                // can drop it back into State on success.
                iced::Task::perform(
                    async move { run_negotiate(payload).await },
                    |result| match result {
                        Ok(out) => Message::NegotiationDone(slot(out)),
                        Err(e) => Message::Failed(e),
                    },
                )
            }
            Message::NegotiationDone(slot_rx) => {
                let link_code = match &self.phase {
                    Phase::Negotiating { link_code } => link_code.clone(),
                    _ => return iced::Task::none(),
                };
                let Some(out) = slot_rx.lock().take() else {
                    return iced::Task::none();
                };
                // Drop the receiver for now — the lobby + match
                // loops in later rounds will own it. Sender + peer
                // conn stay alive in self.conn so the channel
                // doesn't tear down.
                let NegotiationOutput { sender, peer_conn, receiver: _ } = out;
                self.conn = Some(ConnectionHandles {
                    sender,
                    _peer_conn: peer_conn,
                });
                self.phase = Phase::Lobby { link_code };
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

fn slot<T>(payload: T) -> Slot<T> {
    Arc::new(parking_lot::Mutex::new(Some(payload)))
}

/// Split the data channel into its sender + receiver halves and run
/// `protocol::negotiate` (Hello exchange) on top. Returns the post-
/// handshake sender/receiver bundled with the peer-conn handle.
async fn run_negotiate(payload: ConnectionPayload) -> Result<NegotiationOutput, String> {
    let ConnectionPayload { dc, peer_conn } = payload;
    let (dc_tx, dc_rx) = dc.split();
    let mut sender = crate::net::Sender::new(dc_tx);
    let mut receiver = crate::net::Receiver::new(dc_rx);
    crate::net::negotiate(&mut sender, &mut receiver)
        .await
        .map_err(|e| format!("negotiate: {e}"))?;
    Ok(NegotiationOutput {
        sender: Arc::new(tokio::sync::Mutex::new(sender)),
        receiver,
        peer_conn,
    })
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

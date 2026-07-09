//! Connection bring-up: the async tasks that take a Connect /
//! ConnectDirect from "user pressed the button" to an open,
//! version-negotiated pair of data channels, plus the `State`
//! handlers that sequence them (signaling hello → peer await →
//! negotiate → lobby-loop spawn).
//!
//! Copied from `tango/src/netplay/connect.rs`, transformed: handlers
//! return `()` and schedule the async stages via `State::perform`
//! (runtime-handle spawn + event-channel send) instead of returning
//! `iced::Task`s; the lobby loop feeds the main-loop event channel
//! directly, so the per-session futures bridge is gone. The async fn
//! bodies are verbatim.

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::lobby::run_lobby_loop;
use super::{
    map_async_err, slot, AsyncError, ConnectionHandles, ConnectionKind, DirectRole, LinkIdent, MatchmakingReconnect,
    Message, Phase, Slot, State, PROTOCOL_VERSION,
};

/// Intermediate hand-off between `run_signaling_connect` (server
/// hello arrived) and `run_await_peer` (WebRTC handshake done).
/// Wraps `tango_signaling::Connecting` so the Connecting future
/// can ferry through the Slot<T> dispatch.
pub struct SignalingHello {
    pub connecting: tango_signaling::Connecting,
}

impl std::fmt::Debug for SignalingHello {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SignalingHello { .. }")
    }
}

/// Output of the negotiate task — the post-handshake sender /
/// receiver (the lobby + match loops own them from here) and the
/// peer-conn handle they need to stay alive against.
pub struct NegotiationOutput {
    pub sender: Arc<tokio::sync::Mutex<crate::net::Sender>>,
    pub receiver: crate::net::Receiver,
    /// Unreliable in-match channel's send half. Idle until the match starts.
    pub in_match_sender: Arc<tokio::sync::Mutex<crate::net::data::Sender>>,
    /// Unreliable in-match channel's receive half. Parked for the PvP handoff
    /// the moment negotiate completes — nothing flows on it during the lobby,
    /// so unlike the reliable receiver it isn't owned by the lobby loop.
    pub in_match_receiver: crate::net::data::Receiver,
    /// The peer connection. Set by both transports. See
    /// [`ConnectionHandles::peer_conn`] for the lifetime contract.
    pub peer_conn: tango_rtc::PeerConnection,
    /// Pre-computed by the per-transport negotiator. Matchmaking reads
    /// the SDP type; the direct link sets host=true, connect=false.
    pub is_offerer: bool,
    /// The **direct**-link rebuild role, if this is the direct path; `None` on
    /// the matchmaking path, whose reconnect recipe is instead built in
    /// [`State::take_pre_match`] from params stashed at `Connect` plus the
    /// derived `session_id`. Either way the final [`ReconnectRecipe`] lands in
    /// [`PreMatchData::reconnect`].
    ///
    /// [`ReconnectRecipe`]: super::ReconnectRecipe
    /// [`PreMatchData::reconnect`]: super::PreMatchData
    pub reconnect: Option<DirectRole>,
    /// This connection's two DTLS certificate fingerprints, mixed into the
    /// matchmaking reconnect `session_id`. Empty on the direct path (its fabricated
    /// SDP carries no meaningful fingerprint, and it reconnects via `reconnect`).
    pub local_dtls_fingerprint: Vec<u8>,
    pub peer_dtls_fingerprint: Vec<u8>,
}

impl std::fmt::Debug for NegotiationOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NegotiationOutput { .. }")
    }
}

impl State {
    /// `Message::Connect` — start the matchmaking-server connect task.
    pub(super) fn connect(
        &mut self,
        link_code: String,
        endpoint: String,
        use_relay: Option<bool>,
        identity: Option<tango_signaling::ClientIdentity>,
    ) {
        self.cancel_and_renew();
        // Stash what a mid-match re-rendezvous needs (the session_id is derived
        // later from the shared RNG seed; see take_pre_match). Set *after*
        // cancel_and_renew, which clears it.
        self.matchmaking_reconnect = Some(MatchmakingReconnect {
            endpoint: endpoint.clone(),
            use_relay,
            identity: identity.clone(),
        });
        self.phase = Phase::Connecting {
            ident: LinkIdent::Matchmaking(link_code.clone()),
            waiting_for_opponent: false,
        };
        let cancel = self.cancel.clone();
        self.perform(
            run_signaling_connect(endpoint, link_code, use_relay, identity, cancel),
            map_signaling_hello_result,
        );
    }

    /// `Message::ConnectDirect` — start the signaling-free direct path.
    pub(super) fn connect_direct(&mut self, role: DirectRole) {
        self.cancel_and_renew();
        // Host = "waiting for inbound peer" (accept() is
        // the slow await); Connect = "actively dialing"
        // (mirrors the matchmaking-path semantics so the
        // existing waiting-screen UI reads correctly).
        let waiting_for_opponent = matches!(role, DirectRole::Host { .. });
        self.phase = Phase::Connecting {
            ident: LinkIdent::Direct(role.clone()),
            waiting_for_opponent,
        };
        let cancel = self.cancel.clone();
        self.perform(run_direct_rtc_negotiate(role, cancel), map_negotiate_result);
    }

    /// `Message::SignalingHelloReceived` — server hello arrived; flip to
    /// "waiting for opponent" and kick off the WebRTC await task.
    pub(super) fn on_signaling_hello(&mut self, slot_rx: Slot<SignalingHello>) {
        let ident = match &self.phase {
            Phase::Connecting { ident, .. } => ident.clone(),
            // Cancelled / superseded — late delivery, ignore.
            _ => return,
        };
        let Some(hello) = slot_rx.lock().unwrap().take() else {
            return;
        };
        self.phase = Phase::Connecting {
            ident,
            waiting_for_opponent: true,
        };
        let cancel = self.cancel.clone();
        self.perform(run_await_peer(hello, cancel), map_connect_result);
    }

    /// `Message::SignalingDone` — WebRTC handshake resolved; run the protocol
    /// negotiate task before lifecycle moves out of Connecting.
    pub(super) fn on_signaling_done(&mut self, slot_rx: Slot<crate::net::channel::Channels>) {
        let ident = match &self.phase {
            Phase::Connecting { ident, .. } => ident.clone(),
            // Cancelled / superseded — late delivery, ignore.
            _ => return,
        };
        let Some(channels) = slot_rx.lock().unwrap().take() else {
            return;
        };
        self.phase = Phase::Negotiating { ident };
        let cancel = self.cancel.clone();
        self.perform(run_negotiate(channels, cancel), map_negotiate_result);
    }

    /// `Message::NegotiationDone` — protocol handshake complete; install the
    /// connection handles, park the in-match receiver, and spawn the lobby loop.
    pub(super) fn on_negotiation_done(&mut self, slot_rx: Slot<NegotiationOutput>) {
        // Accept both `Connecting` (direct path: the bring-up
        // and negotiate are folded into one task and skip the
        // intermediate Negotiating phase) and `Negotiating`
        // (matchmaking path: signaling + peer-await + negotiate
        // are split stages). Either is a valid
        // Connect-or-Direct-Connect lifecycle that's progressed
        // past the wire handshake.
        let ident = match &self.phase {
            Phase::Negotiating { ident } => ident.clone(),
            Phase::Connecting { ident, .. } => ident.clone(),
            _ => return,
        };
        let Some(out) = slot_rx.lock().unwrap().take() else {
            return;
        };
        let sender = out.sender.clone();
        // Resolve how the transport actually flows for the
        // lobby's ping line. We read the selected ICE pair — a
        // `typ relay` candidate on either end means TURN. The
        // signaling-free direct path only ever forms host
        // candidate pairs, so it resolves to Direct.
        self.lobby.connection_kind = out.peer_conn.selected_candidate_pair().ok().map(|(local, remote)| {
            if local.contains("typ relay") || remote.contains("typ relay") {
                ConnectionKind::Relayed
            } else {
                ConnectionKind::Direct
            }
        });
        // Park the unreliable in-match receiver for the PvP handoff.
        // Nothing flows on it during the lobby (all lobby traffic is on
        // the reliable channel), so — unlike the reliable receiver — it
        // isn't owned by the lobby loop and can be stashed here right
        // away.
        *self.in_match_receiver_slot.lock().unwrap() = Some(out.in_match_receiver);
        self.conn = Some(ConnectionHandles {
            sender: out.sender,
            in_match_sender: out.in_match_sender,
            peer_conn: out.peer_conn,
            is_offerer: out.is_offerer,
            reconnect: out.reconnect,
            local_dtls_fingerprint: out.local_dtls_fingerprint,
            peer_dtls_fingerprint: out.peer_dtls_fingerprint,
        });
        // Spawn the lobby loop as a detached task on the runtime
        // handle. It owns the data-channel receiver and emits its
        // observations straight into the main-loop event channel.
        // The detached task exits only when the cancellation token
        // fires (a phase change can't abort it mid-await and lose
        // the receiver), and on exit it deposits the receiver into
        // the per-session post slot for the PvP handoff to take.
        let event_tx = self.event_tx.clone();
        let cancel = self.cancel.clone();
        let post = self.post_lobby_receiver.clone();
        let receiver = out.receiver;
        self.rt.spawn(async move {
            let receiver = run_lobby_loop(receiver, sender, event_tx, cancel).await;
            *post.lock().unwrap() = Some(receiver);
        });
        self.phase = Phase::Lobby { ident };
    }
}

fn map_signaling_hello_result(result: Result<SignalingHello, AsyncError>) -> Message {
    match result {
        Ok(hello) => Message::SignalingHelloReceived(slot(hello)),
        Err(e) => map_async_err(e),
    }
}

fn map_connect_result(result: Result<crate::net::channel::Channels, AsyncError>) -> Message {
    match result {
        Ok(channels) => Message::SignalingDone(slot(channels)),
        Err(e) => map_async_err(e),
    }
}

fn map_negotiate_result(result: Result<NegotiationOutput, AsyncError>) -> Message {
    match result {
        Ok(out) => Message::NegotiationDone(slot(out)),
        Err(e) => map_async_err(e),
    }
}

/// Stage 1 of the signaling handshake: WebSocket connect +
/// receive the server's Hello (ICE config). Returns the
/// `Connecting` handle to drive stage 2 on. The split lets the
/// UI distinguish "connecting to matchmaking server" from
/// "waiting for opponent" — stage 2's `await` is the slow one,
/// blocked on the peer actually joining.
async fn run_signaling_connect(
    endpoint: String,
    link_code: String,
    use_relay: Option<bool>,
    identity: Option<tango_signaling::ClientIdentity>,
    cancel: CancellationToken,
) -> Result<SignalingHello, AsyncError> {
    let work = async {
        let connecting = tango_signaling::connect(
            &endpoint,
            &link_code,
            // None = let ICE pick: direct when possible, TURN when
            // peers can't reach each other. Some(true) = relay-only
            // transport policy. Some(false) = drop the TURN servers,
            // direct routes only.
            use_relay,
            PROTOCOL_VERSION,
            // Every channel the session needs, created together up front (same
            // specs the direct path uses — see `net::channel`). Order matters:
            // `run_await_peer` maps the returned channels back by this order.
            vec![
                crate::net::channel::control_channel(),
                crate::net::channel::in_match_channel(),
            ],
            // The persistent self-signed identity (threaded from app state),
            // presented as the websocket's mTLS client certificate so the
            // server can log our fingerprint. `None` when it couldn't be
            // loaded — the dial still succeeds, just without a client cert
            // (see `crate::identity`).
            identity,
        )
        .await
        .map_err(|e| AsyncError::Failed(format!("signaling: {e}")))?;
        Ok::<_, AsyncError>(SignalingHello { connecting })
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(AsyncError::Cancelled),
        out = work => out,
    }
}

/// Stage 2: drive the `Connecting` future to completion — peer
/// joins + WebRTC ICE handshake opens the data channel.
async fn run_await_peer(
    hello: SignalingHello,
    cancel: CancellationToken,
) -> Result<crate::net::channel::Channels, AsyncError> {
    let work = async {
        let connected = hello
            .connecting
            .await
            .map_err(|e| AsyncError::Failed(format!("webrtc: {e}")))?;
        // Same split + pairing a mid-match reconnect uses, so both bundle a
        // matchmaking connection identically (see [`Channels::from_signaling`]).
        crate::net::channel::Channels::from_signaling(connected).map_err(|e| AsyncError::Failed(e.to_string()))
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(AsyncError::Cancelled),
        out = work => out,
    }
}

/// Direct signaling-free entry: bring up a peer connection both
/// sides configure locally from fixed ICE creds (host listens on a
/// pinned UDP port; connect dials it), then run the same
/// `protocol::negotiate` handshake the matchmaking WebRTC path
/// uses. No signaling server — see [`crate::net::direct_rtc`].
/// `is_offerer` is set from the role (host = true) so the
/// `pick_local_player_index` symmetry break still has a stable
/// asymmetric input.
async fn run_direct_rtc_negotiate(role: DirectRole, cancel: CancellationToken) -> Result<NegotiationOutput, AsyncError> {
    let is_offerer = matches!(role, DirectRole::Host { .. });
    // The role is also the rebuild recipe: a dropped direct link is
    // re-established by re-running this exact `host`/`connect`, so stash a
    // clone for the in-match reconnect coordinator before the match consumes it.
    let reconnect = Some(role.clone());
    let work = async {
        let channels = match role {
            DirectRole::Host { port } => crate::net::direct_rtc::host(port)
                .await
                .map_err(|e| AsyncError::Failed(format!("direct host: {e}")))?,
            DirectRole::Connect { addr } => crate::net::direct_rtc::connect(&addr)
                .await
                .map_err(|e| AsyncError::Failed(format!("direct connect: {e}")))?,
        };
        let crate::net::channel::Channels {
            control: (mut sender, mut receiver),
            in_match: (in_match_sender, in_match_receiver),
            peer_conn,
            // Empty on the direct path (fabricated SDP, fingerprint verification
            // off); it rebuilds via `reconnect`, not a derived session_id.
            local_dtls_fingerprint,
            peer_dtls_fingerprint,
        } = channels;
        // Handshake on the reliable channel; the unreliable in-match channel
        // shares the association and is open by the time the match starts.
        crate::net::negotiate(&mut sender, &mut receiver)
            .await
            .map_err(negotiation_error_sentinel)?;
        Ok::<_, AsyncError>(NegotiationOutput {
            sender: Arc::new(tokio::sync::Mutex::new(sender)),
            receiver,
            in_match_sender: Arc::new(tokio::sync::Mutex::new(in_match_sender)),
            in_match_receiver,
            peer_conn,
            is_offerer,
            reconnect,
            local_dtls_fingerprint,
            peer_dtls_fingerprint,
        })
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(AsyncError::Cancelled),
        out = work => out,
    }
}

/// Map `net::NegotiationError` to a sentinel string the UI can route to
/// a localized template. The three named variants get fixed-format
/// sentinels; the `Other` catch-all keeps the raw error text so a
/// transport-level failure is still surfaced (just unlocalized).
fn negotiation_error_sentinel(e: crate::net::NegotiationError) -> AsyncError {
    use crate::net::NegotiationError as N;
    AsyncError::Failed(match e {
        N::ExpectedHello => "negotiate-expected-hello".to_string(),
        N::RemoteProtocolVersionTooOld => "negotiate-version-too-old".to_string(),
        N::RemoteProtocolVersionTooNew => "negotiate-version-too-new".to_string(),
        N::Other(inner) => format!("negotiate-other: {inner}"),
    })
}

/// Run `protocol::negotiate` over the already-built channels. Aborts on
/// cancel.
async fn run_negotiate(
    channels: crate::net::channel::Channels,
    cancel: CancellationToken,
) -> Result<NegotiationOutput, AsyncError> {
    let crate::net::channel::Channels {
        control: (mut sender, mut receiver),
        in_match: (in_match_sender, in_match_receiver),
        peer_conn,
        local_dtls_fingerprint,
        peer_dtls_fingerprint,
    } = channels;
    // The channels were paired when the connection was bundled; the handshake
    // runs on the reliable channel.
    let work = crate::net::negotiate(&mut sender, &mut receiver);
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(AsyncError::Cancelled),
        result = work => {
            result.map_err(negotiation_error_sentinel)?;
            let is_offerer = peer_conn
                .local_description()
                .map(|d| matches!(d.sdp_type, tango_rtc::SdpType::Offer))
                .unwrap_or(false);
            Ok(NegotiationOutput {
                sender: Arc::new(tokio::sync::Mutex::new(sender)),
                receiver,
                in_match_sender: Arc::new(tokio::sync::Mutex::new(in_match_sender)),
                in_match_receiver,
                peer_conn,
                is_offerer,
                // Matchmaking can't be re-established without re-running
                // signaling against the server, so transparent reconnection is
                // off for this transport (for now).
                reconnect: None,
                local_dtls_fingerprint,
                peer_dtls_fingerprint,
            })
        }
    }
}

//! Connection bring-up: the async tasks that take a Connect /
//! ConnectDirect from "user pressed the button" to an open,
//! version-negotiated pair of data channels, plus the `State`
//! handlers that sequence them (signaling hello → peer await →
//! negotiate → lobby-loop spawn).

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
/// can ferry through iced's Slot<T> dispatch.
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
    /// Unreliable in-match channel's send half. Idle until the match starts,
    /// when it becomes the `Link`'s `InMatchTx` sink.
    pub in_match_sender: crate::net::data::Sender,
    /// Unreliable in-match channel's receive half. Parked for the PvP handoff
    /// the moment negotiate completes — nothing flows on it during the lobby,
    /// so unlike the reliable receiver it isn't owned by the lobby loop.
    pub in_match_receiver: crate::net::data::Receiver,
    /// The peer connection. Set by both transports. See
    /// [`ConnectionHandles::peer_conn`] for the lifetime contract.
    pub peer_conn: datachannel_wrapper::PeerConnection,
    /// Pre-computed by the per-transport negotiator. Matchmaking reads
    /// the SDP type; the direct link sets host=true, connect=false.
    pub is_offerer: bool,
    /// The **direct**-link rebuild role, if this is the direct path; `None` on
    /// the matchmaking path, whose reconnect recipe is instead built in
    /// [`State::take_pre_match`] from params stashed at `Connect` plus the
    /// derived `session_id`. Either way the final [`ReconnectRecipe`] lands in
    /// [`PreMatchData::reconnect`].
    pub reconnect: Option<DirectRole>,
    /// This connection's two DTLS certificate fingerprints, mixed into the
    /// matchmaking reconnect `session_id`. Empty on the direct path (its fabricated
    /// SDP carries no meaningful fingerprint, and it reconnects via `reconnect`).
    pub local_dtls_fingerprint: Vec<u8>,
    pub peer_dtls_fingerprint: Vec<u8>,
    /// The peer's persistent install identity: SHA-256 of the mTLS client
    /// certificate it presented on its signaling websocket, server-attested
    /// (see [`crate::net::channel::Channels::peer_client_cert_fingerprint`]).
    /// Empty on the direct path or when the peer presented none.
    pub peer_client_cert_fingerprint: Vec<u8>,
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
    ) -> iced::Task<Message> {
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
        iced::Task::perform(
            run_signaling_connect(endpoint, link_code, use_relay, identity, cancel),
            map_signaling_hello_result,
        )
    }

    /// `Message::ConnectDirect` — start the signaling-free direct path.
    pub(super) fn connect_direct(&mut self, role: DirectRole) -> iced::Task<Message> {
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
        iced::Task::perform(run_direct_rtc_negotiate(role, cancel), map_negotiate_result)
    }

    /// `Message::SignalingHelloReceived` — server hello arrived; flip to
    /// "waiting for opponent" and kick off the WebRTC await task.
    pub(super) fn on_signaling_hello(&mut self, slot_rx: Slot<SignalingHello>) -> iced::Task<Message> {
        let ident = match &self.phase {
            Phase::Connecting { ident, .. } => ident.clone(),
            // Cancelled / superseded — late delivery, ignore.
            _ => return iced::Task::none(),
        };
        let Some(hello) = slot_rx.lock().unwrap().take() else {
            return iced::Task::none();
        };
        self.phase = Phase::Connecting {
            ident,
            waiting_for_opponent: true,
        };
        let cancel = self.cancel.clone();
        iced::Task::perform(run_await_peer(hello, cancel), map_connect_result)
    }

    /// `Message::SignalingDone` — WebRTC handshake resolved; run the protocol
    /// negotiate task before lifecycle moves out of Connecting.
    pub(super) fn on_signaling_done(&mut self, slot_rx: Slot<crate::net::channel::Channels>) -> iced::Task<Message> {
        let ident = match &self.phase {
            Phase::Connecting { ident, .. } => ident.clone(),
            // Cancelled / superseded — late delivery, ignore.
            _ => return iced::Task::none(),
        };
        let Some(channels) = slot_rx.lock().unwrap().take() else {
            return iced::Task::none();
        };
        self.phase = Phase::Negotiating { ident };
        let cancel = self.cancel.clone();
        iced::Task::perform(run_negotiate(channels, cancel), map_negotiate_result)
    }

    /// `Message::NegotiationDone` — protocol handshake complete; install the
    /// connection handles, park the in-match receiver, and spawn the lobby loop.
    pub(super) fn on_negotiation_done(&mut self, slot_rx: Slot<NegotiationOutput>) -> iced::Task<Message> {
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
            _ => return iced::Task::none(),
        };
        let Some(out) = slot_rx.lock().unwrap().take() else {
            return iced::Task::none();
        };
        // The peer's install identity, as attested by the matchmaking server —
        // the counterpart of the "client identity loaded" line we log for our
        // own certificate at startup (see [`crate::netplay::identity`]).
        if !out.peer_client_cert_fingerprint.is_empty() {
            log::info!(
                "peer client identity (sha256 fingerprint: {})",
                super::identity::hex(&out.peer_client_cert_fingerprint)
            );
        }
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
        // Channel for the lobby loop to hand the reliable receiver back on
        // cancel-exit. One per session, so a dying loop from a previous
        // session can't deposit a stale receiver into the next one — its send
        // lands on a dropped rx and the receiver is dropped with it.
        let (post_lobby_tx, post_lobby_rx) = tokio::sync::oneshot::channel();
        self.conn = Some(ConnectionHandles {
            sender: out.sender,
            in_match_sender: out.in_match_sender,
            // The unreliable in-match receiver, parked for the PvP handoff.
            // Nothing flows on it during the lobby (all lobby traffic is on
            // the reliable channel), so — unlike the reliable receiver — it
            // isn't owned by the lobby loop and can be stashed here right
            // away.
            in_match_receiver: out.in_match_receiver,
            post_lobby_rx,
            peer_conn: out.peer_conn,
            is_offerer: out.is_offerer,
            reconnect: out.reconnect,
            local_dtls_fingerprint: out.local_dtls_fingerprint,
            peer_dtls_fingerprint: out.peer_dtls_fingerprint,
            peer_client_cert_fingerprint: out.peer_client_cert_fingerprint,
        });
        // Spawn the lobby loop as a detached tokio task.
        // It owns the data-channel receiver and bridges
        // its observations through `event_tx` to the iced
        // subscription. Decoupling the loop from the iced
        // subscription's future means an incidental
        // subscription drop (e.g. take_pre_match flipping
        // phase → Idle before the loop has noticed the
        // cancel) can no longer abort the loop mid-await
        // and lose the receiver.
        let (event_tx, event_rx) = futures::channel::mpsc::unbounded();
        *self.lobby_event_rx_slot.lock().unwrap() = Some(event_rx);
        let cancel = self.cancel.clone();
        let receiver = out.receiver;
        tokio::spawn(async move {
            let receiver = run_lobby_loop(receiver, sender, event_tx, cancel).await;
            let _ = post_lobby_tx.send(receiver);
        });
        self.phase = Phase::Lobby { ident };
        iced::Task::none()
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
            // (see `crate::netplay::identity`).
            identity,
        )
        .await
        .map_err(|e| AsyncError::Failed(super::Error::Other(format!("signaling: {e}"))))?;
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
            .map_err(|e| AsyncError::Failed(super::Error::Other(format!("webrtc: {e}"))))?;
        // Same split + pairing a mid-match reconnect uses, so both bundle a
        // matchmaking connection identically (see [`Channels::from_signaling`]).
        crate::net::channel::Channels::from_signaling(connected)
            .map_err(|e| AsyncError::Failed(super::Error::Other(e.to_string())))
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(AsyncError::Cancelled),
        out = work => out,
    }
}

/// Direct signaling-free entry: bring up a libdatachannel peer
/// connection whose SDP both sides fabricate from fixed ICE creds
/// (host listens on a pinned UDP port; connect dials it), then run
/// the same `protocol::negotiate` handshake the matchmaking WebRTC
/// path uses. No signaling server — see [`crate::net::direct_rtc`].
/// `is_offerer` is set from the role (host = true) so the
/// `pick_local_player_index` symmetry break still has a stable
/// asymmetric input.
async fn run_direct_rtc_negotiate(
    role: DirectRole,
    cancel: CancellationToken,
) -> Result<NegotiationOutput, AsyncError> {
    let is_offerer = matches!(role, DirectRole::Host { .. });
    // The role is also the rebuild recipe: a dropped direct link is
    // re-established by re-running this exact `host`/`connect`, so stash a
    // clone for the in-match reconnect coordinator before the match consumes it.
    let reconnect = Some(role.clone());
    let work = async {
        let channels = match role {
            DirectRole::Host { port } => crate::net::direct_rtc::host(port)
                .await
                .map_err(|e| AsyncError::Failed(super::Error::Other(format!("direct host: {e}"))))?,
            DirectRole::Connect { addr } => crate::net::direct_rtc::connect(&addr)
                .await
                .map_err(|e| AsyncError::Failed(super::Error::Other(format!("direct connect: {e}"))))?,
        };
        let crate::net::channel::Channels {
            control: (mut sender, mut receiver),
            in_match: (in_match_sender, in_match_receiver),
            peer_conn,
            // Empty on the direct path (fabricated SDP, fingerprint verification
            // off); it rebuilds via `reconnect`, not a derived session_id.
            local_dtls_fingerprint,
            peer_dtls_fingerprint,
            // Also empty: there's no signaling server to attest an identity.
            peer_client_cert_fingerprint,
        } = channels;
        // Handshake on the reliable channel; the unreliable in-match channel
        // shares the association and is open by the time the match starts.
        crate::net::negotiate(&mut sender, &mut receiver)
            .await
            .map_err(negotiation_error)?;
        Ok::<_, AsyncError>(NegotiationOutput {
            sender: Arc::new(tokio::sync::Mutex::new(sender)),
            receiver,
            in_match_sender,
            in_match_receiver,
            peer_conn,
            is_offerer,
            reconnect,
            local_dtls_fingerprint,
            peer_dtls_fingerprint,
            peer_client_cert_fingerprint,
        })
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(AsyncError::Cancelled),
        out = work => out,
    }
}

/// Map `net::NegotiationError` to the typed netplay [`super::Error`]
/// the UI routes to a localized template. The three named variants get
/// dedicated variants; the `Other` catch-all keeps the raw error text
/// so a transport-level failure is still surfaced (just unlocalized).
fn negotiation_error(e: crate::net::NegotiationError) -> AsyncError {
    use crate::net::NegotiationError as N;
    AsyncError::Failed(match e {
        N::ExpectedHello => super::Error::NegotiateExpectedHello,
        N::RemoteProtocolVersionTooOld => super::Error::NegotiateVersionTooOld,
        N::RemoteProtocolVersionTooNew => super::Error::NegotiateVersionTooNew,
        N::Other(inner) => super::Error::Negotiate(inner.to_string()),
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
        peer_client_cert_fingerprint,
    } = channels;
    // The channels were paired when the connection was bundled; the handshake
    // runs on the reliable channel.
    let work = crate::net::negotiate(&mut sender, &mut receiver);
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(AsyncError::Cancelled),
        result = work => {
            result.map_err(negotiation_error)?;
            let is_offerer = peer_conn
                .local_description()
                .map(|d| matches!(d.sdp_type, datachannel_wrapper::SdpType::Offer))
                .unwrap_or(false);
            Ok(NegotiationOutput {
                sender: Arc::new(tokio::sync::Mutex::new(sender)),
                receiver,
                in_match_sender,
                in_match_receiver,
                peer_conn,
                is_offerer,
                // Matchmaking can't be re-established without re-running
                // signaling against the server, so transparent reconnection is
                // off for this transport (for now).
                reconnect: None,
                local_dtls_fingerprint,
                peer_dtls_fingerprint,
                peer_client_cert_fingerprint,
            })
        }
    }
}

//! Connection bring-up: getting from "user pressed the button" to an
//! open, version-negotiated pair of data channels.
//!
//! Both transports are one linear `async fn`. The intermediate states a
//! caller can see — "talking to the matchmaking server", "waiting for the
//! opponent", "negotiating" — are reported through [`Progress`] as they
//! happen; the values in between (the websocket, the channels) never
//! leave the function's locals.

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::{DirectRole, Error, Inbound, MatchmakingParams, Progress, Status, PROTOCOL_VERSION};

/// An open, negotiated connection: everything the lobby needs to talk to
/// the peer, and everything the eventual match needs to take it over.
pub struct Connected {
    pub(crate) sender: Arc<tokio::sync::Mutex<tango_session::net::Sender>>,
    pub(crate) receiver: tango_session::net::Receiver,
    /// Unreliable in-match channel's send half. Idle until the match starts,
    /// when it becomes the `Link`'s `InMatchTx` sink.
    pub(crate) in_match_sender: tango_session::net::data::Sender,
    /// Unreliable in-match channel's receive half. Parked for the PvP handoff
    /// the moment negotiate completes — nothing flows on it during the lobby,
    /// so unlike the reliable receiver it isn't owned by the lobby pump.
    pub(crate) in_match_receiver: tango_session::net::data::Receiver,
    /// The peer connection. Set by both transports; kept alive for the
    /// duration of the session.
    pub(crate) peer_conn: datachannel_wrapper::PeerConnection,
    /// `true` iff we're the "offer side" for symmetry-breaking purposes —
    /// i.e. we wrote the SDP offer on the matchmaking path, or we're the
    /// host on the direct link. Drives `pick_local_player_index`.
    pub(crate) is_offerer: bool,
    /// The **direct**-link rebuild role, if this is the direct path; `None`
    /// on the matchmaking path, whose reconnect recipe is instead built in
    /// [`State::take_pre_match`](crate::State::take_pre_match) from the
    /// params stashed at connect time plus the derived `session_id`.
    pub(crate) reconnect: Option<DirectRole>,
    /// This connection's two DTLS certificate fingerprints, mixed into the
    /// matchmaking reconnect `session_id`. Empty on the direct path (its
    /// fabricated SDP carries no meaningful fingerprint, and it reconnects
    /// via `reconnect`).
    pub(crate) local_dtls_fingerprint: Vec<u8>,
    pub(crate) peer_dtls_fingerprint: Vec<u8>,
}

impl std::fmt::Debug for Connected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Connected { .. }")
    }
}

/// Dial the matchmaking server, wait for the opponent, and negotiate the
/// protocol version. Reports every outcome — including its own failure —
/// through `progress`, so a host just spawns this and pumps
/// [`State::apply`](crate::State::apply); there is no result to route.
///
/// Cancel by firing the token that
/// [`State::begin_matchmaking`](crate::State::begin_matchmaking) handed
/// back: each await races it, and a cancelled attempt reports nothing the
/// state machine will act on.
pub async fn connect(params: MatchmakingParams, cancel: CancellationToken, progress: Progress) {
    let reporter = progress.clone();
    let work = async {
        let connecting = tango_signaling::connect(
            &params.endpoint,
            &params.link_code,
            // None = let ICE pick: direct when possible, TURN when
            // peers can't reach each other. Some(true) = relay-only
            // transport policy. Some(false) = drop the TURN servers,
            // direct routes only.
            params.use_relay,
            PROTOCOL_VERSION,
            // Every channel the session needs, created together up front (same
            // specs the direct path uses — see `net::channel`).
            vec![
                tango_session::net::channel::control_channel(),
                tango_session::net::channel::in_match_channel(),
            ],
        )
        .await
        .map_err(signaling_error)?;
        // Server hello is in hand (ICE config settled). From here the
        // await is the slow one — blocked on the peer actually joining.
        // It fails with the same type as the dial did: a server that
        // turns us away does it here, having taken the socket first.
        progress.status(Status::WaitingForOpponent);
        let connected = connecting.await.map_err(signaling_error)?;
        // Same split + pairing a mid-match reconnect uses, so both bundle a
        // matchmaking connection identically (see [`Channels::from_signaling`]).
        let channels = tango_session::net::channel::Channels::from_signaling(connected)
            .map_err(|e| Error::Other(e.to_string()))?;
        progress.status(Status::Negotiating);
        negotiate(channels, None).await
    };
    report(work, cancel, reporter).await;
}

/// Direct signaling-free entry: bring up a libdatachannel peer connection
/// whose SDP both sides fabricate from fixed ICE creds (host listens on a
/// pinned UDP port; connect dials it), then run the same negotiate
/// handshake the matchmaking path uses. See
/// [`tango_session::net::direct_rtc`].
///
/// Native-only — a browser has no UDP socket of its own to pin.
#[cfg(not(target_arch = "wasm32"))]
pub async fn connect_direct(role: DirectRole, cancel: CancellationToken, progress: Progress) {
    let reporter = progress.clone();
    // The role is also the rebuild recipe: a dropped direct link is
    // re-established by re-running this exact `host`/`connect`, so stash a
    // clone for the in-match reconnect coordinator before it's consumed.
    let reconnect = Some(role.clone());
    let work = async {
        let channels = match role {
            DirectRole::Host { port } => tango_session::net::direct_rtc::host(port)
                .await
                .map_err(|e| Error::Other(format!("direct host: {e}")))?,
            DirectRole::Connect { addr } => tango_session::net::direct_rtc::connect(&addr)
                .await
                .map_err(|e| Error::Other(format!("direct connect: {e}")))?,
        };
        progress.status(Status::Negotiating);
        negotiate(channels, reconnect).await
    };
    report(work, cancel, reporter).await;
}

/// Run the protocol-version handshake on the reliable channel and bundle
/// the result. `is_offerer` comes from the SDP on the matchmaking path;
/// on the direct path the role decides it (host = true), which is what
/// keeps `pick_local_player_index`'s symmetry break asymmetric.
async fn negotiate(
    channels: tango_session::net::channel::Channels,
    reconnect: Option<DirectRole>,
) -> Result<Connected, Error> {
    let tango_session::net::channel::Channels {
        control: (mut sender, mut receiver),
        in_match: (in_match_sender, in_match_receiver),
        peer_conn,
        local_dtls_fingerprint,
        peer_dtls_fingerprint,
    } = channels;
    // The channels were paired when the connection was bundled; the
    // handshake runs on the reliable one. The unreliable in-match channel
    // shares the association and is open by the time the match starts.
    tango_session::net::negotiate(&mut sender, &mut receiver)
        .await
        .map_err(negotiation_error)?;
    let is_offerer = match &reconnect {
        Some(role) => matches!(role, DirectRole::Host { .. }),
        None => peer_conn
            .local_description()
            .map(|d| matches!(d.sdp_type, datachannel_wrapper::SdpType::Offer))
            .unwrap_or(false),
    };
    Ok(Connected {
        sender: Arc::new(tokio::sync::Mutex::new(sender)),
        receiver,
        in_match_sender,
        in_match_receiver,
        peer_conn,
        is_offerer,
        // Matchmaking can't be re-established without re-running signaling
        // against the server, so transparent reconnection is off for that
        // transport (for now) — `reconnect` is `None` there by construction.
        reconnect,
        local_dtls_fingerprint,
        peer_dtls_fingerprint,
    })
}

/// Race a bring-up against the cancel token and report where it landed.
/// A cancelled attempt reports nothing: whoever fired the token has
/// already moved the phase on, and a late `Failed` would overwrite it.
async fn report(
    work: impl std::future::Future<Output = Result<Connected, Error>>,
    cancel: CancellationToken,
    progress: Progress,
) {
    let outcome = tokio::select! {
        biased;
        _ = cancel.cancelled() => return,
        out = work => out,
    };
    match outcome {
        Ok(connected) => progress.send(Inbound::Connected(Box::new(connected))),
        Err(e) => progress.send(Inbound::Failed(e)),
    }
}

/// Map `tango_signaling::Error` to the typed netplay [`Error`] the UI
/// routes to a localized template. Both halves of a matchmaking dial —
/// the connect and the wait for the peer — fail with this one type, so
/// both go through here rather than each flattening it into a string
/// under its own prefix. The two reasons a player can act on (their
/// Tango is too old for the server, or newer than it) get dedicated
/// variants; the rest keep their text so a failure is still surfaced,
/// just unlocalized.
fn signaling_error(e: tango_signaling::Error) -> Error {
    use tango_signaling::AbortReason as R;
    use tango_signaling::Error as S;
    match e {
        S::ServerAbort(R::ProtocolVersionTooOld) => Error::SignalingVersionTooOld,
        S::ServerAbort(R::ProtocolVersionTooNew) => Error::SignalingVersionTooNew,
        // `as_str_name` rather than `Debug`: it's the name in the proto,
        // which is what a bug report about one of these wants to quote.
        S::ServerAbort(reason) => Error::SignalingRejected(reason.as_str_name().to_owned()),
        // Never got a usable socket to the server. Tungstenite's Display
        // is the readable one; the variant's own is `{0:?}`.
        S::Websocket(inner) => Error::SignalingUnreachable(inner.to_string()),
        S::Io(inner) => Error::SignalingUnreachable(inner.to_string()),
        e @ (S::PeerConnectionDisconnected | S::PeerConnectionFailed | S::PeerConnectionClosed) => {
            Error::PeerConnection(e.to_string())
        }
        e => Error::Signaling(e.to_string()),
    }
}

/// Map `net::NegotiationError` to the typed netplay [`Error`] the UI
/// routes to a localized template. The three named variants get dedicated
/// variants; the `Other` catch-all keeps the raw error text so a
/// transport-level failure is still surfaced (just unlocalized).
fn negotiation_error(e: tango_session::net::NegotiationError) -> Error {
    use tango_session::net::NegotiationError as N;
    match e {
        N::ExpectedHello => Error::NegotiateExpectedHello,
        N::RemoteProtocolVersionTooOld => Error::NegotiateVersionTooOld,
        N::RemoteProtocolVersionTooNew => Error::NegotiateVersionTooNew,
        N::Other(inner) => Error::Negotiate(inner.to_string()),
    }
}

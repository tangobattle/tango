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

use super::{DirectRole, Error, Inbound, MatchmakingParams, Progress};

/// An open, negotiated connection: everything the lobby needs to talk to
/// the peer, and everything the eventual match needs to take it over.
pub struct Connected {
    pub(crate) sender: Arc<tokio::sync::Mutex<tango_net::Sender>>,
    pub(crate) receiver: tango_net::Receiver,
    /// Unreliable in-match channel's send half. Idle until the match starts,
    /// when it becomes the `Link`'s `InMatchTx` sink.
    pub(crate) in_match_sender: tango_net::data::Sender,
    /// Unreliable in-match channel's receive half. Parked for the PvP handoff
    /// the moment negotiate completes — nothing flows on it during the lobby,
    /// so unlike the reliable receiver it isn't owned by the lobby pump.
    pub(crate) in_match_receiver: tango_net::data::Receiver,
    /// The peer connection. Set by both transports; kept alive for the
    /// duration of the session.
    pub(crate) peer_conn: tango_net::channel::PeerConnection,
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
        // The same bring-up a mid-match reconnect runs, so both bundle a
        // matchmaking connection identically.
        let route = tango_net::connect::Route::Matchmaking {
            endpoint: &params.endpoint,
            session_id: &params.link_code,
            use_relay: params.use_relay,
        };
        let channels = tango_net::open_channels(route, |status| progress.status(status)).await?;
        Ok(bundle(channels, None))
    };
    report(work, cancel, reporter).await;
}

/// Direct signaling-free entry: bring up a libdatachannel peer connection
/// whose SDP both sides fabricate from fixed ICE creds (host listens on a
/// pinned UDP port; connect dials it), then run the same negotiate
/// handshake the matchmaking path uses. See
/// [`tango_net::direct_rtc`].
///
/// Native-only — a browser has no UDP socket of its own to pin.
#[cfg(not(target_arch = "wasm32"))]
pub async fn connect_direct(role: DirectRole, cancel: CancellationToken, progress: Progress) {
    let reporter = progress.clone();
    let work = async {
        let channels = tango_net::open_channels(tango_net::connect::Route::Direct(&role), |status| {
            progress.status(status)
        })
        .await?;
        // The role is also the rebuild recipe: a dropped direct link is
        // re-established by re-running this exact `host`/`connect`.
        Ok(bundle(channels, Some(role.clone())))
    };
    report(work, cancel, reporter).await;
}

/// Bundle a negotiated connection for the lobby.
fn bundle(channels: tango_net::channel::Channels, reconnect: Option<DirectRole>) -> Connected {
    let tango_net::channel::Channels {
        control: (sender, receiver),
        in_match: (in_match_sender, in_match_receiver),
        peer_conn,
        is_offerer,
        local_dtls_fingerprint,
        peer_dtls_fingerprint,
    } = channels;
    Connected {
        sender: Arc::new(tokio::sync::Mutex::new(sender)),
        receiver,
        in_match_sender,
        in_match_receiver,
        peer_conn,
        is_offerer,
        // Matchmaking can't be re-established without re-running signaling
        // against the server, so its recipe is built later, in
        // `take_pre_match` — `reconnect` is `None` there by construction.
        reconnect,
        local_dtls_fingerprint,
        peer_dtls_fingerprint,
    }
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

impl From<tango_net::ConnectError> for Error {
    fn from(e: tango_net::ConnectError) -> Self {
        use tango_net::ConnectError as C;
        match e {
            C::Signaling(e) => signaling_error(e),
            C::Channels(e) => Error::Channels(e.to_string()),
            C::Direct { hosting, source } => Error::Direct {
                hosting,
                message: source.to_string(),
            },
            // Unreachable from a lobby: the direct entry point isn't built
            // for a browser.
            e @ C::DirectUnsupported => Error::Direct {
                hosting: false,
                message: e.to_string(),
            },
            C::Negotiation(e) => negotiation_error(e),
        }
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
fn negotiation_error(e: tango_net::NegotiationError) -> Error {
    use tango_net::NegotiationError as N;
    match e {
        N::ExpectedHello => Error::NegotiateExpectedHello,
        N::RemoteProtocolVersionTooOld => Error::NegotiateVersionTooOld,
        N::RemoteProtocolVersionTooNew => Error::NegotiateVersionTooNew,
        N::Other(inner) => Error::Negotiate(inner.to_string()),
    }
}

//! Opening a negotiated pair of channels to a peer — the one bring-up
//! sequence shared by a lobby's first connect and a match's mid-match
//! rebuild: the transport (signaling rendezvous, or the signaling-free
//! direct link), the [`Channels`] bundle, then the version
//! [`negotiate`](super::negotiate) on the reliable channel.

use super::channel::Channels;
use super::link::{DirectRole, ReconnectRecipe};
use super::NegotiationError;

/// Where to find the peer. Borrowed, so a lobby dial and a
/// [`ReconnectRecipe`] replay can both describe themselves without
/// cloning.
#[derive(Debug, Clone, Copy)]
pub enum Route<'a> {
    /// Rendezvous on the matchmaking server under `session_id` (the
    /// user's link code on a first connect).
    Matchmaking {
        endpoint: &'a str,
        session_id: &'a str,
        /// `None` = let ICE pick, `Some(true)` = relay only,
        /// `Some(false)` = never relay.
        use_relay: Option<bool>,
    },
    /// Signaling-free direct link. Native only: a browser has no UDP
    /// socket of its own, so opening one there fails with
    /// [`ConnectError::DirectUnsupported`].
    Direct(&'a DirectRole),
}

impl ReconnectRecipe {
    /// The route that re-establishes this link.
    pub fn route(&self) -> Route<'_> {
        match self {
            ReconnectRecipe::Direct(role) => Route::Direct(role),
            ReconnectRecipe::Matchmaking {
                endpoint,
                session_id,
                use_relay,
            } => Route::Matchmaking {
                endpoint,
                session_id,
                use_relay: *use_relay,
            },
        }
    }
}

/// How far [`open_channels`] has got, reported as it passes each
/// milestone.
#[derive(Debug, Clone, Copy)]
pub enum Status {
    /// Peer is up; running the protocol-version handshake.
    Negotiating,
    /// Matchmaking server accepted us; blocked on the opponent joining.
    WaitingForOpponent,
}

/// Why [`open_channels`] failed, by stage.
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    /// The matchmaking rendezvous failed: the server, or the WebRTC
    /// handshake it brokers.
    #[error("signaling: {0}")]
    Signaling(#[source] tango_signaling::Error),
    /// The rendezvous succeeded but its channels couldn't be bundled.
    #[error("{0}")]
    Channels(#[source] std::io::Error),
    /// The signaling-free direct link couldn't be hosted or dialed.
    #[error("direct {}: {source}", if *.hosting { "host" } else { "connect" })]
    Direct {
        hosting: bool,
        #[source]
        source: std::io::Error,
    },
    /// A browser can't open a direct link.
    #[error("a direct link can't be opened in a browser")]
    DirectUnsupported,
    /// The version handshake failed.
    #[error("negotiate: {0}")]
    Negotiation(#[from] NegotiationError),
}

/// Bring a transport up along `route`, bundle its channels, and run the
/// version handshake on the reliable one. `on_status` hears each
/// milestone as it passes; on the direct path there is no
/// [`Status::WaitingForOpponent`].
pub async fn open_channels(route: Route<'_>, mut on_status: impl FnMut(Status)) -> Result<Channels, ConnectError> {
    let mut channels = match route {
        Route::Matchmaking {
            endpoint,
            session_id,
            use_relay,
        } => {
            let connecting = tango_signaling::connect(
                endpoint,
                session_id,
                use_relay,
                tango_net_protocol::PROTOCOL_VERSION,
                // Every channel the session needs, created together up
                // front (the same specs the direct path uses).
                vec![super::channel::control_channel(), super::channel::in_match_channel()],
            )
            .await
            .map_err(ConnectError::Signaling)?;
            // Server hello is in hand (ICE config settled). From here the
            // await is the slow one — blocked on the peer joining. It fails
            // with the same type as the dial did: a server that turns us
            // away does it here, having taken the socket first.
            on_status(Status::WaitingForOpponent);
            let connected = connecting.await.map_err(ConnectError::Signaling)?;
            Channels::from_signaling(connected).map_err(ConnectError::Channels)?
        }
        #[cfg(not(target_arch = "wasm32"))]
        Route::Direct(DirectRole::Host { port }) => super::direct_rtc::host(*port)
            .await
            .map_err(|source| ConnectError::Direct { hosting: true, source })?,
        #[cfg(not(target_arch = "wasm32"))]
        Route::Direct(DirectRole::Connect { addr }) => super::direct_rtc::connect(addr)
            .await
            .map_err(|source| ConnectError::Direct { hosting: false, source })?,
        #[cfg(target_arch = "wasm32")]
        Route::Direct(_) => return Err(ConnectError::DirectUnsupported),
    };
    on_status(Status::Negotiating);
    // The channels were paired when the connection was bundled; the
    // handshake runs on the reliable one. The unreliable in-match channel
    // shares the association and is open by the time the match starts.
    super::negotiate(&mut channels.control.0, &mut channels.control.1).await?;
    Ok(channels)
}

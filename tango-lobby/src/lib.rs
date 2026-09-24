//! Netplay state + connection lifecycle: the connection choreography,
//! settings exchange, ready handshake and match handoff, sitting atop
//! [`tango_net`], which owns the wire protocols and channel
//! mechanics.
//!
//! **The shape.** Bringing a connection up is one linear `async fn`
//! ([`connect`] / [`connect_direct`]); once it's up, the lobby is a
//! synchronous state machine ([`State`]) that a host drives by calling
//! methods on it. Neither half asks the host for an architecture: a host
//! spawns the connect future however its runtime spawns futures, and
//! pumps [`State::apply`] with whatever comes down the progress channel.
//!
//! **Phases.** `Idle → Connecting → Negotiating → Lobby` (any → `Failed`
//! on error; any → `Idle` on [`State::disconnect`]). A live
//! [`CancellationToken`] kept on the State aborts the in-flight work when
//! the user disconnects or starts over — without it the orphaned future
//! would keep racing the new one and clobber state when it resolved. The
//! progress channel is per-attempt too, so a dying attempt's reports land
//! on a dropped receiver instead of the live session.

// In a browser the things these `Arc`s hold — the core, the transport —
// are genuinely not `Send`, because the browser's own handles aren't.
// The alternative is cfg-splitting `Arc`/`Rc` through every shared type
// for no gain: wasm is single-threaded, so the atomics cost nothing real.
#![cfg_attr(target_arch = "wasm32", allow(clippy::arc_with_non_send_sync))]

pub mod compat;
pub mod randomcode;

mod connect;
mod handshake;
mod lobby;
mod reconcile;
mod state;

pub use connect::connect;
#[cfg(not(target_arch = "wasm32"))]
pub use connect::connect_direct;
use connect::Connected;
pub use tango_net::link::DirectRole;
use tango_net::link::{LinkParts, ReconnectRecipe};

pub use handshake::ReadyView;
use lobby::Command;
pub use state::{ConnectionKind, HandoffTicket, LobbyState, State};

/// Why a netplay session failed — typed so the UI can route each
/// failure mode to its own localized copy instead of string-matching
/// sentinel values out of a flat string. The variants without their own
/// copy display as the raw text the generic "Connection failed:"
/// template embeds.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// Peer closed the connection cleanly (left the lobby / quit).
    #[error("peer disconnected")]
    PeerDisconnected,
    /// The matchmaking server won't matchmake for a protocol as old as
    /// ours — this Tango needs updating before it can play online.
    /// Distinct from [`Error::NegotiateVersionTooOld`], which is about
    /// the *peer's* version: here there is no peer yet.
    #[error("matchmaking server protocol is newer than ours")]
    SignalingVersionTooOld,
    /// The matchmaking server is older than this Tango.
    #[error("matchmaking server protocol is older than ours")]
    SignalingVersionTooNew,
    /// The server turned us away for some other reason, carrying the
    /// name of the `Abort` reason it gave. The remaining reasons (no
    /// session id, not an upgrade) mean a broken client rather than
    /// anything a player can act on, so they share one variant.
    #[error("matchmaking server rejected us: {0}")]
    SignalingRejected(String),
    /// Never reached the matchmaking server at all: offline, bad
    /// endpoint, DNS, TLS.
    #[error("matchmaking server unreachable: {0}")]
    SignalingUnreachable(String),
    /// Signaling failed some other way — a malformed or unexpected
    /// packet from a server we did reach.
    #[error("signaling: {0}")]
    Signaling(String),
    /// Signaling worked and the peer connection didn't: the WebRTC
    /// connection never came up, or dropped during the SDP exchange.
    #[error("peer connection: {0}")]
    PeerConnection(String),
    /// The rendezvous succeeded but its data channels couldn't be
    /// bundled.
    #[error("{0}")]
    Channels(String),
    /// The signaling-free direct link couldn't be hosted or dialed.
    #[error("direct {}: {message}", if *.hosting { "host" } else { "connect" })]
    Direct { hosting: bool, message: String },
    /// Version negotiate: the first packet wasn't a Hello.
    #[error("peer didn't say hello")]
    NegotiateExpectedHello,
    /// Peer speaks an older protocol version than ours.
    #[error("peer protocol version too old")]
    NegotiateVersionTooOld,
    /// Peer speaks a newer protocol version than ours.
    #[error("peer protocol version too new")]
    NegotiateVersionTooNew,
    /// Negotiate failed below the version check (transport error).
    #[error("negotiate: {0}")]
    Negotiate(String),
    /// The lobby's control channel failed mid-lobby; `operation` names
    /// what was being sent or received.
    #[error("{operation}: {message}")]
    Transport { operation: &'static str, message: String },
    /// Our own reveal couldn't be encoded; `stage` names the step.
    #[error("{stage}: {message}")]
    EncodeState { stage: &'static str, message: String },
    /// The peer's reveal didn't match the commitment it sent.
    #[error("peer commitment mismatch")]
    CommitmentMismatch,
    /// The peer's reveal matched its commitment but won't decode;
    /// `stage` names the step.
    #[error("{stage}: {message}")]
    DecodePeerState { stage: &'static str, message: String },
    /// The peer announced a second reveal length within one reveal.
    #[error("peer sent a second ChunkStart within one reveal")]
    DuplicateChunkStart,
    /// The peer sent more reveal bytes than it announced.
    #[error("peer sent more reveal bytes than announced ({got} > {want})")]
    RevealOverrun { got: u64, want: u64 },
    /// The host couldn't build the live match after the handoff (ROM,
    /// patch, or core construction). Carries the host's own report.
    #[error("{0}")]
    SessionBuild(String),
}

/// Where the lifecycle is right now. Drives the Play tab's status
/// bar + the Cancel button's visibility.
#[derive(Clone, Debug, Default)]
pub enum Phase {
    /// No connection attempt in flight.
    #[default]
    Idle,
    /// Bring-up in flight. `waiting_for_opponent` flips true once the
    /// matchmaking server's Hello arrives; up to that point we're still
    /// negotiating with the server, after we're blocked on the peer
    /// joining + the WebRTC handshake.
    Connecting {
        ident: LinkIdent,
        waiting_for_opponent: bool,
    },
    /// Data channel up; exchanging Hello packets / verifying both
    /// peers speak the same `PROTOCOL_VERSION`.
    Negotiating { ident: LinkIdent },
    /// Both peers agreed on the protocol. The lobby pump is running in
    /// the background; settings exchange + match start come next.
    Lobby { ident: LinkIdent },
    /// Last attempt failed. Stays here until the user starts a new
    /// connection or clears the field.
    Failed { error: Error },
}

/// How far a connection attempt has got. Reported by the connect futures
/// as they pass each milestone; folded into [`Phase`] by [`State::apply`].
pub use tango_net::connect::Status;

/// Structured identifier for the current connection. Kept in
/// `Phase` across the lifecycle, and also the payload of the
/// play-tab's connect action, so consumers (UI header, status
/// line, Discord rich presence, replay filenames) can render or
/// dispatch on the actual structure rather than re-parsing a
/// flat string. Matchmaking carries the raw user-supplied code;
/// `Direct` carries the parsed `DirectRole` describing whether
/// we host or dial.
#[derive(Debug, Clone)]
pub enum LinkIdent {
    Matchmaking(String),
    Direct(DirectRole),
}

impl LinkIdent {
    /// Resolve trimmed link-code input into something to dial: a
    /// `/`-prefixed direct command (see [`DirectRole::parse_command`]) or
    /// a matchmaking code. `None` when the input isn't submittable: empty,
    /// or a malformed direct command.
    pub fn parse(input: &str) -> Option<Self> {
        if input.is_empty() {
            None
        } else if input.starts_with('/') {
            DirectRole::parse_command(input).map(LinkIdent::Direct)
        } else {
            Some(LinkIdent::Matchmaking(input.to_string()))
        }
    }

    /// The code another player can dial to reach the same rendezvous.
    /// Only matchmaking codes have one: a direct command names this
    /// machine's own role and wouldn't reach anyone else. Hosts show it
    /// and offer it as a Discord join secret.
    pub fn matchmaking_code(&self) -> Option<&str> {
        match self {
            LinkIdent::Matchmaking(code) => Some(code.as_str()),
            LinkIdent::Direct(_) => None,
        }
    }
}

/// What a matchmaking dial needs. Also stashed for the duration of the
/// session: a mid-match re-rendezvous replays these params against a
/// `session_id` derived later from the shared RNG seed (see
/// [`State::take_pre_match`]).
#[derive(Clone)]
pub struct MatchmakingParams {
    pub link_code: String,
    pub endpoint: String,
    /// `None` = auto (ICE picks), `Some(true)` = relay only,
    /// `Some(false)` = never relay.
    pub use_relay: Option<bool>,
}

/// Something the connection reported. Opaque on purpose: a host's only
/// job is to move these from the progress channel into [`State::apply`],
/// which is where the meaning lives.
pub struct Incoming(Inbound);

impl std::fmt::Debug for Incoming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Incoming { .. }")
    }
}

pub(crate) enum Inbound {
    Status(Status),
    Connected(Box<Connected>),
    Failed(Error),
    PeerDisconnected,
    Ping(std::time::Duration),
    RemoteSettings(Box<tango_net_protocol::control::Settings>),
    RemoteCommit([u8; 16]),
    RemoteUncommit,
    RemoteChunkStart(u64),
    RemoteChunk(Vec<u8>),
    RemoteStartMatch,
}

/// The reporting end of one connection attempt. Handed to the connect
/// future and held by the lobby pump; everything it reports arrives at
/// [`State::apply`] via the attempt's progress channel.
#[derive(Clone)]
pub struct Progress(futures::channel::mpsc::UnboundedSender<Incoming>);

impl Progress {
    /// Note a milestone in the bring-up (see [`Status`]).
    pub fn status(&self, status: Status) {
        self.send(Inbound::Status(status));
    }

    pub(crate) fn send(&self, inbound: Inbound) {
        let _ = self.0.unbounded_send(Incoming(inbound));
    }

    /// Report a transport error, naming the operation that hit it.
    pub(crate) fn fail(&self, operation: &'static str, e: impl std::fmt::Display) {
        log::warn!("lobby: {operation} failed: {e}");
        self.send(Inbound::Failed(Error::Transport {
            operation,
            message: e.to_string(),
        }));
    }
}

/// What a host has to do something about. Everything else [`State::apply`]
/// handles internally and leaves visible in [`State::phase`] /
/// [`State::lobby`] for the next render.
#[derive(Debug, Clone, Copy)]
pub enum Event {
    /// Both sides have exchanged StartMatch. Drain
    /// [`State::take_pre_match`] and build the live match.
    MatchReady,
}

// The transport owns the handoff contract shared by lobby and session.
// Re-exported so hosts can consume a completed lobby directly.
pub use tango_net::handoff::PreMatchData;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_ident_parses_codes_and_direct_commands() {
        assert!(LinkIdent::parse("").is_none());
        assert!(matches!(
            LinkIdent::parse("abc-def"),
            Some(LinkIdent::Matchmaking(code)) if code == "abc-def"
        ));
        assert!(matches!(
            LinkIdent::parse("/host"),
            Some(LinkIdent::Direct(DirectRole::Host { port })) if port == tango_net::DEFAULT_LOCAL_PORT
        ));
        assert!(matches!(
            LinkIdent::parse("/host 1234"),
            Some(LinkIdent::Direct(DirectRole::Host { port: 1234 }))
        ));
        assert!(LinkIdent::parse("/host nope").is_none());
        assert!(matches!(
            LinkIdent::parse("/connect 10.0.0.2"),
            Some(LinkIdent::Direct(DirectRole::Connect { addr })) if addr == "10.0.0.2:24680"
        ));
        assert!(matches!(
            LinkIdent::parse("/connect [::1]"),
            Some(LinkIdent::Direct(DirectRole::Connect { addr })) if addr == "[::1]:24680"
        ));
        assert!(matches!(
            LinkIdent::parse("/connect 10.0.0.2:99"),
            Some(LinkIdent::Direct(DirectRole::Connect { addr })) if addr == "10.0.0.2:99"
        ));
        assert!(LinkIdent::parse("/connect").is_none());
        assert!(LinkIdent::parse("/dance").is_none());
    }
}

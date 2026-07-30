//! Netplay state + connection lifecycle: the connection choreography,
//! settings exchange, ready handshake and match handoff, sitting atop
//! [`tango_session::net`], which owns the wire protocols and channel
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

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub mod compat;
pub mod randomcode;

mod connect;
mod handshake;
mod lobby;

pub use connect::connect;
#[cfg(not(target_arch = "wasm32"))]
pub use connect::connect_direct;
pub use connect::Connected;
pub use tango_session::net::link::{DirectRole, LinkParts, ReconnectRecipe};

pub use handshake::ReadyView;
use handshake::{Handshake, LocalReady, RemoteReady};
use lobby::Command;

// The protocol version and its version-history changelog live in the
// shared tango-net-protocol crate (every client implementation bumps
// and documents them there).
pub use tango_net_protocol::PROTOCOL_VERSION;

/// Why a netplay session failed — typed so the UI can route each
/// failure mode to its own localized copy instead of string-matching
/// sentinel values out of a flat string.
#[derive(Debug, Clone)]
pub enum Error {
    /// Peer closed the connection cleanly (left the lobby / quit).
    PeerDisconnected,
    /// The matchmaking server won't matchmake for a protocol as old as
    /// ours — this Tango needs updating before it can play online.
    /// Distinct from [`Error::NegotiateVersionTooOld`], which is about
    /// the *peer's* version: here there is no peer yet.
    SignalingVersionTooOld,
    /// The matchmaking server is older than this Tango.
    SignalingVersionTooNew,
    /// The server turned us away for some other reason, carrying the
    /// name of the `Abort` reason it gave. The remaining reasons (no
    /// session id, not an upgrade) mean a broken client rather than
    /// anything a player can act on, so they share one variant.
    SignalingRejected(String),
    /// Never reached the matchmaking server at all: offline, bad
    /// endpoint, DNS, TLS.
    SignalingUnreachable(String),
    /// Signaling failed some other way — a malformed or unexpected
    /// packet from a server we did reach.
    Signaling(String),
    /// Signaling worked and the peer connection didn't: the WebRTC
    /// connection never came up, or dropped during the SDP exchange.
    PeerConnection(String),
    /// Version negotiate: the first packet wasn't a Hello.
    NegotiateExpectedHello,
    /// Peer speaks an older protocol version than ours.
    NegotiateVersionTooOld,
    /// Peer speaks a newer protocol version than ours.
    NegotiateVersionTooNew,
    /// Negotiate failed below the version check (transport error).
    Negotiate(String),
    /// Any other failure, with a short context prefix baked into the
    /// text (e.g. "send_chunk: …"). Surfaced raw through the generic
    /// "Connection failed:" template.
    Other(String),
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
#[derive(Debug, Clone, Copy)]
pub enum Status {
    /// Peer is up; running the protocol-version handshake.
    Negotiating,
    /// Matchmaking server accepted us; blocked on the opponent joining.
    WaitingForOpponent,
}

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
    /// Discord join-secret for the rich-presence "Ask to Join" /
    /// "Join Party" affordances. Only matchmaking codes are
    /// joinable across the internet via Discord's deep-link;
    /// direct codes wouldn't reach anyone else, so we surface
    /// `None` and Discord hides the button.
    pub fn discord_join_secret(&self) -> Option<&str> {
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

    /// Report a transport error, prefixed with the operation that hit it.
    pub(crate) fn fail(&self, what: &str, e: impl std::fmt::Display) {
        log::warn!("lobby: {what} failed: {e}");
        self.send(Inbound::Failed(Error::Other(format!("{what}: {e}"))));
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

pub struct State {
    pub phase: Phase,
    /// Live connection objects, when post-negotiate. Cleared on
    /// disconnect / failure / on the next attempt.
    conn: Option<ConnectionHandles>,
    /// The "ready" commitment exchange — the local + remote ready
    /// ladders. Reset together (`Handshake::default()`) on every
    /// session boundary.
    handshake: Handshake,
    /// Cancellation token shared with every in-flight async task (the
    /// connect future, the lobby pump, the reveal stream). Cancelling it
    /// makes them short-circuit; their late reports then land on the
    /// dropped progress channel of the attempt they belonged to.
    cancel: CancellationToken,
    /// Monotonic counter keying the host's progress bridge. Bumped on
    /// every attempt so the prior bridge is torn down even if the user
    /// reconnects from within [`Phase::Lobby`].
    session_id: u64,
    /// Receiving half of this attempt's progress channel. Installed by
    /// `begin`; the host takes it out on first poll (see
    /// [`State::take_incoming`]). Stored as a once-take slot so a host
    /// can consume it from a `&State`.
    incoming_rx_slot: Arc<std::sync::Mutex<Option<futures::channel::mpsc::UnboundedReceiver<Incoming>>>>,
    /// Sending half, kept so the state machine can report its own
    /// failures the same way the background tasks do.
    progress: Option<Progress>,
    /// Queue into the lobby pump. `None` until the connection is up.
    commands: Option<futures::channel::mpsc::UnboundedSender<Command>>,
    /// Lobby-only state — what each side has advertised so far.
    /// `local` is what we sent; `remote` is what came in over the
    /// Settings packet. Both being `Some` means the lobby pane
    /// can render the symmetric "you vs them" view.
    pub lobby: LobbyState,
    /// Matchmaking params stashed at connect time, used in
    /// `take_pre_match` to build a [`ReconnectRecipe::Matchmaking`].
    /// `None` on the direct path (its recipe rides
    /// `ConnectionHandles::reconnect` instead).
    matchmaking_reconnect: Option<MatchmakingParams>,
}

#[derive(Clone)]
pub struct LobbyState {
    pub local: Option<tango_net_protocol::control::Settings>,
    pub remote: Option<tango_net_protocol::control::Settings>,
    /// Round-trip ping measurements, fed one per Pong. Empty before the
    /// first Pong. Its `latest()` (raw) drives the latency line in the
    /// pane; its `median()` smooths the per-second jitter so the
    /// frame-delay "suggest" button recommends a stable value rather than
    /// chasing the latest spike.
    pub latency_counter: tango_session::net::LatencyCounter,
    /// User-picked match type (mode + subtype). Defaults to (0, 0)
    /// = Single. Local-only UI state; gets folded into Settings
    /// on send.
    pub match_type: (u8, u8),
    /// Per-lobby "blind my setup from the opponent" flag. Crosses
    /// the wire via `protocol::Settings.blind_setup`; each side
    /// picks their own independently. Setups are visible by
    /// default — unless the peer flips this on, the match start
    /// renders their save view alongside ours in the session pane.
    pub blind_setup: bool,
    /// Last `(family, variant)` the App's resend pass applied a
    /// "default match type" for. Used so that switching games
    /// triggers a re-default to Triple (when supported), while
    /// user-explicit picks for the SAME game stick.
    pub default_mt_for_game: Option<(String, u8)>,
    /// How the transport actually flows, resolved once the wire
    /// handshake completes: direct (peer-to-peer, incl. the raw
    /// TCP path) or relayed through a TURN server. `None` when it
    /// couldn't be determined.
    pub connection_kind: Option<ConnectionKind>,
}

/// See [`LobbyState::connection_kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionKind {
    Direct,
    Relayed,
}

impl Default for LobbyState {
    fn default() -> Self {
        Self {
            local: None,
            remote: None,
            // 5 marks at one Pong/second ≈ a 5 s median window, matching the
            // in-match `PvpSession` latency counter.
            latency_counter: tango_session::net::LatencyCounter::new(5),
            match_type: (0, 0),
            blind_setup: false,
            default_mt_for_game: None,
            connection_kind: None,
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            conn: None,
            cancel: CancellationToken::new(),
            session_id: 0,
            incoming_rx_slot: Arc::new(std::sync::Mutex::new(None)),
            progress: None,
            commands: None,
            lobby: LobbyState::default(),
            handshake: Handshake::default(),
            matchmaking_reconnect: None,
        }
    }
}

/// Handles we hang onto for the duration of a connected session. The
/// PvP-handoff path (`take_pre_match`) drains these into the PvpSession.
struct ConnectionHandles {
    /// Reliable, ordered control/lobby channel sender. Shared by the lobby
    /// pump and (parked, idle) the match.
    sender: Arc<tokio::sync::Mutex<tango_session::net::Sender>>,
    /// Unreliable, unordered in-match channel sender — idle during the lobby,
    /// handed to the PvP session to carry the live `data::wire` datagrams.
    in_match_sender: tango_session::net::data::Sender,
    /// Unreliable in-match channel's receive half, parked here the moment the
    /// connection lands (nothing flows on it during the lobby, so — unlike
    /// the reliable receiver — it isn't owned by the pump).
    in_match_receiver: tango_session::net::data::Receiver,
    /// The reliable receiver, sent by the pump on cancel-exit. One oneshot
    /// per session, so a dying pump from a previous session can't deposit a
    /// stale receiver into the next one.
    post_lobby_rx: tokio::sync::oneshot::Receiver<tango_session::net::Receiver>,
    /// The peer connection, kept alive for the duration of the session.
    /// Both transports bring one up.
    peer_conn: datachannel_wrapper::PeerConnection,
    /// See [`Connected::is_offerer`].
    is_offerer: bool,
    /// Direct-link rebuild recipe for transparent mid-match reconnection,
    /// or `None` for the matchmaking transport.
    reconnect: Option<DirectRole>,
    /// This connection's two DTLS certificate fingerprints, captured at
    /// connect time and folded into the matchmaking reconnect `session_id`
    /// once the shared RNG seed exists (see [`State::take_pre_match`]).
    /// Empty on the direct path.
    local_dtls_fingerprint: Vec<u8>,
    peer_dtls_fingerprint: Vec<u8>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a matchmaking attempt. Returns what [`connect`] needs: the
    /// token that cancels it and the channel it reports on.
    pub fn begin_matchmaking(&mut self, params: &MatchmakingParams) -> (CancellationToken, Progress) {
        let out = self.begin(LinkIdent::Matchmaking(params.link_code.clone()), false);
        // Set *after* `begin`, which clears it. The session_id half of the
        // recipe is derived later, from the shared RNG seed.
        self.matchmaking_reconnect = Some(params.clone());
        out
    }

    /// Start a direct attempt. Returns what [`connect_direct`] needs.
    ///
    /// Host = "waiting for inbound peer" (accept is the slow await);
    /// Connect = "actively dialing" — mirroring the matchmaking-path
    /// semantics so the existing waiting-screen UI reads correctly.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn begin_direct(&mut self, role: &DirectRole) -> (CancellationToken, Progress) {
        let waiting = matches!(role, DirectRole::Host { .. });
        self.begin(LinkIdent::Direct(role.clone()), waiting)
    }

    fn begin(&mut self, ident: LinkIdent, waiting_for_opponent: bool) -> (CancellationToken, Progress) {
        self.cancel_and_renew();
        self.phase = Phase::Connecting {
            ident,
            waiting_for_opponent,
        };
        let (tx, rx) = futures::channel::mpsc::unbounded();
        *self.incoming_rx_slot.lock().unwrap() = Some(rx);
        let progress = Progress(tx);
        self.progress = Some(progress.clone());
        (self.cancel.clone(), progress)
    }

    /// Reset the cancellation token + bump session_id. Called from every
    /// transition that starts or stops async work so the background tasks
    /// notice and the host's bridge rekeys. We replace the per-session
    /// rx slot Arc rather than clearing it, so a dying pump from the
    /// previous session can't deposit into the next session's slot — it
    /// scribbles into the orphaned Arc and the payload gets dropped along
    /// with it. (The receiver handback needs no such guard: its oneshot
    /// is per-session by construction, and dropping `conn` drops the
    /// receiving end.)
    fn cancel_and_renew(&mut self) {
        self.cancel.cancel();
        self.cancel = CancellationToken::new();
        self.session_id = self.session_id.wrapping_add(1);
        self.incoming_rx_slot = Arc::new(std::sync::Mutex::new(None));
        self.progress = None;
        self.commands = None;
        self.conn = None;
        self.lobby = LobbyState::default();
        self.handshake = Handshake::default();
        self.matchmaking_reconnect = None;
    }

    /// A monotonic id for the current attempt, bumped each time one
    /// starts. A host keys its progress bridge on this so the previous
    /// one is torn down even when the user reconnects without leaving
    /// [`Phase::Lobby`].
    pub fn session_id(&self) -> u64 {
        self.session_id
    }

    /// Take the receiving half of this attempt's progress channel, if it
    /// hasn't been taken yet. A host polls this once per `session_id` and
    /// forwards everything it yields into [`Self::apply`].
    ///
    /// Once-take rather than borrowed so a host can consume it from a
    /// `&State`; a fresh attempt installs a new channel behind a new
    /// `session_id`.
    pub fn take_incoming(&self) -> Option<futures::channel::mpsc::UnboundedReceiver<Incoming>> {
        self.incoming_rx_slot.lock().unwrap().take()
    }

    /// Fold one report from the connection into the state machine.
    pub fn apply(&mut self, incoming: Incoming) -> Option<Event> {
        match incoming.0 {
            Inbound::Status(status) => {
                self.on_status(status);
                None
            }
            Inbound::Connected(connected) => {
                self.enter_lobby(*connected);
                None
            }
            Inbound::Failed(e) => {
                self.cancel_and_renew();
                self.phase = Phase::Failed { error: e };
                None
            }
            // Remote side closed the data channel. Park in Failed (with a
            // peer-cancelled marker the UI surfaces) rather than silently
            // dropping back to Idle, so the user sees what happened and
            // clears it explicitly.
            Inbound::PeerDisconnected => {
                self.fail_keeping_lobby(Error::PeerDisconnected);
                None
            }
            Inbound::Ping(dur) => {
                self.lobby.latency_counter.mark(dur);
                None
            }
            Inbound::RemoteSettings(settings) => {
                self.on_remote_settings(*settings);
                None
            }
            Inbound::RemoteCommit(c) => {
                // A fresh commitment starts a fresh reveal — any prior
                // chunks / StartMatch belonged to the pairing it replaces.
                self.handshake.remote = RemoteReady::Committed {
                    commitment: c,
                    expected: None,
                    chunks: Vec::new(),
                    revealed: false,
                    start_match: false,
                };
                // The reveal goes out once both sides have committed.
                // Until then we just sit ready.
                self.maybe_kick_reveal();
                None
            }
            Inbound::RemoteUncommit => {
                // Their reveal (and any StartMatch riding on the voided
                // pairing) goes with the commitment; our own StartMatch
                // was predicated on that reveal, so it regresses too.
                self.handshake.remote = RemoteReady::NotReady;
                self.handshake.local.revoke_start_match();
                None
            }
            Inbound::RemoteChunkStart(len) => self.on_remote_chunk_start(len),
            Inbound::RemoteChunk(c) => self.on_remote_chunk(c),
            Inbound::RemoteStartMatch => {
                match &mut self.handshake.remote {
                    RemoteReady::Committed { start_match, .. } => *start_match = true,
                    RemoteReady::NotReady => {
                        // StartMatch can only follow the peer's Commit on
                        // the ordered channel — protocol violation; drop it.
                        log::warn!("netplay: ignoring StartMatch received before Commit");
                    }
                }
                self.match_ready_event()
            }
        }
    }

    /// A bring-up milestone. Ignored unless we're still bringing up —
    /// a late report from a superseded attempt has nothing to advance.
    fn on_status(&mut self, status: Status) {
        let Phase::Connecting { ident, .. } = &self.phase else {
            return;
        };
        let ident = ident.clone();
        self.phase = match status {
            Status::WaitingForOpponent => Phase::Connecting {
                ident,
                waiting_for_opponent: true,
            },
            Status::Negotiating => Phase::Negotiating { ident },
        };
    }

    /// The connection is up: install the handles, park the in-match
    /// receiver, and start the lobby pump.
    fn enter_lobby(&mut self, connected: Connected) {
        // Accept both `Negotiating` and `Connecting` — a direct attempt
        // can land straight from `Connecting` if its status report and
        // its completion arrive back to back.
        let ident = match &self.phase {
            Phase::Negotiating { ident } | Phase::Connecting { ident, .. } => ident.clone(),
            // Cancelled / superseded — nothing to attach this to.
            _ => return,
        };
        // Resolve how the transport actually flows for the lobby's ping
        // line. We read the selected ICE pair — a `typ relay` candidate on
        // either end means TURN. The signaling-free direct path only ever
        // forms host candidate pairs, so it resolves to Direct.
        self.lobby.connection_kind = connected
            .peer_conn
            .selected_candidate_pair()
            .ok()
            .map(|(local, remote)| {
                if local.contains("typ relay") || remote.contains("typ relay") {
                    ConnectionKind::Relayed
                } else {
                    ConnectionKind::Direct
                }
            });
        // Channel for the pump to hand the reliable receiver back on
        // cancel-exit. One per session, so a dying pump from a previous
        // session can't deposit a stale receiver into the next one — its
        // send lands on a dropped rx and the receiver is dropped with it.
        let (post_lobby_tx, post_lobby_rx) = tokio::sync::oneshot::channel();
        let sender = connected.sender.clone();
        self.conn = Some(ConnectionHandles {
            sender: connected.sender,
            in_match_sender: connected.in_match_sender,
            in_match_receiver: connected.in_match_receiver,
            post_lobby_rx,
            peer_conn: connected.peer_conn,
            is_offerer: connected.is_offerer,
            reconnect: connected.reconnect,
            local_dtls_fingerprint: connected.local_dtls_fingerprint,
            peer_dtls_fingerprint: connected.peer_dtls_fingerprint,
        });
        let (cmd_tx, cmd_rx) = futures::channel::mpsc::unbounded();
        self.commands = Some(cmd_tx);
        let Some(progress) = self.progress.clone() else {
            return;
        };
        let cancel = self.cancel.clone();
        let receiver = connected.receiver;
        tango_session::platform::spawn(async move {
            let receiver = lobby::run_pump(receiver, sender, cmd_rx, progress, cancel).await;
            let _ = post_lobby_tx.send(receiver);
        });
        self.phase = Phase::Lobby { ident };
    }

    /// Queue a wire send. Silently drops when there's no live pump — every
    /// caller is a lobby-phase action, and a stale one has nothing to say.
    fn send(&self, cmd: Command) {
        if let Some(commands) = &self.commands {
            let _ = commands.unbounded_send(cmd);
        }
    }

    /// Report a local failure the same way the background tasks do, so it
    /// arrives through [`Self::apply`] and takes the same path.
    pub(crate) fn fail(&self, error: Error) {
        if let Some(progress) = &self.progress {
            progress.send(Inbound::Failed(error));
        }
    }

    /// Tear the active or pending connection down. Cancels the in-flight
    /// work and drops the handles.
    pub fn disconnect(&mut self) {
        self.cancel_and_renew();
        self.phase = Phase::Idle;
    }

    /// Push our Settings packet (Lobby only), deduping against the last
    /// sent value and dropping the local commit on a material change.
    pub fn send_local_settings(&mut self, settings: tango_net_protocol::control::Settings) {
        // Only meaningful in Lobby phase; ignore late calls after a
        // disconnect / failure.
        if !matches!(self.phase, Phase::Lobby { .. }) {
            return;
        }
        // Dedupe — the host rebuilds this on every dispatch and most of
        // those don't actually change anything that crosses the wire.
        if self.lobby.local.as_ref() == Some(&settings) {
            return;
        }
        // If the material parts of Settings changed (game selection /
        // match type — i.e. anything the commitment was implicitly tied
        // to) drop the local commit so the peer doesn't think we're still
        // committed to the old save. Nickname / available-games churn is
        // excluded so harmless metadata refreshes don't kick the user out
        // of the ready state.
        if self
            .lobby
            .local
            .as_ref()
            .is_some_and(|prev| settings_materially_differ(prev, &settings))
        {
            self.invalidate_local_commit();
        }
        self.lobby.local = Some(settings.clone());
        self.send(Command::Settings(Box::new(settings)));
    }

    /// Peer's Settings landed; record them and drop our commit if they
    /// downgraded visibility.
    fn on_remote_settings(&mut self, settings: tango_net_protocol::control::Settings) {
        // Visibility downgrade (peer's setup used to be visible, now
        // they've blinded it): drop our local commit so we re-commit
        // explicitly under the new visibility contract.
        let downgrade = self
            .lobby
            .remote
            .as_ref()
            .map(|prev| !prev.blind_setup && settings.blind_setup)
            .unwrap_or(false);
        self.lobby.remote = Some(settings);
        if downgrade {
            self.invalidate_local_commit();
        }
    }

    /// The user picked a match type. The host follows this with a settings
    /// resend, and `send_local_settings`'s material-diff check does the
    /// unready — so deliberately not done here.
    pub fn set_match_type(&mut self, match_type: (u8, u8)) {
        self.lobby.match_type = match_type;
    }

    /// The user toggled the blind-setup checkbox.
    pub fn set_blind_setup(&mut self, v: bool) {
        let prev = self.lobby.blind_setup;
        self.lobby.blind_setup = v;
        // Downgrading our own visibility (blind flips on): drop the
        // *peer's* commit so they re-commit under the new visibility
        // contract. Our own StartMatch was predicated on their now-voided
        // reveal, so it regresses a rung with it.
        if !prev && v {
            self.handshake.remote = RemoteReady::NotReady;
            self.handshake.local.revoke_start_match();
        }
        // The host fires a settings resend after this. The
        // `send_local_settings` material-diff check doesn't include
        // blind_setup, so a same-game blind toggle doesn't drop our own
        // commit unnecessarily.
    }

    /// Peer announced their reveal's total length. Chunks before it are
    /// strays from a voided pairing and get dropped.
    fn on_remote_chunk_start(&mut self, len: u64) -> Option<Event> {
        match &mut self.handshake.remote {
            RemoteReady::Committed { expected, revealed, .. } if expected.is_none() => {
                *expected = Some(len);
                if len == 0 {
                    // Degenerate but well-formed: a zero-length reveal is
                    // complete on arrival (verification rejects it
                    // downstream).
                    *revealed = true;
                    return self.maybe_finish_handshake();
                }
            }
            RemoteReady::Committed { .. } => {
                self.fail(Error::Other(
                    "peer sent a second ChunkStart within one reveal".to_string(),
                ));
            }
            RemoteReady::NotReady => {
                // No commitment on hand — a straggler from a voided pairing
                // (the peer's reveal outliving its Uncommit); drop it like
                // stray chunks.
                log::warn!("netplay: ignoring ChunkStart received before Commit");
            }
        }
        None
    }

    /// Chunk bytes accumulate into the remote ladder's reveal buffer until
    /// the announced length is all here — there's no end-of-stream
    /// sentinel on the wire.
    fn on_remote_chunk(&mut self, chunk: Vec<u8>) -> Option<Event> {
        match &mut self.handshake.remote {
            RemoteReady::Committed { revealed: true, .. } => {
                // The reveal is already complete — anything more is a stray
                // from a voided pairing; drop it.
                log::warn!("netplay: ignoring chunk received after complete reveal");
            }
            RemoteReady::Committed {
                expected: Some(expected),
                chunks,
                revealed,
                ..
            } => {
                chunks.extend_from_slice(&chunk);
                let (got, want) = (chunks.len() as u64, *expected);
                if got > want {
                    self.fail(Error::Other(format!(
                        "peer sent more reveal bytes than announced ({got} > {want})"
                    )));
                } else if got == want {
                    *revealed = true;
                    return self.maybe_finish_handshake();
                }
            }
            RemoteReady::Committed { expected: None, .. } => {
                // On the ordered channel a reveal's ChunkStart always
                // precedes its chunks, so these are strays from a voided
                // pairing; drop them.
                log::warn!("netplay: ignoring chunk received before ChunkStart");
            }
            RemoteReady::NotReady => {
                // Chunks with no commitment to verify against — protocol
                // violation; drop them.
                log::warn!("netplay: ignoring chunk received before Commit");
            }
        }
        None
    }

    /// The host failed to build the PvP session after the handoff (rom /
    /// patch / core construction). Park netplay in the same sticky Failed
    /// state every other netplay failure lands in — the lobby chrome is
    /// still on screen at this point (`handoff_pending` kept it up while
    /// the session was being built), so the failure shows in the status
    /// line the user is already looking at.
    pub fn fail_session_build(&mut self, error: Error) {
        self.fail_keeping_lobby(error);
    }

    /// Tear the live connection down into a sticky `Phase::Failed`, but
    /// deliberately do NOT wipe `self.lobby` — the opponent's card stays
    /// populated with their last-known nickname / game so the failure
    /// banner has a face attached to it.
    fn fail_keeping_lobby(&mut self, error: Error) {
        self.cancel.cancel();
        self.cancel = CancellationToken::new();
        self.session_id = self.session_id.wrapping_add(1);
        self.incoming_rx_slot = Arc::new(std::sync::Mutex::new(None));
        self.progress = None;
        self.commands = None;
        self.conn = None;
        self.handshake = Handshake::default();
        self.phase = Phase::Failed { error };
    }

    /// Drain everything the PvP session needs to take over the
    /// data channel. Returns `None` if either we're not at the
    /// handoff point yet, or it's already been drained. After
    /// this call the netplay subsystem retains no live handles
    /// — the cancellation token fires (which tears down the
    /// lobby pump), and the host owns sender / receiver /
    /// peer_conn / negotiated state.
    ///
    /// `phase` and `lobby` are deliberately NOT cleared here —
    /// the lobby UI keeps rendering its post-ready snapshot while
    /// the host builds the live session in the background, so
    /// the user doesn't see the bottom strip flash back to the
    /// singleplayer chrome. The host calls [`finish_handoff`]
    /// once the PvP session is built — or [`fail_session_build`]
    /// if the build fails, parking the failure in the lobby's
    /// sticky Failed status.
    ///
    /// [`finish_handoff`]: State::finish_handoff
    /// [`fail_session_build`]: State::fail_session_build
    pub fn take_pre_match(&mut self) -> Option<PreMatchData> {
        if !(self.handshake.local.match_ready() && self.handshake.remote.start_match()) {
            return None;
        }
        let handles = self.conn.take()?;
        // Drain our commit off the ladder; the `HandedOff` rung keeps
        // the lobby chrome rendering ready-state until finish_handoff.
        let local_commit = match std::mem::replace(&mut self.handshake.local, LocalReady::HandedOff) {
            LocalReady::StartMatchSent(commit) => commit,
            // Already drained (match_ready() also covers HandedOff) —
            // but conn.take() above already returned None in that case.
            _ => return None,
        };
        let RemoteReady::Committed {
            chunks: remote_chunks, ..
        } = &self.handshake.remote
        else {
            return None;
        };
        let local_settings = self.lobby.local.clone()?;
        let remote_settings = self.lobby.remote.clone()?;
        // Decompress + decode peer's NegotiatedState — we already
        // verified its hash in maybe_finish_handshake; this is just
        // to recover the nonce + save_data.
        let peer_state_bytes = match zstd::stream::decode_all(std::io::Cursor::new(remote_chunks)) {
            Ok(b) => b,
            Err(e) => return self.fail_handoff(Error::Other(format!("zstd decode: {e}"))),
        };
        let peer_state = match tango_net_protocol::control::NegotiatedState::deserialize(&peer_state_bytes) {
            Ok(s) => s,
            Err(e) => return self.fail_handoff(Error::Other(format!("decode peer state: {e}"))),
        };
        // Direct codes carry no remote-discoverable identity, so the
        // replay metadata's `link_code` slot is left empty for them — the
        // replay filename and view substitute their own placeholder.
        // Matchmaking codes round-trip verbatim so a recorded match can be
        // cross-referenced with the matchmaking-server logs.
        let link_code = match &self.phase {
            Phase::Lobby {
                ident: LinkIdent::Matchmaking(code),
            } => code.clone(),
            Phase::Lobby {
                ident: LinkIdent::Direct(_),
            } => String::new(),
            _ => return None,
        };
        // RNG seed for the in-match shared RNG (XOR of the two nonces)
        // and the match clock (the offerer's commit-time wall clock, so
        // both peers pin every core's cart RTC to the same instant —
        // see PreMatchData::match_ts). Both are determinism-critical,
        // so the constructions live in the shared protocol crate.
        let rng_seed = tango_net_protocol::derive::derive_rng_seed(&local_commit.state.nonce, &peer_state.nonce);
        let match_ts =
            tango_net_protocol::derive::pick_match_ts(handles.is_offerer, local_commit.state.ts, peer_state.ts);
        // Cancel the pump so it returns ownership of the receiver via the
        // handles' oneshot. It sends the receiver down it on cancel-exit;
        // `Link::bring_up` awaits it.
        self.cancel.cancel();
        // Build the mid-match reconnect recipe. The direct path carries its
        // recipe on ConnectionHandles; the matchmaking path combines the params
        // stashed at connect time with a session_id derived from the shared RNG
        // seed (now known), so both peers re-rendezvous on the same secret id.
        let recipe = if let Some(role) = handles.reconnect {
            Some(ReconnectRecipe::Direct(role))
        } else {
            self.matchmaking_reconnect
                .take()
                .map(|mm| ReconnectRecipe::Matchmaking {
                    endpoint: mm.endpoint,
                    session_id: tango_session::net::link::derive_reconnect_session_id(
                        &rng_seed,
                        &handles.local_dtls_fingerprint,
                        &handles.peer_dtls_fingerprint,
                    ),
                    use_relay: mm.use_relay,
                })
        };
        let pre_match = PreMatchData {
            link_parts: LinkParts {
                control_sender: handles.sender,
                control_receiver_rx: handles.post_lobby_rx,
                in_match_sender: handles.in_match_sender,
                in_match_receiver: handles.in_match_receiver,
                peer_conn: handles.peer_conn,
                recipe,
                rng_seed,
            },
            is_offerer: handles.is_offerer,
            rng_seed,
            match_ts,
            local_save_data: local_commit.state.save_data,
            remote_save_data: peer_state.save_data,
            local_session_payload: local_commit.state.session_payload,
            remote_session_payload: peer_state.session_payload,
            local_settings,
            remote_settings,
            link_code,
            match_type: self.lobby.match_type,
        };
        Some(pre_match)
    }

    /// A handoff-time decode failure: the peer's revealed state won't parse
    /// even though its hash matched the commitment (checked back in
    /// `maybe_finish_handshake`). By this point `take_pre_match` has already
    /// consumed the connection handles, so the session can't proceed — tear it
    /// down into a visible Failed banner. Returning a bare `None` instead
    /// would read as "already drained" to the host, leaving the lobby stuck on
    /// its "Starting match…" chrome with no error.
    fn fail_handoff(&mut self, error: Error) -> Option<PreMatchData> {
        self.cancel_and_renew();
        self.phase = Phase::Failed { error };
        None
    }

    /// Clear the lobby snapshot that `take_pre_match` left visible.
    /// Called by the host once the session build resolves (either the PvP
    /// view has taken over, or the build failed and the error is showing).
    /// Idempotent.
    pub fn finish_handoff(&mut self) {
        self.phase = Phase::Idle;
        self.lobby = LobbyState::default();
        self.handshake = Handshake::default();
    }

    /// True once both sides have exchanged StartMatch and the
    /// connection handles have been drained into a PreMatchData,
    /// but before [`finish_handoff`](State::finish_handoff) fires.
    /// The lobby UI uses this to lock the committed controls (save /
    /// game selection, match type, blind setup) and show a "Starting
    /// match…" placeholder while the session is built — leaving the
    /// lobby stays possible throughout (the host detects the
    /// mid-build disconnect and winds the finished build down).
    pub fn handoff_pending(&self) -> bool {
        matches!(self.handshake.local, LocalReady::HandedOff) && self.handshake.remote.start_match()
    }
}

// The matchmaking→session handoff bundle lives beside its consumer in
// the session crate; re-exported so netplay callers keep their path.
pub use tango_session::pvp::PreMatchData;

/// Does this settings change warrant auto-unready? `true` for
/// game-info or match-type changes (the user's effectively
/// changed what they're offering up), `false` for nickname /
/// available-games churn (cosmetic / metadata-only). Lets
/// `send_local_settings` drop stale commits without forcing
/// the user back to the Ready button every time their roms
/// scanner repopulates.
fn settings_materially_differ(
    a: &tango_net_protocol::control::Settings,
    b: &tango_net_protocol::control::Settings,
) -> bool {
    a.game_info != b.game_info || a.match_type != b.match_type
}

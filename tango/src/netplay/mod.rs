//! Netplay state + connection lifecycle: the session-layer state
//! machine (connection choreography, settings exchange, match
//! handoff) sitting atop [`crate::net`], which owns the wire
//! protocols and channel mechanics.
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
//! The lobby background loop (post-negotiate) is spawned as a
//! detached `tokio::spawn` task in the `NegotiationDone` handler.
//! It owns the data-channel `Receiver` and emits its observations
//! through an unbounded futures channel; the iced subscription is
//! just a thin Stream pull from the receiving half, re-keyed on
//! `session_id` so a fresh Connect tears the bridge down. Keeping
//! the loop OUT of the subscription's future closure means an
//! incidental subscription drop (phase change → re-render) can no
//! longer abort the loop mid-`.await` and lose the data-channel
//! receiver. The detached task exits only when the cancellation
//! token fires, and on exit it sends the receiver down the
//! per-session oneshot for the PvP handoff to take.

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub mod compat;

mod connect;
mod handshake;
mod lobby;

pub use connect::{NegotiationOutput, SignalingHello};
pub use crate::net::link::{DirectRole, LinkParts, ReconnectRecipe};
pub use lobby::subscription;

use handshake::Handshake;

// 0x47: in-match Input/EndOfRound/EndOfMatch moved off the reliable lobby
// channel onto a separate unreliable channel with the `data::wire` redundancy
// protocol. Incompatible with 0x46 peers, so the version gate rejects them.
// 0x48: the data frame's piggybacked ack is now a signed delta from `base`
// instead of an absolute frontier (smaller on the wire). Incompatible with 0x47.
// 0x4a: mid-match disconnect reworked — a bare channel close reconnects on a
// short window, and a deliberate quit announces itself with a `Closing` marker
// so the peer ends at once. Incompatible with the interim 0x49 `Reconnecting`
// marker, which sat at the same packet tag with the opposite meaning.
// 0x4b: `NegotiatedState` gained `ts` — the commit-time wall clock whose
// offerer-side value becomes the match clock every core pins its cart RTC to
// (deterministic exe45 PvP/replays). Old peers can't decode the reveal.
// 0x4c: the WebRTC stack moved from libdatachannel to tango-rtc (str0m).
// Data channels are now negotiated in-band (DCEP) instead of pre-agreed
// stream ids, so a 0x4b peer's channels would never open against ours; the
// version gate keeps the two stacks from ever pairing.
// (Still 0x4c: the `Closing` marker was removed — the transport's own DTLS
// close_notify already hands the peer a prompt EOF on a deliberate quit. No
// bump: an older 0x4c peer's marker decodes as stray traffic, which the
// mid-match reliable-channel watch ignores.)
pub const PROTOCOL_VERSION: u32 = 0x4c;

/// Why a netplay session failed — typed so the UI can route each
/// failure mode to its own localized copy instead of string-matching
/// sentinel values out of a flat string.
#[derive(Debug, Clone)]
pub enum Error {
    /// Peer closed the connection cleanly (left the lobby / quit).
    PeerDisconnected,
    /// Version negotiate: the first packet wasn't a Hello.
    NegotiateExpectedHello,
    /// Peer speaks an older protocol version than ours.
    NegotiateVersionTooOld,
    /// Peer speaks a newer protocol version than ours.
    NegotiateVersionTooNew,
    /// Negotiate failed below the version check (transport error).
    Negotiate(String),
    /// Any other failure, with a short context prefix baked into the
    /// text (e.g. "signaling: …", "send_chunk: …"). Surfaced raw
    /// through the generic "Connection failed:" template.
    Other(String),
}

/// Where the lifecycle is right now. Drives the Play tab's status
/// bar + the Cancel button's visibility.
#[derive(Clone, Debug, Default)]
pub enum Phase {
    /// No connection attempt in flight.
    #[default]
    Idle,
    /// Signaling task in flight. `waiting_for_opponent` flips true
    /// once the matchmaking server's Hello arrives; up to that
    /// point we're still negotiating with the server, after we're
    /// blocked on the peer joining + the WebRTC handshake.
    Connecting {
        ident: LinkIdent,
        waiting_for_opponent: bool,
    },
    /// Data channel up; exchanging Hello packets / verifying both
    /// peers speak the same `protocol::VERSION`.
    Negotiating { ident: LinkIdent },
    /// Both peers agreed on the protocol. Lobby loop is running in
    /// the background; settings exchange + match start come next.
    Lobby { ident: LinkIdent },
    /// Last attempt failed. Stays here until the user starts a new
    /// connection or clears the field.
    Failed { error: Error },
}

/// Structured identifier for the current connection. Kept in
/// `Phase` across the lifecycle, and also the payload of the
/// play-tab's connect Effect, so consumers (UI header, status
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

pub struct State {
    pub phase: Phase,
    /// Live connection objects, when post-negotiate. Cleared on
    /// Disconnect / Failed / on the next Connect.
    conn: Option<ConnectionHandles>,
    /// The "ready" commitment exchange — our commit, the peer's
    /// commitment + reassembled chunks, and the chunk-send guard.
    /// Reset together on every session boundary (see
    /// [`Handshake::reset`]).
    handshake: Handshake,
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
    /// Receiving half of the bridge between the detached lobby
    /// task and the iced subscription. Spawn-time `NegotiationDone`
    /// installs a fresh `(tx, rx)` pair; the subscription takes
    /// `rx` out on first poll. Stored as a once-take slot so the
    /// subscription's `fn(&D)` builder can consume without `&mut`.
    lobby_event_rx_slot: Arc<std::sync::Mutex<Option<futures::channel::mpsc::UnboundedReceiver<Message>>>>,
    /// Lobby-only state — what each side has advertised so far.
    /// `local` is what we sent; `remote` is what came in over the
    /// Settings packet. Both being `Some` means the lobby pane
    /// can render the symmetric "you vs them" view.
    pub lobby: LobbyState,

    /// Matchmaking connection params stashed at `Connect`, used in
    /// `take_pre_match` to build a [`ReconnectRecipe::Matchmaking`]. `None` on
    /// the direct path (its recipe rides `ConnectionHandles::reconnect` instead).
    matchmaking_reconnect: Option<MatchmakingReconnect>,
}

#[derive(Clone)]
pub struct LobbyState {
    pub local: Option<crate::net::protocol::Settings>,
    pub remote: Option<crate::net::protocol::Settings>,
    /// Round-trip ping measurements, fed one-per-Pong by `PingMeasured` from
    /// the lobby loop. Empty before the first Pong. Its `latest()` (raw) drives
    /// the latency line in the pane; its `median()` smooths the per-second
    /// jitter so the frame-delay "suggest" button recommends a stable value
    /// rather than chasing the latest spike.
    pub latency_counter: crate::net::LatencyCounter,
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
    /// We've sent a Commit packet — flagged in the UI as
    /// "you: ready". Cleared on Uncommit + on every settings
    /// change (any selection move invalidates the commitment).
    pub local_ready: bool,
    /// Peer has sent us a Commit packet. Same semantics.
    pub remote_ready: bool,
    /// We've verified peer's chunks + sent our StartMatch. Half
    /// of the "both sides ready" condition for PvP handoff.
    pub match_ready: bool,
    /// Peer sent us a StartMatch packet. Other half.
    pub remote_match_ready: bool,
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
            latency_counter: crate::net::LatencyCounter::new(5),
            match_type: (0, 0),
            blind_setup: false,
            local_ready: false,
            remote_ready: false,
            match_ready: false,
            remote_match_ready: false,
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
            lobby_event_rx_slot: Arc::new(std::sync::Mutex::new(None)),
            lobby: LobbyState::default(),
            handshake: Handshake::default(),
            matchmaking_reconnect: None,
        }
    }
}

/// Handles we hang onto for the duration of a connected session: the
/// Sender (locked behind a tokio Mutex because the lobby loop and the
/// eventual battle loop share it), and the peer-connection itself so
/// the underlying RTC stays up. The PvP-handoff path
/// (`take_pre_match`) drains these into the PvpSession.
struct ConnectionHandles {
    /// Reliable, ordered control/lobby channel sender. Shared by the lobby loop
    /// and (parked, idle) the match.
    sender: Arc<tokio::sync::Mutex<crate::net::Sender>>,
    /// Unreliable, unordered in-match channel sender — idle during the lobby,
    /// handed to the PvP session to carry the live `data::wire` datagrams.
    in_match_sender: crate::net::data::Sender,
    /// Unreliable in-match channel's receive half, parked here the moment
    /// `NegotiationDone` fires (nothing flows on it during the lobby, so it
    /// isn't owned by the lobby loop).
    in_match_receiver: crate::net::data::Receiver,
    /// The reliable receiver, sent by the lobby loop on cancel-exit. One
    /// oneshot per session, so a dying loop from a previous session can't
    /// deposit a stale receiver into the next one.
    post_lobby_rx: tokio::sync::oneshot::Receiver<crate::net::Receiver>,
    /// The peer connection, kept alive for the duration of the
    /// session. Both transports (matchmaking WebRTC and the
    /// signaling-free direct link) bring one up.
    peer_conn: tango_rtc::PeerConnection,
    /// `true` iff we're the "offer side" for symmetry-breaking
    /// purposes — i.e. we wrote the SDP offer on the matchmaking path,
    /// or we're the host on the direct link. Drives the
    /// `Match::pick_local_player_index` tie-break.
    is_offerer: bool,
    /// Direct-link rebuild recipe for transparent mid-match reconnection,
    /// or `None` for the matchmaking transport. See
    /// [`NegotiationOutput::reconnect`].
    reconnect: Option<DirectRole>,
    /// This connection's two DTLS certificate fingerprints, captured at connect
    /// time and folded into the matchmaking reconnect `session_id` once the
    /// shared RNG seed exists (see [`State::take_pre_match`]). Empty on the
    /// direct path.
    local_dtls_fingerprint: Vec<u8>,
    peer_dtls_fingerprint: Vec<u8>,
}

/// Messages the netplay subsystem emits + accepts. App routes
/// these via `Message::Netplay(_)`.
#[derive(Debug, Clone)]
pub enum Message {
    /// User pressed Play with a link code. Kicks off the async
    /// connect task. `use_relay` is `config.relay_mode` at press
    /// time, in the form `tango_signaling::connect` expects: `None`
    /// = auto, `Some(true)` = relay only, `Some(false)` = never.
    Connect {
        link_code: String,
        endpoint: String,
        use_relay: Option<bool>,
        /// The App's persistent client identity (cloned from app state),
        /// presented as the signaling websocket's mTLS client certificate.
        /// `None` when no identity could be loaded — dial without one.
        identity: Option<tango_signaling::ClientIdentity>,
    },
    /// Direct local-play entry. Bypasses the signaling server —
    /// runs the protocol-version negotiate handshake over a
    /// signaling-free peer connection both sides configure from
    /// fixed ICE creds (see [`crate::net::direct_rtc`]). `role`
    /// says whether we're the host (pins the UDP port) or the dialer;
    /// the UI-side identifier is derived from it (see [`LinkIdent`]).
    ConnectDirect { role: DirectRole },
    /// Tear down the active / pending connection. Cancels the
    /// running async task; drops the connection handles.
    Disconnect,
    /// Internal: matchmaking-server hello arrived (ICE config in
    /// hand, awaiting peer). Flips Connecting.waiting_for_opponent
    /// true and kicks off the WebRTC await task.
    SignalingHelloReceived(Slot<SignalingHello>),
    /// Internal: the signaling + WebRTC handshake resolved. We then
    /// kick off the protocol negotiate task before lifecycle moves
    /// out of Connecting.
    SignalingDone(Slot<crate::net::channel::Channels>),
    /// Internal: protocol negotiate succeeded. Receiver is parked
    /// in the slot for the lobby subscription to take.
    NegotiationDone(Slot<NegotiationOutput>),
    /// Internal: any step (signaling, WebRTC, negotiate, or
    /// lobby loop) failed. Carries the typed failure the UI
    /// localizes.
    Failed(Error),
    /// Internal: the running async task short-circuited because the
    /// cancellation token fired (user clicked Disconnect, or a
    /// fresh Connect superseded us). No-op — phase has already
    /// been moved to Idle by whoever cancelled.
    Cancelled,
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
    /// Internal: ack that some background wire send (Settings,
    /// Commit, Uncommit, Chunk-stream, StartMatch) made it onto
    /// the wire. No-op message; just bumps the state-changed
    /// counter so iced re-renders.
    WireOpDone,
    /// User changed the match-type pick. Lobby state updates and
    /// the App resends the Settings packet.
    SetMatchType((u8, u8)),
    /// User toggled the "blind setup" checkbox. Triggers a
    /// Settings resend (the flag's part of the wire format).
    SetBlindSetup(bool),
    /// User pressed the Ready button. Payload is the local
    /// save's raw SRAM — packed into NegotiatedState, zstd'd,
    /// committed to, then Chunk'd over the wire.
    Commit { save_sram: Vec<u8> },
    /// User un-pressed Ready (or a settings change invalidated
    /// the commitment). Sends an Uncommit packet so the peer
    /// knows we're no longer ready.
    Uncommit,
    /// Internal: peer sent us a Commit packet.
    RemoteCommit([u8; 16]),
    /// Internal: peer sent us an Uncommit packet.
    RemoteUncommit,
    /// Internal: peer sent us a Chunk packet.
    RemoteChunk(Vec<u8>),
    /// Internal: peer sent us a StartMatch packet. Once both
    /// sides have exchanged StartMatch, the App picks this up
    /// via `MatchHandoffReady` and spins up a PvpSession.
    RemoteStartMatch,
    /// Internal: both peers have committed, exchanged chunks,
    /// verified commitments, and both StartMatch packets are
    /// accounted for. The App handler drains
    /// `take_pre_match()` and constructs the live match.
    MatchHandoffReady,
}

/// Single-take Arc<Mutex<Option<T>>> we use to pass non-Clone /
/// non-Sync payloads through iced's `Task::perform` boundary. The
/// runtime needs `Message: Clone + Send`, and DataChannel /
/// PeerConnection aren't Clone — this wrapper papers over that by
/// taking the inner once on receipt and going None afterwards.
pub type Slot<T> = Arc<std::sync::Mutex<Option<T>>>;

/// Matchmaking connection params stashed at `Connect` (before the shared RNG
/// seed exists), combined with the derived `session_id` in [`take_pre_match`]
/// to form a [`ReconnectRecipe::Matchmaking`].
///
/// [`take_pre_match`]: State::take_pre_match
#[derive(Clone)]
struct MatchmakingReconnect {
    endpoint: String,
    use_relay: Option<bool>,
    identity: Option<tango_signaling::ClientIdentity>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset the cancellation token + bump session_id. Called from
    /// every transition that starts or stops async work so the
    /// background tasks notice and the subscription rekeys. We
    /// replace the per-session event-rx slot Arc rather than
    /// clearing it, so a dying lobby task from the previous session
    /// can't deposit into the next session's slot — it scribbles
    /// into the orphaned Arc and the payload gets dropped along
    /// with it. (The receiver handback needs no such guard: its
    /// oneshot is per-session by construction, and dropping `conn`
    /// drops the receiving end.)
    fn cancel_and_renew(&mut self) {
        self.cancel.cancel();
        self.cancel = CancellationToken::new();
        self.session_id = self.session_id.wrapping_add(1);
        self.lobby_event_rx_slot = Arc::new(std::sync::Mutex::new(None));
        self.conn = None;
        self.lobby = LobbyState::default();
        self.handshake = Handshake::default();
        self.matchmaking_reconnect = None;
    }

    /// Apply a Message. Returns the iced Task to schedule for any
    /// async follow-up.
    pub fn update(&mut self, msg: Message) -> iced::Task<Message> {
        match msg {
            Message::Connect {
                link_code,
                endpoint,
                use_relay,
                identity,
            } => self.connect(link_code, endpoint, use_relay, identity),
            Message::ConnectDirect { role } => self.connect_direct(role),
            Message::SignalingHelloReceived(slot_rx) => self.on_signaling_hello(slot_rx),
            Message::SignalingDone(slot_rx) => self.on_signaling_done(slot_rx),
            Message::NegotiationDone(slot_rx) => self.on_negotiation_done(slot_rx),
            Message::PingMeasured(dur) => {
                self.lobby.latency_counter.mark(dur);
                iced::Task::none()
            }
            Message::SendLocalSettings(settings) => self.send_local_settings(settings),
            Message::WireOpDone => iced::Task::none(),
            Message::RemoteSettings(settings) => self.on_remote_settings(*settings),
            Message::SetMatchType(mt) => {
                self.lobby.match_type = mt;
                // Don't unready here directly — the App fires a
                // settings resend right after this, and
                // SendLocalSettings handles the unready via the
                // material-diff check.
                iced::Task::none()
            }
            Message::SetBlindSetup(v) => self.set_blind_setup(v),
            Message::Commit { save_sram } => self.commit_local(save_sram),
            Message::Uncommit => self.invalidate_local_commit(),
            Message::RemoteCommit(c) => {
                self.handshake.remote_commitment = Some(c);
                self.handshake.remote_chunks.clear();
                self.lobby.remote_ready = true;
                // First chunk send happens once both sides have
                // committed. Until then we just sit ready.
                self.maybe_kick_chunk_exchange()
            }
            Message::RemoteUncommit => {
                self.handshake.remote_commitment = None;
                self.handshake.remote_chunks.clear();
                self.lobby.remote_ready = false;
                self.lobby.match_ready = false;
                iced::Task::none()
            }
            Message::RemoteChunk(c) => {
                // Empty chunk = end-of-stream sentinel. Anything
                // non-empty just accumulates into remote_chunks.
                // NB: empty-sentinel flushing; lets us stream
                // save states of any size in single-byte-of-
                // overhead chunks.
                if c.is_empty() {
                    self.try_finish_handshake()
                } else {
                    self.handshake.remote_chunks.extend_from_slice(&c);
                    iced::Task::none()
                }
            }
            Message::RemoteStartMatch => {
                self.lobby.remote_match_ready = true;
                self.maybe_signal_pvp_handoff()
            }
            Message::MatchHandoffReady => {
                // Pure signal — the App picks it up and pulls
                // pre-match data via take_pre_match. We just
                // re-render here.
                iced::Task::none()
            }
            Message::Failed(e) => {
                self.cancel_and_renew();
                self.phase = Phase::Failed { error: e };
                iced::Task::none()
            }
            Message::Cancelled => iced::Task::none(),
            Message::PeerDisconnected => self.on_peer_disconnected(),
            Message::Disconnect => {
                self.cancel_and_renew();
                self.phase = Phase::Idle;
                iced::Task::none()
            }
        }
    }

    /// `Message::SendLocalSettings` — push our Settings packet (Lobby only),
    /// deduping against the last sent value and dropping the local commit on a
    /// material change.
    fn send_local_settings(&mut self, settings: Box<crate::net::protocol::Settings>) -> iced::Task<Message> {
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
        // If the material parts of Settings changed (game
        // selection / match type — i.e. anything the
        // commitment was implicitly tied to) drop the
        // local commit so the peer doesn't think we're
        // still committed to the old save. Nickname /
        // available-games churn is excluded so harmless
        // metadata refreshes don't kick the user out of
        // the ready state.
        let invalidate = match self.lobby.local.as_ref() {
            Some(prev) if settings_materially_differ(prev, &settings) => self.invalidate_local_commit(),
            _ => iced::Task::none(),
        };
        self.lobby.local = Some(*settings.clone());
        let send = iced::Task::perform(
            async move {
                sender
                    .lock()
                    .await
                    .send_settings(*settings)
                    .await
                    .map_err(|e| Error::Other(format!("send_settings: {e}")))
            },
            |r| match r {
                Ok(()) => Message::WireOpDone,
                Err(e) => Message::Failed(e),
            },
        );
        iced::Task::batch([invalidate, send])
    }

    /// `Message::RemoteSettings` — peer's Settings landed; record them and
    /// drop our commit if they downgraded visibility.
    fn on_remote_settings(&mut self, settings: crate::net::protocol::Settings) -> iced::Task<Message> {
        // Visibility downgrade (peer's setup used to be
        // visible, now they've blinded it): drop our local
        // commit so we re-commit explicitly under the new
        // visibility contract. Matches the legacy app
        // (gui/play_pane.rs::handle_settings).
        let downgrade = self
            .lobby
            .remote
            .as_ref()
            .map(|prev| !prev.blind_setup && settings.blind_setup)
            .unwrap_or(false);
        self.lobby.remote = Some(settings);
        if downgrade {
            self.invalidate_local_commit()
        } else {
            iced::Task::none()
        }
    }

    /// `Message::SetBlindSetup` — toggle our blind-setup flag; flipping it on
    /// drops the peer's commit (they must re-commit under the new contract).
    fn set_blind_setup(&mut self, v: bool) -> iced::Task<Message> {
        let prev = self.lobby.blind_setup;
        self.lobby.blind_setup = v;
        // Downgrading our own visibility (blind flips on):
        // drop the *peer's* commit so they re-commit under
        // the new visibility contract. Matches legacy
        // `set_reveal_setup` in gui/play_pane.rs.
        if !prev && v {
            self.handshake.remote_commitment = None;
            self.handshake.remote_chunks.clear();
            self.lobby.remote_ready = false;
            self.lobby.remote_match_ready = false;
            self.lobby.match_ready = false;
        }
        // App fires a settings resend after this. The
        // SendLocalSettings material-diff check doesn't
        // include blind_setup, so a same-game blind
        // toggle doesn't drop our own commit unnecessarily.
        iced::Task::none()
    }

    /// `Message::PeerDisconnected` — peer closed the data channel cleanly.
    /// Tear the live connection down into a sticky Failed banner, but keep
    /// `self.lobby` so the opponent card still has a face on it.
    fn on_peer_disconnected(&mut self) -> iced::Task<Message> {
        // Remote side cancelled / closed the data channel.
        // Park netplay in Failed (with a peer-cancelled
        // marker the UI surfaces) instead of silently
        // dropping back to Idle, so the user sees what
        // happened and clears it explicitly. We tear
        // down the live connection here but deliberately
        // do NOT wipe `self.lobby` — the opponent's
        // card stays populated with their last-known
        // nickname / game so the "they left" banner has
        // a face attached to it.
        self.cancel.cancel();
        self.cancel = CancellationToken::new();
        self.session_id = self.session_id.wrapping_add(1);
        self.lobby_event_rx_slot = Arc::new(std::sync::Mutex::new(None));
        self.conn = None;
        self.handshake = Handshake::default();
        self.phase = Phase::Failed {
            error: Error::PeerDisconnected,
        };
        iced::Task::none()
    }

    /// Drain everything the PvP session needs to take over the
    /// data channel. Returns `None` if either we're not at the
    /// handoff point yet, or it's already been drained. After
    /// this call the netplay subsystem retains no live handles
    /// — the cancellation token fires (which tears down the
    /// lobby loop), and the App owns sender / receiver /
    /// peer_conn / negotiated state.
    ///
    /// `phase` and `lobby` are deliberately NOT cleared here —
    /// the lobby UI keeps rendering its post-ready snapshot while
    /// `spawn_pvp` builds the live session in the background, so
    /// the user doesn't see the bottom strip flash back to the
    /// singleplayer Fight/link-code chrome. The App calls
    /// [`finish_handoff`] when the PvP session is built (or its
    /// build fails) to clear that state.
    pub fn take_pre_match(&mut self) -> Option<PreMatchData> {
        if !(self.lobby.match_ready && self.lobby.remote_match_ready) {
            return None;
        }
        let handles = self.conn.take()?;
        let local_commit = self.handshake.local_commit.take()?;
        let local_settings = self.lobby.local.clone()?;
        let remote_settings = self.lobby.remote.clone()?;
        // Decompress + decode peer's NegotiatedState — we already
        // verified its hash in try_finish_handshake; this is just
        // to recover the nonce + save_data.
        let peer_state_bytes = match zstd::stream::decode_all(std::io::Cursor::new(&self.handshake.remote_chunks)) {
            Ok(b) => b,
            Err(e) => return self.fail_handoff(Error::Other(format!("zstd decode: {e}"))),
        };
        let peer_state = match crate::net::protocol::NegotiatedState::deserialize(&peer_state_bytes) {
            Ok(s) => s,
            Err(e) => return self.fail_handoff(Error::Other(format!("decode peer state: {e}"))),
        };
        // Direct-TCP codes carry no remote-discoverable identity,
        // so the replay metadata's `link_code` slot is left empty
        // for them — the replay filename and view substitute
        // their own placeholder. Matchmaking codes round-trip
        // verbatim so a recorded match can be cross-referenced
        // with the matchmaking-server logs.
        let link_code = match &self.phase {
            Phase::Lobby {
                ident: LinkIdent::Matchmaking(code),
            } => code.clone(),
            Phase::Lobby {
                ident: LinkIdent::Direct(_),
            } => String::new(),
            _ => return None,
        };
        // RNG seed for the in-match shared RNG: XOR of the two
        // nonces. Same construction as the legacy app.
        let rng_seed: [u8; 16] = std::array::from_fn(|i| local_commit.state.nonce[i] ^ peer_state.nonce[i]);
        // Match clock: the offerer's commit-time wall clock, so both peers pin
        // every core's cart RTC to the same instant (see PreMatchData::match_ts).
        let match_ts = if handles.is_offerer {
            local_commit.state.ts
        } else {
            peer_state.ts
        };
        // Cancel the lobby loop so it returns ownership of the receiver via
        // the handles' oneshot. The loop sends the receiver down it on
        // cancel-exit; `Link::bring_up` awaits it.
        self.cancel.cancel();
        // Build the mid-match reconnect recipe. The direct path carries its
        // recipe on ConnectionHandles; the matchmaking path combines the params
        // stashed at Connect with a session_id derived from the shared RNG seed
        // (now known), so both peers re-rendezvous on the same secret id.
        let recipe = if let Some(role) = handles.reconnect {
            Some(ReconnectRecipe::Direct(role))
        } else {
            self.matchmaking_reconnect
                .take()
                .map(|mm| ReconnectRecipe::Matchmaking {
                    endpoint: mm.endpoint,
                    session_id: crate::net::link::derive_reconnect_session_id(
                        &rng_seed,
                        &handles.local_dtls_fingerprint,
                        &handles.peer_dtls_fingerprint,
                    ),
                    use_relay: mm.use_relay,
                    identity: mm.identity,
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
            local_settings,
            remote_settings,
            link_code,
            match_type: self.lobby.match_type,
        };
        Some(pre_match)
    }

    /// A handoff-time decode failure: the peer's revealed state won't parse
    /// even though its hash matched the commitment (checked back in
    /// `try_finish_handshake`). By this point `take_pre_match` has already
    /// consumed the connection handles, so the session can't proceed — tear it
    /// down into a visible Failed banner. Returning a bare `None` instead
    /// would read as "already drained" to the App, leaving the lobby stuck on
    /// its "Starting match…" chrome with no error.
    fn fail_handoff(&mut self, error: Error) -> Option<PreMatchData> {
        self.cancel_and_renew();
        self.phase = Phase::Failed { error };
        None
    }

    /// Clear the lobby snapshot that `take_pre_match` left visible.
    /// Called by the App once `spawn_pvp` resolves (either the PvP
    /// view has taken over, or the build failed and `last_error`
    /// is showing). Idempotent.
    pub fn finish_handoff(&mut self) {
        self.phase = Phase::Idle;
        self.lobby = LobbyState::default();
        self.handshake.remote_commitment = None;
        self.handshake.remote_chunks.clear();
        self.handshake.local_chunks_sent = false;
    }

    /// True once both sides have exchanged StartMatch and the
    /// connection handles have been drained into a PreMatchData,
    /// but before [`finish_handoff`] fires. The lobby UI uses
    /// this to disable the ready / cancel chrome and show a
    /// "Starting match…" placeholder while `spawn_pvp` runs.
    pub fn handoff_pending(&self) -> bool {
        self.lobby.match_ready && self.lobby.remote_match_ready && self.conn.is_none()
    }
}

/// Everything the App needs to build a PvpSession. Drained
/// from netplay::State after both sides exchanged StartMatch.
pub struct PreMatchData {
    /// The transport bundle — every live handle the peer link owns for the
    /// match's lifetime (channels, peer connection, reconnect recipe).
    /// `Link::bring_up` assembles it.
    pub link_parts: LinkParts,
    pub is_offerer: bool,
    pub rng_seed: [u8; 16],
    /// The match clock, milliseconds since the unix epoch: the offerer's
    /// commit-time wall clock, identical on both peers. Every core (primary,
    /// shadow, re-sim stepper) pins its cart RTC here so RTC-reading games
    /// (exe45) stay deterministic, and the replay metadata records it as `ts`
    /// so playback pins to the same value.
    pub match_ts: u64,
    pub local_save_data: Vec<u8>,
    pub remote_save_data: Vec<u8>,
    pub local_settings: crate::net::protocol::Settings,
    pub remote_settings: crate::net::protocol::Settings,
    pub link_code: String,
    pub match_type: (u8, u8),
}

// The channel/peer-conn handles aren't `Debug`; a placeholder keeps the
// enclosing `Message` (which carries a `Slot<PreMatchData>`) derivable, same as
// `Channels`.
impl std::fmt::Debug for PreMatchData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreMatchData { .. }")
    }
}

/// Does this settings change warrant auto-unready? `true` for
/// game-info or match-type changes (the user's effectively
/// changed what they're offering up), `false` for nickname /
/// available-games churn (cosmetic / metadata-only). Lets the
/// SendLocalSettings handler drop stale commits without forcing
/// the user back to the Ready button every time their roms
/// scanner repopulates.
fn settings_materially_differ(a: &crate::net::protocol::Settings, b: &crate::net::protocol::Settings) -> bool {
    a.game_info != b.game_info || a.match_type != b.match_type
}

fn slot<T>(payload: T) -> Slot<T> {
    Arc::new(std::sync::Mutex::new(Some(payload)))
}

/// Distinct error variants for the async tasks so the message
/// handler can tell a user-initiated Disconnect (Cancelled, no
/// UI noise) from a real error (Failed, surface to the user).
enum AsyncError {
    Cancelled,
    Failed(Error),
}

fn map_async_err(e: AsyncError) -> Message {
    match e {
        AsyncError::Cancelled => Message::Cancelled,
        AsyncError::Failed(s) => Message::Failed(s),
    }
}

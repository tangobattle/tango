//! Netplay state + connection lifecycle.
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
//! token fires, and on exit it deposits the receiver into the
//! per-session post slot for the PvP handoff to take.

use std::sync::Arc;
use subtle::ConstantTimeEq;
use tokio_util::sync::CancellationToken;

pub mod compat;

pub const PROTOCOL_VERSION: u32 = 0x43;

/// Where the lifecycle is right now. Drives the Play tab's status
/// bar + the Cancel button's visibility.
#[derive(Clone, Debug)]
pub enum Phase {
    /// No connection attempt in flight.
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
    Failed { error: String },
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

impl Default for Phase {
    fn default() -> Self {
        Self::Idle
    }
}

pub struct State {
    pub phase: Phase,
    /// Live connection objects, when post-negotiate. Cleared on
    /// Disconnect / Failed / on the next Connect.
    conn: Option<ConnectionHandles>,
    /// Local ready handshake data: the random nonce we picked,
    /// the zstd-compressed serialized NegotiatedState we
    /// committed to, and the commitment we sent. Cleared on
    /// Uncommit + on every settings change.
    local_commit: Option<LocalCommit>,
    /// Peer's most recently received Commit hash.
    remote_commitment: Option<[u8; 16]>,
    /// Reassembled peer chunks (zstd-compressed
    /// NegotiatedState). Cleared whenever either side
    /// uncommits / disconnects / fails. Finalized once an
    /// empty-sentinel `Chunk` arrives — see the
    /// `Message::RemoteChunk` handler.
    remote_chunks: Vec<u8>,
    /// Guards `maybe_kick_chunk_exchange` so it spawns the
    /// chunk-sender task at most once per commit pairing.
    /// Cleared on Uncommit / Disconnect / Failed.
    local_chunks_sent: bool,
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
    /// Receiver handed back from the lobby loop on cancel-exit.
    /// PvP handoff (`take_pre_match`) drains this into the
    /// PvpReceiver adapter; otherwise it just sits None. Reset to
    /// a fresh `Arc` on every session boundary so a dying loop
    /// from a previous session can't deposit a stale receiver
    /// into the next one.
    post_lobby_receiver: Arc<std::sync::Mutex<Option<crate::net::Receiver>>>,
    /// Lobby-only state — what each side has advertised so far.
    /// `local` is what we sent; `remote` is what came in over the
    /// Settings packet. Both being `Some` means the lobby pane
    /// can render the symmetric "you vs them" view.
    pub lobby: LobbyState,
}

#[derive(Clone)]
pub struct LobbyState {
    pub local: Option<crate::net::protocol::Settings>,
    pub remote: Option<crate::net::protocol::Settings>,
    /// Most recent measured round-trip ping. None before the first
    /// Pong; updated by `PingMeasured` from the lobby loop.
    pub latency: Option<std::time::Duration>,
    /// User-picked match type (mode + subtype). Defaults to (0, 0)
    /// = Single. Local-only UI state; gets folded into Settings
    /// on send.
    pub match_type: (u8, u8),
    /// Per-lobby "reveal my setup to the opponent" flag. Crosses
    /// the wire via `protocol::Settings.reveal_setup`; each side
    /// picks their own independently. When the peer flips it on
    /// + the match starts, we render their save view alongside
    /// ours in the session pane.
    pub reveal_setup: bool,
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
}

impl Default for LobbyState {
    fn default() -> Self {
        Self {
            local: None,
            remote: None,
            latency: None,
            match_type: (0, 0),
            reveal_setup: false,
            local_ready: false,
            remote_ready: false,
            match_ready: false,
            remote_match_ready: false,
            default_mt_for_game: None,
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
            post_lobby_receiver: Arc::new(std::sync::Mutex::new(None)),
            lobby: LobbyState::default(),
            local_commit: None,
            remote_commitment: None,
            remote_chunks: Vec::new(),
            local_chunks_sent: false,
        }
    }
}

#[derive(Clone)]
struct LocalCommit {
    /// Pre-`StartMatch` view of our negotiated state. Used to
    /// (a) derive the post-handshake RNG seed (`local.nonce XOR
    /// remote.nonce`) and (b) pass our save bytes into the PvP
    /// session once the match starts.
    state: crate::net::protocol::NegotiatedState,
    /// `zstd(bincode(state))` — the bytes we hash for our
    /// commitment and slice into the Chunk packets.
    compressed: Vec<u8>,
}

/// Handles we hang onto for the duration of a connected session:
/// the Sender (locked behind a tokio Mutex because the lobby loop
/// + the eventual battle loop share it), and the peer-connection
/// itself so the underlying RTC stays up. The PvP-handoff path
/// (`take_pre_match`) drains these into the PvpSession.
struct ConnectionHandles {
    sender: Arc<tokio::sync::Mutex<crate::net::Sender>>,
    /// `Some` for WebRTC peer connections (kept alive for the
    /// duration of the session); `None` for the direct-TCP local
    /// transport, whose lifetime is owned by the Sender/Receiver
    /// halves themselves.
    peer_conn: Option<datachannel_wrapper::PeerConnection>,
    /// `true` iff we're the "offer side" for symmetry-breaking
    /// purposes — i.e. we wrote the SDP offer on the WebRTC path,
    /// or we're the listener on the direct-TCP path. Drives the
    /// `Match::pick_local_player_index` tie-break.
    is_offerer: bool,
}

/// Messages the netplay subsystem emits + accepts. App routes
/// these via `Message::Netplay(_)`.
#[derive(Debug, Clone)]
pub enum Message {
    /// User pressed Play with a link code. Kicks off the async
    /// connect task.
    Connect { link_code: String, endpoint: String },
    /// Direct-TCP local-play entry. Bypasses signaling + WebRTC
    /// entirely — runs the protocol-version negotiate handshake
    /// over a raw length-prefixed TCP connection (see
    /// [`crate::net::tcp`]). `role` says whether we're the
    /// listener or the dialer; the UI-side identifier is derived
    /// from it (see [`LinkIdent`]).
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
    SignalingDone(Slot<ConnectionPayload>),
    /// Internal: protocol negotiate succeeded. Receiver is parked
    /// in the slot for the lobby subscription to take.
    NegotiationDone(Slot<NegotiationOutput>),
    /// Internal: any step (signaling, datachannel, negotiate, or
    /// lobby loop) failed. Includes the user-readable error
    /// message.
    Failed(String),
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
    /// User toggled the "reveal setup" checkbox. Triggers a
    /// Settings resend (the flag's part of the wire format).
    SetRevealSetup(bool),
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

/// Which side of a direct-TCP connection the local instance is.
/// Drives the offer/answer symmetry breaker the WebRTC path gets
/// for free from SDP role assignment.
#[derive(Debug, Clone)]
pub enum DirectRole {
    /// Listen on the given port and accept the first inbound peer.
    Host { port: u16 },
    /// Dial the given `host:port` string.
    Connect { addr: String },
}

pub struct ConnectionPayload {
    pub dc: datachannel_wrapper::DataChannel,
    pub peer_conn: datachannel_wrapper::PeerConnection,
}

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

impl std::fmt::Debug for ConnectionPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ConnectionPayload { .. }")
    }
}

/// Output of the negotiate task — the post-handshake sender /
/// receiver (the lobby + match loops own them from here) and the
/// peer-conn handle they need to stay alive against.
pub struct NegotiationOutput {
    pub sender: Arc<tokio::sync::Mutex<crate::net::Sender>>,
    pub receiver: crate::net::Receiver,
    /// `None` for the direct-TCP local transport. See
    /// [`ConnectionHandles::peer_conn`] for the lifetime contract.
    pub peer_conn: Option<datachannel_wrapper::PeerConnection>,
    /// Pre-computed by the per-transport negotiator. WebRTC reads
    /// the SDP type; TCP sets host=true, connect=false.
    pub is_offerer: bool,
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

    /// Reset the cancellation token + bump session_id. Called from
    /// every transition that starts or stops async work so the
    /// background tasks notice and the subscription rekeys. We
    /// replace the per-session Arcs (event-rx slot and post slot)
    /// rather than clearing them, so a dying lobby task from the
    /// previous session can't deposit its receiver into the next
    /// session's slot — it scribbles into the orphaned Arc and
    /// the receiver gets dropped along with the Arc.
    fn cancel_and_renew(&mut self) {
        self.cancel.cancel();
        self.cancel = CancellationToken::new();
        self.session_id = self.session_id.wrapping_add(1);
        self.lobby_event_rx_slot = Arc::new(std::sync::Mutex::new(None));
        self.post_lobby_receiver = Arc::new(std::sync::Mutex::new(None));
        self.conn = None;
        self.lobby = LobbyState::default();
        self.local_commit = None;
        self.remote_commitment = None;
        self.remote_chunks.clear();
        self.local_chunks_sent = false;
    }

    /// Apply a Message. Returns the iced Task to schedule for any
    /// async follow-up.
    pub fn update(&mut self, msg: Message) -> iced::Task<Message> {
        match msg {
            Message::Connect { link_code, endpoint } => {
                self.cancel_and_renew();
                self.phase = Phase::Connecting {
                    ident: LinkIdent::Matchmaking(link_code.clone()),
                    waiting_for_opponent: false,
                };
                let cancel = self.cancel.clone();
                iced::Task::perform(
                    run_signaling_connect(endpoint, link_code, cancel),
                    map_signaling_hello_result,
                )
            }
            Message::ConnectDirect { role } => {
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
                iced::Task::perform(run_tcp_negotiate(role, cancel), map_negotiate_result)
            }
            Message::SignalingHelloReceived(slot_rx) => {
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
            Message::SignalingDone(slot_rx) => {
                let ident = match &self.phase {
                    Phase::Connecting { ident, .. } => ident.clone(),
                    // Cancelled / superseded — late delivery, ignore.
                    _ => return iced::Task::none(),
                };
                let Some(payload) = slot_rx.lock().unwrap().take() else {
                    return iced::Task::none();
                };
                self.phase = Phase::Negotiating { ident };
                let cancel = self.cancel.clone();
                iced::Task::perform(run_negotiate(payload, cancel), map_negotiate_result)
            }
            Message::NegotiationDone(slot_rx) => {
                // Accept both `Connecting` (direct-TCP path: the
                // negotiate is folded into the single TCP task and
                // skips the intermediate Negotiating phase) and
                // `Negotiating` (WebRTC path: signaling +
                // peer-await + negotiate are split stages). Either
                // is a valid Connect-or-Direct-Connect lifecycle
                // that's progressed past the wire handshake.
                let ident = match &self.phase {
                    Phase::Negotiating { ident } => ident.clone(),
                    Phase::Connecting { ident, .. } => ident.clone(),
                    _ => return iced::Task::none(),
                };
                let Some(out) = slot_rx.lock().unwrap().take() else {
                    return iced::Task::none();
                };
                let sender = out.sender.clone();
                self.conn = Some(ConnectionHandles {
                    sender: out.sender,
                    peer_conn: out.peer_conn,
                    is_offerer: out.is_offerer,
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
                let post = self.post_lobby_receiver.clone();
                let receiver = out.receiver;
                tokio::spawn(async move {
                    let receiver = run_lobby_loop(receiver, sender, event_tx, cancel).await;
                    *post.lock().unwrap() = Some(receiver);
                });
                self.phase = Phase::Lobby { ident };
                iced::Task::none()
            }
            Message::PingMeasured(dur) => {
                self.lobby.latency = Some(dur);
                iced::Task::none()
            }
            Message::SendLocalSettings(settings) => {
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
                            .map_err(|e| format!("send_settings: {e}"))
                    },
                    |r| match r {
                        Ok(()) => Message::WireOpDone,
                        Err(e) => Message::Failed(e),
                    },
                );
                iced::Task::batch([invalidate, send])
            }
            Message::WireOpDone => iced::Task::none(),
            Message::RemoteSettings(settings) => {
                // Reveal-status downgrade (peer used to reveal,
                // now doesn't): drop our local commit so we
                // re-commit explicitly under the new visibility
                // contract. Matches the legacy app
                // (gui/play_pane.rs::handle_settings).
                let downgrade = self
                    .lobby
                    .remote
                    .as_ref()
                    .map(|prev| prev.reveal_setup && !settings.reveal_setup)
                    .unwrap_or(false);
                self.lobby.remote = Some(*settings);
                if downgrade {
                    self.invalidate_local_commit()
                } else {
                    iced::Task::none()
                }
            }
            Message::SetMatchType(mt) => {
                self.lobby.match_type = mt;
                // Don't unready here directly — the App fires a
                // settings resend right after this, and
                // SendLocalSettings handles the unready via the
                // material-diff check.
                iced::Task::none()
            }
            Message::SetRevealSetup(v) => {
                let prev = self.lobby.reveal_setup;
                self.lobby.reveal_setup = v;
                // Downgrading our own reveal (true → false): drop
                // the *peer's* commit so they re-commit under the
                // new visibility contract. Matches legacy
                // `set_reveal_setup` in gui/play_pane.rs.
                if prev && !v {
                    self.remote_commitment = None;
                    self.remote_chunks.clear();
                    self.lobby.remote_ready = false;
                    self.lobby.remote_match_ready = false;
                    self.lobby.match_ready = false;
                }
                // App fires a settings resend after this. The
                // SendLocalSettings material-diff check doesn't
                // include reveal_setup, so a same-game reveal
                // toggle doesn't drop our own commit unnecessarily.
                iced::Task::none()
            }
            Message::Commit { save_sram } => self.commit_local(save_sram),
            Message::Uncommit => self.invalidate_local_commit(),
            Message::RemoteCommit(c) => {
                self.remote_commitment = Some(c);
                self.remote_chunks.clear();
                self.lobby.remote_ready = true;
                // First chunk send happens once both sides have
                // committed. Until then we just sit ready.
                self.maybe_kick_chunk_exchange()
            }
            Message::RemoteUncommit => {
                self.remote_commitment = None;
                self.remote_chunks.clear();
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
                    self.remote_chunks.extend_from_slice(&c);
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
            Message::PeerDisconnected => {
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
                self.post_lobby_receiver = Arc::new(std::sync::Mutex::new(None));
                self.conn = None;
                self.local_commit = None;
                self.remote_commitment = None;
                self.remote_chunks.clear();
                self.local_chunks_sent = false;
                self.phase = Phase::Failed {
                    error: "peer-disconnected".to_string(),
                };
                iced::Task::none()
            }
            Message::Disconnect => {
                self.cancel_and_renew();
                self.phase = Phase::Idle;
                iced::Task::none()
            }
        }
    }

    /// Drop the local commitment + reset the related lobby flags.
    /// If we had previously sent a Commit, also fires an Uncommit
    /// packet so the peer doesn't sit waiting for our chunks.
    fn invalidate_local_commit(&mut self) -> iced::Task<Message> {
        let had_commit = self.local_commit.is_some();
        self.local_commit = None;
        self.local_chunks_sent = false;
        self.lobby.local_ready = false;
        self.lobby.match_ready = false;
        if !had_commit {
            return iced::Task::none();
        }
        let Some(sender) = self.conn.as_ref().map(|c| c.sender.clone()) else {
            return iced::Task::none();
        };
        iced::Task::perform(
            async move {
                sender
                    .lock()
                    .await
                    .send_uncommit()
                    .await
                    .map_err(|e| format!("send_uncommit: {e}"))
            },
            |r| match r {
                Ok(()) => Message::WireOpDone,
                Err(e) => Message::Failed(e),
            },
        )
    }

    /// Build a NegotiatedState from a fresh nonce + the local
    /// save's SRAM, zstd-compress it, hash it for the commitment,
    /// send the Commit packet, then kick the chunk exchange if
    /// the peer has already committed.
    fn commit_local(&mut self, save_sram: Vec<u8>) -> iced::Task<Message> {
        if !matches!(self.phase, Phase::Lobby { .. }) {
            return iced::Task::none();
        }
        let Some(sender) = self.conn.as_ref().map(|c| c.sender.clone()) else {
            return iced::Task::none();
        };
        let mut nonce = [0u8; 16];
        rand::Rng::fill(&mut rand::thread_rng(), &mut nonce);
        let state = crate::net::protocol::NegotiatedState {
            nonce,
            save_data: save_sram,
        };
        let bin = match state.serialize() {
            Ok(b) => b,
            Err(e) => {
                return iced::Task::done(Message::Failed(format!("serialize state: {e}")));
            }
        };
        let compressed = match zstd::stream::encode_all(std::io::Cursor::new(&bin), 3) {
            Ok(c) => c,
            Err(e) => {
                return iced::Task::done(Message::Failed(format!("zstd encode: {e}")));
            }
        };
        let commitment = make_commitment(&compressed);
        self.local_commit = Some(LocalCommit { state, compressed });
        self.local_chunks_sent = false;
        self.lobby.local_ready = true;

        let send_commit = iced::Task::perform(
            async move {
                sender
                    .lock()
                    .await
                    .send_commit(commitment)
                    .await
                    .map_err(|e| format!("send_commit: {e}"))
            },
            |r| match r {
                Ok(()) => Message::WireOpDone,
                Err(e) => Message::Failed(e),
            },
        );
        iced::Task::batch([send_commit, self.maybe_kick_chunk_exchange()])
    }

    /// If both sides have committed and we haven't sent our
    /// chunks yet, spawn the chunk-streaming task. Idempotent:
    /// called from both Commit and RemoteCommit handlers, fires
    /// the task exactly once per commit pairing.
    fn maybe_kick_chunk_exchange(&mut self) -> iced::Task<Message> {
        if self.local_chunks_sent || self.local_commit.is_none() || self.remote_commitment.is_none() {
            return iced::Task::none();
        }
        let Some(sender) = self.conn.as_ref().map(|c| c.sender.clone()) else {
            return iced::Task::none();
        };
        let compressed = self.local_commit.as_ref().unwrap().compressed.clone();
        self.local_chunks_sent = true;
        let cancel = self.cancel.clone();
        iced::Task::perform(
            async move {
                // bincode-framed Packet caps at 64 KB; 32 KB
                // payload leaves room for the discriminant +
                // length prefix.
                const CHUNK_SIZE: usize = 32 * 1024;
                for chunk in compressed.chunks(CHUNK_SIZE) {
                    let buf = chunk.to_vec();
                    let sender = sender.clone();
                    let result: std::io::Result<()> = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => return Err(AsyncError::Cancelled),
                        r = async move { sender.lock().await.send_chunk(buf).await } => r,
                    };
                    result.map_err(|e| AsyncError::Failed(format!("send_chunk: {e}")))?;
                }
                // Empty sentinel = end-of-stream.
                sender
                    .lock()
                    .await
                    .send_chunk(Vec::new())
                    .await
                    .map_err(|e| AsyncError::Failed(format!("send_chunk-end: {e}")))?;
                Ok::<(), AsyncError>(())
            },
            |r| match r {
                Ok(()) => Message::WireOpDone,
                Err(e) => map_async_err(e),
            },
        )
    }

    /// Called when the empty-sentinel chunk arrives. Verifies
    /// the peer's commitment matches the hash of the accumulated
    /// chunks, decodes their NegotiatedState (sanity), flips
    /// `match_ready`, then fires StartMatch.
    fn try_finish_handshake(&mut self) -> iced::Task<Message> {
        let Some(remote_commitment) = self.remote_commitment else {
            return iced::Task::done(Message::Failed("peer sent end-of-chunks before Commit".to_string()));
        };
        if self.local_commit.is_none() {
            // Their stream is here but we haven't committed yet —
            // just hold the bytes; finalization runs once we
            // commit + their stream re-finalizes via the
            // duplicate trip through this handler. Easier to
            // just bail until both sides are ready.
            return iced::Task::none();
        }
        let actual = make_commitment(&self.remote_chunks);
        if !bool::from(actual.ct_eq(&remote_commitment)) {
            return iced::Task::done(Message::Failed("peer commitment mismatch".to_string()));
        }
        // Decompress + decode the peer's NegotiatedState. We
        // don't use it for anything until round 6 (PvP session
        // handoff), but verifying that it parses now means we
        // catch wire-format breakage before the user hits Play.
        let peer_state_bytes = match zstd::stream::decode_all(std::io::Cursor::new(&self.remote_chunks)) {
            Ok(b) => b,
            Err(e) => {
                return iced::Task::done(Message::Failed(format!("zstd decode: {e}")));
            }
        };
        if let Err(e) = crate::net::protocol::NegotiatedState::deserialize(&peer_state_bytes) {
            return iced::Task::done(Message::Failed(format!("decode peer state: {e}")));
        }
        self.lobby.match_ready = true;

        let Some(sender) = self.conn.as_ref().map(|c| c.sender.clone()) else {
            return iced::Task::none();
        };
        let send_sm = iced::Task::perform(
            async move {
                sender
                    .lock()
                    .await
                    .send_start_match()
                    .await
                    .map_err(|e| format!("send_start_match: {e}"))
            },
            |r| match r {
                Ok(()) => Message::WireOpDone,
                Err(e) => Message::Failed(e),
            },
        );
        iced::Task::batch([send_sm, self.maybe_signal_pvp_handoff()])
    }

    /// Both sides have sent + received StartMatch — emit the
    /// signal the App listens for to spin up the live match.
    /// No-op until both halves are present.
    fn maybe_signal_pvp_handoff(&mut self) -> iced::Task<Message> {
        if self.lobby.match_ready && self.lobby.remote_match_ready {
            iced::Task::done(Message::MatchHandoffReady)
        } else {
            iced::Task::none()
        }
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
        let local_commit = self.local_commit.take()?;
        let local_settings = self.lobby.local.clone()?;
        let remote_settings = self.lobby.remote.clone()?;
        // Decompress + decode peer's NegotiatedState — we already
        // verified its hash in try_finish_handshake; this is just
        // to recover the nonce + save_data.
        let peer_state_bytes = zstd::stream::decode_all(std::io::Cursor::new(&self.remote_chunks)).ok()?;
        let peer_state = crate::net::protocol::NegotiatedState::deserialize(&peer_state_bytes).ok()?;
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
        let mut rng_seed = [0u8; 16];
        for i in 0..16 {
            rng_seed[i] = local_commit.state.nonce[i] ^ peer_state.nonce[i];
        }
        // Cancel the lobby loop so it returns ownership of the
        // receiver via post_lobby_receiver. The loop drops the
        // receiver into that slot on cancel-exit.
        self.cancel.cancel();
        // The receiver might not be in post_lobby_receiver yet
        // (the loop hasn't observed the cancel) — but the App
        // also takes a clone of the slot Arc and reads
        // asynchronously below.
        let pre_match = PreMatchData {
            sender: handles.sender,
            peer_conn: handles.peer_conn,
            is_offerer: handles.is_offerer,
            receiver_slot: self.post_lobby_receiver.clone(),
            rng_seed,
            local_save_data: local_commit.state.save_data,
            remote_save_data: peer_state.save_data,
            local_settings,
            remote_settings,
            link_code,
            match_type: self.lobby.match_type,
        };
        Some(pre_match)
    }

    /// Clear the lobby snapshot that `take_pre_match` left visible.
    /// Called by the App once `spawn_pvp` resolves (either the PvP
    /// view has taken over, or the build failed and `last_error`
    /// is showing). Idempotent.
    pub fn finish_handoff(&mut self) {
        self.phase = Phase::Idle;
        self.lobby = LobbyState::default();
        self.remote_commitment = None;
        self.remote_chunks.clear();
        self.local_chunks_sent = false;
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
    pub sender: Arc<tokio::sync::Mutex<crate::net::Sender>>,
    /// `None` for the direct-TCP local transport; see
    /// [`ConnectionHandles::peer_conn`].
    pub peer_conn: Option<datachannel_wrapper::PeerConnection>,
    pub is_offerer: bool,
    /// Receiver slot the lobby loop drops into on cancel-exit.
    /// PvP setup waits on this (one-shot poll on a tick).
    pub receiver_slot: Arc<std::sync::Mutex<Option<crate::net::Receiver>>>,
    pub rng_seed: [u8; 16],
    pub local_save_data: Vec<u8>,
    pub remote_save_data: Vec<u8>,
    pub local_settings: crate::net::protocol::Settings,
    pub remote_settings: crate::net::protocol::Settings,
    pub link_code: String,
    pub match_type: (u8, u8),
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

/// `Shake128("tango:lobby:" || buf)` truncated to 16 bytes.
/// Matches the legacy app's commitment construction
/// (`tango/src/net.rs::make_commitment`).
fn make_commitment(buf: &[u8]) -> [u8; 16] {
    use sha3::digest::{ExtendableOutput, Update, XofReader};
    let mut h = sha3::Shake128::default();
    h.update(b"tango:lobby:");
    h.update(buf);
    let mut out = [0u8; 16];
    h.finalize_xof().read(&mut out);
    out
}

/// Subscription that forwards messages from the detached lobby
/// task to the iced event loop. Re-keyed on `session_id` so a
/// fresh Connect tears the previous bridge down; short-circuits
/// to empty when we're not in the lobby phase. The actual loop
/// runs on a `tokio::spawn` task owned by [`NegotiationDone`],
/// so dropping this subscription cannot abort the loop or strand
/// the data-channel receiver.
pub fn subscription(state: &State) -> iced::Subscription<Message> {
    if !matches!(state.phase, Phase::Lobby { .. }) {
        return iced::Subscription::none();
    }
    iced::Subscription::run_with(
        LobbyTag {
            session_id: state.session_id,
            event_rx_slot: state.lobby_event_rx_slot.clone(),
        },
        build_lobby_stream,
    )
}

/// Identity + payload for the lobby subscription. iced 0.14
/// hashes this to decide whether to keep the existing stream or
/// tear it down + restart: only `session_id` is mixed into the
/// hash, so the Arc can change freely without re-keying.
struct LobbyTag {
    session_id: u64,
    event_rx_slot: Arc<std::sync::Mutex<Option<futures::channel::mpsc::UnboundedReceiver<Message>>>>,
}

impl std::hash::Hash for LobbyTag {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        // Tag string + session id only. cancel_and_renew bumps
        // session_id so a fresh Connect re-keys the subscription.
        "netplay-lobby".hash(h);
        self.session_id.hash(h);
    }
}

/// Body of the lobby subscription. Pulled out as a free `fn`
/// because iced 0.14's `run_with` takes a function pointer, not
/// a closure, so the only state available is what comes in
/// through the [`LobbyTag`] argument. Just a passthrough that
/// drains the per-session event channel — owns no transport
/// state, so dropping it is harmless.
fn build_lobby_stream(tag: &LobbyTag) -> impl futures::Stream<Item = Message> {
    use futures::StreamExt;
    let rx = tag.event_rx_slot.lock().unwrap().take();
    match rx {
        Some(rx) => rx.left_stream(),
        // Re-key polled an already-consumed slot. Empty stream
        // until a new session repopulates lobby_event_rx_slot
        // (which only happens behind a fresh session_id, i.e.
        // a re-keyed Subscription anyway).
        None => futures::stream::empty().right_stream(),
    }
}

fn slot<T>(payload: T) -> Slot<T> {
    Arc::new(std::sync::Mutex::new(Some(payload)))
}

/// Distinct error variants for the async tasks so the message
/// handler can tell a user-initiated Disconnect (Cancelled, no
/// UI noise) from a real error (Failed, surface to the user).
enum AsyncError {
    Cancelled,
    Failed(String),
}

fn map_async_err(e: AsyncError) -> Message {
    match e {
        AsyncError::Cancelled => Message::Cancelled,
        AsyncError::Failed(s) => Message::Failed(s),
    }
}

fn map_signaling_hello_result(result: Result<SignalingHello, AsyncError>) -> Message {
    match result {
        Ok(hello) => Message::SignalingHelloReceived(slot(hello)),
        Err(e) => map_async_err(e),
    }
}

fn map_connect_result(result: Result<ConnectionPayload, AsyncError>) -> Message {
    match result {
        Ok(payload) => Message::SignalingDone(slot(payload)),
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
    cancel: CancellationToken,
) -> Result<SignalingHello, AsyncError> {
    let work = async {
        let connecting = tango_signaling::connect(
            &endpoint,
            &link_code,
            // None = let the server pick: STUN by default, TURN
            // when peers can't reach each other directly.
            None,
            PROTOCOL_VERSION,
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
async fn run_await_peer(hello: SignalingHello, cancel: CancellationToken) -> Result<ConnectionPayload, AsyncError> {
    let work = async {
        let (dc, peer_conn) = hello
            .connecting
            .await
            .map_err(|e| AsyncError::Failed(format!("webrtc: {e}")))?;
        Ok::<_, AsyncError>(ConnectionPayload { dc, peer_conn })
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(AsyncError::Cancelled),
        out = work => out,
    }
}

/// Direct-TCP entry: bind-and-accept (host) or dial (connect),
/// then run the same `protocol::negotiate` handshake the WebRTC
/// path uses. No signaling server, no peer-connection — the TCP
/// stream halves carry the whole transport lifetime themselves.
/// `is_offerer` is set from the role (host = true) so the
/// `pick_local_player_index` symmetry break still has a stable
/// asymmetric input.
async fn run_tcp_negotiate(role: DirectRole, cancel: CancellationToken) -> Result<NegotiationOutput, AsyncError> {
    let is_offerer = matches!(role, DirectRole::Host { .. });
    let work = async {
        let (mut sender, mut receiver) = match role {
            DirectRole::Host { port } => crate::net::tcp::host(port)
                .await
                .map_err(|e| AsyncError::Failed(format!("tcp host: {e}")))?,
            DirectRole::Connect { addr } => crate::net::tcp::connect(&addr)
                .await
                .map_err(|e| AsyncError::Failed(format!("tcp connect: {e}")))?,
        };
        crate::net::negotiate(&mut sender, &mut receiver)
            .await
            .map_err(negotiation_error_sentinel)?;
        Ok::<_, AsyncError>(NegotiationOutput {
            sender: Arc::new(tokio::sync::Mutex::new(sender)),
            receiver,
            peer_conn: None,
            is_offerer,
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

/// Split the data channel + run `protocol::negotiate`. Aborts on
/// cancel.
async fn run_negotiate(payload: ConnectionPayload, cancel: CancellationToken) -> Result<NegotiationOutput, AsyncError> {
    let ConnectionPayload { dc, peer_conn } = payload;
    let (mut sender, mut receiver) = crate::net::datachannel::pair(dc);
    let work = crate::net::negotiate(&mut sender, &mut receiver);
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(AsyncError::Cancelled),
        result = work => {
            result.map_err(negotiation_error_sentinel)?;
            let is_offerer = peer_conn
                .local_description()
                .map(|d| d.sdp_type == datachannel_wrapper::SdpType::Offer)
                .unwrap_or(false);
            Ok(NegotiationOutput {
                sender: Arc::new(tokio::sync::Mutex::new(sender)),
                receiver,
                peer_conn: Some(peer_conn),
                is_offerer,
            })
        }
    }
}

/// Lobby background loop: pings every second, reads incoming
/// packets, responds to Ping with Pong, measures Pong RTT. Any
/// other packet kind for now is logged and ignored. Exits
/// cleanly when the cancel token fires; emits `PeerDisconnected`
/// on a clean channel close, `Failed` on a transport error.
///
/// `tx` is an unbounded sender so sends are non-blocking — that's
/// important, because the only awaits in this loop are inside
/// `select!` arm heads (`cancel.cancelled()`, `ping_timer.tick()`,
/// `receiver.receive()`). If sends could block, a stuck consumer
/// would prevent the cancel arm from being re-polled and the
/// task could hang past `cancel.cancel()`.
async fn run_lobby_loop(
    mut receiver: crate::net::Receiver,
    sender: Arc<tokio::sync::Mutex<crate::net::Sender>>,
    tx: futures::channel::mpsc::UnboundedSender<Message>,
    cancel: CancellationToken,
) -> crate::net::Receiver {
    let mut ping_timer = tokio::time::interval(crate::net::PING_INTERVAL);
    // First interval tick fires immediately by default; skip so
    // we don't ping before the peer is ready.
    ping_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_ping_sent: Option<std::time::SystemTime> = None;
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return receiver,
            _ = ping_timer.tick() => {
                let ts = std::time::SystemTime::now();
                last_ping_sent = Some(ts);
                if let Err(e) = sender.lock().await.send_ping(ts).await {
                    log::warn!("lobby: send_ping failed: {e}");
                    let _ = tx.unbounded_send(Message::Failed(format!("ping: {e}")));
                    return receiver;
                }
            }
            packet = receiver.receive() => {
                match packet {
                    Ok(crate::net::protocol::Packet::Ping(p)) => {
                        if let Err(e) = sender.lock().await.send_pong(p.ts).await {
                            log::warn!("lobby: send_pong failed: {e}");
                            let _ = tx.unbounded_send(Message::Failed(format!("pong: {e}")));
                            return receiver;
                        }
                    }
                    Ok(crate::net::protocol::Packet::Pong(p)) => {
                        if let Ok(dt) = std::time::SystemTime::now().duration_since(p.ts) {
                            let _ = tx.unbounded_send(Message::PingMeasured(dt));
                        }
                        let _ = last_ping_sent.take();
                    }
                    Ok(crate::net::protocol::Packet::Settings(s)) => {
                        let _ = tx.unbounded_send(Message::RemoteSettings(Box::new(s)));
                    }
                    Ok(crate::net::protocol::Packet::Commit(c)) => {
                        let _ = tx.unbounded_send(Message::RemoteCommit(c.commitment));
                    }
                    Ok(crate::net::protocol::Packet::Uncommit(_)) => {
                        let _ = tx.unbounded_send(Message::RemoteUncommit);
                    }
                    Ok(crate::net::protocol::Packet::Chunk(c)) => {
                        let _ = tx.unbounded_send(Message::RemoteChunk(c.chunk));
                    }
                    Ok(crate::net::protocol::Packet::StartMatch(_)) => {
                        let _ = tx.unbounded_send(Message::RemoteStartMatch);
                    }
                    Ok(other) => {
                        // Hello (already handled in negotiate) and
                        // Input (only after StartMatch — round 6)
                        // land here today. Logged + ignored so they
                        // don't kill the lobby connection.
                        log::debug!("lobby: ignoring {:?}", std::mem::discriminant(&other));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        log::info!("lobby: peer disconnected (channel closed)");
                        let _ = tx.unbounded_send(Message::PeerDisconnected);
                        return receiver;
                    }
                    Err(e) => {
                        log::warn!("lobby: receive failed: {e}");
                        let _ = tx.unbounded_send(Message::Failed(format!("recv: {e}")));
                        return receiver;
                    }
                }
            }
        }
    }
}

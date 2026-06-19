//! Netplay state + connection lifecycle.
//!
//! Phase transitions: `Idle → Connecting → (handoff) → Idle` (→ `Failed` on
//! error). The lobby brokers the whole match, so once the connection is up the
//! control handshake is a single self-contained async task:
//!
//! ```text
//! Hello(version, settings, commitment) → Chunk… (reveal) → StartMatch
//! ```
//!
//! Both transports — the lobby-relayed [`run_lobby_match`] and the dormant
//! signaling-free [`run_direct_match`] — run that task to completion and hand
//! back a [`PreMatchData`] (the channel handles, both save states, the RNG
//! seed) for the App to spin up a `PvpSession`. There's no in-connection
//! settings/ready negotiation or background loop any more: the live
//! [`CancellationToken`] aborts the in-flight task on a fresh connect, and a
//! late result no-ops because the handler checks `phase`.

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub mod compat;

// 0x47: in-match Input/EndOfRound/EndOfMatch moved off the reliable lobby
// channel onto a separate unreliable channel with the `data::wire` redundancy
// protocol. Incompatible with 0x46 peers, so the version gate rejects them.
// 0x48: the data frame's piggybacked ack is now a signed delta from `base`
// instead of an absolute frontier (smaller on the wire). Incompatible with 0x47.
// 0x49: control handshake collapsed to `Hello(version, settings, commitment) ->
// Chunk… -> StartMatch` — Ping/Pong + the separate Settings/Commit packets are
// gone (the lobby brokers all setup now). Incompatible with 0x48.
pub const PROTOCOL_VERSION: u32 = 0x49;

/// Where the lifecycle is right now. Drives the Play tab's status
/// bar + the Cancel button's visibility.
#[derive(Clone, Debug)]
pub enum Phase {
    /// No connection attempt in flight.
    Idle,
    /// WebRTC bring-up + protocol negotiate task in flight.
    /// `waiting_for_opponent` is true for the passive side (the
    /// direct host blocked on `accept()`, or the lobby answerer
    /// waiting on the peer's offer) so the waiting-screen UI reads
    /// correctly; the active dialer sees plain "Connecting".
    Connecting {
        ident: LinkIdent,
        // Set by both connect paths; read only by the (removed) bottom band's
        // waiting screen. Kept for the direct-connect ready-up re-expose.
        #[allow(dead_code)]
        waiting_for_opponent: bool,
    },
    /// Last attempt failed. Stays here until the user starts a new
    /// connection or clears the field.
    Failed {
        // Read only by the (removed) band's failure banner; the lobby-match
        // path just resets to Idle. Kept for the direct-connect re-expose.
        #[allow(dead_code)]
        error: String,
    },
}

/// Structured identifier for the current connection. Kept in
/// `Phase` across the lifecycle, and also the payload of the
/// play-tab's connect Effect, so consumers (UI header, status
/// line, Discord rich presence, replay filenames) can render or
/// dispatch on the actual structure rather than re-parsing a
/// flat string. `Direct` carries the parsed `DirectRole` describing
/// whether we host or dial; `Lobby` is a presence-driven challenge.
#[derive(Debug, Clone)]
pub enum LinkIdent {
    // The role detail is read only by the (removed) band; kept for the
    // direct-connect re-expose.
    Direct(#[allow(dead_code)] DirectRole),
    /// A lobby-server challenge (presence-driven). No shareable code.
    Lobby,
}

impl LinkIdent {
    /// Discord join-secret for the rich-presence "Ask to Join" /
    /// "Join Party" affordances. Neither transport exposes one: a
    /// direct host listens on its own machine and a lobby challenge
    /// is presence-driven, so there's no internet-reachable code to
    /// deep-link. We surface `None` and Discord hides the button.
    pub fn discord_join_secret(&self) -> Option<&str> {
        match self {
            LinkIdent::Direct(_) | LinkIdent::Lobby => None,
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
    /// Cancellation token shared with the in-flight bring-up task. A new
    /// connect / `Failed` renews it so the prior task's late result no-ops.
    cancel: CancellationToken,
    /// The pending challenge's local proposal settings (match type + blind
    /// flag). Set by the sidebar before challenging; read by the App's
    /// `current_proposal`. Not connection state — just where the proposal
    /// pickers' values live (kept the historical `lobby` name).
    pub lobby: LobbyState,
    /// While a lobby-brokered RTC bring-up is in flight, the channel its task
    /// awaits the peer's relayed SDP on. `feed_lobby_sdp` pushes it here.
    pending_lobby_sdp_tx: Option<tokio::sync::mpsc::Sender<String>>,
    /// A fully-built `PreMatchData` from a finished bring-up, parked for the
    /// App's PvP spawn (`take_pre_match`).
    pending_pre_match: Option<PreMatchData>,
    /// Set once `pending_pre_match` is taken by the App, cleared by
    /// `finish_handoff` — the window while `spawn_pvp` builds the live session.
    /// Gates the loadout strip so a mid-spawn change can't fight it.
    handoff_pending: bool,
}

/// The local match-proposal settings the sidebar edits before issuing a
/// challenge — `current_proposal` folds these into the lobby `MatchProposal`.
/// (Named `LobbyState` historically; it no longer holds connection state — the
/// lobby brokers the match and the handshake collapsed to `Hello`.)
#[derive(Clone, Default)]
pub struct LobbyState {
    /// Match type (mode + subtype) to propose — defaulted to the game's Triple
    /// (where supported) by `App::apply_default_match_type`.
    pub match_type: (u8, u8),
    /// "Blind my setup from the opponent" flag.
    pub blind_setup: bool,
    /// The `(family, variant)` we last defaulted `match_type` for — so the
    /// "default to Triple" fires once per game change and a user's explicit
    /// pick for the same game sticks.
    pub default_mt_for_game: Option<(String, u8)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            cancel: CancellationToken::new(),
            lobby: LobbyState::default(),
            pending_lobby_sdp_tx: None,
            pending_pre_match: None,
            handoff_pending: false,
        }
    }
}

/// Messages the netplay subsystem emits + accepts. App routes
/// these via `Message::Netplay(_)`.
#[derive(Debug, Clone)]
pub enum Message {
    /// Direct local-play entry. Brings up a signaling-free libdatachannel peer
    /// connection whose SDP both sides fabricate from fixed ICE creds (see
    /// [`crate::net::direct_rtc`]) — host pins the UDP port, connect dials it —
    /// then runs the same `Hello → Chunk → StartMatch` flow as the lobby path,
    /// building a `PreMatchData` for the PvP spawn. Carries the local settings +
    /// reveal so the inline flow has everything (no lobby to broker them).
    ///
    /// Dispatched by the sidebar's direct-connect view (see
    /// `App::start_direct`).
    ConnectDirect {
        role: DirectRole,
        local_settings: crate::net::protocol::Settings,
        local_compressed: Vec<u8>,
        match_type: (u8, u8),
    },
    /// A bring-up finished its `Hello → Chunk → StartMatch` exchange; the built
    /// `PreMatchData` is parked for the App's PvP spawn (no lobby screen).
    MatchReady(Slot<PreMatchData>),
    /// Internal: any step (bring-up, negotiate, reveal exchange) failed.
    /// Includes the user-readable error message.
    Failed(String),
    /// Internal: the running async task short-circuited because the
    /// cancellation token fired (a fresh connect superseded us). No-op.
    Cancelled,
    /// Internal: a `PreMatchData` is parked; the App drains `take_pre_match()`
    /// and spins up the live PvpSession.
    MatchHandoffReady,
}

/// Single-take Arc<Mutex<Option<T>>> we use to pass non-Clone /
/// non-Sync payloads through iced's `Task::perform` boundary. The
/// runtime needs `Message: Clone + Send`, and DataChannel /
/// PeerConnection aren't Clone — this wrapper papers over that by
/// taking the inner once on receipt and going None afterwards.
pub type Slot<T> = Arc<std::sync::Mutex<Option<T>>>;

/// Which side of a direct (signaling-free) connection the local
/// instance is. Drives the offer/answer symmetry breaker, and which
/// side pins the UDP port vs. dials it.
#[derive(Debug, Clone)]
pub enum DirectRole {
    /// Pin the given UDP port and accept the first inbound peer.
    Host { port: u16 },
    /// Dial the given `host:port` string.
    Connect { addr: String },
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// Renew the cancellation token (so the prior bring-up task's late result
    /// no-ops) and drop any pending bring-up / handoff state. The proposal
    /// settings (`self.lobby`) persist — they're the sidebar's, not the
    /// connection's.
    fn cancel_and_renew(&mut self) {
        self.cancel.cancel();
        self.cancel = CancellationToken::new();
        self.pending_lobby_sdp_tx = None;
        self.pending_pre_match = None;
        self.handoff_pending = false;
    }

    /// Apply a Message. Returns the iced Task to schedule for any
    /// async follow-up.
    pub fn update(&mut self, msg: Message) -> iced::Task<Message> {
        match msg {
            Message::ConnectDirect {
                role,
                local_settings,
                local_compressed,
                match_type,
            } => self.connect_direct(role, local_settings, local_compressed, match_type),
            Message::MatchReady(slot_rx) => self.on_match_ready(slot_rx),
            Message::MatchHandoffReady => {
                // Pure signal — the App picks it up and pulls pre-match data via
                // take_pre_match. We just re-render here.
                iced::Task::none()
            }
            Message::Failed(e) => {
                self.cancel_and_renew();
                self.phase = Phase::Failed { error: e };
                iced::Task::none()
            }
            Message::Cancelled => iced::Task::none(),
        }
    }

    /// `Message::ConnectDirect` — start the signaling-free direct path, running
    /// the same inline `Hello → Chunk → StartMatch` flow as the lobby path.
    fn connect_direct(
        &mut self,
        role: DirectRole,
        local_settings: crate::net::protocol::Settings,
        local_compressed: Vec<u8>,
        match_type: (u8, u8),
    ) -> iced::Task<Message> {
        self.cancel_and_renew();
        // Host = "waiting for inbound peer" (accept() is the slow await);
        // Connect = "actively dialing".
        let waiting_for_opponent = matches!(role, DirectRole::Host { .. });
        self.phase = Phase::Connecting {
            ident: LinkIdent::Direct(role.clone()),
            waiting_for_opponent,
        };
        let cancel = self.cancel.clone();
        iced::Task::perform(
            run_direct_match(role, local_settings, local_compressed, match_type, cancel),
            map_match_result,
        )
    }

    /// Start a lobby-server-brokered match: bring up the WebRTC connection by
    /// relaying SDP through `lobby` (we offer as the challenger, answer as the
    /// accepter), then feed it into the normal negotiate → lobby → handshake
    /// flow. `feed_lobby_sdp` delivers the peer's relayed SDP into the bring-up.
    pub fn connect_lobby_match(
        &mut self,
        role: crate::net::lobby_rtc::LobbyRole,
        ice_servers: Vec<String>,
        use_relay: Option<bool>,
        lobby: tango_lobby::Lobby,
        peer: tango_lobby::FriendCode,
        local_compressed: Vec<u8>,
        peer_commitment: [u8; 16],
        local_settings: crate::net::protocol::Settings,
        remote_settings: crate::net::protocol::Settings,
        match_type: (u8, u8),
    ) -> iced::Task<Message> {
        self.cancel_and_renew();
        self.phase = Phase::Connecting {
            ident: LinkIdent::Lobby,
            waiting_for_opponent: matches!(role, crate::net::lobby_rtc::LobbyRole::Answerer),
        };
        let cancel = self.cancel.clone();
        let (sdp_tx, sdp_rx) = tokio::sync::mpsc::channel(2);
        self.pending_lobby_sdp_tx = Some(sdp_tx);
        let is_offerer = matches!(role, crate::net::lobby_rtc::LobbyRole::Offerer);
        let send_local_sdp = move |sdp: String| {
            if is_offerer {
                lobby.rtc_offer(&peer, sdp);
            } else {
                lobby.rtc_answer(&peer, sdp);
            }
        };
        iced::Task::perform(
            run_lobby_match(
                role,
                ice_servers,
                use_relay,
                send_local_sdp,
                sdp_rx,
                local_compressed,
                peer_commitment,
                local_settings,
                remote_settings,
                match_type,
                cancel,
            ),
            map_match_result,
        )
    }

    /// Park a finished match's `PreMatchData` and signal handoff — the App's
    /// `MatchHandoffReady` handler then `take_pre_match`es it and spawns PvP.
    fn on_match_ready(&mut self, slot_rx: Slot<PreMatchData>) -> iced::Task<Message> {
        let Some(pre_match) = slot_rx.lock().unwrap().take() else {
            return iced::Task::none();
        };
        self.pending_pre_match = Some(pre_match);
        iced::Task::done(Message::MatchHandoffReady)
    }

    /// Relay a peer SDP (an RtcOffer/RtcAnswer from the lobby) into the in-flight
    /// bring-up. No-op if no lobby connect is pending.
    pub fn feed_lobby_sdp(&self, sdp: String) {
        if let Some(tx) = &self.pending_lobby_sdp_tx {
            let _ = tx.try_send(sdp);
        }
    }


    /// Hand the finished bring-up's `PreMatchData` to the App's PvP spawn. The
    /// bring-up task built it (handles, save states, RNG seed) and parked it via
    /// `MatchReady`; this just drains it and flags the handoff so the loadout
    /// strip locks while `spawn_pvp` runs. `phase` is deliberately left as-is
    /// (cleared by [`finish_handoff`]) so the UI doesn't flash back to idle
    /// mid-spawn.
    pub fn take_pre_match(&mut self) -> Option<PreMatchData> {
        let pre_match = self.pending_pre_match.take()?;
        self.handoff_pending = true;
        Some(pre_match)
    }

    /// Clear the handoff state once `spawn_pvp` resolves (the PvP view has taken
    /// over, or its build failed and `last_error` is showing). Idempotent.
    pub fn finish_handoff(&mut self) {
        self.phase = Phase::Idle;
        self.handoff_pending = false;
    }

    /// True from `take_pre_match` until [`finish_handoff`] — the window while
    /// `spawn_pvp` builds the live session. Gates the loadout strip so a
    /// mid-spawn selection change can't fight it.
    pub fn handoff_pending(&self) -> bool {
        self.handoff_pending
    }
}

/// Everything the App needs to build a PvpSession. Drained
/// from netplay::State after both sides exchanged StartMatch.
pub struct PreMatchData {
    /// Reliable control/lobby channel sender. Idle in-match (all live traffic
    /// is on the unreliable channel), but held open so its close doesn't
    /// surface as a spurious disconnect on the peer's reliable-channel watch.
    pub lobby_sender: Arc<tokio::sync::Mutex<crate::net::Sender>>,
    /// Unreliable in-match channel sender — the live match's `data::wire`
    /// datagrams.
    pub in_match_sender: Arc<tokio::sync::Mutex<crate::net::Sender>>,
    /// The peer connection; brought up by both transports. See
    /// [`ConnectionHandles::peer_conn`].
    pub peer_conn: datachannel_wrapper::PeerConnection,
    pub is_offerer: bool,
    /// Reliable receiver slot the lobby loop drops into on cancel-exit. The PvP
    /// session watches it only for the disconnect signal (the unreliable
    /// datagram channel has no clean close event).
    pub reliable_receiver_slot: Arc<std::sync::Mutex<Option<crate::net::Receiver>>>,
    /// Unreliable in-match receiver slot, parked at negotiate time. PvP setup
    /// waits on this (one-shot poll on a tick).
    pub in_match_receiver_slot: Arc<std::sync::Mutex<Option<crate::net::Receiver>>>,
    pub rng_seed: [u8; 16],
    pub local_save_data: Vec<u8>,
    pub remote_save_data: Vec<u8>,
    pub local_settings: crate::net::protocol::Settings,
    pub remote_settings: crate::net::protocol::Settings,
    pub link_code: String,
    pub match_type: (u8, u8),
}

// The channel/peer-conn handles aren't `Debug`; a placeholder keeps the
// enclosing `Message` (which carries a `Slot<PreMatchData>`) derivable, same as
// `ConnectionPayload`.
impl std::fmt::Debug for PreMatchData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreMatchData { .. }")
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

fn map_match_result(result: Result<PreMatchData, AsyncError>) -> Message {
    match result {
        Ok(pre_match) => Message::MatchReady(slot(pre_match)),
        Err(e) => map_async_err(e),
    }
}

/// Direct signaling-free match: bring up a libdatachannel peer connection whose
/// SDP both sides fabricate from fixed ICE creds (host pins a UDP port; connect
/// dials it), then run the same `Hello → Chunk → StartMatch` flow as the lobby
/// path and build `PreMatchData` for the PvP handoff. No lobby brokers the
/// match here, so the peer's reveal commitment comes from their `Hello`.
/// `is_offerer` is the role (host = true) for the `pick_local_player_index`
/// symmetry break. See [`crate::net::direct_rtc`].
async fn run_direct_match(
    role: DirectRole,
    local_settings: crate::net::protocol::Settings,
    local_compressed: Vec<u8>,
    match_type: (u8, u8),
    cancel: CancellationToken,
) -> Result<PreMatchData, AsyncError> {
    let is_offerer = matches!(role, DirectRole::Host { .. });
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
        } = channels;
        let local_commitment = crate::net::protocol::make_commitment(&local_compressed);
        let peer_hello = crate::net::negotiate(&mut sender, &mut receiver, local_settings.clone(), local_commitment)
            .await
            .map_err(negotiation_error_sentinel)?;
        let remote_compressed =
            exchange_reveal(&mut sender, &mut receiver, &local_compressed, &peer_hello.commitment).await?;
        let local_state =
            decode_state(&local_compressed).map_err(|e| AsyncError::Failed(format!("decode local reveal: {e}")))?;
        let remote_state =
            decode_state(&remote_compressed).map_err(|e| AsyncError::Failed(format!("decode peer reveal: {e}")))?;
        let rng_seed: [u8; 16] = std::array::from_fn(|i| local_state.nonce[i] ^ remote_state.nonce[i]);
        sender
            .send_start_match()
            .await
            .map_err(|e| AsyncError::Failed(format!("send start match: {e}")))?;
        wait_for_start_match(&mut receiver).await?;
        Ok::<_, AsyncError>(PreMatchData {
            lobby_sender: Arc::new(tokio::sync::Mutex::new(sender)),
            in_match_sender: Arc::new(tokio::sync::Mutex::new(in_match_sender)),
            peer_conn,
            is_offerer,
            reliable_receiver_slot: Arc::new(std::sync::Mutex::new(Some(receiver))),
            in_match_receiver_slot: Arc::new(std::sync::Mutex::new(Some(in_match_receiver))),
            rng_seed,
            local_save_data: local_state.save_data,
            remote_save_data: remote_state.save_data,
            local_settings,
            remote_settings: peer_hello.settings,
            link_code: String::new(),
            match_type,
        })
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(AsyncError::Cancelled),
        out = work => out,
    }
}

/// Full lobby-match task: bring up the connection by relaying SDP through the
/// lobby, run the protocol-version `negotiate`, exchange the reveal (verifying
/// the peer's against the lobby-supplied commitment), then build `PreMatchData`
/// for an immediate PvP handoff — no netplay lobby screen.
#[allow(clippy::too_many_arguments)]
async fn run_lobby_match(
    role: crate::net::lobby_rtc::LobbyRole,
    ice_servers: Vec<String>,
    use_relay: Option<bool>,
    send_local_sdp: impl FnOnce(String) + Send + 'static,
    sdp_rx: tokio::sync::mpsc::Receiver<String>,
    local_compressed: Vec<u8>,
    peer_commitment: [u8; 16],
    local_settings: crate::net::protocol::Settings,
    remote_settings: crate::net::protocol::Settings,
    match_type: (u8, u8),
    cancel: CancellationToken,
) -> Result<PreMatchData, AsyncError> {
    let is_offerer = matches!(role, crate::net::lobby_rtc::LobbyRole::Offerer);
    let work = async {
        let crate::net::channel::Channels {
            control: (mut sender, mut receiver),
            in_match: (in_match_sender, in_match_receiver),
            peer_conn,
        } = crate::net::lobby_rtc::bring_up(ice_servers, role, use_relay, send_local_sdp, sdp_rx)
            .await
            .map_err(|e| AsyncError::Failed(format!("lobby rtc: {e}")))?;
        // The lobby already brokered the peer's commitment; require their Hello
        // to present the same one before trusting the reveal exchange, so a peer
        // can't swap in a different reveal than the one they committed to via
        // the lobby. (The reveal is then verified against this commitment too.)
        use subtle::ConstantTimeEq;
        let local_commitment = crate::net::protocol::make_commitment(&local_compressed);
        let peer_hello = crate::net::negotiate(&mut sender, &mut receiver, local_settings.clone(), local_commitment)
            .await
            .map_err(negotiation_error_sentinel)?;
        if !bool::from(peer_hello.commitment.ct_eq(&peer_commitment)) {
            return Err(AsyncError::Failed(
                "peer commitment mismatch (lobby vs handshake)".to_string(),
            ));
        }

        let remote_compressed =
            exchange_reveal(&mut sender, &mut receiver, &local_compressed, &peer_commitment).await?;
        let local_state =
            decode_state(&local_compressed).map_err(|e| AsyncError::Failed(format!("decode local reveal: {e}")))?;
        let remote_state =
            decode_state(&remote_compressed).map_err(|e| AsyncError::Failed(format!("decode peer reveal: {e}")))?;
        let rng_seed: [u8; 16] = std::array::from_fn(|i| local_state.nonce[i] ^ remote_state.nonce[i]);

        sender
            .send_start_match()
            .await
            .map_err(|e| AsyncError::Failed(format!("send start match: {e}")))?;
        wait_for_start_match(&mut receiver).await?;

        Ok::<_, AsyncError>(PreMatchData {
            lobby_sender: Arc::new(tokio::sync::Mutex::new(sender)),
            in_match_sender: Arc::new(tokio::sync::Mutex::new(in_match_sender)),
            peer_conn,
            is_offerer,
            reliable_receiver_slot: Arc::new(std::sync::Mutex::new(Some(receiver))),
            in_match_receiver_slot: Arc::new(std::sync::Mutex::new(Some(in_match_receiver))),
            rng_seed,
            local_save_data: local_state.save_data,
            remote_save_data: remote_state.save_data,
            local_settings,
            remote_settings,
            link_code: String::new(),
            match_type,
        })
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(AsyncError::Cancelled),
        out = work => out,
    }
}

fn decode_state(compressed: &[u8]) -> anyhow::Result<crate::net::protocol::NegotiatedState> {
    let bin = zstd::stream::decode_all(std::io::Cursor::new(compressed))?;
    Ok(crate::net::protocol::NegotiatedState::deserialize(&bin)?)
}

/// Stream our reveal and reassemble the peer's concurrently (so neither blocks
/// on the other's send buffer), then verify the peer's against `peer_commitment`.
async fn exchange_reveal(
    sender: &mut crate::net::Sender,
    receiver: &mut crate::net::Receiver,
    local_compressed: &[u8],
    peer_commitment: &[u8; 16],
) -> Result<Vec<u8>, AsyncError> {
    use subtle::ConstantTimeEq;
    const CHUNK_SIZE: usize = 32 * 1024;
    let send = async {
        for chunk in local_compressed.chunks(CHUNK_SIZE) {
            sender.send_chunk(chunk.to_vec()).await?;
        }
        sender.send_chunk(Vec::new()).await // empty sentinel = end of stream
    };
    let recv = async {
        let mut remote = Vec::new();
        loop {
            match receiver.receive().await? {
                crate::net::protocol::Packet::Chunk(c) if c.chunk.is_empty() => return Ok(remote),
                crate::net::protocol::Packet::Chunk(c) => remote.extend_from_slice(&c.chunk),
                _ => {} // ignore anything else on the wire mid-exchange
            }
        }
    };
    let (send_res, recv_res): (std::io::Result<()>, std::io::Result<Vec<u8>>) = tokio::join!(send, recv);
    send_res.map_err(|e| AsyncError::Failed(format!("send reveal chunk: {e}")))?;
    let remote = recv_res.map_err(|e| AsyncError::Failed(format!("recv reveal chunk: {e}")))?;
    if !bool::from(crate::net::protocol::make_commitment(&remote).ct_eq(peer_commitment)) {
        return Err(AsyncError::Failed("peer commitment mismatch".to_string()));
    }
    Ok(remote)
}

async fn wait_for_start_match(receiver: &mut crate::net::Receiver) -> Result<(), AsyncError> {
    loop {
        match receiver.receive().await {
            Ok(crate::net::protocol::Packet::StartMatch(_)) => return Ok(()),
            Ok(_) => continue,
            Err(e) => return Err(AsyncError::Failed(format!("await start match: {e}"))),
        }
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

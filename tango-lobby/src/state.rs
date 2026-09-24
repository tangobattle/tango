//! The lobby state machine a host drives once a connection is up: the
//! phase, the connection's handles, the settings exchange, and the
//! handoff into a match. The ready handshake, the handoff, and the
//! reconcile policy extend it from their own modules.

use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::handshake::{Handshake, RemoteReady};
use crate::lobby::Command;
use crate::*;

mod handoff;
pub use handoff::HandoffTicket;

pub struct State {
    pub phase: Phase,
    /// Live connection objects, when post-negotiate. Cleared on
    /// disconnect / failure / on the next attempt.
    conn: Option<ConnectionHandles>,
    /// The "ready" commitment exchange — the local + remote ready
    /// ladders. Reset together (`Handshake::default()`) on every
    /// session boundary.
    pub(crate) handshake: Handshake,
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
    pub(crate) commands: Option<futures::channel::mpsc::UnboundedSender<Command>>,
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
    pub latency_counter: tango_net::LatencyCounter,
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
    /// Last family [`State::apply_default_match_type`] applied a
    /// default for (or [`State::pick_match_type`] stamped). Used so that switching families triggers a
    /// re-default, while user-explicit picks within the SAME family
    /// stick — the family is what the picker offers, and the two
    /// versions of one are the same game to a player choosing between
    /// Single and Triple.
    pub default_mt_for_family: Option<String>,
    /// How the transport actually flows, resolved once the wire
    /// handshake completes: direct (peer-to-peer, including the
    /// signaling-free direct link) or relayed through a TURN server.
    /// `None` when it couldn't be determined.
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
            latency_counter: tango_net::LatencyCounter::new(5),
            match_type: (0, 0),
            blind_setup: false,
            default_mt_for_family: None,
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
    sender: Arc<tokio::sync::Mutex<tango_net::Sender>>,
    /// Unreliable, unordered in-match channel sender — idle during the lobby,
    /// handed to the PvP session to carry the live `data::wire` datagrams.
    in_match_sender: tango_net::data::Sender,
    /// Unreliable in-match channel's receive half, parked here the moment the
    /// connection lands (nothing flows on it during the lobby, so — unlike
    /// the reliable receiver — it isn't owned by the pump).
    in_match_receiver: tango_net::data::Receiver,
    /// The reliable receiver, sent by the pump on cancel-exit. One oneshot
    /// per session, so a dying pump from a previous session can't deposit a
    /// stale receiver into the next one.
    post_lobby_rx: tokio::sync::oneshot::Receiver<tango_net::Receiver>,
    /// The peer connection, kept alive for the duration of the session.
    /// Both transports bring one up.
    peer_conn: tango_net::channel::PeerConnection,
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
                // Our StartMatch was predicated on the reveal this
                // commitment replaces, and their re-commit dropped their
                // record of it — so it regresses a rung and gets re-sent
                // once we've verified the reveal that follows. Same
                // regression an Uncommit causes, for the same reason.
                self.handshake.local.revoke_start_match();
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
        // line.
        self.lobby.connection_kind = tango_net::channel::Channels::relayed(&connected.peer_conn).map(|relayed| {
            if relayed {
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
        tango_platform::spawn(async move {
            let receiver = lobby::run_pump(receiver, sender, cmd_rx, progress, cancel).await;
            let _ = post_lobby_tx.send(receiver);
        });
        self.phase = Phase::Lobby { ident };
    }

    /// Queue a wire send. Silently drops when there's no live pump — every
    /// caller is a lobby-phase action, and a stale one has nothing to say.
    pub(crate) fn send(&self, cmd: Command) {
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
}

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

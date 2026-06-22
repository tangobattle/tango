// Some lobby plumbing (a few `MyMatch` reveal fields, helper accessors) is only
// read on certain paths; keep the module quiet rather than peppering it with
// per-item allows.
#![allow(dead_code)]

//! App-level glue to the lobby server (`tango_lobby`): owns the persistent
//! presence connection, mirrors the roster + incoming challenges, and carries
//! the transient sidebar view-state (open profile, status menu, add-by-code
//! draft). The rendering lives in [`view`]; reaching the lobby is non-fatal
//! (the app just shows an offline sidebar).

pub mod view;

use std::sync::Arc;

use futures::StreamExt;
use tango_lobby::{Event, FriendCode, IceServer, Lobby, MatchProposal, Welcome};

/// Lobby wire-protocol version (matches the server's `SERVER_PROTOCOL_VERSION`).
/// 2: Status split into independent `invisible` + `now_playing`.
const LOBBY_PROTOCOL_VERSION: u32 = 2;

/// Non-Clone async payloads threaded through a `Message`, same trick as netplay.
type Slot<T> = Arc<std::sync::Mutex<Option<T>>>;
fn slot<T>(value: T) -> Slot<T> {
    Arc::new(std::sync::Mutex::new(Some(value)))
}

type EventRx = tokio::sync::mpsc::UnboundedReceiver<Event>;

/// Your own presence, IM-client style. Online and Invisible are both
/// *connected* (you see the roster and can challenge); Invisible hides you
/// from everyone else's roster. Offline tears the presence connection down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SelfStatus {
    #[default]
    Online,
    Invisible,
    Offline,
}

pub enum Connection {
    /// No identity loaded — the lobby requires an mTLS cert, so we never dial.
    NoIdentity,
    Connecting,
    /// Live session — carries all the connection-scoped presence state ([`Live`]),
    /// so connect builds it fresh and any disconnect/reconnect just drops it.
    Connected(Live),
    Disconnected(String),
}

/// A challenge someone has offered us, awaiting our accept/decline. Keyed by
/// the challenger (at most one pending per peer), so there's no id.
#[derive(Debug, Clone)]
pub struct IncomingChallenge {
    pub proposal: MatchProposal,
    pub commitment: Vec<u8>,
    /// Monotonic arrival stamp (see [`State::challenge_seq`]) — orders the
    /// challenges list by time received once friends are floated to the top.
    pub seq: u64,
}

/// Our role in a match we're setting up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchRole {
    /// We sent the Challenge — we'll be the WebRTC offerer.
    Challenger,
    /// We accepted a Challenge — we'll be the WebRTC answerer.
    Accepter,
}

/// What's currently occupying us, if anything ([`Live::my_match`]). Its mere
/// presence is what "busy" means — there's no separate in-match flag.
enum MyMatch {
    /// A lobby-brokered match: the challenge negotiation, then — once
    /// `take_match_start` hands it to netplay — the live match. The negotiation
    /// data stays put; `started` just flips so the hand-off fires once and we
    /// keep showing busy through play.
    Lobby(LobbyMatch),
    /// A direct-link match, live now (no lobby negotiation). Carries the
    /// proposal so we can report it as our now-playing to the lobby.
    Direct(MatchProposal),
}

/// The lobby-brokered match we're negotiating (then running). Holds our reveal
/// (streamed peer-to-peer once connected) and, once the peer commits, their
/// commitment + proposal + ICE servers — everything the WebRTC bring-up needs.
struct LobbyMatch {
    peer: FriendCode,
    role: MatchRole,
    nonce: [u8; 16],
    compressed_reveal: Vec<u8>,
    proposal: MatchProposal,
    peer_commitment: Option<[u8; 16]>,
    peer_proposal: Option<MatchProposal>,
    ice_servers: Vec<IceServer>,
    /// Flipped by `take_match_start` once the match is handed to netplay: the
    /// negotiation is done and it's live, so we stay busy but don't re-hand-off.
    started: bool,
}

/// Everything netplay needs to bring up a lobby match, pulled from `MyMatch`
/// once both sides have committed (peer commitment + ICE servers are known).
pub struct LobbyMatchStart {
    pub is_offerer: bool,
    pub peer: FriendCode,
    pub ice_servers: Vec<IceServer>,
    pub local_compressed: Vec<u8>,
    pub peer_commitment: [u8; 16],
    pub local_proposal: MatchProposal,
    pub peer_proposal: MatchProposal,
}

/// Everything that exists only while we hold a live lobby session: our assigned
/// friend code, the roster, the challenges offered to us, and the match we're
/// negotiating or in. Built fresh on connect (and on each resync) and dropped
/// whole when the session ends — so going offline, being displaced, or a
/// reconnect needs no field-by-field reset, it just drops this. (The [`Lobby`]
/// handle and event plumbing live on [`State`] instead: they're the transport,
/// and survive the transparent reconnects this gets rebuilt across.)
pub(crate) struct Live {
    your_friend_code: FriendCode,
    /// Visible roster, keyed by friend code; the value is `now_playing`
    /// (`Some` => in a match). One entry per identity.
    roster: std::collections::BTreeMap<FriendCode, Option<MatchProposal>>,
    /// Challenges offered to us, keyed by the challenger.
    incoming: std::collections::BTreeMap<FriendCode, IncomingChallenge>,
    /// Monotonic stamp handed to each arriving [`IncomingChallenge`] so the
    /// challenges list can sort by time received.
    challenge_seq: u64,
    /// What's occupying us, if anything (a lobby challenge/match or a direct
    /// match). `Some` ⇔ busy.
    my_match: Option<MyMatch>,
}

impl Live {
    /// A fresh session from a welcome / resync: the given roster, nothing else.
    fn new(your_friend_code: FriendCode, roster: Vec<tango_lobby::RosterEntry>) -> Self {
        Self {
            your_friend_code,
            roster: roster.into_iter().map(|e| (e.friend_code, e.now_playing)).collect(),
            incoming: std::collections::BTreeMap::new(),
            challenge_seq: 0,
            my_match: None,
        }
    }
}

pub struct State {
    endpoint: String,
    identity: Option<tango_lobby::ClientIdentity>,
    /// Our own visibility toggle (local; drives what we send via SetStatus).
    invisible: bool,
    /// Connection status; the [`Connection::Connected`] variant owns all the
    /// per-session presence state (see [`Live`]).
    pub connection: Connection,
    /// The live lobby handle — transport, not presence: it survives the
    /// transparent reconnects that rebuild [`Live`], and is dropped only on a
    /// terminal disconnect. `None` whenever we're not / no longer connected.
    handle: Option<Lobby>,
    /// The live event receiver is parked here for the subscription to take;
    /// `epoch` keys the subscription so it respawns on each (re)connect.
    event_rx_slot: Slot<EventRx>,
    epoch: u64,

    // ---- transient sidebar view-state ----
    /// Which peer's profile is open in the sidebar (Telegram-style
    /// master→detail), if any. A `FriendCode` rather than a roster index so it
    /// survives roster churn and works for offline friends + just-typed codes.
    pub open_peer: Option<FriendCode>,
    /// Slide in/out of the open profile. While animating *out* the profile
    /// keeps rendering even though `open_peer` is still set; the view drops it
    /// once this is no longer visible.
    pub profile_vis: crate::anim::Transition,
    /// Whether the self-status menu (online / invisible / offline) is open.
    pub status_menu_open: bool,
    /// Draft of the "find by friend code" bar. Submitting opens that code's
    /// profile so you can nickname it.
    pub add_draft: String,
    /// Whether the overflow (⋮) menu by the find-friend bar is open.
    pub menu_open: bool,
    /// When set, the sidebar shows the direct-connect form (host / join by
    /// address) in place of the roster.
    pub direct_connect: bool,
    /// Draft address for the direct-connect "join" field (`host` or
    /// `host:port`).
    pub direct_addr: String,
    /// Set by the App when a direct bring-up reached a peer but their netplay
    /// settings were incompatible — the direct view shows the localized reason.
    /// The `Verdict` (not a string) so the view localizes at point of use.
    pub direct_error: Option<crate::netplay::compat::Verdict>,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Connected (or reconnected): the handle, the welcome, and the event
    /// receiver. `Slot` because none of these are cheap to move through a
    /// `Clone` message.
    Connected(Slot<(Lobby, Welcome, EventRx)>),
    ConnectFailed(String),
    /// A decoded server event off the live stream.
    Event(Event),
    /// UI: pick your own presence (online / invisible / offline).
    SetSelfStatus(SelfStatus),
    /// UI: challenge a roster player. Intercepted by the App (it builds the
    /// proposal + commitment from the loadout + loaded save).
    IssueChallenge(FriendCode),
    /// UI: accept an incoming challenge. Also App-intercepted.
    AcceptIncoming(FriendCode),
    /// UI: decline an incoming challenge.
    DeclineIncoming(FriendCode),
    /// UI: withdraw our outstanding outgoing challenge.
    CancelChallenge,
    /// UI: open a peer's profile. Carries the code (not a roster index) so it
    /// works for offline friends and just-typed codes too.
    OpenPeer(FriendCode),
    /// UI: close the open profile (animates it out).
    ClosePeer,
    /// UI: open/close the self-status menu.
    ToggleStatusMenu,
    /// UI: edit the "add by friend code" draft.
    AddDraftChanged(String),
    /// UI: submit the add-by-code draft — opens that code's profile to name.
    ConfirmAddContact,
    /// UI: set/clear a peer's local nickname. Intercepted by the App (persists
    /// to `config.friends`); never reaches [`State::update`].
    SetNickname { code: String, name: String },
    /// UI: choose the match type to propose. Intercepted by the App (routes to
    /// `netplay` so `current_proposal` picks it up).
    SetMatchType((u8, u8)),
    /// UI: toggle proposing a blind setup. Also App-intercepted (→ netplay).
    SetBlindSetup(bool),
    /// UI: copy text (a friend code) to the clipboard, lighting the `flash`
    /// copy button's feedback. App-intercepted.
    CopyText { text: String, flash: &'static str },
    /// UI: open/close the overflow (⋮) menu by the find-friend bar.
    ToggleMenu,
    /// UI: enter the direct-connect view (host / join by address).
    OpenDirectConnect,
    /// UI: leave the direct-connect view, back to the roster.
    CloseDirectConnect,
    /// UI: edit the direct-connect join-address draft.
    DirectAddrChanged(String),
    /// UI: start hosting a direct link. App-intercepted — it builds the local
    /// settings + reveal and dispatches netplay's `ConnectDirect`.
    DirectHost,
    /// UI: dial the typed address for a direct link. Also App-intercepted.
    DirectJoin,
    /// UI: abort an in-flight direct bring-up (the connecting screen's Cancel).
    /// App-intercepted — resets netplay to idle.
    CancelDirect,
}

impl State {
    pub fn new(endpoint: String, identity: Option<tango_lobby::ClientIdentity>, initial_status: SelfStatus) -> Self {
        // Restore the user's last presence: Offline stays disconnected until
        // they pick Online/Invisible; otherwise we dial (Invisible just comes up
        // hidden — see `connect`).
        let invisible = initial_status == SelfStatus::Invisible;
        let connection = if initial_status == SelfStatus::Offline {
            Connection::Disconnected("offline".to_string())
        } else if identity.is_some() {
            Connection::Connecting
        } else {
            Connection::NoIdentity
        };
        Self {
            endpoint,
            identity,
            invisible,
            connection,
            handle: None,
            event_rx_slot: Arc::new(std::sync::Mutex::new(None)),
            epoch: 0,
            open_peer: None,
            profile_vis: crate::anim::Transition::new(false),
            status_menu_open: false,
            add_draft: String::new(),
            menu_open: false,
            direct_connect: false,
            direct_addr: String::new(),
            direct_error: None,
        }
    }

    /// The per-session presence state, if we're connected.
    fn live(&self) -> Option<&Live> {
        match &self.connection {
            Connection::Connected(live) => Some(live),
            _ => None,
        }
    }

    fn live_mut(&mut self) -> Option<&mut Live> {
        match &mut self.connection {
            Connection::Connected(live) => Some(live),
            _ => None,
        }
    }

    /// This client's own friend code, once connected.
    pub fn friend_code(&self) -> Option<FriendCode> {
        self.live().map(|l| l.your_friend_code)
    }

    /// A clone of the live lobby handle, if connected.
    pub fn handle(&self) -> Option<Lobby> {
        self.handle.clone()
    }

    /// The roster entry for `fc`: `None` if not connected or not in the roster,
    /// `Some(None)` if present and idle, `Some(Some(p))` if in a match.
    pub fn roster_get(&self, fc: &FriendCode) -> Option<&Option<MatchProposal>> {
        self.live().and_then(|l| l.roster.get(fc))
    }

    /// Friend codes currently in the roster (empty when not connected).
    pub fn roster_codes(&self) -> impl Iterator<Item = FriendCode> + '_ {
        self.live().into_iter().flat_map(|l| l.roster.keys().copied())
    }

    /// The incoming challenge from `fc`, if any.
    pub fn incoming_get(&self, fc: &FriendCode) -> Option<&IncomingChallenge> {
        self.live().and_then(|l| l.incoming.get(fc))
    }

    /// Friend codes who've challenged us (empty when not connected).
    pub fn incoming_codes(&self) -> impl Iterator<Item = FriendCode> + '_ {
        self.live().into_iter().flat_map(|l| l.incoming.keys().copied())
    }

    /// Whether anyone has an incoming challenge waiting on us.
    pub fn has_incoming(&self) -> bool {
        self.live().is_some_and(|l| !l.incoming.is_empty())
    }

    /// Every incoming challenge, by challenger (empty when not connected).
    pub fn incoming_iter(&self) -> impl Iterator<Item = (&FriendCode, &IncomingChallenge)> + '_ {
        self.live().into_iter().flat_map(|l| l.incoming.iter())
    }

    /// Our presence as the status menu should reflect it. Offline *is* the
    /// disconnected state — there's no separate "user signed off" flag, so a
    /// deliberate sign-off and being kicked (displaced / connect failure) read
    /// identically. Intent (Online vs Invisible) only shows through while we're
    /// connected or mid-reconnect (a transient `Connecting` keeps the pick).
    pub fn self_status(&self) -> SelfStatus {
        if matches!(self.connection, Connection::Disconnected(_)) {
            SelfStatus::Offline
        } else if self.invisible {
            SelfStatus::Invisible
        } else {
            SelfStatus::Online
        }
    }

    /// Whether we're occupied with a match the client is tracking: a challenge
    /// in flight (we challenged and are waiting, or we accepted and are
    /// negotiating) *or* a match that's actually live. Drives the busy dot on
    /// our own "You" chip so it matches how peers see us, from the moment we
    /// challenge through the end of the match. Clears when the challenge
    /// resolves without a match (decline / cancel / disconnect) or the match
    /// ends ([`Self::report_idle`]).
    pub fn self_busy(&self) -> bool {
        self.live().is_some_and(|l| l.my_match.is_some())
    }

    /// The peer we have an outstanding *outgoing* challenge to (we're the
    /// challenger and the match hasn't started yet), if any — drives the
    /// "waiting…" + Cancel state in that peer's profile.
    pub fn outgoing_peer(&self) -> Option<FriendCode> {
        match self.live().and_then(|l| l.my_match.as_ref()) {
            Some(MyMatch::Lobby(m)) if m.role == MatchRole::Challenger && !m.started => Some(m.peer),
            _ => None,
        }
    }

    /// Once the peer has committed (ChallengeAccepted/Confirmed filled their
    /// commitment + ICE servers into the lobby match), pull out everything to
    /// start the match. `None` if there's no lobby match ready, or it's already
    /// been handed off. Marks the match `started` (we stay busy through play, but
    /// it won't hand off twice); `report_idle` clears it on match end.
    pub fn take_match_start(&mut self) -> Option<LobbyMatchStart> {
        let live = self.live_mut()?;
        let Some(MyMatch::Lobby(m)) = &mut live.my_match else { return None };
        if m.started {
            return None;
        }
        let peer_commitment = m.peer_commitment?;
        let peer_proposal = m.peer_proposal.clone()?;
        if m.ice_servers.is_empty() {
            return None;
        }
        let start = LobbyMatchStart {
            is_offerer: m.role == MatchRole::Challenger,
            peer: m.peer,
            ice_servers: m.ice_servers.clone(),
            local_compressed: m.compressed_reveal.clone(),
            peer_commitment,
            local_proposal: m.proposal.clone(),
            peer_proposal,
        };
        m.started = true;
        Some(start)
    }

    /// Dial the lobby (no-op without an identity). Kicked once at startup; the
    /// `tango_lobby` driver handles transparent reconnects after that. Comes up
    /// Invisible if that's the user's current pick.
    pub fn connect(&mut self) -> iced::Task<Message> {
        // Offline (Disconnected) — don't dial until the user picks
        // Online/Invisible, which leaves that state first (see `set_self_status`).
        if matches!(self.connection, Connection::Disconnected(_)) {
            return iced::Task::none();
        }
        let Some(identity) = self.identity.clone() else {
            self.connection = Connection::NoIdentity;
            return iced::Task::none();
        };
        self.connection = Connection::Connecting;
        let endpoint = self.endpoint.clone();
        // Fresh dial: visibility from our pick, no now-playing yet (no live
        // session). The driver re-sends the latest status on its own reconnects.
        let status = tango_lobby::Status {
            invisible: self.invisible,
            now_playing: self.match_now_playing(),
        };
        iced::Task::perform(
            async move { tango_lobby::connect(&endpoint, identity, LOBBY_PROTOCOL_VERSION, status).await },
            |result| match result {
                Ok((handle, welcome, rx)) => Message::Connected(slot((handle, welcome, rx))),
                Err(e) => Message::ConnectFailed(format!("{e}")),
            },
        )
    }

    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        let now = iced::time::Instant::now();
        match message {
            Message::Connected(payload) => {
                let Some((handle, welcome, rx)) = payload.lock().unwrap().take() else {
                    return iced::Task::none();
                };
                log::info!("lobby connected as {}", welcome.your_friend_code);
                self.connection =
                    Connection::Connected(Live::new(welcome.your_friend_code, welcome.roster));
                self.handle = Some(handle);
                *self.event_rx_slot.lock().unwrap() = Some(rx);
                // Restart the subscription so it picks up the parked receiver.
                self.epoch += 1;
                iced::Task::none()
            }
            Message::ConnectFailed(e) => {
                log::warn!("lobby connect failed: {e}");
                self.connection = Connection::Disconnected(e);
                self.handle = None;
                self.return_to_roster();
                iced::Task::none()
            }
            Message::Event(event) => {
                self.apply_event(event);
                iced::Task::none()
            }
            Message::SetSelfStatus(status) => self.set_self_status(status),
            Message::DeclineIncoming(peer) => {
                let removed = self.live_mut().is_some_and(|l| l.incoming.remove(&peer).is_some());
                if removed {
                    if let Some(handle) = &self.handle {
                        handle.decline(&peer);
                    }
                }
                iced::Task::none()
            }
            Message::CancelChallenge => {
                self.cancel_outgoing();
                iced::Task::none()
            }
            Message::OpenPeer(peer) => {
                self.open_peer = Some(peer);
                self.status_menu_open = false;
                self.profile_vis.set(true, now);
                iced::Task::none()
            }
            Message::ClosePeer => {
                // Animate out — keep `open_peer` set so the profile keeps
                // rendering while it glides away; the view drops it once the
                // transition is no longer visible.
                self.profile_vis.set(false, now);
                iced::Task::none()
            }
            Message::ToggleStatusMenu => {
                self.status_menu_open = !self.status_menu_open;
                self.menu_open = false;
                iced::Task::none()
            }
            Message::ToggleMenu => {
                self.menu_open = !self.menu_open;
                self.status_menu_open = false;
                iced::Task::none()
            }
            Message::OpenDirectConnect => {
                self.direct_connect = true;
                self.menu_open = false;
                self.direct_error = None;
                // The direct view replaces the list area — drop any open profile
                // so returning lands back on the roster, not a stale card.
                self.open_peer = None;
                self.profile_vis.set(false, now);
                iced::Task::none()
            }
            Message::CloseDirectConnect => {
                self.direct_connect = false;
                self.direct_error = None;
                iced::Task::none()
            }
            Message::DirectAddrChanged(s) => {
                self.direct_addr = s.chars().take(128).collect();
                iced::Task::none()
            }
            Message::AddDraftChanged(s) => {
                // Friend codes are Crockford base32 grouped with hyphens; keep
                // alphanumerics + dashes + spaces (FromStr normalizes the rest).
                self.add_draft = s
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == ' ')
                    .take(64)
                    .collect();
                iced::Task::none()
            }
            Message::ConfirmAddContact => {
                let parsed: Result<FriendCode, _> = self.add_draft.trim().parse();
                if let Ok(peer) = parsed {
                    self.add_draft.clear();
                    self.open_peer = Some(peer);
                    self.status_menu_open = false;
                    self.profile_vis.set(true, now);
                }
                iced::Task::none()
            }
            // App-intercepted (see App::handle_lobby_message): these need the
            // app's loadout / save / config / clipboard, so they never reach
            // here.
            Message::IssueChallenge(_)
            | Message::AcceptIncoming(_)
            | Message::SetNickname { .. }
            | Message::SetMatchType(_)
            | Message::SetBlindSetup(_)
            | Message::CopyText { .. }
            | Message::DirectHost
            | Message::DirectJoin
            | Message::CancelDirect => iced::Task::none(),
        }
    }

    /// The match we're in (lobby or direct), reported to the lobby as our
    /// now-playing. A challenge still negotiating is *not* "now playing" — the
    /// server derives that busy from its own pending state.
    fn match_now_playing(&self) -> Option<MatchProposal> {
        match self.live().and_then(|l| l.my_match.as_ref())? {
            MyMatch::Lobby(m) if m.started => Some(m.proposal.clone()),
            MyMatch::Direct(proposal) => Some(proposal.clone()),
            _ => None,
        }
    }

    /// Push our current presence — visibility plus now-playing — to the lobby.
    /// Visibility and now-playing are orthogonal on the wire, so this is safe to
    /// call on either kind of change; it always sends the full, current state.
    fn sync_status(&self) {
        if let Some(handle) = &self.handle {
            handle.set_status(tango_lobby::Status {
                invisible: self.invisible,
                now_playing: self.match_now_playing(),
            });
        }
    }

    /// Report that we're in a match the lobby didn't broker (a direct link), so
    /// the roster shows us unavailable. Lobby-brokered matches are marked
    /// server-side on accept instead. No-op if not connected (no `Live` to hold
    /// it, and we're hidden anyway); invisibility is preserved by `sync_status`.
    pub fn report_busy(&mut self, proposal: MatchProposal) {
        match self.live_mut() {
            Some(live) => live.my_match = Some(MyMatch::Direct(proposal)),
            None => return,
        }
        self.sync_status();
    }

    /// Clear our now-playing — call when a match ends. The server drops the
    /// busy it derived/recorded; visibility is unchanged.
    pub fn report_idle(&mut self) {
        if let Some(live) = self.live_mut() {
            live.my_match = None;
        }
        self.sync_status();
    }

    /// Apply a self-status pick: toggle visibility live when connected, or
    /// re-dial / tear down when crossing the connected boundary.
    pub fn set_self_status(&mut self, status: SelfStatus) -> iced::Task<Message> {
        self.status_menu_open = false;
        match status {
            SelfStatus::Online | SelfStatus::Invisible => {
                self.invisible = status == SelfStatus::Invisible;
                match &self.connection {
                    // Visibility flips independently of now-playing now, so just
                    // re-push the full status (it carries our current match).
                    Connection::Connected(_) => {
                        self.sync_status();
                        iced::Task::none()
                    }
                    // Re-dial from an offline / failed / never-connected state.
                    // Leave the disconnected state first so `connect` doesn't
                    // treat this as "stay offline" and bail; it comes up at the
                    // chosen visibility (see `connect`).
                    _ => {
                        self.connection = Connection::Connecting;
                        self.connect()
                    }
                }
            }
            SelfStatus::Offline => {
                self.disconnect();
                iced::Task::none()
            }
        }
    }

    /// Tear the presence connection down (deliberate sign-off). Best-effort:
    /// dropping the handle ends our sending + the event subscription; the
    /// server reaps us when the socket closes.
    fn disconnect(&mut self) {
        self.handle = None;
        // Dropping the `Connected` state drops all presence (roster, challenges,
        // in-flight/in match) in one go — no field-by-field reset.
        self.connection = Connection::Disconnected("offline".to_string());
        self.return_to_roster();
        // Rekey the subscription (handle is None ⇒ it goes idle) so a later
        // reconnect spawns a clean one.
        self.epoch = self.epoch.wrapping_add(1);
        *self.event_rx_slot.lock().unwrap() = None;
    }

    /// Snap the sidebar back to the roster list: drop any open peer profile and
    /// close the popovers. Called whenever we drop offline — deliberately or
    /// kicked (displaced / connect failure) — so the user lands on the roster
    /// (which then renders the offline state) instead of being stranded on a
    /// now-meaningless profile card. The direct-connect form is left untouched:
    /// it's signaling-free and stays usable without lobby presence.
    fn return_to_roster(&mut self) {
        self.open_peer = None;
        self.profile_vis.set(false, iced::time::Instant::now());
        self.status_menu_open = false;
        self.menu_open = false;
    }

    /// Begin an outgoing challenge: stash our reveal, send the Challenge.
    /// Called by the App, which built the proposal + commitment.
    pub fn start_outgoing(
        &mut self,
        peer: FriendCode,
        proposal: MatchProposal,
        reveal: crate::net::protocol::LocalReveal,
    ) {
        let commitment = reveal.commitment.to_vec();
        let m = LobbyMatch {
            peer,
            role: MatchRole::Challenger,
            nonce: reveal.nonce,
            compressed_reveal: reveal.compressed,
            proposal: proposal.clone(),
            peer_commitment: None,
            peer_proposal: None,
            ice_servers: Vec::new(),
            started: false,
        };
        let Some(live) = self.live_mut() else { return }; // can't challenge while offline
        live.my_match = Some(MyMatch::Lobby(m));
        if let Some(handle) = &self.handle {
            handle.challenge(&peer, proposal, commitment);
        }
    }

    /// Withdraw our outstanding outgoing challenge — only a still-negotiating
    /// lobby challenge can be cancelled (a live match isn't withdrawn this way).
    pub fn cancel_outgoing(&mut self) {
        let Some(live) = self.live_mut() else { return };
        let peer = match &live.my_match {
            Some(MyMatch::Lobby(m)) if !m.started => m.peer,
            _ => return,
        };
        live.my_match = None;
        if let Some(handle) = &self.handle {
            handle.cancel(&peer);
        }
    }

    /// Accept an incoming challenge: stash our reveal + the peer's commitment,
    /// send the Accept. Called by the App, which built our proposal + commitment.
    pub fn accept_incoming(
        &mut self,
        peer: FriendCode,
        incoming: IncomingChallenge,
        proposal: MatchProposal,
        reveal: crate::net::protocol::LocalReveal,
    ) {
        let commitment = reveal.commitment.to_vec();
        let m = LobbyMatch {
            peer,
            role: MatchRole::Accepter,
            nonce: reveal.nonce,
            compressed_reveal: reveal.compressed,
            proposal: proposal.clone(),
            peer_commitment: incoming.commitment.as_slice().try_into().ok(),
            peer_proposal: Some(incoming.proposal),
            ice_servers: Vec::new(),
            started: false,
        };
        let Some(live) = self.live_mut() else { return };
        live.my_match = Some(MyMatch::Lobby(m));
        live.incoming.remove(&peer);
        if let Some(handle) = &self.handle {
            handle.accept(&peer, proposal, commitment);
        }
    }

    /// Stamp + record an arriving challenge, preserving the original arrival
    /// order if the same peer re-challenges before we've answered.
    fn record_incoming(live: &mut Live, peer: FriendCode, proposal: MatchProposal, commitment: Vec<u8>) {
        let seq = live.incoming.get(&peer).map(|c| c.seq).unwrap_or_else(|| {
            let s = live.challenge_seq;
            live.challenge_seq = live.challenge_seq.wrapping_add(1);
            s
        });
        live.incoming.insert(peer, IncomingChallenge { proposal, commitment, seq });
    }

    fn apply_event(&mut self, event: Event) {
        match event {
            Event::RosterUpsert(entry) => {
                if let Some(live) = self.live_mut() {
                    live.roster.insert(entry.friend_code, entry.now_playing);
                }
            }
            Event::RosterLeave(fc) => {
                if let Some(live) = self.live_mut() {
                    live.roster.remove(&fc);
                    live.incoming.remove(&fc);
                }
            }
            Event::ChallengeIncoming {
                peer,
                proposal,
                commitment,
            } => {
                if let Some(live) = self.live_mut() {
                    Self::record_incoming(live, peer, proposal, commitment);
                }
            }
            Event::ChallengeWithdrawn { peer, .. } => {
                if let Some(live) = self.live_mut() {
                    live.incoming.remove(&peer);
                }
            }
            Event::Resynced {
                your_friend_code,
                roster,
            } => {
                // Reconnected: rebuild a fresh session from the resync snapshot.
                self.connection = Connection::Connected(Live::new(your_friend_code, roster));
            }
            Event::Reconnecting => {
                // The socket dropped: the server tears our challenges down (the
                // old socket closing, or the reconnect re-joining and displacing
                // it), so a pending negotiation can't survive the blip. Dropping
                // the `Connected` state discards it — and the now-stale roster —
                // wholesale; `Resynced` rebuilds fresh. The handle stays: the
                // driver reconnects underneath it.
                self.connection = Connection::Connecting;
            }
            Event::Displaced => {
                log::warn!("lobby: displaced by another connection for this identity");
                // Kicked: drop the handle and the whole session state, and snap
                // back to the roster (which renders the offline notice).
                self.handle = None;
                self.connection = Connection::Disconnected("signed in on another device".to_string());
                self.return_to_roster();
            }
            Event::ChallengeAccepted {
                peer,
                proposal,
                commitment,
                ice_servers,
            } => {
                if let Some(MyMatch::Lobby(m)) = self.live_mut().and_then(|l| l.my_match.as_mut()) {
                    if m.peer == peer && !m.started {
                        m.peer_commitment = commitment.as_slice().try_into().ok();
                        m.peer_proposal = Some(proposal);
                        m.ice_servers = ice_servers;
                        log::info!("{peer} accepted; starting match as offerer");
                    }
                }
            }
            Event::ChallengeConfirmed { peer, ice_servers } => {
                if let Some(MyMatch::Lobby(m)) = self.live_mut().and_then(|l| l.my_match.as_mut()) {
                    if m.peer == peer && !m.started {
                        m.ice_servers = ice_servers;
                        log::info!("{peer} confirmed our accept; starting match as answerer");
                    }
                }
            }
            Event::ChallengeDeclined { peer, .. } => {
                if let Some(live) = self.live_mut() {
                    if matches!(&live.my_match, Some(MyMatch::Lobby(m)) if m.peer == peer && !m.started) {
                        log::info!("{peer} declined our challenge");
                        live.my_match = None;
                    }
                }
            }
            // WebRTC SDP relay is fed straight into the in-flight bring-up by
            // the App (see `handle_lobby_event`); nothing to mirror here.
            Event::RtcOffer { .. } | Event::RtcAnswer { .. } => {}
        }
    }
}

pub fn subscription(state: &State) -> iced::Subscription<Message> {
    if state.handle.is_none() {
        return iced::Subscription::none();
    }
    iced::Subscription::run_with(
        LobbyTag {
            epoch: state.epoch,
            event_rx_slot: state.event_rx_slot.clone(),
        },
        build_event_stream,
    )
}

struct LobbyTag {
    epoch: u64,
    event_rx_slot: Slot<EventRx>,
}

impl std::hash::Hash for LobbyTag {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        "app-lobby".hash(h);
        self.epoch.hash(h);
    }
}

fn build_event_stream(tag: &LobbyTag) -> impl futures::Stream<Item = Message> {
    let rx = tag.event_rx_slot.lock().unwrap().take();
    match rx {
        Some(rx) => futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (Message::Event(event), rx))
        })
        .left_stream(),
        None => futures::stream::empty().right_stream(),
    }
}

// The challenge UI + the challenge -> SDP-relay -> match-start flow are
// follow-up increments, so the collected `incoming` challenge state is still
// write-only; silence dead-code noise at the module level until it lands.
#![allow(dead_code)]

//! App-level glue to the lobby server (`tango_lobby`): owns the persistent
//! presence connection, mirrors the roster, and collects incoming challenges.
//!
//! Additive — this does not touch the existing link-code netplay path. The
//! presence connection runs alongside it; failing to reach the lobby is
//! non-fatal (the app just shows no roster).

use std::sync::Arc;

use futures::StreamExt;
use iced::widget::{button, container, scrollable, text};
use iced::{Element, Fill, Length};
use sweeten::widget::{column, row};
use tango_lobby::{Event, FriendCode, IceServer, Lobby, MatchProposal, Welcome};
use unic_langid::LanguageIdentifier;

use crate::{style, widgets};

/// Lobby wire-protocol version (matches the server's `SERVER_PROTOCOL_VERSION`).
const LOBBY_PROTOCOL_VERSION: u32 = 1;

/// Non-Clone async payloads threaded through a `Message`, same trick as netplay.
type Slot<T> = Arc<std::sync::Mutex<Option<T>>>;
fn slot<T>(value: T) -> Slot<T> {
    Arc::new(std::sync::Mutex::new(Some(value)))
}

type EventRx = tokio::sync::mpsc::UnboundedReceiver<Event>;

#[derive(Debug, Clone)]
pub enum Connection {
    /// No identity loaded — the lobby requires an mTLS cert, so we never dial.
    NoIdentity,
    Connecting,
    Connected { your_friend_code: FriendCode },
    Disconnected(String),
}

/// A challenge someone has offered us, awaiting our accept/decline. Keyed by
/// the challenger (at most one pending per peer), so there's no id.
#[derive(Debug, Clone)]
pub struct IncomingChallenge {
    pub proposal: MatchProposal,
    pub commitment: Vec<u8>,
}

/// Our role in a match we're setting up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchRole {
    /// We sent the Challenge — we'll be the WebRTC offerer.
    Challenger,
    /// We accepted a Challenge — we'll be the WebRTC answerer.
    Accepter,
}

/// The one match we're currently negotiating. Holds our reveal (streamed
/// peer-to-peer once connected) and, once the peer commits, their commitment +
/// proposal + ICE servers — everything the WebRTC bring-up (sub-increment B)
/// needs to start the match.
struct MyMatch {
    peer: FriendCode,
    role: MatchRole,
    nonce: [u8; 16],
    compressed_reveal: Vec<u8>,
    proposal: MatchProposal,
    peer_commitment: Option<[u8; 16]>,
    peer_proposal: Option<MatchProposal>,
    ice_servers: Vec<IceServer>,
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

pub struct State {
    endpoint: String,
    identity: Option<tango_lobby::ClientIdentity>,
    /// Our own visibility toggle (local; drives what we send via SetStatus).
    invisible: bool,
    pub connection: Connection,
    /// Visible roster, keyed by friend code; the value is `now_playing`
    /// (`Some` => in a match). One entry per identity.
    pub roster: std::collections::BTreeMap<FriendCode, Option<MatchProposal>>,
    /// Challenges offered to us, keyed by the challenger.
    pub incoming: std::collections::BTreeMap<FriendCode, IncomingChallenge>,
    /// The match we're currently negotiating (at most one), if any.
    my_match: Option<MyMatch>,
    handle: Option<Lobby>,
    /// The live event receiver is parked here for the subscription to take;
    /// `epoch` keys the subscription so it respawns on each (re)connect.
    event_rx_slot: Slot<EventRx>,
    epoch: u64,
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
    /// UI: toggle our own visibility (Online <-> Invisible).
    SetInvisible(bool),
    /// UI: challenge a roster player. Intercepted by the App (it builds the
    /// proposal + commitment from the loadout + loaded save).
    IssueChallenge(FriendCode),
    /// UI: accept an incoming challenge. Also App-intercepted.
    AcceptIncoming(FriendCode),
    /// UI: decline an incoming challenge.
    DeclineIncoming(FriendCode),
}

impl State {
    pub fn new(endpoint: String, identity: Option<tango_lobby::ClientIdentity>) -> Self {
        let connection = if identity.is_some() {
            Connection::Connecting
        } else {
            Connection::NoIdentity
        };
        Self {
            endpoint,
            identity,
            invisible: false,
            connection,
            roster: std::collections::BTreeMap::new(),
            incoming: std::collections::BTreeMap::new(),
            my_match: None,
            handle: None,
            event_rx_slot: Arc::new(std::sync::Mutex::new(None)),
            epoch: 0,
        }
    }

    /// This client's own friend code, once connected.
    pub fn friend_code(&self) -> Option<FriendCode> {
        match &self.connection {
            Connection::Connected { your_friend_code } => Some(*your_friend_code),
            _ => None,
        }
    }

    /// A clone of the live lobby handle, if connected.
    pub fn handle(&self) -> Option<Lobby> {
        self.handle.clone()
    }

    /// Once the peer has committed (ChallengeAccepted/Confirmed filled their
    /// commitment + ICE servers into `my_match`), pull out everything to start
    /// the match. Clears `my_match`. `None` if no match is ready.
    pub fn take_match_start(&mut self) -> Option<LobbyMatchStart> {
        let m = self.my_match.as_ref()?;
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
        self.my_match = None;
        Some(start)
    }

    /// Dial the lobby (no-op without an identity). Kicked once at startup; the
    /// `tango_lobby` driver handles transparent reconnects after that.
    pub fn connect(&mut self) -> iced::Task<Message> {
        let Some(identity) = self.identity.clone() else {
            self.connection = Connection::NoIdentity;
            return iced::Task::none();
        };
        self.connection = Connection::Connecting;
        let endpoint = self.endpoint.clone();
        iced::Task::perform(
            async move {
                tango_lobby::connect(
                    &endpoint,
                    identity,
                    LOBBY_PROTOCOL_VERSION,
                    tango_lobby::Status::Online,
                )
                .await
            },
            |result| match result {
                Ok((handle, welcome, rx)) => Message::Connected(slot((handle, welcome, rx))),
                Err(e) => Message::ConnectFailed(format!("{e}")),
            },
        )
    }

    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::Connected(payload) => {
                let Some((handle, welcome, rx)) = payload.lock().unwrap().take() else {
                    return iced::Task::none();
                };
                log::info!("lobby connected as {}", welcome.your_friend_code);
                self.connection = Connection::Connected {
                    your_friend_code: welcome.your_friend_code,
                };
                self.replace_roster(welcome.roster);
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
                iced::Task::none()
            }
            Message::Event(event) => {
                self.apply_event(event);
                iced::Task::none()
            }
            Message::SetInvisible(invisible) => {
                self.invisible = invisible;
                if let Some(handle) = &self.handle {
                    handle.set_status(if invisible {
                        tango_lobby::Status::Invisible
                    } else {
                        tango_lobby::Status::Online
                    });
                }
                iced::Task::none()
            }
            Message::DeclineIncoming(peer) => {
                if self.incoming.remove(&peer).is_some() {
                    if let Some(handle) = &self.handle {
                        handle.decline(&peer);
                    }
                }
                iced::Task::none()
            }
            // IssueChallenge / AcceptIncoming need the app's loadout + save, so
            // the App intercepts them (see App::handle_lobby_message) and they
            // never reach here.
            Message::IssueChallenge(_) | Message::AcceptIncoming(_) => iced::Task::none(),
        }
    }

    /// Begin an outgoing challenge: stash our reveal, send the Challenge.
    /// Called by the App, which built the proposal + commitment.
    pub fn start_outgoing(
        &mut self,
        peer: FriendCode,
        proposal: MatchProposal,
        reveal: crate::net::protocol::LocalReveal,
    ) {
        self.my_match = Some(MyMatch {
            peer,
            role: MatchRole::Challenger,
            nonce: reveal.nonce,
            compressed_reveal: reveal.compressed,
            proposal: proposal.clone(),
            peer_commitment: None,
            peer_proposal: None,
            ice_servers: Vec::new(),
        });
        if let Some(handle) = &self.handle {
            handle.challenge(&peer, proposal, reveal.commitment.to_vec());
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
        self.my_match = Some(MyMatch {
            peer,
            role: MatchRole::Accepter,
            nonce: reveal.nonce,
            compressed_reveal: reveal.compressed,
            proposal: proposal.clone(),
            peer_commitment: incoming.commitment.as_slice().try_into().ok(),
            peer_proposal: Some(incoming.proposal),
            ice_servers: Vec::new(),
        });
        if let Some(handle) = &self.handle {
            handle.accept(&peer, proposal, reveal.commitment.to_vec());
        }
        self.incoming.remove(&peer);
    }

    fn replace_roster(&mut self, entries: Vec<tango_lobby::RosterEntry>) {
        self.roster = entries.into_iter().map(|e| (e.friend_code, e.now_playing)).collect();
        self.incoming.clear();
    }

    fn apply_event(&mut self, event: Event) {
        match event {
            Event::RosterUpsert(entry) => {
                self.roster.insert(entry.friend_code, entry.now_playing);
            }
            Event::RosterLeave(fc) => {
                self.roster.remove(&fc);
                self.incoming.remove(&fc);
            }
            Event::ChallengeIncoming {
                peer,
                proposal,
                commitment,
            } => {
                self.incoming.insert(peer, IncomingChallenge { proposal, commitment });
            }
            Event::ChallengeWithdrawn { peer, .. } => {
                self.incoming.remove(&peer);
            }
            Event::Resynced {
                your_friend_code,
                roster,
            } => {
                self.connection = Connection::Connected { your_friend_code };
                self.replace_roster(roster);
            }
            Event::Reconnecting => {
                self.connection = Connection::Connecting;
            }
            Event::Displaced => {
                log::warn!("lobby: displaced by another connection for this identity");
                self.connection = Connection::Disconnected("signed in on another device".to_string());
                self.handle = None;
            }
            Event::ChallengeAccepted {
                peer,
                proposal,
                commitment,
                ice_servers,
            } => {
                if let Some(m) = &mut self.my_match {
                    if m.peer == peer {
                        m.peer_commitment = commitment.as_slice().try_into().ok();
                        m.peer_proposal = Some(proposal);
                        m.ice_servers = ice_servers;
                        // TODO(sub-increment B): start the WebRTC offer here.
                        log::info!("{peer} accepted; ready to start match as offerer (WebRTC bring-up pending)");
                    }
                }
            }
            Event::ChallengeConfirmed { peer, ice_servers } => {
                if let Some(m) = &mut self.my_match {
                    if m.peer == peer {
                        m.ice_servers = ice_servers;
                        // TODO(sub-increment B): start the WebRTC answerer here.
                        log::info!("{peer} confirmed our accept; ready to start match as answerer (WebRTC bring-up pending)");
                    }
                }
            }
            Event::ChallengeDeclined { peer, .. } => {
                if self.my_match.as_ref().map(|m| m.peer) == Some(peer) {
                    log::info!("{peer} declined our challenge");
                    self.my_match = None;
                }
            }
            // WebRTC SDP relay — wired in sub-increment B.
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

// ---- the "Who's around" sidebar (rendered alongside the Play tab) ----

const SIDEBAR_WIDTH: f32 = 280.0;

/// Full-height roster pane: incoming challenges, who's online (with a Challenge
/// button), and a "You" footer with the visibility toggle. Composed at the app
/// level so `tabs/play` stays untouched. `can_challenge` is whether a game +
/// save are loaded (else challenge/accept are disabled).
pub fn sidebar<'a>(state: &'a State, lang: &'a LanguageIdentifier, can_challenge: bool) -> Element<'a, Message> {
    let header = text(crate::t!(lang, "lobby-whos-around")).size(style::TEXT_TITLE);

    let body: Element<'a, Message> = match &state.connection {
        Connection::NoIdentity => hint(crate::t!(lang, "lobby-no-identity")),
        Connection::Connecting => hint(crate::t!(lang, "lobby-connecting")),
        Connection::Disconnected(_) => hint(crate::t!(lang, "lobby-offline")),
        Connection::Connected { .. } => connected_body(state, lang, can_challenge),
    };

    let mut col = column![header, body].spacing(style::PANE_GAP).width(Fill).height(Fill);
    if let Some(me) = state.friend_code() {
        col = col.push(you_footer(state, me, lang));
    }

    container(col)
        .padding(style::PANE_PADDING)
        .width(Length::Fixed(SIDEBAR_WIDTH))
        .height(Fill)
        .style(widgets::pane)
        .into()
}

fn connected_body<'a>(state: &'a State, lang: &'a LanguageIdentifier, can_challenge: bool) -> Element<'a, Message> {
    let mut col = column![].spacing(style::PANE_GAP).width(Fill).height(Fill);

    if !state.incoming.is_empty() {
        col = col.push(text(crate::t!(lang, "lobby-challenges")).size(style::TEXT_HEADING));
        for (peer, inc) in &state.incoming {
            col = col.push(incoming_row(*peer, inc, can_challenge, lang));
        }
    }

    if state.roster.is_empty() {
        col = col.push(hint(crate::t!(lang, "lobby-empty")));
    } else {
        let mut list = column![].spacing(2);
        for (fc, now_playing) in &state.roster {
            let in_match = now_playing.is_some();
            // Can't challenge an in-match player, or someone who's already
            // challenged us.
            let challengeable = can_challenge && !in_match && !state.incoming.contains_key(fc);
            list = list.push(roster_row(*fc, in_match, challengeable, lang));
        }
        col = col.push(scrollable(list).style(widgets::chunky_scrollable).height(Fill));
    }

    col.into()
}

fn roster_row<'a>(fc: FriendCode, in_match: bool, challengeable: bool, lang: &'a LanguageIdentifier) -> Element<'a, Message> {
    let status = if in_match {
        crate::t!(lang, "lobby-in-match")
    } else {
        crate::t!(lang, "lobby-online")
    };
    let info = container(
        column![
            text(fc.to_string()).size(style::TEXT_BODY),
            text(status).size(style::TEXT_CAPTION).style(widgets::muted_text_style),
        ]
        .spacing(2),
    )
    .width(Fill);

    let mut row_el = row![info].spacing(8);
    if challengeable {
        row_el = row_el.push(
            button(text(crate::t!(lang, "lobby-challenge")).size(style::TEXT_CAPTION))
                .padding(style::STANDARD_PADDING)
                .style(widgets::neutral)
                .on_press(Message::IssueChallenge(fc)),
        );
    }
    container(row_el).padding(style::ROW_PADDING).width(Fill).into()
}

fn incoming_row<'a>(
    peer: FriendCode,
    inc: &IncomingChallenge,
    can_challenge: bool,
    lang: &'a LanguageIdentifier,
) -> Element<'a, Message> {
    let accept = button(text(crate::t!(lang, "lobby-accept")).size(style::TEXT_CAPTION))
        .padding(style::STANDARD_PADDING)
        .style(widgets::primary_button);
    // Disabled until a game + save are loaded to accept with.
    let accept = if can_challenge {
        accept.on_press(Message::AcceptIncoming(peer))
    } else {
        accept
    };
    let decline = button(text(crate::t!(lang, "lobby-decline")).size(style::TEXT_CAPTION))
        .padding(style::STANDARD_PADDING)
        .style(widgets::neutral)
        .on_press(Message::DeclineIncoming(peer));

    container(
        column![
            text(peer.to_string()).size(style::TEXT_BODY),
            text(proposal_label(&inc.proposal)).size(style::TEXT_CAPTION).style(widgets::muted_text_style),
            row![accept, decline].spacing(8),
        ]
        .spacing(4),
    )
    .padding(style::ROW_PADDING)
    .width(Fill)
    .style(widgets::pane)
    .into()
}

/// Compact "family · mode-subtype" label for a proposal.
fn proposal_label(p: &MatchProposal) -> String {
    let game = p.game_info.as_ref().map(|g| g.family.as_str()).unwrap_or("?");
    match &p.match_type {
        Some(m) => format!("{game} · {}-{}", m.mode, m.subtype),
        None => game.to_string(),
    }
}

fn you_footer<'a>(state: &'a State, me: FriendCode, lang: &'a LanguageIdentifier) -> Element<'a, Message> {
    let status_word = if state.invisible {
        crate::t!(lang, "lobby-status-invisible")
    } else {
        crate::t!(lang, "lobby-status-online")
    };
    let toggle_label = if state.invisible {
        crate::t!(lang, "lobby-go-online")
    } else {
        crate::t!(lang, "lobby-go-invisible")
    };
    let toggle = button(text(toggle_label).size(style::TEXT_CAPTION))
        .padding(style::STANDARD_PADDING)
        .style(widgets::neutral)
        .on_press(Message::SetInvisible(!state.invisible));

    container(
        column![
            text(crate::t!(lang, "lobby-you")).size(style::TEXT_CAPTION).style(widgets::muted_text_style),
            text(me.to_string()).size(style::TEXT_BODY),
            row![text(status_word).size(style::TEXT_CAPTION), toggle].spacing(8),
        ]
        .spacing(4),
    )
    .padding(style::ROW_PADDING)
    .width(Fill)
    .style(widgets::pane)
    .into()
}

fn hint<'a>(message: String) -> Element<'a, Message> {
    text(message).size(style::TEXT_CAPTION).style(widgets::muted_text_style).into()
}

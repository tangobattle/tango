//! Netplay, as this frontend drives it.
//!
//! The state machine — connection choreography, settings exchange, ready
//! handshake, match handoff — lives in the [`tango_netplay`] crate, which
//! knows nothing about iced. This module re-exports its surface (so
//! `crate::netplay::*` keeps resolving) and supplies the two pieces that
//! are genuinely iced-shaped:
//!
//! * [`run`]: turns the crate's [`Effect`] into an `iced::Task`.
//! * [`subscription`]: bridges the detached lobby loop's event channel
//!   into the update loop.
//!
//! [`identity`] also stays here: it presents a client certificate as the
//! matchmaking websocket's mTLS credential, which a browser cannot do at
//! all — so a web frontend needs a different scheme rather than a port of
//! this one.

pub mod identity;

pub use tango_netplay::{
    compat, randomcode, ConnectionKind, DirectRole, Effect, Error, LinkIdent, LobbyState, Message, Phase, PreMatchData,
    ReadyView, Slot, State,
};

use std::sync::Arc;
use tango_netplay::Step;

/// Run an [`Effect`] as an iced `Task`.
///
/// Every step's message goes back into [`State::update`], which is what
/// the `iced::Task` this replaced used to do implicitly.
pub fn run(effect: Effect) -> iced::Task<Message> {
    iced::Task::batch(effect.into_steps().into_iter().map(|step| match step {
        Step::Emit(msg) => iced::Task::done(msg),
        Step::Spawn(work) => iced::Task::perform(work, |msg| msg),
    }))
}

/// Subscription that forwards messages from the detached lobby task to
/// the iced event loop. Re-keyed on the state's session id so a fresh
/// Connect tears the previous bridge down; short-circuits to empty when
/// we're not in the lobby phase. The actual loop runs on a `tokio::spawn`
/// task owned by the crate, so dropping this subscription cannot abort
/// the loop or strand the data-channel receiver.
pub fn subscription(state: &State) -> iced::Subscription<Message> {
    if !matches!(state.phase, Phase::Lobby { .. }) {
        return iced::Subscription::none();
    }
    iced::Subscription::run_with(
        LobbyTag {
            session_id: state.session_id(),
            events: Arc::new(std::sync::Mutex::new(state.take_lobby_events())),
        },
        build_lobby_stream,
    )
}

/// Identity + payload for the lobby subscription. iced 0.14 hashes this
/// to decide whether to keep the existing stream or tear it down +
/// restart: only `session_id` is mixed into the hash, so the payload can
/// change freely without re-keying.
struct LobbyTag {
    session_id: u64,
    events: Arc<std::sync::Mutex<Option<futures::channel::mpsc::UnboundedReceiver<Message>>>>,
}

impl std::hash::Hash for LobbyTag {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        // Tag string + session id only. A fresh Connect bumps the
        // session id, which re-keys the subscription.
        "netplay-lobby".hash(h);
        self.session_id.hash(h);
    }
}

/// Body of the lobby subscription. Pulled out as a free `fn` because
/// iced 0.14's `run_with` takes a function pointer, not a closure, so the
/// only state available is what comes in through the [`LobbyTag`]
/// argument. Just a passthrough that drains the per-session event
/// channel — owns no transport state, so dropping it is harmless.
fn build_lobby_stream(tag: &LobbyTag) -> impl futures::Stream<Item = Message> {
    use futures::StreamExt;
    match tag.events.lock().unwrap().take() {
        Some(rx) => rx.left_stream(),
        // Re-key polled an already-consumed slot. Empty stream until a
        // new session installs another channel (which only happens
        // behind a fresh session id, i.e. a re-keyed Subscription
        // anyway).
        None => futures::stream::empty().right_stream(),
    }
}

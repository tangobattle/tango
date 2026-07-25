//! Netplay, as this frontend drives it.
//!
//! The state machine — connection choreography, settings exchange, ready
//! handshake, match handoff — lives in the [`tango_netplay`] crate, which
//! knows nothing about iced. This module re-exports its surface (so
//! `crate::netplay::*` keeps resolving) and supplies the three pieces
//! that are genuinely iced-shaped:
//!
//! * [`connect`] / [`connect_direct`]: run a bring-up as an `iced::Task`.
//! * [`subscription`]: bridge the connection's progress channel into the
//!   update loop.
//! * [`Delivery`]: iced routes messages by value and demands `Clone`;
//!   what comes down that channel owns a live data channel and can't be.
//!
//! [`identity`] also stays here: it presents a client certificate as the
//! matchmaking websocket's mTLS credential, which a browser cannot do at
//! all — so a web frontend needs a different scheme rather than a port of
//! this one.

pub mod identity;

pub use tango_netplay::{
    compat, randomcode, ConnectionKind, DirectRole, Error, Event, Incoming, LinkIdent, LobbyState, MatchmakingParams,
    Phase, PreMatchData, ReadyView, State,
};

use std::sync::Arc;

/// Start a matchmaking attempt and run it to wherever it lands. The task
/// resolves to nothing: the bring-up reports its own progress — including
/// its own failure — down the channel [`subscription`] is draining.
pub fn connect(state: &mut State, params: MatchmakingParams) -> iced::Task<crate::app::Message> {
    let (cancel, progress) = state.begin_matchmaking(&params);
    iced::Task::future(tango_netplay::connect(params, cancel, progress)).discard()
}

/// Start a direct (signaling-free) attempt. See [`connect`].
pub fn connect_direct(state: &mut State, role: DirectRole) -> iced::Task<crate::app::Message> {
    let (cancel, progress) = state.begin_direct(&role);
    iced::Task::future(tango_netplay::connect_direct(role, cancel, progress)).discard()
}

/// One item off the connection's progress channel, wrapped so it can ride
/// an `iced::Message`. iced requires messages be `Clone`, and an
/// [`Incoming`] can carry a live data channel — so it travels in a
/// once-take cell and the app hands it straight to
/// [`State::apply`](tango_netplay::State::apply). That tax belongs here,
/// in the iced-shaped layer, rather than in the crate every frontend
/// shares.
#[derive(Clone)]
pub struct Delivery(Arc<std::sync::Mutex<Option<Incoming>>>);

impl Delivery {
    /// Take the payload. `None` on a re-delivery — nothing to apply.
    pub fn take(&self) -> Option<Incoming> {
        self.0.lock().unwrap().take()
    }
}

impl std::fmt::Debug for Delivery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Delivery { .. }")
    }
}

/// Subscription that forwards the connection's reports into the iced
/// event loop. Re-keyed on the state's session id so a fresh attempt
/// tears the previous bridge down; empty while idle, when there's no
/// connection to report anything. The tasks doing the reporting are
/// owned by the crate, so dropping this subscription can't abort them or
/// strand the data-channel receiver.
pub fn subscription(state: &State) -> iced::Subscription<Delivery> {
    if matches!(state.phase, Phase::Idle | Phase::Failed { .. }) {
        return iced::Subscription::none();
    }
    iced::Subscription::run_with(
        ProgressTag {
            session_id: state.session_id(),
            incoming: Arc::new(std::sync::Mutex::new(state.take_incoming())),
        },
        build_progress_stream,
    )
}

/// Identity + payload for the progress subscription. iced 0.14 hashes
/// this to decide whether to keep the existing stream or tear it down +
/// restart: only `session_id` is mixed into the hash, so the payload can
/// change freely without re-keying.
struct ProgressTag {
    session_id: u64,
    incoming: Arc<std::sync::Mutex<Option<futures::channel::mpsc::UnboundedReceiver<Incoming>>>>,
}

impl std::hash::Hash for ProgressTag {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        // Tag string + session id only. A fresh attempt bumps the session
        // id, which re-keys the subscription.
        "netplay-progress".hash(h);
        self.session_id.hash(h);
    }
}

/// Body of the progress subscription. Pulled out as a free `fn` because
/// iced 0.14's `run_with` takes a function pointer, not a closure, so the
/// only state available is what comes in through the [`ProgressTag`]
/// argument. Just a passthrough that drains the per-session channel —
/// owns no transport state, so dropping it is harmless.
fn build_progress_stream(tag: &ProgressTag) -> impl futures::Stream<Item = Delivery> {
    use futures::StreamExt;
    match tag.incoming.lock().unwrap().take() {
        Some(rx) => rx
            .map(|i| Delivery(Arc::new(std::sync::Mutex::new(Some(i)))))
            .left_stream(),
        // Re-key polled an already-consumed slot. Empty stream until a
        // new session installs another channel (which only happens
        // behind a fresh session id, i.e. a re-keyed Subscription
        // anyway).
        None => futures::stream::empty().right_stream(),
    }
}

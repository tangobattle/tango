//! Netplay, as this frontend drives it.
//!
//! The choreography — connection bring-up, settings exchange, the
//! commit/reveal ready handshake, the handoff into a live match — is all
//! [`tango_lobby`], unmodified. That crate was written so a host
//! supplies a spawner and a pump rather than an architecture: bringing a
//! connection up is one linear future, and the lobby is a synchronous
//! state machine you call methods on. So what's here is the spawner
//! (`spawn_local`), the pump (a task draining the progress channel), and
//! the one genuinely frontend-shaped step — turning a `PreMatchData`
//! into a booted session, which on a desktop means a blocking thread and
//! here means a yield and a few seconds of the main thread.

use std::cell::RefCell;

use futures::StreamExt as _;

use tango_lobby::{compat, Event, LinkIdent, MatchmakingParams, Phase, State};
use tango_net_protocol::control as protocol;

use crate::loadout::Loadout;

#[derive(Clone)]
pub struct Handle {
    state: std::rc::Rc<RefCell<Link>>,
    library: crate::library::Handle,
    engine: crate::engine::Handle,
}
impl Handle {
    pub fn new(library: crate::library::Handle, engine: crate::engine::Handle) -> Self {
        Self {
            state: Default::default(),
            library,
            engine,
        }
    }
    fn read<R>(&self, f: impl FnOnce(&Link) -> R) -> R {
        f(&self.state.borrow())
    }
    fn update<R>(&self, f: impl FnOnce(&mut Link) -> R) -> R {
        f(&mut self.state.borrow_mut())
    }
}

#[derive(Default)]
struct Link {
    net: State,
    /// What we're bringing. Pushed in by the UI whenever the pick
    /// changes to build Settings. A handoff uses the committed settings
    /// and snapshot from the lobby, independent of later UI changes.
    loadout: Loadout,
    nickname: String,
    /// The session is being built — between both StartMatch packets and
    /// the first emulated frame. Seconds of priming, so it gets its own
    /// visible state rather than a frozen lobby.
    starting: bool,
}

/// How the lobby reads to the UI. A flat, comparable projection: the
/// real state isn't `Clone` and half of it is live channels.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Snapshot {
    pub phase: PhaseView,
    pub link_code: String,
    pub local_ready: bool,
    pub remote_ready: bool,
    pub match_ready: bool,
    pub starting: bool,
    /// Opponent's nickname, once their Settings have arrived.
    pub opponent: Option<String>,
    /// What they're bringing, as a label.
    pub opponent_game: Option<String>,
    pub verdict: Option<Verdict>,
    pub latency_ms: Option<u32>,
    pub relayed: Option<bool>,
    pub match_type: (u8, u8),
    pub error: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum PhaseView {
    #[default]
    Idle,
    /// Dialing the server, or (once `true`) sitting in the room waiting
    /// for the other side to turn up.
    Connecting {
        waiting_for_opponent: bool,
    },
    Negotiating,
    Lobby,
    Failed,
}

/// [`compat::Verdict`], flattened for display. The fetchable case keeps
/// its payload because the UI acts on it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Verdict {
    Compatible,
    MissingGame,
    MissingRom,
    Fetching { name: String },
    DifferentVersions,
    SimVersionTooOld,
    SimVersionTooNew,
    DifferentMatchTypes,
}

pub fn snapshot(handle: &Handle) -> Snapshot {
    handle.read(|link| {
        let (phase, link_code, error) = match &link.net.phase {
            Phase::Idle => (PhaseView::Idle, String::new(), None),
            Phase::Connecting {
                ident,
                waiting_for_opponent,
            } => (
                PhaseView::Connecting {
                    waiting_for_opponent: *waiting_for_opponent,
                },
                ident_code(ident),
                None,
            ),
            Phase::Negotiating { ident } => (PhaseView::Negotiating, ident_code(ident), None),
            Phase::Lobby { ident } => (PhaseView::Lobby, ident_code(ident), None),
            Phase::Failed { error } => (PhaseView::Failed, String::new(), Some(describe(error))),
        };
        let ready = link.net.ready_view();
        let remote = link.net.lobby.remote.as_ref();
        Snapshot {
            phase,
            link_code,
            local_ready: ready.local_ready,
            remote_ready: ready.remote_ready,
            match_ready: ready.match_ready,
            starting: link.starting || link.net.handoff_pending(),
            opponent: remote.map(|s| s.nickname.clone()),
            opponent_game: remote.and_then(|s| s.game_info.as_ref()).map(describe_game_info),
            verdict: verdict(handle, link),
            // `latest`, not `median`: the lobby line is a live readout,
            // and it is `Option` — an empty counter means no Pong has
            // come back yet, which is not the same as 0 ms.
            latency_ms: link.net.lobby.latency_counter.latest().map(|d| d.as_millis() as u32),
            relayed: link
                .net
                .lobby
                .connection_kind
                .map(|k| k == tango_lobby::ConnectionKind::Relayed),
            match_type: link.net.lobby.match_type,
            error,
        }
    })
}

fn ident_code(ident: &LinkIdent) -> String {
    ident.matchmaking_code().unwrap_or_default().to_owned()
}

fn describe(error: &tango_lobby::Error) -> String {
    use tango_lobby::Error as E;
    match error {
        E::PeerDisconnected => "Opponent disconnected.".into(),
        E::SignalingVersionTooOld => "This Tango is too old for the matchmaking server. Reload to update.".into(),
        E::SignalingVersionTooNew => "The matchmaking server is out of date for this Tango.".into(),
        E::SignalingRejected(reason) => format!("The matchmaking server refused the connection: {reason}"),
        E::SignalingUnreachable(inner) => format!("Couldn't reach the matchmaking server: {inner}"),
        E::Signaling(inner) => format!("Matchmaking failed: {inner}"),
        E::PeerConnection(inner) => format!("Couldn't connect to the opponent: {inner}"),
        E::NegotiateExpectedHello => "The other side didn't speak Tango.".into(),
        E::NegotiateVersionTooOld => "Their Tango is too old for this one.".into(),
        E::NegotiateVersionTooNew => "Their Tango is newer — update this one.".into(),
        E::Negotiate(inner) => format!("Handshake failed: {inner}"),
        e @ (E::Channels(_)
        | E::Direct { .. }
        | E::Transport { .. }
        | E::EncodeState { .. }
        | E::CommitmentMismatch
        | E::DecodePeerState { .. }
        | E::DuplicateChunkStart
        | E::RevealOverrun { .. }
        | E::SessionBuild(_)) => e.to_string(),
    }
}

fn describe_game_info(info: &protocol::GameInfo) -> String {
    let (family, variant) = &info.family_and_variant;
    // Off the wire, so it may name a game this build has no support
    // for — `game_name_of` falls back to the raw pair rather than
    // pretending not to know what they said.
    let base = crate::lang::game_name_of(family, *variant);
    match &info.patch {
        Some(patch) => format!("{base} · {} {}", patch.name, patch.version),
        None => base,
    }
}

/// Run the real compatibility check — the one the desktop runs, over the
/// same ROM map and patch catalog. Doing it any other way is how a
/// matchup that can't work reaches the ready button.
fn verdict(handle: &Handle, link: &Link) -> Option<Verdict> {
    let verdict = crate::library::with(&handle.library, |library| {
        link.net
            .verdict(|local, remote| library.catalog.compatibility_facts(local, remote))
    })??;
    Some(match verdict {
        compat::Verdict::Compatible => Verdict::Compatible,
        compat::Verdict::MissingGame => Verdict::MissingGame,
        compat::Verdict::MissingRom => Verdict::MissingRom,
        // Fetchable, and we do fetch it — see `after_state_change`.
        compat::Verdict::MissingPatch { name, .. } => Verdict::Fetching { name },
        compat::Verdict::DifferentVersions => Verdict::DifferentVersions,
        compat::Verdict::SimVersionTooOld => Verdict::SimVersionTooOld,
        compat::Verdict::SimVersionTooNew => Verdict::SimVersionTooNew,
        compat::Verdict::DifferentMatchTypes => Verdict::DifferentMatchTypes,
    })
}

// ---------------------------------------------------------------------
// Actions

/// Dial a link code. The bring-up reports its own progress, including
/// its own failure, so there is nothing to route back here.
pub fn connect(handle: &Handle, link_code: String, nickname: String) {
    let Some(endpoint) = crate::library::with(&handle.library, |library| {
        library.config.borrow().matchmaking_endpoint.clone()
    }) else {
        return;
    };
    let params = MatchmakingParams {
        link_code,
        endpoint,
        // Let ICE pick: direct when the peers can reach each other,
        // TURN when they can't.
        use_relay: None,
    };
    let (cancel, progress) = handle.update(|link| {
        link.nickname = nickname;
        link.net.begin_matchmaking(&params)
    });
    let incoming = handle.read(|link| link.net.take_incoming());
    wasm_bindgen_futures::spawn_local(tango_lobby::connect(params, cancel, progress));
    if let Some(mut incoming) = incoming {
        let owned = handle.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let handle = &owned;
            while let Some(report) = incoming.next().await {
                let event = handle.update(|link| link.net.apply(report));
                match event {
                    Some(Event::MatchReady) => {
                        let owned = handle.clone();
                        wasm_bindgen_futures::spawn_local(async move { start_match(&owned).await });
                    }
                    None => {}
                }
                after_state_change(handle);
            }
        });
    }
}

pub fn disconnect(handle: &Handle) {
    handle.update(|link| {
        link.net.disconnect();
        link.starting = false;
    });
}

/// The user picked a different game / save / patch. Both a settings
/// resend and the record the handoff will build the match from.
pub fn set_loadout(handle: &Handle, loadout: Loadout) {
    let changed = handle.update(|link| {
        if link.loadout == loadout {
            return false;
        }
        link.loadout = loadout;
        true
    });
    if changed {
        after_state_change(handle);
    }
}

/// The user picked a match type. Remembered per family in the config,
/// so coming back to a family offers the mode it was last played in.
pub fn set_match_type(handle: &Handle, match_type: (u8, u8)) {
    let family = handle.update(|link| {
        let family = link.loadout.game().map(|g| g.family_and_variant().0);
        link.net.pick_match_type(family, match_type);
        family
    });
    if let Some(family) = family {
        crate::library::remember_match_type(&handle.library, family, match_type);
    }
    // The resend's material-difference check does the auto-unready, so
    // it deliberately isn't done here.
    after_state_change(handle);
}

/// The lobby's default match-type policy
/// ([`State::apply_default_match_type`]) for the game we're bringing,
/// with this family's remembered pick.
fn apply_default_match_type(handle: &Handle) {
    let Some(game) = handle.read(|link| link.loadout.game()) else {
        return;
    };
    let family = game.family_and_variant().0;
    let remembered = crate::library::with(&handle.library, |library| {
        library.config.borrow().last_match_type_per_family.get(family).copied()
    })
    .flatten();
    handle.update(|link| {
        link.net
            .apply_default_match_type(family, game.family.match_types, remembered)
    });
}

/// Press or un-press Ready. Pressing commits to a hash of the save we're
/// bringing; the reveal follows once the peer has committed too, which
/// is what stops either side picking a save in response to the other's.
pub fn set_ready(handle: &Handle, ready: bool) {
    if !ready {
        handle.update(|link| link.net.uncommit());
        return;
    }
    let Some(save) = handle.read(|link| crate::loadout::save_bytes(&link.loadout, &handle.library)) else {
        log::warn!("netplay: ready with no save loaded");
        return;
    };
    // No session payload: this frontend embeds no save view, so there
    // is nothing to have picked one.
    let event = handle.update(|link| link.net.commit(save));
    if let Some(Event::MatchReady) = event {
        let owned = handle.clone();
        wasm_bindgen_futures::spawn_local(async move { start_match(&owned).await });
    }
}

/// Everything that has to happen after anything moves — a report from
/// the connection, a change of pick, a match-type tap. The lobby's own
/// follow-up ([`State::reconcile`]) is idempotent, which is what lets
/// this hang off every transition rather than being threaded through
/// each one: it makes sure the peer has our current Settings (also how
/// they get sent on lobby entry — without it both sides sit on
/// "Waiting…" forever), unreadies us if the matchup stopped being
/// compatible, and names the patch to go and get if that's all that's
/// missing.
fn after_state_change(handle: &Handle) {
    apply_default_match_type(handle);
    let missing = crate::library::with(&handle.library, |library| {
        handle.update(|link| {
            let nickname = link.nickname.clone();
            let game_info = link.loadout.game_info();
            link.net.reconcile(
                |lobby| protocol::Settings {
                    nickname,
                    match_type: lobby.match_type,
                    game_info,
                    // No blind-setup toggle: this build has no save viewer
                    // to blind, so there is nothing for the flag to hide.
                    blind_setup: false,
                },
                |local, remote| library.catalog.compatibility_facts(local, remote),
            )
        })
    })
    .flatten();
    let Some((name, version)) = missing else { return };
    let owned = handle.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let handle = &owned;
        log::info!("netplay: fetching {name} {version} for this matchup");
        if let Err(e) = crate::library::install_patch(&handle.library, name, version).await {
            log::warn!("netplay: patch fetch failed: {e}");
        }
    });
}

// ---------------------------------------------------------------------
// Handoff

/// Both sides sent StartMatch: drain the lobby into a `PreMatchData` and
/// build the live match from it.
async fn start_match(handle: &Handle) {
    let pending = handle.update(|link| {
        let handoff = link.net.take_pre_match()?;
        link.starting = true;
        Some(handoff)
    });
    let Some((ticket, pre_match)) = pending else { return };

    let outcome = build(handle, ticket, pre_match).await;
    handle.update(|link| {
        if link.net.is_current(ticket) {
            link.starting = false;
        }
        link.net.complete_handoff(ticket, outcome);
    });
}

async fn build(
    handle: &Handle,
    ticket: tango_lobby::HandoffTicket,
    pre_match: tango_lobby::PreMatchData,
) -> Result<(), String> {
    let prepared = crate::library::with(&handle.library, |library| {
        library
            .catalog
            .resolver(&library.files, &library.config.borrow())
            .prepare_match(
                &pre_match.terms.local_settings,
                &pre_match.terms.remote_settings,
                [&pre_match.terms.local_save_data, &pre_match.terms.remote_save_data],
            )
    })
    .ok_or_else(|| "library not open".to_owned())?
    .map_err(|e| e.to_string())?;
    let local = tango_session::pvp::Seat {
        game: prepared.local.prepared.game,
        sram: prepared.local.match_sram(),
        rom: prepared.local.rom,
    };
    let remote = tango_session::pvp::Seat {
        game: prepared.remote.prepared.game,
        sram: prepared.remote.match_sram(),
        rom: prepared.remote.rom,
    };
    let sink = handle.engine.audio_sink().await;
    if !handle.read(|link| link.net.is_current(ticket)) {
        return Ok(());
    }
    let (session, driver, stream) = tango_session::pvp::PvpSession::new(tango_session::pvp::PvpSessionArgs {
        local,
        remote,
        pre_match,
        frame_delay: frame_delay(handle),
        disable_bgm: false,
        // Recorded into storage rather than a file — see
        // `crate::recording`. This host does not persist stats sidecars.
        replays: Some(&crate::recording::BrowserReplayStore(handle.library.clone())),
        stats_sink: None,
        sample_rate: crate::audio::sample_rate(),
    })
    .await
    .map_err(|e| e.to_string())?;

    if !handle.read(|link| link.net.is_current(ticket)) {
        return Ok(());
    }
    // Priming the pair to a live link battle is seconds of emulation
    // with no thread to put it on — it happens on the first pumped
    // tick, under a session that is already up and reporting it
    // (`PvpSession::is_booting`).
    crate::engine::start_pvp(&handle.engine, session, driver, stream, sink);
    Ok(())
}

/// Local display lag, in frames. Suggested from the lobby's smoothed
/// ping the first time — a phone on mobile data has a very different
/// answer here than a desktop on ethernet, and asking the user to guess
/// is worse than guessing for them.
fn frame_delay(handle: &Handle) -> u32 {
    // The median rather than the latest, so one spike doesn't set the
    // whole match's display lag. It reads `ZERO` when no Pong has come
    // back yet, which is the "we don't know" case, not a 0 ms link.
    let median = handle.read(|link| link.net.lobby.latency_counter.median());
    tango_session::pvp::initial_frame_delay(median, tango_session::pvp::DEFAULT_FRAME_DELAY)
}

/// A fresh random link code, in the user's language, for the "make me
/// one" button.
pub fn random_code() -> String {
    tango_lobby::randomcode::generate(&tango_library::lang::FALLBACK_LANG)
}

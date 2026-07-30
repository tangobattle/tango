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

use tango_library::game;
use tango_net_protocol::control as protocol;
use tango_lobby::{compat, Event, LinkIdent, MatchmakingParams, Phase, State};

use crate::loadout::Loadout;

thread_local! {
    static LINK: RefCell<Link> = RefCell::new(Link::default());
}

#[derive(Default)]
struct Link {
    net: State,
    /// What we're bringing. Pushed in by the UI whenever the pick
    /// changes; used both to build the Settings packet and, at handoff,
    /// to build the local side of the match.
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
    DifferentMatchTypes,
}

pub fn snapshot() -> Snapshot {
    LINK.with(|l| {
        let link = l.borrow();
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
            verdict: verdict(&link),
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
    match ident {
        LinkIdent::Matchmaking(code) => code.clone(),
        LinkIdent::Direct(_) => String::new(),
    }
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
        E::Other(inner) => inner.clone(),
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
fn verdict(link: &Link) -> Option<Verdict> {
    if !matches!(link.net.phase, Phase::Lobby { .. }) {
        return None;
    }
    let (local, remote) = (link.net.lobby.local.as_ref()?, link.net.lobby.remote.as_ref()?);
    crate::library::with(|library| {
        let roms = library.roms.read();
        let catalog = library.patches.read();
        match compat::check(local, remote, &roms, &catalog) {
            compat::Verdict::Compatible => Verdict::Compatible,
            compat::Verdict::MissingGame => Verdict::MissingGame,
            compat::Verdict::MissingRom => Verdict::MissingRom,
            // Fetchable, and we do fetch it — see `fetch_missing_patch`.
            compat::Verdict::MissingPatch { name, .. } => Verdict::Fetching { name },
            compat::Verdict::DifferentVersions => Verdict::DifferentVersions,
            compat::Verdict::DifferentMatchTypes => Verdict::DifferentMatchTypes,
        }
    })
}

// ---------------------------------------------------------------------
// Actions

/// Dial a link code. The bring-up reports its own progress, including
/// its own failure, so there is nothing to route back here.
pub fn connect(link_code: String, nickname: String) {
    let Some(endpoint) = crate::library::with(|library| library.config.matchmaking_endpoint.clone()) else {
        return;
    };
    let params = MatchmakingParams {
        link_code,
        endpoint,
        // Let ICE pick: direct when the peers can reach each other,
        // TURN when they can't.
        use_relay: None,
    };
    let (cancel, progress) = LINK.with(|l| {
        let mut link = l.borrow_mut();
        link.nickname = nickname;
        link.net.begin_matchmaking(&params)
    });
    let incoming = LINK.with(|l| l.borrow().net.take_incoming());
    wasm_bindgen_futures::spawn_local(tango_lobby::connect(params, cancel, progress));
    if let Some(mut incoming) = incoming {
        wasm_bindgen_futures::spawn_local(async move {
            while let Some(report) = incoming.next().await {
                let event = LINK.with(|l| l.borrow_mut().net.apply(report));
                match event {
                    Some(Event::MatchReady) => wasm_bindgen_futures::spawn_local(start_match()),
                    None => {}
                }
                after_state_change();
            }
        });
    }
}

pub fn disconnect() {
    LINK.with(|l| {
        let mut link = l.borrow_mut();
        link.net.disconnect();
        link.starting = false;
    });
}

/// The user picked a different game / save / patch. Both a settings
/// resend and the record the handoff will build the match from.
pub fn set_loadout(loadout: Loadout) {
    let changed = LINK.with(|l| {
        let mut link = l.borrow_mut();
        if link.loadout == loadout {
            return false;
        }
        link.loadout = loadout;
        true
    });
    if changed {
        after_state_change();
    }
}

pub fn set_match_type(match_type: (u8, u8)) {
    LINK.with(|l| l.borrow_mut().net.set_match_type(match_type));
    // The resend's material-difference check does the auto-unready, so
    // it deliberately isn't done here.
    after_state_change();
}

/// Default the match type to Triple where the game has one — that is
/// what people actually play, and both sides have to agree on it before
/// either can ready up, so defaulting to the less-used mode costs every
/// pair a negotiation.
///
/// Re-defaults when the game changes but leaves an explicit pick for the
/// *same* game alone, which is what `default_mt_for_game` remembers. Also
/// repairs a pick the current game doesn't have — match-type tables
/// differ per family, so a pick carried over from another game can be out
/// of range.
fn apply_default_match_type() {
    LINK.with(|l| {
        let mut link = l.borrow_mut();
        let Some(game) = link.loadout.game else { return };
        // Entry `i` is how many subtypes mode `i` has; mode 1 is Triple.
        let table = game.match_types;
        let (family, variant) = game.family_and_variant();
        let key = (family.to_string(), variant);

        let game_changed = link.net.lobby.default_mt_for_game.as_ref() != Some(&key);
        let (mode, subtype) = link.net.lobby.match_type;
        let in_range = table
            .get(mode as usize)
            .is_some_and(|subtypes| (subtype as usize) < *subtypes);
        if !game_changed && in_range {
            return;
        }
        link.net.lobby.match_type = if table.get(1).copied().unwrap_or(0) > 0 {
            (1, 0)
        } else {
            (0, 0)
        };
        link.net.lobby.default_mt_for_game = Some(key);
    });
}

/// Push the current pick to the peer. A no-op outside the lobby, and
/// deduped against the last value sent, so it is safe to call from any
/// state change — which is exactly how it gets sent on lobby entry,
/// where there is no user action to hang it off.
fn push_settings() {
    let settings = LINK.with(|l| {
        let link = l.borrow();
        protocol::Settings {
            nickname: link.nickname.clone(),
            match_type: link.net.lobby.match_type,
            game_info: link.loadout.game_info(),
            // No blind-setup toggle: this build has no save viewer to
            // blind, so there is nothing for the flag to hide.
            blind_setup: false,
        }
    });
    LINK.with(|l| l.borrow_mut().net.send_local_settings(settings));
}

/// Press or un-press Ready. Pressing commits to a hash of the save we're
/// bringing; the reveal follows once the peer has committed too, which
/// is what stops either side picking a save in response to the other's.
pub fn set_ready(ready: bool) {
    if !ready {
        LINK.with(|l| l.borrow_mut().net.uncommit());
        return;
    }
    let Some(save) = LINK.with(|l| l.borrow().loadout.save_bytes()) else {
        log::warn!("netplay: ready with no save loaded");
        return;
    };
    // No session payload: this frontend embeds no save view, so there
    // is nothing to have picked one.
    let event = LINK.with(|l| l.borrow_mut().net.commit(save, None));
    if let Some(Event::MatchReady) = event {
        wasm_bindgen_futures::spawn_local(start_match());
    }
}

/// Everything that has to happen after anything moves — a report from
/// the connection, a change of pick, a match-type tap. Two things, both
/// idempotent, which is what lets this hang off every transition rather
/// than being threaded through each one:
///
/// * make sure the peer has our current Settings (this is also how they
///   get sent on lobby entry, where there is no user action to hang it
///   off — and without it both sides sit on "Waiting…" forever);
/// * if the matchup needs a patch we don't have, go and get it. The
///   verdict resolves from the index, so we know the matchup would be
///   playable before the package is anywhere near this device.
fn after_state_change() {
    apply_default_match_type();
    push_settings();

    let missing = LINK.with(|l| {
        let link = l.borrow();
        if !matches!(link.net.phase, Phase::Lobby { .. }) {
            return None;
        }
        let (local, remote) = (link.net.lobby.local.as_ref()?, link.net.lobby.remote.as_ref()?);
        crate::library::with(|library| {
            let roms = library.roms.read();
            let catalog = library.patches.read();
            compat::check(local, remote, &roms, &catalog)
                .fetchable()
                .map(|(name, version)| (name.to_string(), version.clone()))
        })?
    });
    let Some((name, version)) = missing else { return };
    wasm_bindgen_futures::spawn_local(async move {
        log::info!("netplay: fetching {name} {version} for this matchup");
        if let Err(e) = crate::library::install_patch(name, version).await {
            log::warn!("netplay: patch fetch failed: {e}");
        }
    });
}

// ---------------------------------------------------------------------
// Handoff

/// Both sides sent StartMatch: drain the lobby into a `PreMatchData` and
/// build the live match from it.
async fn start_match() {
    let pre_match = LINK.with(|l| l.borrow_mut().net.take_pre_match());
    let Some(pre_match) = pre_match else { return };
    LINK.with(|l| l.borrow_mut().starting = true);

    let outcome = build(pre_match).await;
    LINK.with(|l| {
        let mut link = l.borrow_mut();
        link.starting = false;
        match outcome {
            Ok(()) => link.net.finish_handoff(),
            Err(e) => {
                log::error!("netplay: building the match failed: {e}");
                link.net.fail_session_build(tango_lobby::Error::Other(e));
            }
        }
    });
}

async fn build(pre_match: tango_lobby::PreMatchData) -> Result<(), String> {
    let loadout = LINK.with(|l| l.borrow().loadout.clone());
    let local_game = loadout.game.ok_or_else(|| "no game selected".to_string())?;
    let local_rom = loadout.rom()?;

    // The match runs the peer's game here too — the pair is simulated in
    // full on both sides — so their ROM has to be one we have, with
    // their patch applied to it.
    let remote_info = pre_match
        .remote_settings
        .game_info
        .clone()
        .ok_or_else(|| "opponent sent no game info".to_string())?;
    let (family, variant) = &remote_info.family_and_variant;
    let remote_game = game::find_by_family_and_variant(family, *variant)
        .ok_or_else(|| format!("unknown opponent game {family} v{variant}"))?;
    let remote_patch = remote_info.patch.as_ref().map(|p| (p.name.clone(), p.version.clone()));
    let remote_rom = crate::library::patched_rom(remote_game, remote_patch.as_ref())?;

    let sink = crate::audio::sink().await;
    let (session, boot) = tango_session::pvp::PvpSession::new(tango_session::pvp::PvpSessionArgs {
        local_game,
        local_rom: std::sync::Arc::new(local_rom),
        remote_game,
        remote_rom: std::sync::Arc::new(remote_rom),
        pre_match,
        frame_delay: frame_delay(),
        disable_bgm: false,
        // Recorded into storage rather than a file — see
        // `crate::recording`. The stats sidecar has no browser
        // counterpart and is compiled out on wasm entirely.
        replays: Some(&crate::recording::BrowserReplayStore),
        expected_fps: local_game.pvp.frame_timing().fps() as f32,
        sample_rate: crate::audio::sample_rate(),
    })
    .await
    .map_err(|e| e.to_string())?;

    // Booting primes the pair to a live link battle — seconds of
    // emulation, and there is no thread to put it on. Yield first so the
    // "Starting match…" state actually reaches the screen before the
    // main thread goes away.
    tango_session::platform::sleep(std::time::Duration::from_millis(32)).await;
    let (driver, stream) = boot.boot().map_err(|e| e.to_string())?;

    crate::engine::start_pvp(session, driver, stream, sink);
    Ok(())
}

/// Local display lag, in frames. Suggested from the lobby's smoothed
/// ping the first time — a phone on mobile data has a very different
/// answer here than a desktop on ethernet, and asking the user to guess
/// is worse than guessing for them.
fn frame_delay() -> u32 {
    // The median rather than the latest, so one spike doesn't set the
    // whole match's display lag. It reads `ZERO` when no Pong has come
    // back yet, which is the "we don't know" case, not a 0 ms link.
    let suggested = LINK.with(|l| {
        let median = l.borrow().net.lobby.latency_counter.median();
        (!median.is_zero()).then(|| tango_session::pvp::suggest_frame_delay(median))
    });
    suggested
        .unwrap_or(2)
        .clamp(tango_session::pvp::MIN_FRAME_DELAY, tango_session::pvp::MAX_FRAME_DELAY)
}

/// A fresh random link code, in the user's language, for the "make me
/// one" button.
pub fn random_code() -> String {
    tango_lobby::randomcode::generate(&tango_library::lang::FALLBACK_LANG)
}

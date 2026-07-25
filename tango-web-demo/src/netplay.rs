//! Netplay from a browser tab: the lobby, and the live match it hands off to.
//!
//! Same choreography the desktop runs, because it's the same crate —
//! [`tango_netplay`] brings the connection up, exchanges settings, runs
//! the ready/commitment handshake, and produces a `PreMatchData`. What
//! this file supplies is the two things a host owes it: somewhere to
//! spawn the bring-up (`spawn_local`), and something to pump its reports
//! (the page, once a frame).
//!
//! Everything the page can see is a string or a bool. A lobby has a lot
//! of state, and none of it needs to cross the wasm boundary as
//! structure just to be rendered as a status line.

use tango_netplay::{Event, LinkIdent, MatchmakingParams, Phase};
use wasm_bindgen::prelude::*;

use crate::{Demo, GAMES};

/// A connection attempt and, if it gets that far, the lobby it lands in.
///
/// Held by the page across frames: it calls [`Netplay::poll`] to advance
/// it, reads [`Netplay::phase`] / [`Netplay::status`] to render it, and
/// finally [`Netplay::start_match`] to trade it in for a running match.
#[wasm_bindgen]
pub struct Netplay {
    state: tango_netplay::State,
    /// This attempt's reports. Taken out of the state on first poll —
    /// re-taken whenever a fresh attempt installs a new one, which the
    /// session id identifies.
    incoming: Option<futures::channel::mpsc::UnboundedReceiver<tango_netplay::Incoming>>,
    session_id: u64,
    /// What the match will be built from once the handshake completes.
    /// The lobby announces the game to the peer and commits to the save,
    /// so both are needed before there's a match to speak of.
    rom: std::sync::Arc<Vec<u8>>,
    game: &'static tango_gamesupport::Game,
    save_sram: Vec<u8>,
    nickname: String,
    sample_rate: u32,
    /// True once both sides have exchanged StartMatch — the page's cue
    /// to call [`Netplay::start_match`].
    match_ready: bool,
}

#[wasm_bindgen]
impl Netplay {
    /// Dial `endpoint` with `link_code` — whoever else types the same
    /// code is the opponent.
    ///
    /// `rom` and `save` are this side's match setup: the ROM (with any
    /// patch already applied) and a raw SRAM dump. The save is what gets
    /// committed to and revealed, so the peer's client can build its
    /// mirror of this side.
    pub fn connect(
        endpoint: String,
        link_code: String,
        nickname: String,
        rom: Vec<u8>,
        save: Vec<u8>,
        sample_rate: u32,
    ) -> Result<Netplay, JsValue> {
        let game = tango_gamesupport::detect(&GAMES, &rom)
            .ok_or_else(|| JsValue::from_str("this ROM isn't a game Tango knows"))?;
        let mut state = tango_netplay::State::new();
        let params = MatchmakingParams {
            link_code,
            endpoint,
            // Let ICE pick: direct when the peers can reach each other,
            // TURN when they can't.
            use_relay: None,
            // A browser can't present a client certificate on its
            // websocket, so it dials without an install identity. The
            // server treats that as an anonymous client.
            identity: None,
        };
        let (cancel, progress) = state.begin_matchmaking(&params);
        // The bring-up is a plain future; a browser's runtime spawns it
        // the way a browser spawns futures. It reports its own failure,
        // so there's nothing to await here.
        tango_session::platform::spawn(tango_netplay::connect(params, cancel, progress));
        Ok(Netplay {
            state,
            incoming: None,
            session_id: u64::MAX,
            rom: std::sync::Arc::new(rom),
            game,
            save_sram: save,
            nickname,
            sample_rate,
            match_ready: false,
        })
    }

    /// Drain whatever the connection has reported since the last call.
    /// The page calls this once a frame; it never blocks.
    ///
    /// Returns `true` while there's still something to poll — `false`
    /// once the attempt has ended, which is the page's cue to stop.
    pub fn poll(&mut self) -> bool {
        // A fresh attempt installs a fresh channel behind a fresh
        // session id; pick it up on the first poll that sees one.
        if self.session_id != self.state.session_id() {
            self.incoming = self.state.take_incoming();
            self.session_id = self.state.session_id();
        }
        let mut was_lobby = matches!(self.state.phase, Phase::Lobby { .. });
        while let Some(Some(incoming)) = self.incoming.as_mut().map(|rx| rx.try_next().ok().flatten()) {
            if let Some(Event::MatchReady) = self.state.apply(incoming) {
                self.match_ready = true;
            }
            // Announce ourselves the moment the lobby opens, and again
            // whenever a report might have invalidated what the peer
            // knows. `send_local_settings` dedupes, so this is cheap.
            let is_lobby = matches!(self.state.phase, Phase::Lobby { .. });
            if is_lobby && !was_lobby {
                self.announce();
            }
            was_lobby = is_lobby;
        }
        !matches!(self.state.phase, Phase::Idle | Phase::Failed { .. })
    }

    /// Tell the peer who we are and what we're bringing.
    fn announce(&mut self) {
        let (family, variant) = (self.game.family.to_string(), self.game.variant);
        self.state.send_local_settings(tango_net_protocol::control::Settings {
            nickname: self.nickname.clone(),
            match_type: self.state.lobby.match_type,
            game_info: Some(tango_net_protocol::control::GameInfo {
                family_and_variant: (family, variant),
                // The demo plays vanilla ROMs: whatever the page loaded
                // is what runs, with no patch registry to name.
                patch: None,
            }),
            blind_setup: false,
        });
    }

    /// Where the lifecycle is, as a word the page can switch on:
    /// `idle`, `connecting`, `waiting`, `negotiating`, `lobby`,
    /// `failed`.
    pub fn phase(&self) -> String {
        match &self.state.phase {
            Phase::Idle => "idle",
            Phase::Connecting {
                waiting_for_opponent: false,
                ..
            } => "connecting",
            Phase::Connecting { .. } => "waiting",
            Phase::Negotiating { .. } => "negotiating",
            Phase::Lobby { .. } => "lobby",
            Phase::Failed { .. } => "failed",
        }
        .to_string()
    }

    /// A human-readable line for whatever's on screen: the failure when
    /// there is one, the opponent and the ready state once there's a
    /// lobby, and the link code before that.
    pub fn status(&self) -> String {
        match &self.state.phase {
            Phase::Idle => "not connected".to_string(),
            Phase::Connecting {
                ident,
                waiting_for_opponent,
            } => {
                let code = link_code(ident);
                if *waiting_for_opponent {
                    format!("waiting for an opponent on {code}")
                } else {
                    format!("connecting to the matchmaking server as {code}")
                }
            }
            Phase::Negotiating { .. } => "checking we speak the same protocol".to_string(),
            Phase::Lobby { .. } => self.lobby_line(),
            Phase::Failed { error } => format!("failed: {}", describe(error)),
        }
    }

    fn lobby_line(&self) -> String {
        let ready = self.state.ready_view();
        let opponent = match &self.state.lobby.remote {
            Some(s) => {
                let game = s
                    .game_info
                    .as_ref()
                    .map(|g| format!("{}-{}", g.family_and_variant.0, g.family_and_variant.1))
                    .unwrap_or_else(|| "no game".to_string());
                format!("{} ({game})", s.nickname)
            }
            None => "opponent (still announcing)".to_string(),
        };
        let latency = match self.state.lobby.latency_counter.median() {
            d if d.is_zero() => String::new(),
            d => format!(" — {} ms", d.as_millis()),
        };
        let you = if ready.match_ready {
            "starting"
        } else if ready.local_ready {
            "ready"
        } else {
            "not ready"
        };
        let them = if ready.remote_ready { "ready" } else { "not ready" };
        format!("vs {opponent}{latency} · you: {you} · them: {them}")
    }

    /// Both sides have exchanged StartMatch — call [`Netplay::start_match`].
    pub fn is_match_ready(&self) -> bool {
        self.match_ready
    }

    /// Press Ready: commit to the save this lobby was opened with.
    pub fn ready(&mut self) {
        if let Some(Event::MatchReady) = self.state.commit(self.save_sram.clone()) {
            self.match_ready = true;
        }
    }

    /// Un-press Ready.
    pub fn unready(&mut self) {
        self.state.uncommit();
    }

    /// Leave. The connection and everything running under it goes with it.
    pub fn disconnect(&mut self) {
        self.state.disconnect();
    }

    /// Trade the finished lobby for a running match.
    ///
    /// Consumes the lobby: past this point the transport belongs to the
    /// match. Awaits the lobby pump handing back the data channel, then
    /// boots and primes both cores — a couple of seconds during which
    /// this tab does nothing else, exactly as the priming is a couple of
    /// seconds of a desktop's drive thread.
    pub async fn start_match(mut self, replays_dir: String) -> Result<Demo, JsValue> {
        let pre_match = self
            .state
            .take_pre_match()
            .ok_or_else(|| JsValue::from_str("the handshake hasn't finished"))?;
        // The peer's ROM is this one: the demo has no ROM library to
        // resolve theirs from, so both sides must load the same file.
        // The lobby's game announcement is what makes a mismatch visible
        // before anyone presses Ready.
        let (session, boot) = tango_session::pvp::PvpSession::new(tango_session::pvp::PvpSessionArgs {
            local_game: self.game,
            local_rom: self.rom.clone(),
            remote_game: self.game,
            remote_rom: self.rom.clone(),
            pre_match,
            // No frame-delay control in the demo; the engine's rollback
            // covers the network either way.
            frame_delay: 0,
            disable_bgm: false,
            // A browser has no filesystem: the replay writer fails to
            // open and the match runs unrecorded, which the session
            // already treats as non-fatal.
            replays_path: std::path::Path::new(&replays_dir),
            cache_path: std::path::Path::new(&replays_dir),
            sample_rate: self.sample_rate,
        })
        .await
        .map_err(|e| JsValue::from_str(&format!("{e}")))?;
        let (driver, audio) = boot.boot().map_err(|e| JsValue::from_str(&format!("{e}")))?;
        Ok(Demo::wrap(Box::new(session), Box::new(driver), audio))
    }
}

fn link_code(ident: &LinkIdent) -> String {
    match ident {
        LinkIdent::Matchmaking(code) => code.clone(),
        LinkIdent::Direct(_) => "a direct link".to_string(),
    }
}

/// The typed failures, in the demo's own words. The desktop routes these
/// to localized templates; a demo just says what happened.
fn describe(error: &tango_netplay::Error) -> String {
    use tango_netplay::Error as E;
    match error {
        E::PeerDisconnected => "the opponent left".to_string(),
        E::NegotiateExpectedHello => "the peer didn't say hello".to_string(),
        E::NegotiateVersionTooOld => "the opponent is on an older Tango".to_string(),
        E::NegotiateVersionTooNew => "the opponent is on a newer Tango".to_string(),
        E::Negotiate(e) | E::Other(e) => e.clone(),
    }
}

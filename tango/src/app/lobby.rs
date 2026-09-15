//! Lobby settings, compatibility, and handoff into a live session.

use super::{App, Message};
use crate::{netplay, session};

impl App {
    /// Default match-type policy:
    ///   - Family JUST changed (or first selection in this lobby):
    ///     the mode this family was last picked in
    ///     ([`crate::config::Config::last_match_type_per_family`]), or, failing that,
    ///     Triple (mode=1) if the game supports it, else Single.
    ///     Keyed off `default_mt_for_family` so it only fires once per
    ///     (lobby, family) pair.
    ///   - Same family, current value invalid for this game: same
    ///     fallback (paranoia — the versions of a family can differ).
    ///   - Same family, valid value: leave alone — sticky user pick.
    ///
    /// Called any time the current game or lobby state could have
    /// changed in a way that affects the right default: on Connect
    /// (cancel_and_renew wiped the lobby), on selection change,
    /// and defensively inside `resend_settings_if_lobby`.
    pub(super) fn apply_default_match_type(&mut self) {
        let Some(game) = self.loadout.game else { return };
        let mt_table = game.family.match_types;
        let family = game.family_and_variant().0;
        let family_changed = self.netplay.lobby.default_mt_for_family.as_deref() != Some(family);
        let (mode, sub) = self.netplay.lobby.match_type;
        let current_valid =
            (mode as usize) < mt_table.len() && (sub as usize) < *mt_table.get(mode as usize).unwrap_or(&0);
        if family_changed || !current_valid {
            // What this family was last played in, if the game still
            // offers it — a remembered pick outranks the built-in
            // default, and a stale one (the table shrank under a patch)
            // falls through to it.
            let remembered = self
                .config
                .last_match_type_per_family
                .get(family)
                .copied()
                .filter(|&(mode, sub)| {
                    (mode as usize) < mt_table.len() && (sub as usize) < *mt_table.get(mode as usize).unwrap_or(&0)
                });
            let new_mt = remembered.unwrap_or_else(|| {
                if mt_table.get(1).copied().unwrap_or(0) > 0 {
                    (1, 0) // Triple
                } else {
                    (0, 0) // Single
                }
            });
            self.netplay.lobby.match_type = new_mt;
            self.netplay.lobby.default_mt_for_family = Some(family.to_string());
        }
    }

    /// Both sides have exchanged StartMatch: drain the lobby-side state
    /// into a `PreMatchData` and kick off async PvP setup. The lobby pump
    /// has been cancel-signaled; `spawn_pvp` polls the receiver-handoff
    /// slot until it releases ownership. On success we land back in
    /// `Message::PvpSessionBuilt`.
    pub(super) fn start_pvp_handoff(&mut self) -> iced::Task<Message> {
        let Some(pre_match) = self.netplay.take_pre_match() else {
            return iced::Task::none();
        };
        // Stamp the attempt this build belongs to. Leaving the lobby
        // mid-build (always allowed) bumps the id via
        // `netplay.disconnect()`, so the PvpSessionBuilt handler can
        // tell an abandoned build from a live one.
        let attempt = self.netplay.session_id();
        let scanners = self.scanners.clone();
        let config = self.config.clone();
        let audio_binder = self.audio_binder.clone();
        let local_game = self.loadout.game;
        let local_patch = self.loadout.patch.clone().zip(self.loadout.patch_version.clone());
        iced::Task::perform(
            async move {
                let Some(local_game) = local_game else {
                    return Err(anyhow::anyhow!("no local game selected"));
                };
                session::spawn_pvp(scanners, config, audio_binder, local_game, local_patch, pre_match).await
            },
            move |result| Message::PvpSessionBuilt(attempt, std::sync::Arc::new(std::sync::Mutex::new(Some(result)))),
        )
    }

    /// Build the current Settings packet and push it to the peer — only
    /// meaningful while netplay is in Lobby phase; outside that this
    /// returns `Task::none()`. Wrapped in a helper because it has three
    /// callers: lobby entry, selection change, and match-type change.
    pub(super) fn resend_settings_if_lobby(&mut self) -> iced::Task<Message> {
        if !matches!(self.netplay.phase, netplay::Phase::Lobby { .. }) {
            return iced::Task::none();
        }
        self.apply_default_match_type();
        let settings = self.make_local_settings();
        self.netplay.send_local_settings(settings);
        iced::Task::none()
    }

    /// If a netplay state change just flipped the compat verdict to
    /// anything other than Compatible while we're still flagged
    /// ready, fire an Uncommit so the local commit doesn't outlive
    /// the agreement it was based on. Covers the cases the netplay
    /// handlers don't catch — peer changing their game/patch/
    /// match_type, or our own available_patches shrinking out from
    /// under a previously-valid commit.
    pub(super) fn uncommit_if_incompat(&mut self) {
        if !matches!(self.netplay.phase, netplay::Phase::Lobby { .. }) || !self.netplay.local_ready() {
            return;
        }
        // Scoped so the scanner read guards (and the borrows of
        // `netplay.lobby`) are released before the uncommit.
        let compatible = {
            let (Some(local), Some(remote)) = (self.netplay.lobby.local.as_ref(), self.netplay.lobby.remote.as_ref())
            else {
                return;
            };
            let roms = self.scanners.roms.read();
            let patches = self.scanners.patches.read();
            matches!(
                netplay::compat::check(local, remote, &roms, &patches),
                netplay::compat::Verdict::Compatible
            )
        };
        if !compatible {
            self.netplay.uncommit();
        }
    }

    /// Fetch a patch the lobby needs but doesn't have.
    ///
    /// The compatibility check resolves the peer's patch from the repo
    /// index, so we know a matchup is playable before the package is on
    /// disk — and the only thing standing in the way is a download we
    /// can start ourselves. Idempotent: the tab tracks in-flight
    /// downloads, and this fires on every lobby state change.
    pub(super) fn fetch_missing_patch(&mut self) -> iced::Task<Message> {
        if !matches!(self.netplay.phase, netplay::Phase::Lobby { .. }) {
            return iced::Task::none();
        }
        let (Some(local), Some(remote)) = (self.netplay.lobby.local.as_ref(), self.netplay.lobby.remote.as_ref())
        else {
            return iced::Task::none();
        };
        let verdict = {
            let roms = self.scanners.roms.read();
            let patches = self.scanners.patches.read();
            netplay::compat::check(local, remote, &roms, &patches)
        };
        let Some((name, version)) = verdict.fetchable() else {
            return iced::Task::none();
        };
        let key = (name.to_owned(), version.clone());
        log::info!("lobby needs {} {}, fetching", key.0, key.1);
        self.install_patch(key)
    }

    /// Build a `protocol::Settings` packet from the App's current
    /// state: nickname from config, match_type defaults to (0, 0),
    /// game_info from the local loadout. (No available-games /
    /// available-patches lists cross the wire — possession of the
    /// peer's setup is checked locally by `compat::check`.)
    fn make_local_settings(&self) -> tango_net_protocol::control::Settings {
        self.loadout.make_local_settings(&self.config, &self.netplay.lobby)
    }

    pub(super) fn update_netplay(&mut self, delivery: netplay::Delivery) -> iced::Task<Message> {
        // A re-delivery of an already-applied report: nothing left
        // in the cell (see `netplay::Delivery`).
        let Some(incoming) = delivery.take() else {
            return iced::Task::none();
        };
        // Always resend after a report: this covers the
        // Negotiating → Lobby transition (first announce) and
        // lobby-state mutations. The dedupe inside
        // `send_local_settings` makes unchanged dispatches a no-op.
        let was_lobby = matches!(self.netplay.phase, netplay::Phase::Lobby { .. });
        let event = self.netplay.apply(incoming);
        let task = match event {
            Some(netplay::Event::MatchReady) => self.start_pvp_handoff(),
            None => iced::Task::none(),
        };
        let became_lobby = !was_lobby && matches!(self.netplay.phase, netplay::Phase::Lobby { .. });
        // Opponent just completed the handshake — flash the
        // taskbar / bounce the dock so the lobby host
        // notices even if Tango isn't focused. No-op if the
        // window is already focused (per iced docs).
        let attention = if became_lobby {
            iced::window::latest()
                .and_then(|id| iced::window::request_user_attention(id, Some(iced::window::UserAttention::Critical)))
        } else {
            iced::Task::none()
        };
        let resend = self.resend_settings_if_lobby();
        self.uncommit_if_incompat();
        let fetch = self.fetch_missing_patch();
        iced::Task::batch([task, resend, fetch, attention])
    }

    pub(super) fn finish_pvp_handoff(
        &mut self,
        attempt: u64,
        result: anyhow::Result<session::Launch>,
    ) -> iced::Task<Message> {
        if attempt != self.netplay.session_id() {
            // Dropping an abandoned launch stops and joins its workers.
            if let Err(e) = result {
                log::info!("pvp session build failed after the lobby was left: {e:#}");
            }
            return iced::Task::none();
        }
        match result {
            Ok(launch) => {
                self.session.install(launch, &self.audio_binder, &self.config);
                // Drop the post-handoff lobby snapshot now
                // that the PvP view is taking over the
                // screen. take_pre_match deliberately left
                // it in place so the bottom strip didn't
                // flash blank while spawn_pvp ran.
                self.netplay.finish_handoff();
            }
            Err(e) => {
                // Surface the failure where every other netplay
                // failure lands: the lobby band's sticky Failed
                // status, which is still on screen (the handoff
                // kept it up while the session was built).
                log::error!("pvp session build failed: {e:#}");
                self.netplay.fail_session_build(netplay::Error::Other(format!("{e:#}")));
            }
        }
        iced::Task::none()
    }
}

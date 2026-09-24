//! Lobby settings, compatibility, and handoff into a live session.

use super::{App, Message};
use crate::{netplay, session};

impl App {
    /// Re-apply the lobby's default match-type policy
    /// ([`netplay::State::apply_default_match_type`]) for the current
    /// game, with this family's remembered pick from
    /// [`crate::config::Config::last_match_type_per_family`].
    ///
    /// Called any time the current game or lobby state could have
    /// changed in a way that affects the right default: on Connect
    /// (cancel_and_renew wiped the lobby), on selection change,
    /// and defensively inside `resend_settings_if_lobby`.
    pub(super) fn apply_default_match_type(&mut self) {
        let Some(game) = self.loadout.game() else { return };
        let family = game.family_and_variant().0;
        let remembered = self.config.last_match_type_per_family.get(family).copied();
        self.netplay
            .apply_default_match_type(family, game.family.match_types, remembered);
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
        iced::Task::perform(
            async move { session::spawn_pvp(scanners, config, audio_binder, pre_match).await },
            move |result| Message::PvpSessionBuilt(attempt, std::sync::Arc::new(std::sync::Mutex::new(Some(result)))),
        )
    }

    /// Run the lobby's follow-up ([`netplay::State::reconcile`]) — only
    /// meaningful while netplay is in Lobby phase; outside that this
    /// returns `Task::none()`. Pushes our current Settings (deduped),
    /// unreadies us if the verdict is no longer Compatible, and fetches a
    /// patch the lobby needs but doesn't have. Called after every
    /// netplay report and every Play-tab dispatch.
    pub(super) fn resend_settings_if_lobby(&mut self) -> iced::Task<Message> {
        if !matches!(self.netplay.phase, netplay::Phase::Lobby { .. }) {
            return iced::Task::none();
        }
        self.apply_default_match_type();
        let missing = {
            let (loadout, config, scanners) = (&self.loadout, &self.config, &self.scanners);
            self.netplay.reconcile(
                |lobby| crate::tabs::play::loadout_strip::local_settings(loadout, config, lobby),
                |local, remote| {
                    tango_library::loadout::compatibility_facts(
                        local,
                        remote,
                        &scanners.roms.read(),
                        &scanners.patches.read(),
                    )
                },
            )
        };
        // Idempotent: the download tracker ignores a key already in
        // flight, and this fires on every lobby state change.
        let Some(key) = missing else {
            return iced::Task::none();
        };
        log::info!("lobby needs {} {}, fetching", key.0, key.1);
        self.install_patch(key)
    }

    pub(super) fn update_netplay(&mut self, delivery: netplay::Delivery) -> iced::Task<Message> {
        // A re-delivery of an already-applied report: nothing left
        // in the cell (see `netplay::Delivery`).
        let Some(incoming) = delivery.take() else {
            return iced::Task::none();
        };
        // Always reconcile after a report: this covers the
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
        let followup = self.resend_settings_if_lobby();
        iced::Task::batch([task, followup, attention])
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
                self.netplay
                    .fail_session_build(netplay::Error::SessionBuild(format!("{e:#}")));
            }
        }
        iced::Task::none()
    }
}

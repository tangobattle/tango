//! Session events that affect application settings, replays, or the library.

use super::{App, Message, RescanFollowup};
use crate::{session, tabs};

impl App {
    pub(super) fn update_session(&mut self, message: session::Message) -> iced::Task<Message> {
        // Detect all close paths (button, Esc, and a match ending itself).
        // Session teardown flushes the save/recording before the rescan starts.
        let rescan_on_close = if self
            .session
            .active_as::<session::singleplayer::SinglePlayerSession>()
            .is_some()
        {
            Some(RescanFollowup::ForceRebuildLoaded)
        } else if self.session.active_as::<session::pvp::PvpSession>().is_some() {
            Some(RescanFollowup::RefreshAndReplayStats)
        } else {
            None
        };
        let effect = match self.session.update(message, &self.config) {
            Some(effect) => self.perform_session_effect(effect),
            None => iced::Task::none(),
        };
        let rescan = match rescan_on_close.filter(|_| !self.session.is_active()) {
            Some(followup) => self.rescan_off_thread(followup),
            None => iced::Task::none(),
        };
        let queue = self.advance_replay_queue();
        iced::Task::batch([effect, rescan, queue])
    }

    /// Persist the preferences a session changed (the session has already
    /// applied their live effects) and run the replay workflows it asked for.
    fn perform_session_effect(&mut self, effect: session::update::Effect) -> iced::Task<Message> {
        use session::update::Effect as E;
        match effect {
            E::Task(task) => return task.map(Message::Session),
            E::PersistFrameDelay(delay) => self.config.frame_delay = delay,
            E::PersistPaneWidths(widths) => self.config.pvp_setup_pane_widths = widths,
            E::PersistOpponentView(view) => self.config.opponent_view = view,
            E::PersistReplayInputs(show) => self.config.show_replay_inputs = show,
            E::PersistCustomScreenSpeedup(enabled) => self.config.replay_custom_screen_speedup = enabled,
            E::SetClipExportScale(scale) => {
                return self.update_replays(tabs::replays::Message::Export(tabs::replays::ExportMessage::SetScale(
                    scale,
                )))
            }
            E::ExportClip {
                replay,
                clip,
                swap_sides,
            } => return self.export_replay_clip(replay, clip, swap_sides),
            E::CancelClipExport(replay) => {
                return self.update_replays(tabs::replays::Message::Export(tabs::replays::ExportMessage::Cancel(
                    replay,
                )))
            }
            E::WatchReplay(path) => return self.watch_replay(path),
            E::SkipToQueued => return self.watch_next_replay(),
        }
        self.persist_config();
        iced::Task::none()
    }
}

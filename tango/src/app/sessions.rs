//! Session events that affect application settings, replays, or the library.

use super::{App, Message, RescanFollowup};
use crate::{session, tabs};

impl App {
    pub(super) fn update_session(&mut self, message: session::Message) -> iced::Task<Message> {
        use session::view::{pvp, replay, results, training};
        use session::Message as S;

        // Persist preferences here; the session applies their live effects.
        let persist = match &message {
            S::Pvp(pvp::Message::SetFrameDelay(delay)) => {
                self.config.frame_delay = *delay;
                true
            }
            S::Pvp(pvp::Message::EndPaneResize) => {
                if let Some(panes) = self.session.pvp_panes.as_ref() {
                    self.config.pvp_setup_pane_widths = panes.pane_widths;
                }
                true
            }
            S::Replay(replay::Message::ToggleInputDisplay) => {
                self.config.show_replay_inputs = !self.config.show_replay_inputs;
                true
            }
            S::Replay(replay::Message::ToggleCustomScreenSpeedup) => {
                self.config.replay_custom_screen_speedup = !self.config.replay_custom_screen_speedup;
                true
            }
            S::Replay(replay::Message::SetOpponentView(view))
            | S::Training(training::Message::SetOpponentView(view)) => {
                self.config.opponent_view = *view;
                true
            }
            S::Replay(replay::Message::SetClipExportScale(scale)) => {
                return self.update_replays(tabs::replays::Message::Export(tabs::replays::ExportMessage::SetScale(
                    *scale,
                )));
            }
            S::Replay(replay::Message::ExportClip { start, end }) => return self.export_replay_clip(*start, *end),
            S::Replay(replay::Message::CancelClipExport) => {
                return match self.session.replay_path.clone() {
                    Some(path) => self.update_replays(tabs::replays::Message::Export(
                        tabs::replays::ExportMessage::Cancel(path),
                    )),
                    None => iced::Task::none(),
                };
            }
            S::Replay(replay::Message::SkipToQueued) => return self.watch_next_replay(),
            S::Results(results::Message::WatchReplay) => {
                return match self.session.results.as_ref().and_then(|r| r.replay_path.clone()) {
                    Some(path) => self.watch_replay(path),
                    None => iced::Task::none(),
                };
            }
            _ => false,
        };
        if persist {
            self.persist_config();
        }

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
        let task = self
            .session
            .update(message, &self.config.input_mapping, &self.config.language)
            .map(Message::Session);
        let rescan = match rescan_on_close.filter(|_| !self.session.is_active()) {
            Some(followup) => self.rescan_off_thread(followup),
            None => iced::Task::none(),
        };
        let queue = self.advance_replay_queue();
        iced::Task::batch([task, rescan, queue])
    }
}

//! Session controls: what each session kind's messages do to the live
//! session and its presentation state. The `view` modules only render;
//! anything that needs the application's configuration or library comes
//! back from [`State::update`](super::State::update) as an [`Effect`].

pub mod pvp;
pub mod replay;
pub mod training;

/// Work a session message asks of the application, which owns the
/// configuration, the replays tab, and session launches.
pub enum Effect {
    /// Follow-up work inside the session, from a save view's update.
    Task(iced::Task<super::Message>),
    /// Remember the local frame delay the PvP slider set.
    PersistFrameDelay(u32),
    /// Remember the PvP setup drawers' widths after a resize.
    PersistPaneWidths([f32; 2]),
    /// Remember how the opponent's screen is presented.
    PersistOpponentView(crate::config::OpponentView),
    /// Show or hide the replay input display.
    PersistReplayInputs(bool),
    /// Remember whether open custom screens speed replays up.
    PersistCustomScreenSpeedup(bool),
    /// Set the export quality the replays tab's form shares with the clip
    /// strip.
    SetClipExportScale(u8),
    /// Export a clip of the playing replay: ask where to write it, then
    /// start the replays tab's export job.
    ExportClip {
        replay: std::path::PathBuf,
        clip: crate::replay_render::Clip,
        /// Render the opposite seat's perspective, as the player's swap
        /// toggle stood when the export started.
        swap_sides: bool,
    },
    /// Cancel the export job running for this replay.
    CancelClipExport(std::path::PathBuf),
    /// Play back a recorded match.
    WatchReplay(std::path::PathBuf),
    /// Abandon the playing replay for the next queued one.
    SkipToQueued,
}

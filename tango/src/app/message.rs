//! Application messages and navigation destinations.

use crate::platform::audio;
use crate::{netplay, session, tabs};

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Play,
    Replays,
    Patches,
    Settings,
}

impl Tab {
    /// What should happen after this tab's click-triggered disk scan.
    /// Play is the explicit "reload my setup" gesture: unlike the other
    /// scanner-backed tabs, it must discard the same-key loaded bundle so
    /// staged save edits and already-decoded ROM/patch assets are rebuilt
    /// from the newly scanned disk state.
    pub(super) fn rescan_followup(self) -> Option<RescanFollowup> {
        match self {
            Self::Play => Some(RescanFollowup::ForceRebuildLoaded),
            Self::Replays => Some(RescanFollowup::RefreshAndReplayStats),
            Self::Patches => Some(RescanFollowup::Refresh),
            Self::Settings => None,
        }
    }
}

/// Top-level Message. Tab-specific messages live in each tab module
/// and are wrapped here; the dispatch in `App::update` routes them to
/// per-tab `update_*` methods below.
#[derive(Debug, Clone)]
pub enum Message {
    /// No-op message — used by overlay layers (e.g. the
    /// settings-modal panel itself) to swallow clicks without
    /// triggering any state change.
    NoOp,
    /// Emitted by the `window::frames()` subscription while a UI
    /// animation is mid-flight. Carries no state change — its
    /// only job is to drive another update → view pass so the
    /// animation can sample a fresh `Instant`.
    AnimTick,
    TabSelected(Tab),
    Play(tabs::play::Message),
    Patches(tabs::patches::Message),
    Replays(tabs::replays::Message),
    Settings(tabs::settings::Message),
    Welcome(tabs::welcome::Message),
    Session(session::Message),
    Netplay(netplay::Delivery),
    /// Carries the freshly-constructed PvP session (plus its setup-pane
    /// presentation state and audio binding) back into the App after the
    /// async build task in `spawn_pvp` resolves. Ferried in a once-take
    /// cell because PvpSession isn't Clone. The leading `u64` is the
    /// netplay attempt id captured at handoff: leaving the lobby stays
    /// possible while the build runs (it bumps the id), and a stale id
    /// here means the session has no lobby behind it and gets wound
    /// down instead of installed.
    #[allow(clippy::type_complexity)]
    PvpSessionBuilt(
        u64,
        std::sync::Arc<
            std::sync::Mutex<
                Option<
                    anyhow::Result<(
                        session::pvp::PvpSession,
                        session::PvpPanes,
                        Option<audio::Binding>,
                        std::thread::JoinHandle<()>,
                    )>,
                >,
            >,
        >,
    ),
    /// 1 Hz tick: refresh Discord rich-presence + drain any
    /// Discord-initiated join secret into the play link-code
    /// field.
    DiscordTick,
    /// Raw window event (open, resize, move, etc.). Filtered in the
    /// handler — only Opened / Resized / Moved trigger anything.
    Window(iced::window::Id, iced::window::Event),
    /// Result of an `iced::window::get_maximized` task spawned
    /// after a Resized event. Carries the resize-time size so the
    /// handler can decide whether to persist it (only if the
    /// window isn't maximized).
    WindowMaximizedQueried {
        size: iced::Size,
        maximized: bool,
    },
    /// Result of the `iced::window::scale_factor` query spawned when
    /// the window opens — the monitor's DPI scale, which the handler
    /// turns into the window's max size.
    WindowScaleQueried {
        id: iced::window::Id,
        scale: f32,
    },
    /// Request an orderly application exit. Fired by both the fullscreen top
    /// bar and the intercepted OS `CloseRequested` event.
    Quit,
    /// PvP's bounded close handshake finished; the runtime may now exit.
    Exit,
    /// Fired when a backgrounded `Scanners::rescan` task completes.
    /// `followup` tells the handler which post-scan work to do —
    /// most paths just want `Refresh` (re-validate `self.loaded`),
    /// a rescan with the Replays tab on screen also warms the stats
    /// cache, and the save-delete handler asks for a fresh "first
    /// save" pick now that the scan results are in.
    Rescanned(RescanFollowup),
}

/// Per-call-site cue for `Message::Rescanned`. Lets one handler
/// arm cover every rescan we kick off without dispatching a
/// distinct Message variant per call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RescanFollowup {
    /// The startup scan's first stage — roms, saves and patches, the
    /// only scanners the play tab reads. Resolves the saved selection
    /// now that there is something to resolve it against, and flips
    /// `library_scanned`: the play and patches tabs say "scanning…"
    /// rather than "your library is empty" until it does.
    Boot,
    /// The startup scan's second stage — the replay index. Flips
    /// `replays_scanned` and warms the stats cache.
    BootReplays,
    /// A patch a replay was waiting on just installed — start playback
    /// now that the scan can see it.
    RetryPendingWatch,
    /// Just re-validate `self.loaded` against the fresh scan.
    Refresh,
    /// Refresh + warm the replays-tab stats cache (used when a
    /// rescan runs with the Replays tab on screen, and after a PvP
    /// session closes).
    RefreshAndReplayStats,
    /// Refresh + if `local_save` is `None`, auto-pick the first
    /// remaining save for the local game. Used by the save-delete
    /// handler so the picker doesn't strand on an empty selection.
    RefreshAndPickFirstSave,
    /// Drop `self.loaded` first so `refresh_loaded` rebuilds it
    /// from scratch (bypassing the same-key dedupe). Used after a
    /// single-player session writes back to its SRAM, and when Play
    /// is clicked to explicitly reload the selected save, ROM and
    /// patch from the freshly scanned disk state.
    ForceRebuildLoaded,
}

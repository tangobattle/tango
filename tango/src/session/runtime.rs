//! Desktop session ownership, pacing, and save persistence.
//!
//! `RunningSession` stops audio, drops the session to cancel its drivers,
//! and joins every worker on all exit paths, including failed startup.

use super::{replay, singleplayer, Session};
use crate::platform::audio;

pub(super) struct RunningSession {
    session: Option<Box<dyn Session + Send>>,
    stream: Option<audio::Stream>,
    binding: Option<audio::Binding>,
    threads: Vec<std::thread::JoinHandle<()>>,
    save: Option<SaveBackup>,
}

impl RunningSession {
    pub(super) fn new(session: impl Session + Send, stream: audio::Stream) -> Self {
        Self {
            session: Some(Box::new(session)),
            stream: Some(stream),
            binding: None,
            threads: Vec::new(),
            save: None,
        }
    }

    /// Bind only after installation. An abandoned asynchronous launch never
    /// claims the output device or interrupts the session currently playing.
    pub(super) fn bind_audio(&mut self, binder: &audio::LateBinder) {
        let Some(stream) = self.stream.take() else { return };
        match binder.bind(Some(Box::new(stream))) {
            Ok(binding) => self.binding = Some(binding),
            Err(e) => log::warn!("session audio bind failed: {e:?}"),
        }
    }

    pub(super) fn add_thread(&mut self, thread: std::thread::JoinHandle<()>) {
        self.threads.push(thread);
    }

    pub(super) fn spawn_driver(
        &mut self,
        name: &str,
        driver: impl tango_session::Drive + Send + 'static,
    ) -> std::io::Result<()> {
        self.add_thread(spawn_drive_thread(name, driver)?);
        Ok(())
    }

    pub(super) fn save_to(&mut self, path: std::path::PathBuf, initial: Vec<u8>) {
        self.save = Some(SaveBackup::new(path, initial));
    }

    pub(super) fn autosave(&mut self) {
        let Some(backup) = self.save.as_mut() else { return };
        if std::time::Instant::now() < backup.next_check {
            return;
        }
        backup.next_check = std::time::Instant::now() + SaveBackup::INTERVAL;
        self.flush_save();
    }

    fn flush_save(&mut self) {
        if self.save.is_none() {
            return;
        }
        let game = self.local_game();
        let image = self
            .downcast_ref::<singleplayer::SinglePlayerSession>()
            .and_then(|s| s.export_save());
        if let Some(backup) = self.save.as_mut() {
            backup.store(game, image.as_deref());
        }
    }
}

impl std::ops::Deref for RunningSession {
    type Target = dyn Session;

    fn deref(&self) -> &Self::Target {
        // Taken only inside Drop, after the last consumer has gone away.
        self.session.as_deref().unwrap()
    }
}

impl AsRef<dyn Session> for RunningSession {
    fn as_ref(&self) -> &(dyn Session + 'static) {
        &**self
    }
}

impl Drop for RunningSession {
    fn drop(&mut self) {
        self.request_close();
        self.flush_save();
        self.binding.take();
        self.stream.take();
        self.session.take();
        for thread in self.threads.drain(..) {
            if thread.join().is_err() {
                log::error!("session worker panicked during shutdown");
            }
        }
    }
}

/// Where a single-player session's savedata goes.
///
/// Single-player keeps SRAM in memory. Copy it to disk periodically and
/// once at teardown; [`singleplayer::SaveWriteback`] decides what, if
/// anything, each copy writes.
struct SaveBackup {
    path: std::path::PathBuf,
    writeback: singleplayer::SaveWriteback,
    next_check: std::time::Instant,
}

impl SaveBackup {
    /// How often the running session's savedata is compared against
    /// what's on disk. Long enough to be free, short enough that a
    /// crash costs a few seconds of play rather than the session.
    const INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

    fn new(path: std::path::PathBuf, initial: Vec<u8>) -> Self {
        Self {
            path,
            writeback: singleplayer::SaveWriteback::new(initial),
            next_check: std::time::Instant::now() + Self::INTERVAL,
        }
    }

    /// Write the cart's `image` back if it holds a changed save.
    /// Failures are logged, not surfaced: a full disk shouldn't take the
    /// session down mid-battle, and the next check tries again.
    fn store(&mut self, game: &tango_gamesupport::Game, image: Option<&[u8]>) {
        let Some(file) = self.writeback.next_write(game, image) else {
            return;
        };
        if let Err(e) = std::fs::write(&self.path, &file) {
            log::error!("writing {}: {e}", self.path.display());
            return;
        }
        self.writeback.mark_written(file);
    }
}

/// Wall-clock frame pacer for the drive threads below. It accumulates
/// absolute `1/fps` deadlines (drift-free on average) and sleeps to
/// each; a loop that falls far behind — a debugger pause, a laptop lid —
/// resynchronizes its cadence instead of sprinting to catch up.
///
/// This is the host's job, not the session's: a session only knows how
/// to advance a frame and what rate it wants. A browser host paces the
/// same drivers off its event loop, with nothing to sleep at all.
pub(super) struct Pacer {
    next_tick: std::time::Instant,
}

impl Pacer {
    /// How far behind the deadline a loop must fall before the pacer
    /// gives up catching up and resynchronizes from now.
    const RESYNC_AFTER: std::time::Duration = std::time::Duration::from_millis(250);

    pub(super) fn new() -> Self {
        Self {
            next_tick: std::time::Instant::now(),
        }
    }

    /// Sleep until the next `1/fps` deadline — call once per emulated
    /// frame, after stepping it. A non-positive `fps` degrades to 60 (a
    /// should-never-happen guard; the drivers only ever ask for a
    /// positive rate).
    pub(super) fn wait(&mut self, fps: f32) {
        let fps = if fps > 0.0 { fps } else { 60.0 };
        self.next_tick += std::time::Duration::from_secs_f64(1.0 / fps as f64);
        let now = std::time::Instant::now();
        if self.next_tick > now {
            std::thread::sleep(self.next_tick - now);
        } else if now - self.next_tick > Self::RESYNC_AFTER {
            // Fell way behind: don't sprint to catch up, just
            // resynchronize the cadence.
            self.next_tick = now;
        }
    }

    /// Restart the cadence from now — after a park or a stall, so the
    /// idle time doesn't accrue pacing debt the next `wait` burns off.
    pub(super) fn resync(&mut self) {
        self.next_tick = std::time::Instant::now();
    }
}

/// Where a playback session's prefetch pass reports the match-stats
/// analysis it folds: throttled previews while it runs, and the finished
/// stats, parked before the sender drops so whoever drains `partial_rx`
/// reads them on close.
pub struct PrefetchStatsFeed {
    pub partial_tx: futures::channel::mpsc::UnboundedSender<tango_match::analysis::MatchStats>,
    pub done: std::sync::Arc<std::sync::Mutex<Option<tango_match::analysis::MatchStats>>>,
}

/// Race the prefetch pair through the whole replay: keyframes so
/// seeking backwards works, round marks for recordings without them,
/// and — when the tab asked for it — the match-stats analysis, previewed
/// as it folds and handed over when it finishes.
pub(super) fn run_prefetch_pass(worker: replay::PrefetchWorker, stats: Option<PrefetchStatsFeed>) {
    /// Live-preview cadence. Each report clones the folded rounds and
    /// becomes a chart rebuild on the UI thread, so pace it to the
    /// display rather than to the simulation.
    const PREVIEW_EVERY: std::time::Duration = std::time::Duration::from_millis(33);

    let mut worker = worker;
    let mut last_preview = std::time::Instant::now();
    // Progress and round marks publish once per slice, so the slice
    // size is also the scrub-bar overlay's refresh granularity.
    while worker.step(256) {
        let now = std::time::Instant::now();
        if now.duration_since(last_preview) < PREVIEW_EVERY {
            continue;
        }
        last_preview = now;
        let (Some(feed), Some(preview)) = (stats.as_ref(), worker.preview()) else {
            continue;
        };
        let _ = feed.partial_tx.unbounded_send(preview);
    }
    let (Some(feed), Some(finished)) = (stats, worker.finished()) else {
        return;
    };
    *feed.done.lock().unwrap() = Some(finished);
}

/// Run a session's driver on a thread of its own, paced to the fps the
/// session publishes — the desktop's answer to "who turns the crank".
/// (A browser host pumps the same driver from its event loop instead.)
///
/// The loop ends when the session is dropped, which is what makes
/// `tick` return false; the caller joins the handle after dropping it.
fn spawn_drive_thread(
    name: &str,
    mut driver: impl tango_session::Drive + Send + 'static,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    // The thread runs inside our runtime's context, so a driver that
    // wants to fire an async send or a timer just calls `tokio::spawn`
    // — no runtime handle to thread down to the call site.
    let rt = tokio::runtime::Handle::current();
    std::thread::Builder::new().name(name.to_owned()).spawn(move || {
        let _guard = rt.enter();
        let mut pacer = Pacer::new();
        while driver.tick() {
            pacer.wait(driver.fps_target());
        }
        // The session is over: wind it down rather than dropping it, or
        // a PvP match's replay never gets its end-of-stream sentinel and
        // reads back as truncated.
        driver.finish();
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    type Events = Arc<Mutex<Vec<&'static str>>>;

    struct TestSession {
        events: Events,
        stopped: std::sync::mpsc::Sender<()>,
    }

    impl Session for TestSession {
        fn local_game(&self) -> &'static tango_gamesupport::Game {
            unreachable!()
        }
        fn screen_layout(&self) -> tango_match::ScreenLayout {
            tango_match::ScreenLayout::single(1, 1)
        }
        fn frame(&self) -> Vec<u8> {
            vec![0; 4]
        }
        fn wake(&self) -> Arc<tokio::sync::Notify> {
            Arc::new(tokio::sync::Notify::new())
        }
        fn request_close(&self) {
            self.events.lock().unwrap().push("close");
        }
    }

    impl Drop for TestSession {
        fn drop(&mut self) {
            self.events.lock().unwrap().push("session");
            let _ = self.stopped.send(());
        }
    }

    struct AudioLifetime(Events);
    impl Drop for AudioLifetime {
        fn drop(&mut self) {
            self.0.lock().unwrap().push("audio");
        }
    }

    fn runtime(events: &Events) -> RunningSession {
        let (stopped, receiver) = std::sync::mpsc::channel();
        let session = TestSession {
            events: events.clone(),
            stopped,
        };
        let (_, output) = tango_match::audio::channel(32);
        let lifetime = AudioLifetime(events.clone());
        let stream = audio::Stream::new(
            output,
            60.0,
            move || {
                let _ = &lifetime;
                60.0
            },
            48000,
        );
        let mut runtime = RunningSession::new(session, stream);
        let events = events.clone();
        runtime.add_thread(std::thread::spawn(move || {
            receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("session must stop before joining its workers");
            events.lock().unwrap().push("worker");
        }));
        runtime
    }

    #[test]
    fn abandoned_launch_stops_audio_and_joins_workers() {
        let events = Events::default();
        let launch = super::super::Launch {
            runtime: runtime(&events),
            pvp_panes: None,
            replay_path: None,
        };
        drop(launch);
        assert_eq!(*events.lock().unwrap(), ["close", "audio", "session", "worker"]);
    }

    #[test]
    fn replacing_a_session_releases_audio_and_resets_transient_ui() {
        let binder = audio::LateBinder::new();
        let config = crate::config::Config::default();
        let mut state = super::super::State::new();
        let old = Events::default();
        let new = Events::default();
        state.install(
            super::super::Launch {
                runtime: runtime(&old),
                pvp_panes: None,
                replay_path: None,
            },
            &binder,
            &config,
        );
        state.settings.open();
        state.opponent_panel.open();
        state.scrub.tools_open = true;
        state.speed_up_engaged = true;
        let seq = state.session_seq;
        state.install(
            super::super::Launch {
                runtime: runtime(&new),
                pvp_panes: None,
                replay_path: None,
            },
            &binder,
            &config,
        );
        assert_eq!(*old.lock().unwrap(), ["close", "audio", "session", "worker"]);
        assert!(!state.settings.shown());
        assert!(!state.opponent_panel.shown());
        assert!(!state.scrub.tools_open);
        assert!(!state.speed_up_engaged);
        assert_ne!(state.session_seq, seq);
        assert!(matches!(binder.bind(None), Err(audio::BindingError::AlreadyBound)));
        drop(state);
        assert_eq!(*new.lock().unwrap(), ["close", "audio", "session", "worker"]);
        assert!(binder.bind(None).is_ok());
    }
}

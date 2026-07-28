//! A netplay session on any engine.
//!
//! It drives a [`RunningMatch`] — whatever a game's registration handed
//! back — so nothing here knows which emulator is underneath, or how
//! many screens the console has. A GBA cable match and a DS wireless
//! match are the same object to this code.
//!
//! What a caller supplies is the match, the transport's two ends, and
//! the screen layout the frames carry.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tango_match::{RunningMatch, ScreenLayout};

/// A running netplay session.
pub struct MatchSession {
    game: &'static tango_gamesupport::Game,
    /// Whatever the local player is holding, sampled once per frame.
    joyflags: Arc<AtomicU32>,
    /// The frame the host uploads, RGBA8, both screens stacked.
    frame: Arc<Mutex<Vec<u8>>>,
    wake: Arc<tokio::sync::Notify>,
    ended: Arc<AtomicBool>,
    layout: ScreenLayout,
    /// Set on drop so the drive thread stops.
    cancel: Arc<AtomicBool>,
}

impl MatchSession {
    /// Start driving `match_`, which must already be in a battle.
    ///
    /// `outgoing` receives each tick's local input for the peer;
    /// `incoming` supplies the peer's, in tick order. Both are plain
    /// channels so the transport stays the caller's business.
    /// `frame_duration` is one frame at the console's refresh rate.
    pub fn new(
        game: &'static tango_gamesupport::Game,
        mut match_: Box<dyn RunningMatch>,
        layout: ScreenLayout,
        frame_duration: std::time::Duration,
        outgoing: std::sync::mpsc::Sender<(u32, u32)>,
        incoming: std::sync::mpsc::Receiver<(u32, i16)>,
    ) -> Self {
        let joyflags = Arc::new(AtomicU32::new(0));
        let frame = Arc::new(Mutex::new(Vec::new()));
        let wake = Arc::new(tokio::sync::Notify::new());
        let ended = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));

        let session = MatchSession {
            game,
            joyflags: joyflags.clone(),
            frame: frame.clone(),
            wake: wake.clone(),
            ended: ended.clone(),
            layout,
            cancel: cancel.clone(),
        };

        std::thread::Builder::new()
            .name("netplay".to_string())
            .spawn(move || {
                while !cancel.load(Ordering::Relaxed) {
                    let started = std::time::Instant::now();

                    // Everything the peer has sent settles before this
                    // tick speculates past it.
                    while let Ok((keys, tick_advantage)) = incoming.try_recv() {
                        match_.add_remote_input(keys, tick_advantage);
                    }

                    let local = joyflags.load(Ordering::Relaxed);
                    let (tick, keys, _tick_advantage) = match match_.advance(local) {
                        Ok(advanced) => advanced,
                        Err(e) => {
                            log::error!("ds session: {e}");
                            ended.store(true, Ordering::Release);
                            wake.notify_one();
                            return;
                        }
                    };
                    if outgoing.send((tick, keys)).is_err() {
                        // The peer's transport is gone.
                        ended.store(true, Ordering::Release);
                        wake.notify_one();
                        return;
                    }

                    if let Some(pixels) = match_.frame() {
                        *frame.lock().unwrap() = pixels;
                    }
                    wake.notify_one();

                    // The clock governor shaves fps when this side runs
                    // ahead; without a peer to sync against, hold the
                    // console's own rate.
                    if let Some(rest) = frame_duration.checked_sub(started.elapsed()) {
                        std::thread::sleep(rest);
                    }
                }
            })
            .expect("spawn ds session thread");

        session
    }

    /// The screens a frame from this session carries.
    pub fn screen_layout(&self) -> &ScreenLayout {
        &self.layout
    }
}

impl Drop for MatchSession {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl crate::Session for MatchSession {
    fn local_game(&self) -> &'static tango_gamesupport::Game {
        self.game
    }

    fn screen_layout(&self) -> ScreenLayout {
        self.layout.clone()
    }

    fn frame(&self) -> Vec<u8> {
        self.frame.lock().unwrap().clone()
    }

    fn wake(&self) -> Arc<tokio::sync::Notify> {
        self.wake.clone()
    }

    fn set_joyflags(&self, joyflags: u32) {
        self.joyflags.store(joyflags, Ordering::Relaxed);
    }

    fn request_close(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    fn is_ended(&self) -> bool {
        self.ended.load(Ordering::Acquire)
    }
}

//! Standalone (no-netplay) emulator session: one machine on a link
//! with nobody on the other end. Boots a ROM with the user-selected
//! save and accepts joyflag input from the host's tick loop. The video
//! frame plumbing mirrors the other sessions — the driver publishes the
//! console's frame into the session's own
//! [`Framebuffer`](crate::Framebuffer).
//!
//! The console comes from the game's own registration
//! ([`start_solo`](tango_match::Backend::start_solo)), so this
//! session never learns which emulator is underneath and a game whose
//! engine offers no single-player ride simply says so.
//!
//! The console runs wherever the host drives it from. [`Driver::tick`]
//! is one emulated frame,
//! and the host decides what turns it: a desktop runs it on a thread of
//! its own, paced to [`Driver::fps_target`]; a browser calls it from the
//! event loop. Neither the thread nor the pacing lives here. Audio
//! follows as a
//! pure consumer through the shared
//! [`Stream`](crate::audio::Stream) rate control, so a
//! stalled or torn-down audio device costs sound, never the session.
//!
//! No priming happens: this is a vanilla ride for one player, where
//! netplay's traps would have nothing to prime towards.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::local::Pacing;
use crate::InputCell;

pub struct SinglePlayerSession {
    game: &'static tango_gamesupport::Game,
    /// The seam's solo ride, clone-shared with whatever drives it (the
    /// session reads the save off its own handle).
    console: tango_match::Solo,
    layout: tango_match::ScreenLayout,
    input: Arc<InputCell>,
    pacing: Pacing,
    stop: Arc<AtomicBool>,
    screen: Arc<crate::Framebuffer>,
    wake: Arc<tokio::sync::Notify>,
}

impl SinglePlayerSession {
    /// Boot the session. Also returns the driver the host will tick and
    /// the session's audio stream — its samples resampled to
    /// `sample_rate` — for the host to route to its output; dropping the
    /// stream just costs sound (pacing doesn't depend on the audio
    /// device at all).
    ///
    /// `save` is the cartridge's savedata image, or `None` for a cart
    /// that starts blank; the game's writes land in memory and the host
    /// persists them from [`export_save`](Self::export_save). `rtc`
    /// pins the cart clock — `None` leaves it on the real one, which is
    /// what a desktop wants and what a browser (where there is no such
    /// clock to read) must fill in.
    pub fn new(
        game: &'static tango_gamesupport::Game,
        rom: Arc<Vec<u8>>,
        save: Option<Vec<u8>>,
        rtc: Option<std::time::SystemTime>,
        sample_rate: u32,
    ) -> Result<(Self, Driver, crate::audio::Stream), crate::Error> {
        // The console pushes into the ring on its way out of every
        // tick; the stream plays the other end without ever reaching
        // for the console.
        let (audio_in, audio_out) = crate::audio::ring();
        let console = game.pvp.start_solo(tango_match::SoloConfig {
            rom: rom.as_ref(),
            save: save.as_deref(),
            rtc,
            audio: Some(audio_in),
        })?;

        let layout = game.pvp.screen_layout(tango_match::SessionMode::Solo);
        let input = InputCell::new();
        let pacing = Pacing::new(game);
        let stop = Arc::new(AtomicBool::new(false));
        let screen = crate::Framebuffer::new(&layout);
        let wake = Arc::new(tokio::sync::Notify::new());

        let audio = pacing.audio_stream(audio_out, sample_rate);
        let driver = Driver {
            console: console.clone(),
            input: input.clone(),
            pacing: pacing.clone(),
            stop: stop.clone(),
            screen: screen.clone(),
            wake: wake.clone(),
        };

        Ok((
            Self {
                game,
                console,
                layout,
                input,
                pacing,
                stop,
                screen,
                wake,
            },
            driver,
            audio,
        ))
    }

    /// The cartridge's savedata as it stands right now, or `None` for a
    /// game that has never written any. The host owns persisting it —
    /// nothing here writes files — so a desktop host should also take a
    /// copy periodically rather than only at teardown.
    pub fn export_save(&self) -> Option<Vec<u8>> {
        self.console.export_save()
    }
}

/// What a single-player session's savedata write-back should put in its
/// file next. Nothing here touches storage: the host schedules the
/// checks, writes the bytes [`next_write`](Self::next_write) returns,
/// and reports a successful write back with
/// [`mark_written`](Self::mark_written).
///
/// A write is refused unless the cart's image parses as a save for its
/// game. Before the cartridge has written its SRAM even once,
/// [`SinglePlayerSession::export_save`] hands back the image as it
/// powers on — 32KB of `0xff` — and persisting that would overwrite a
/// real save with a blank one just for booting a game and backing out
/// before the title screen.
pub struct SaveWriteback {
    /// The file as it was when the session booted. A cart's live
    /// savedata can be shorter than its file — BN1's SRAM is 32K inside
    /// a 64K `.sav` — so the tail is preserved rather than truncated
    /// away.
    original: Vec<u8>,
    /// The file's contents as last written, so an unchanged save costs
    /// nothing.
    written: Vec<u8>,
}

impl SaveWriteback {
    /// `initial` is the save file's contents as the session booted it.
    pub fn new(initial: Vec<u8>) -> Self {
        Self {
            written: initial.clone(),
            original: initial,
        }
    }

    /// The file contents to write for the cart's `image`, or `None` when
    /// there is nothing to write: no image, one that isn't a save for
    /// `game` yet, or one that changes nothing since the last write.
    pub fn next_write(&self, game: &tango_gamesupport::Game, image: Option<&[u8]>) -> Option<Vec<u8>> {
        self.merge(image?, |image| game.parse_save(image).is_ok())
    }

    /// Record that `file` (as [`next_write`](Self::next_write) returned
    /// it) reached storage. A host whose write failed skips this, and
    /// the next check offers the same bytes again.
    pub fn mark_written(&mut self, file: Vec<u8>) {
        self.written = file;
    }

    fn merge(&self, image: &[u8], is_save: impl FnOnce(&[u8]) -> bool) -> Option<Vec<u8>> {
        if !is_save(image) {
            return None;
        }
        let file = if image.len() >= self.original.len() {
            image.to_vec()
        } else {
            let mut file = self.original.clone();
            file[..image.len()].copy_from_slice(image);
            file
        };
        (file != self.written).then_some(file)
    }
}

impl crate::Session for SinglePlayerSession {
    fn local_game(&self) -> &'static tango_gamesupport::Game {
        self.game
    }

    fn frame(&self) -> Vec<u8> {
        self.screen.read()
    }

    fn screen_layout(&self) -> tango_match::ScreenLayout {
        self.layout.clone()
    }

    fn wake(&self) -> Arc<tokio::sync::Notify> {
        self.wake.clone()
    }

    fn set_input(&self, input: crate::HostInput) {
        self.input.store(input);
    }

    fn set_speed(&self, factor: f32) {
        self.pacing.set_speed(factor);
    }
}

impl Drop for SinglePlayerSession {
    /// Tell whoever is driving to stop. A host running the driver on a
    /// thread joins it itself; the next `tick` there returns false.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The session's emulation step, and everything it needs for the
/// session's life — each an `Arc` shared with the session (and, for the
/// machine, the audio pull). Whoever holds this turns the crank: a
/// drive thread on the desktop, the event loop in a browser.
pub struct Driver {
    console: tango_match::Solo,
    input: Arc<InputCell>,
    pacing: Pacing,
    stop: Arc<AtomicBool>,
    screen: Arc<crate::Framebuffer>,
    wake: Arc<tokio::sync::Notify>,
}

impl crate::Drive for Driver {
    fn tick(&mut self) -> bool {
        Driver::tick(self)
    }

    fn fps_target(&self) -> f32 {
        Driver::fps_target(self)
    }
}

impl Driver {
    /// Run one emulated frame and publish it. `false` once the session
    /// has been dropped, or once emulation has failed — a corrupt core
    /// ends the session rather than panicking the host.
    pub fn tick(&self) -> bool {
        if self.stop.load(Ordering::Relaxed) {
            return false;
        }
        if let Err(e) = self.console.tick(self.input.load()) {
            log::error!("single-player emulation failed: {e}");
            self.stop.store(true, Ordering::Relaxed);
            return false;
        }
        if let Some(frame) = self.console.frame() {
            self.screen.write(&frame);
        }
        // Wake the host's frame subscription so the UI rebuilds the
        // texture for this frame. Notify coalesces — a slow UI doesn't
        // queue up wakes.
        self.wake.notify_one();
        true
    }

    /// The session's current pacing target in fps — what a host paces
    /// `tick` to, and what the audio stream's faux clock follows.
    pub fn fps_target(&self) -> f32 {
        self.pacing.fps_target()
    }
}

#[cfg(test)]
mod tests {
    use super::SaveWriteback;

    fn not_blank(image: &[u8]) -> bool {
        image.iter().any(|&b| b != 0xff)
    }

    #[test]
    fn a_blank_image_is_never_written() {
        let writeback = SaveWriteback::new(vec![1, 2, 3, 4]);
        assert_eq!(writeback.merge(&[0xff; 4], not_blank), None);
    }

    #[test]
    fn a_short_image_keeps_the_file_tail() {
        let mut writeback = SaveWriteback::new(vec![1, 2, 3, 4]);
        let file = writeback.merge(&[5, 6], not_blank).unwrap();
        assert_eq!(file, [5, 6, 3, 4]);
        writeback.mark_written(file);
        assert_eq!(
            writeback.merge(&[7, 8, 9, 10, 11], not_blank).unwrap(),
            [7, 8, 9, 10, 11]
        );
    }

    #[test]
    fn an_unchanged_save_is_skipped_until_written() {
        let mut writeback = SaveWriteback::new(vec![1, 2, 3, 4]);
        assert_eq!(writeback.merge(&[1, 2], not_blank), None);
        // A failed write isn't marked, so the next check retries it.
        assert!(writeback.merge(&[5, 6], not_blank).is_some());
        let file = writeback.merge(&[5, 6], not_blank).unwrap();
        writeback.mark_written(file);
        assert_eq!(writeback.merge(&[5, 6], not_blank), None);
    }
}

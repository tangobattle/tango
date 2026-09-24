//! Plumbing the locally-simulated sessions share: the display surfaces
//! a pair of seats publishes into, and the speed dial single-player and
//! training pace to.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use num_traits::ToPrimitive;

/// The engine's native tick rate for `game`, in frames per second —
/// what a speed dial's 1.0× means and what an audio stream starts at.
pub(crate) fn native_fps(game: &tango_gamesupport::Game) -> f32 {
    game.pvp.tps().to_f32().unwrap()
}

/// A session's display: the main screen, the auxiliary (opponent/PiP)
/// screen, and the repaint wake, clone-shared between the session and
/// whatever publishes into it.
#[derive(Clone)]
pub(crate) struct Surfaces {
    pub(crate) screen: Arc<crate::Framebuffer>,
    /// The other seat's screen, written once per published frame while
    /// the auxiliary surface is on.
    pip: Arc<crate::Framebuffer>,
    /// Whether `pip` holds a frame from the current activation (cleared
    /// while off, so a stale capture never flashes on re-toggle).
    pip_fresh: Arc<AtomicBool>,
    show_pip: Arc<AtomicBool>,
    /// Repaint wake, fired once per published frame pair.
    pub(crate) wake: Arc<tokio::sync::Notify>,
}

impl Surfaces {
    pub(crate) fn new(layout: &tango_match::ScreenLayout, show_pip: bool) -> Self {
        Self {
            screen: crate::Framebuffer::new(layout),
            pip: crate::Framebuffer::new(layout),
            pip_fresh: Arc::new(AtomicBool::new(false)),
            show_pip: Arc::new(AtomicBool::new(show_pip)),
            wake: Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub(crate) fn set_pip_visible(&self, visible: bool) {
        self.show_pip.store(visible, Ordering::Relaxed);
    }

    pub(crate) fn pip_visible(&self) -> bool {
        self.show_pip.load(Ordering::Relaxed)
    }

    /// Copy a (main, other) frame pair into the surfaces and wake the
    /// renderer. Either side may be absent — that surface keeps its last
    /// frame.
    pub(crate) fn publish(&self, main: Option<&[u8]>, other: Option<&[u8]>) {
        if let Some(main) = main {
            self.screen.write(main);
        }
        if self.pip_visible() {
            if let Some(other) = other {
                self.pip.write(other);
                self.pip_fresh.store(true, Ordering::Relaxed);
            }
        } else {
            self.pip_fresh.store(false, Ordering::Relaxed);
        }
        // One wake for the pair, after both surfaces are up.
        self.wake.notify_one();
    }

    /// The auxiliary screen — `None` while it is off or before its first
    /// captured frame.
    pub(crate) fn pip_frame(&self) -> Option<Vec<u8>> {
        (self.show_pip.load(Ordering::Relaxed) && self.pip_fresh.load(Ordering::Relaxed)).then(|| self.pip.read())
    }
}

/// A local session's speed dial: the engine's native rate and the
/// pacing target the drive loop and audio stream follow, as f32 bits.
/// Realtime by default; fast-forward raises it and the audio stream
/// compresses to match.
#[derive(Clone)]
pub(crate) struct Pacing {
    expected_fps: f32,
    fps_bits: Arc<AtomicU32>,
}

impl Pacing {
    pub(crate) fn new(game: &tango_gamesupport::Game) -> Self {
        Self::at(native_fps(game))
    }

    /// A dial for an engine whose native rate is `expected_fps`.
    pub(crate) fn at(expected_fps: f32) -> Self {
        Self {
            expected_fps,
            fps_bits: Arc::new(AtomicU32::new(expected_fps.to_bits())),
        }
    }

    /// An audio stream over `audio_out` whose rate control follows this
    /// dial.
    pub(crate) fn audio_stream(&self, audio_out: tango_match::AudioOut, sample_rate: u32) -> crate::audio::Stream {
        crate::audio::Stream::new(
            audio_out,
            self.expected_fps,
            crate::audio::Stream::fps_from_bits(self.fps_bits.clone()),
            sample_rate,
        )
    }

    /// Fast-forward pacing target: native rate × `factor`, clamped to
    /// `[1, 4×native]`. Above ~4× one audio callback interval's
    /// production overshoots the [`Stream`](crate::audio::Stream)
    /// discard cap and fast-forward turns into constant skips, so the
    /// clamp keeps it coherent.
    pub(crate) fn set_speed(&self, factor: f32) {
        let target = (self.expected_fps * factor).clamp(1.0, self.expected_fps * 4.0);
        self.fps_bits.store(target.to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn fps_target(&self) -> f32 {
        f32::from_bits(self.fps_bits.load(Ordering::Relaxed))
    }
}

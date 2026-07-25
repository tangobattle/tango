//! The encoder interface [`crate::Session`] drives.
//!
//! Two implementations exist — ffmpeg subprocesses on native,
//! WebCodecs in the browser — and everything downstream of this trait
//! (timing, interleaving, muxing, chapters) is shared. Tracks are
//! numbered the way the containers number them: 0 is video, 1 and up are
//! the audio tracks in order.
//!
//! Encoders are pipelines, not functions: work goes in, packets come out
//! later, and the ffmpeg path has an operating system pipe in between.
//! So the trait is a submit/poll pair rather than something that returns
//! a packet per frame, and the shutdown is two-phase — ask the encoders
//! to finish, then poll until they have. That's what lets one session
//! serve a blocking backend and an event-loop one without either
//! pretending to be the other.

use crate::Packet;

/// Track index of the video stream. Audio tracks follow it.
pub const VIDEO_TRACK: usize = 0;

pub trait Backend {
    /// Hand over one frame, tightly packed RGBA at the input size from
    /// [`crate::VideoSettings`]. May block while the encoder catches up,
    /// which is the backpressure that stops an export from running ahead
    /// of its encoders and into memory.
    fn submit_video(&mut self, frame: &[u8]) -> crate::Result<()>;

    /// Hand over interleaved samples for one audio track.
    fn submit_audio(&mut self, track: usize, samples: &[i16]) -> crate::Result<()>;

    /// Collect whatever the encoders have finished, as `(track, packet)`
    /// pairs with timestamps already in each track's timebase. Must not
    /// block: a backend with nothing ready returns an empty vector.
    fn poll(&mut self) -> crate::Result<Vec<(usize, Packet)>>;

    /// Frames submitted that the encoders haven't produced packets for
    /// yet — the backpressure signal for a caller that can't be blocked.
    ///
    /// A backend whose [`Backend::submit_video`] blocks until the
    /// encoder keeps up (the ffmpeg one, whose pipes do it for free)
    /// needs none and reports 0 — the default. One that accepts
    /// everything and queues it (WebCodecs, which can't block an event
    /// loop) reports its queue, so an export can stop feeding it
    /// instead of running ahead into memory.
    fn queue_depth(&self) -> u32 {
        0
    }

    /// A track's codec configuration in the form containers want it
    /// (`avcC` for H.264, `AudioSpecificConfig` for AAC, `fLaC` magic
    /// and metadata blocks for FLAC), or `None` while the encoder hasn't
    /// revealed it yet.
    ///
    /// This is why a container's header can't be written when an export
    /// opens: H.264's parameter sets don't exist until a frame has been
    /// encoded.
    fn codec_private(&self, track: usize) -> Option<Vec<u8>>;

    /// Encoder priming on an audio track, in samples — what a player
    /// must discard to stay in sync. 0 for video and for codecs with
    /// none.
    fn codec_delay_samples(&self, track: usize) -> u64;

    /// Stop accepting input and start finishing. Packets keep arriving
    /// through [`Backend::poll`] afterwards.
    fn begin_flush(&mut self) -> crate::Result<()>;

    /// Whether the encoders are done. Called repeatedly after
    /// [`Backend::begin_flush`].
    ///
    /// Backends that can block (native, on its own thread) may simply
    /// wait here and return `true`; a backend on an event loop must
    /// return `false` and let the caller come back.
    fn poll_flush(&mut self) -> crate::Result<bool>;
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod ffmpeg;
#[cfg(target_arch = "wasm32")]
pub(crate) mod webcodecs;

#[cfg(not(target_arch = "wasm32"))]
pub use ffmpeg::FfmpegBackend;
#[cfg(target_arch = "wasm32")]
pub use webcodecs::WebCodecsBackend;

/// The encoder this target has — ffmpeg subprocesses natively, WebCodecs
/// in the browser.
///
/// There is exactly one per platform, so which to use isn't a choice a
/// caller makes: it's what [`Session::new`](crate::Session::new) opens,
/// and the reason nothing above this crate has to know a backend exists.
#[cfg(not(target_arch = "wasm32"))]
pub type PlatformBackend = FfmpegBackend;
#[cfg(target_arch = "wasm32")]
pub type PlatformBackend = WebCodecsBackend;

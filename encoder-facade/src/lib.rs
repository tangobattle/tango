//! Video/audio encoding behind one API.
//!
//! An encoder [`Backend`] feeds one [`Session`], which hands its packets
//! to one set of Rust muxers ([`mux`]). Only the encoding is
//! platform-specific; timing, interleaving, containers and chapters are
//! shared:
//!
//! ```text
//!   frames + samples ──► Backend ──► [`Packet`] ──► [`Session`] ──► [`mux`] ──► bytes
//! ```
//!
//! Two backends ship here, one per target: `FfmpegBackend` runs a
//! subprocess per stream and reads back elementary streams that [`es`]
//! turns into packets, and `WebCodecsBackend` drives the browser's
//! `VideoEncoder`/`AudioEncoder` on wasm32. The seam is [`Packet`] — one
//! encoded access unit, timestamped in its track's own integer timebase
//! — so a third encoder plugs in by implementing [`Backend`] and nothing
//! downstream changes.
//!
//! The browser is why [`Backend`] looks the way it does: submit/poll
//! rather than a call that returns a packet, and a two-phase finish, so
//! an event loop can drive an export without anything blocking on it.
//!
//! Nothing here does I/O. A session produces bytes to append and, at the
//! close, [`mux::Patch`]es to write back over positions already passed —
//! so a caller holding a [`std::fs::File`] and one awaiting a browser's
//! file stream drive the same session the same way. Native callers can
//! hand both to [`Output`].

pub mod mux;

mod backend;
mod cancel;
mod codec;
mod error;
mod packet;
mod session;

use error::check;

pub use backend::{Backend, VIDEO_TRACK};
pub use cancel::Canceller;
pub use error::{Error, Result};
pub use mux::{Chapter, Container};
pub use packet::{AudioTrackInfo, ColorInfo, Packet, VideoTrackInfo};
pub use session::Session;

#[cfg(not(target_arch = "wasm32"))]
pub mod es;
#[cfg(not(target_arch = "wasm32"))]
mod ffmpeg;
#[cfg(not(target_arch = "wasm32"))]
mod output;

#[cfg(not(target_arch = "wasm32"))]
pub use ffmpeg::FfmpegBackend;
#[cfg(not(target_arch = "wasm32"))]
pub use output::Output;

#[cfg(target_arch = "wasm32")]
mod webcodecs;
#[cfg(target_arch = "wasm32")]
pub use webcodecs::WebCodecsBackend;

/// Video codecs a backend can produce and a muxer can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    Vp8,
    Vp9,
}

/// Audio codecs a backend can produce and a muxer can carry.
///
/// [`AudioCodec::PcmS16Le`] never reaches an encoder — the samples the
/// caller already holds *are* the packets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioCodec {
    Aac,
    Opus,
    Flac,
    PcmS16Le,
}

/// How hard the video encoder should work. Quality-targeted rather than
/// bitrate-targeted: a replay is a fixed-size screen capture, so a
/// quality floor gives predictable output where a bitrate target would
/// swing with the content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoQuality {
    /// Mathematically lossless. H.264 only, and it implies the RGB
    /// encoder — no chroma subsampling to lose — which is why it can't
    /// be muxed into MP4; see [`Container`].
    Lossless,
    /// x264's constant-rate-factor scale: lower is better, 18 is a good
    /// default, 51 is the floor.
    Crf(u8),
    /// Target bitrate in bits per second. What WebCodecs takes, and the
    /// only knob VP8/VP9 gets here.
    Bitrate(u32),
}

/// The video half of an export.
#[derive(Clone, Debug)]
pub struct VideoSettings {
    pub codec: VideoCodec,
    pub quality: VideoQuality,
    /// Size of the frames the caller will push.
    pub width: u32,
    pub height: u32,
    /// Integer nearest-neighbor upscale applied before encoding, so
    /// players that smooth-scale can't blur pixel art. 1 encodes frames
    /// at their input size.
    ///
    /// Which side scales is the backend's business: ffmpeg scales in its
    /// own filtergraph (keeping the frame pipe small — at 10× the
    /// pre-scaled bytes would be hundreds of megabytes a second),
    /// WebCodecs has no scaler so that backend expands in Rust. Either
    /// way callers push frames at the input size.
    pub scale: u32,
    /// Frames between forced keyframes. Bounds seek granularity, and on
    /// Matroska it bounds cluster length too.
    pub keyframe_interval: u32,
    /// Ticks per second for this track's timestamps.
    pub timescale: u32,
    /// One frame's length in `timescale` ticks. Constant — an emulator
    /// export has no variable frame timing — which is what lets a
    /// backend stamp packets from a frame counter instead of trying to
    /// recover timestamps from an encoder.
    pub frame_duration: u64,
    /// Color tags for the container. The H.264 encoder additionally
    /// writes them into the bitstream's VUI, which is what decoders
    /// actually read; this is for players that trust the container.
    pub color: Option<ColorInfo>,
}

impl VideoSettings {
    /// Encoded frame width: the input size times the scale.
    pub fn output_width(&self) -> u32 {
        self.width * self.scale
    }

    pub fn output_height(&self) -> u32 {
        self.height * self.scale
    }

    /// Bytes in one RGBA frame at the input size.
    pub fn frame_bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}

/// The audio half of an export. One [`AudioSettings`] covers every audio
/// track — a two-sided export encodes both sides the same way.
#[derive(Clone, Debug)]
pub struct AudioSettings {
    pub codec: AudioCodec,
    pub sample_rate: u32,
    pub channels: u8,
    /// Per-track bitrate in bits per second. Ignored by the lossless
    /// codecs.
    pub bitrate: u32,
}

/// Everything an export needs that isn't the frames themselves.
#[derive(Clone, Debug)]
pub struct Settings {
    pub video: VideoSettings,
    pub audio: AudioSettings,
    pub container: Container,
    /// How many audio tracks to open. A two-sided layout carries one per
    /// side.
    pub audio_tracks: usize,
}

impl Settings {
    /// Check the codec, container and geometry choices, so a bad
    /// combination fails before an encoder is spawned rather than at the
    /// first packet.
    pub fn validate(&self) -> crate::Result<()> {
        check!(self.audio_tracks > 0, "an export needs at least one audio track");
        check!(self.video.scale >= 1, "scale must be at least 1");
        check!(
            self.video.width > 0 && self.video.height > 0,
            "an export needs a nonzero frame size"
        );
        check!(self.video.frame_duration > 0, "frame duration must be nonzero");
        check!(self.video.timescale > 0, "timescale must be nonzero");
        check!(self.audio.sample_rate > 0, "sample rate must be nonzero");
        self.container.accepts(self.video.codec, self.audio.codec)?;
        if self.video.quality == VideoQuality::Lossless {
            check!(
                self.video.codec == VideoCodec::H264,
                "lossless video is only supported for H.264, not {:?}",
                self.video.codec
            );
        }
        Ok(())
    }
}

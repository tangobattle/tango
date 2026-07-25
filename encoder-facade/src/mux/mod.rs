//! Muxing: the half of the pipeline both backends share.
//!
//! A muxer takes [`Packet`]s and produces container bytes. It never
//! touches the filesystem: bytes accumulate in an internal buffer that
//! the caller drains with [`Muxer::take_output`] and appends wherever
//! the output is going, and the few fields that can only be filled in
//! once the stream ends come back from [`Muxer::finish`] as
//! [`Patch`]es to apply at absolute offsets.
//!
//! That inversion is what lets one muxer serve both targets. Native
//! writes to a [`std::fs::File`] synchronously; in the browser the same
//! bytes go to a `FileSystemWritableFileStream` whose writes are all
//! `await`ed. Neither concern reaches the muxers, which stay
//! synchronous, allocation-bounded and testable against a `Vec<u8>`.

use crate::{AudioCodec, AudioTrackInfo, Packet, VideoCodec, VideoTrackInfo};

mod matroska;
mod mp4;

/// Which container to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    Mp4,
    /// Matroska proper: carries every codec here, including the
    /// lossless pairing MP4 can't.
    Matroska,
    /// WebM — Matroska restricted to the codecs the format allows, and
    /// what the WebCodecs backend produces.
    WebM,
}

impl Container {
    /// Whether this container may carry `video` alongside `audio`,
    /// checked before anything is encoded so a bad pairing can't fail
    /// halfway through an export.
    pub fn accepts(self, video: VideoCodec, audio: AudioCodec) -> crate::Result<()> {
        let ok = match self {
            // Everything here is muxed the way ISO/IEC 14496 defines
            // it; VP8/VP9 in MP4 exist but nothing needs them, so
            // they're refused rather than half-supported.
            Container::Mp4 => matches!(
                (video, audio),
                (VideoCodec::H264, AudioCodec::Aac) | (VideoCodec::H264, AudioCodec::Opus)
            ),
            Container::Matroska => true,
            Container::WebM => {
                matches!(video, VideoCodec::Vp8 | VideoCodec::Vp9) && audio == AudioCodec::Opus
            }
        };
        if !ok {
            return Err(crate::Error::CodecNotInContainer {
                container: self,
                video,
                audio,
            });
        }
        Ok(())
    }

    /// The file extension conventionally used for this container.
    pub fn extension(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Matroska => "mkv",
            Container::WebM => "webm",
        }
    }
}

/// A named span of the output, for the container's chapter list.
///
/// Bounds are in *output video frames* — frames actually written, not
/// source ticks — so a chapter list stays contiguous even when an
/// export skips parts of its source.
#[derive(Clone, Debug)]
pub struct Chapter {
    pub title: String,
    pub start_frame: u64,
    pub end_frame: u64,
}

/// Bytes that have to be written back over a position already passed:
/// a size that wasn't known until the stream ended, or a header field
/// reserved up front and filled in at the close.
#[derive(Clone, Debug)]
pub struct Patch {
    pub position: u64,
    pub bytes: Vec<u8>,
}

/// Everything a muxer needs about the streams it's about to carry.
/// Assembled once the backend knows each track's codec configuration.
#[derive(Clone, Debug)]
pub struct MuxConfig {
    pub container: Container,
    pub video: VideoTrackInfo,
    pub audio: Vec<AudioTrackInfo>,
    pub writing_app: String,
}

/// A container writer. One per export.
pub enum Muxer {
    Mp4(mp4::Mp4Muxer),
    Matroska(matroska::MatroskaMuxer),
}

impl Muxer {
    /// Open a container and write its header. Fails if any track's
    /// codec doesn't belong in this container.
    pub fn new(config: MuxConfig) -> crate::Result<Self> {
        for audio in &config.audio {
            config.container.accepts(config.video.codec, audio.codec)?;
        }
        Ok(match config.container {
            Container::Mp4 => Muxer::Mp4(mp4::Mp4Muxer::new(config)?),
            Container::Matroska | Container::WebM => Muxer::Matroska(matroska::MatroskaMuxer::new(config)?),
        })
    }

    pub fn write_video(&mut self, packet: &Packet) -> crate::Result<()> {
        match self {
            Muxer::Mp4(m) => m.write(mp4::VIDEO_TRACK, packet),
            Muxer::Matroska(m) => m.write(matroska::VIDEO_TRACK, packet),
        }
    }

    /// `track` indexes [`MuxConfig::audio`].
    pub fn write_audio(&mut self, track: usize, packet: &Packet) -> crate::Result<()> {
        match self {
            Muxer::Mp4(m) => m.write(mp4::VIDEO_TRACK + 1 + track, packet),
            Muxer::Matroska(m) => m.write(matroska::VIDEO_TRACK + 1 + track, packet),
        }
    }

    /// Container bytes produced so far. Call regularly — this is what
    /// keeps a muxer's memory bounded rather than growing with the
    /// length of the export.
    pub fn take_output(&mut self) -> Vec<u8> {
        match self {
            Muxer::Mp4(m) => m.take_output(),
            Muxer::Matroska(m) => m.take_output(),
        }
    }

    /// Close the container: write everything that belongs at the end
    /// (indexes, chapters, cues) and report the patches that finish the
    /// parts written earlier. Drain [`Muxer::take_output`] once more
    /// after this, then apply the patches.
    pub fn finish(&mut self, chapters: &[Chapter]) -> crate::Result<Vec<Patch>> {
        match self {
            Muxer::Mp4(m) => m.finish(chapters),
            Muxer::Matroska(m) => m.finish(chapters),
        }
    }
}

/// Convert a chapter's frame bounds to nanoseconds using the video
/// track's timebase.
pub(crate) fn chapter_bounds_ns(chapter: &Chapter, video: &VideoTrackInfo) -> (u64, u64) {
    let ticks = |frame: u64| frame.saturating_mul(video.frame_duration);
    (
        crate::packet::ticks_to_ns(ticks(chapter.start_frame), video.timescale),
        crate::packet::ticks_to_ns(ticks(chapter.end_frame), video.timescale),
    )
}

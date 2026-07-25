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

use crate::settings::{AudioCodec, VideoCodec};
use crate::{AudioTrackInfo, Packet, VideoTrackInfo};

mod matroska;
mod mp4;

/// Which container to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    Mp4,
    /// Matroska: carries every codec here, including the lossless
    /// pairing MP4 can't.
    Matroska,
}

impl Container {
    /// Whether this container may carry `video` alongside `audio`,
    /// checked before anything is encoded so a bad pairing can't fail
    /// halfway through an export.
    pub fn accepts(self, video: VideoCodec, audio: AudioCodec) -> crate::Result<()> {
        // FLAC in MP4 is defined, but the lossless pairing goes to
        // Matroska anyway, so MP4 carries the one combination ISO/IEC
        // 14496 is unambiguous about rather than half-supporting a
        // second.
        let ok = match self {
            Container::Mp4 => matches!(audio, AudioCodec::Aac { .. }),
            Container::Matroska => true,
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

/// A correction to bytes already written, reported when a container
/// closes and applied in the order given.
#[derive(Clone, Debug)]
pub enum Fixup {
    /// Write these bytes over a position already passed: a size that
    /// wasn't known until the stream ended, or a header field reserved up
    /// front and filled in at the close.
    Overwrite { position: u64, bytes: Vec<u8> },
    /// Put these bytes *into* the output at this position, moving
    /// everything after it along.
    ///
    /// This is how MP4 reaches `faststart` layout: its index can only be
    /// built once the stream ends, but belongs in front of the media, so
    /// the media has to shift to make room. Costs one pass over the file,
    /// which is why it only happens when asked for.
    Insert { position: u64, bytes: Vec<u8> },
}

impl Fixup {
    pub fn position(&self) -> u64 {
        match self {
            Fixup::Overwrite { position, .. } | Fixup::Insert { position, .. } => *position,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        match self {
            Fixup::Overwrite { bytes, .. } | Fixup::Insert { bytes, .. } => bytes,
        }
    }
}

/// Everything a muxer needs about the streams it's about to carry.
/// Assembled once the backend knows each track's codec configuration.
#[derive(Clone, Debug)]
pub struct MuxConfig {
    pub container: Container,
    pub video: VideoTrackInfo,
    pub audio: Vec<AudioTrackInfo>,
    pub writing_app: String,
    /// Put the index in front of the media, so a player reading the file
    /// in order can start before it has all of it. MP4 only; Matroska
    /// always writes a seek index at the head.
    pub faststart: bool,
}

/// Everything a container has left to say once it's closed.
#[derive(Debug, Default)]
pub struct Finished {
    /// The last bytes to append — indexes, chapters, cues.
    pub bytes: Vec<u8>,
    /// Corrections to what was written earlier, to apply in order.
    pub fixups: Vec<Fixup>,
}

/// A container writer. One per export.
pub trait Muxer {
    /// Add one packet to a track, numbered as [`crate::VIDEO_TRACK`] and
    /// the audio tracks after it. Packets arrive in time order across
    /// tracks — [`crate::Session`] guarantees that much — so a muxer
    /// never has to reorder.
    fn write(&mut self, track: usize, packet: &Packet) -> crate::Result<()>;

    /// Container bytes produced so far. Called regularly, which is what
    /// keeps a muxer's memory flat rather than growing with the length of
    /// the export.
    fn take_output(&mut self) -> Vec<u8>;

    /// Close the container. Takes the muxer by value: closing is the last
    /// thing that happens to it, so nothing can be written afterwards and
    /// nothing can close it twice.
    fn finish(self: Box<Self>, chapters: &[Chapter]) -> crate::Result<Finished>;
}

/// Open a container and write its header. Fails if any track's codec
/// doesn't belong in the container asked for.
pub fn open(config: MuxConfig) -> crate::Result<Box<dyn Muxer>> {
    for audio in &config.audio {
        config.container.accepts(config.video.codec, audio.codec)?;
    }
    Ok(match config.container {
        Container::Mp4 => Box::new(mp4::Mp4Muxer::new(config)?),
        Container::Matroska => Box::new(matroska::MatroskaMuxer::new(config)?),
    })
}

/// Apply fixups to a finished file in memory, the way a caller's
/// [`crate::Output`] does to one on disk — so the tests exercise the
/// same application path the real thing uses.
#[cfg(test)]
pub(crate) fn apply(file: Vec<u8>, fixups: &[Fixup]) -> Vec<u8> {
    let output = crate::Output::new(std::io::Cursor::new(file));
    output.finish(fixups).expect("apply the fixups").into_inner()
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

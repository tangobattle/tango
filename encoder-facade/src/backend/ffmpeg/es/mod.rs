//! Elementary-stream parsing: what the ffmpeg backend reads back from
//! its encoders.
//!
//! ffmpeg is used strictly as an *encoder* here — it writes a bare
//! elementary stream (`-f h264`, `-f adts`, `-f ogg`, `-f ivf`) on its
//! stdout and never muxes anything. These parsers turn that byte
//! stream back into discrete [`crate::Packet`]s plus the codec
//! configuration a container needs, so the muxing is ours regardless
//! of which backend did the encoding.
//!
//! Every parser is a push parser: bytes arrive in whatever sizes a
//! pipe read produced, and complete units come out. None of them
//! trusts the stream for timing — an elementary stream mostly has none
//! — so each packet reports its *duration* in the stream's own unit
//! (samples for audio, frames for video) and the backend accumulates
//! presentation times from a running total. That keeps timestamps
//! exact and monotonic without a timestamp ever crossing a pipe.

use crate::{AudioCodec, VideoCodec};

mod aac;
mod h264;
mod ivf;
mod xiph;

/// One parsed access unit, not yet timestamped.
#[derive(Clone, Debug)]
pub struct EsPacket {
    pub data: Vec<u8>,
    pub keyframe: bool,
    /// Length of this unit in the stream's natural unit: PCM samples
    /// per channel for audio, frames for video.
    pub duration: u64,
}

/// A push parser for one elementary stream.
pub trait EsParser {
    /// Take the next slice of encoder output. Whatever completes gets
    /// queued for [`EsParser::take_packets`].
    fn push(&mut self, bytes: &[u8]) -> crate::Result<()>;

    /// End of stream. Only formats whose units are delimited by the
    /// *start* of the next one — H.264 access units — have anything
    /// left to emit here.
    fn flush(&mut self) -> crate::Result<()> {
        Ok(())
    }

    fn take_packets(&mut self) -> Vec<EsPacket>;

    /// Codec-private data in the form containers want it, once the
    /// stream has revealed it: `None` until then. For H.264 that means
    /// after the first frame, for Ogg after the first page. Codecs that
    /// need none (VP8/VP9 carry their configuration in the bitstream)
    /// report an empty vector immediately.
    fn codec_private(&self) -> Option<Vec<u8>>;

    /// Encoder priming this stream declares for itself, in samples.
    /// Only Opus does, in its `OpusHead`; for everything else the
    /// backend knows its encoder's delay and supplies it.
    fn declared_codec_delay(&self) -> Option<u64> {
        None
    }
}

pub fn for_video(codec: VideoCodec) -> Box<dyn EsParser> {
    match codec {
        VideoCodec::H264 => Box::new(h264::H264Parser::new()),
        VideoCodec::Vp8 => Box::new(ivf::IvfParser::new(VideoCodec::Vp8)),
        VideoCodec::Vp9 => Box::new(ivf::IvfParser::new(VideoCodec::Vp9)),
    }
}

/// `None` for [`AudioCodec::PcmS16Le`], which never goes through an
/// encoder — the caller's samples are already the packets.
pub fn for_audio(codec: AudioCodec) -> Option<Box<dyn EsParser>> {
    match codec {
        AudioCodec::Aac => Some(Box::new(aac::AacParser::new())),
        AudioCodec::Opus => Some(Box::new(xiph::XiphParser::new(xiph::Mapping::Opus))),
        AudioCodec::Flac => Some(Box::new(xiph::XiphParser::new(xiph::Mapping::Flac))),
        AudioCodec::PcmS16Le => None,
    }
}

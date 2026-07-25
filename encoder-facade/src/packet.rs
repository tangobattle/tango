//! What flows from an encoder to a container: one encoded access unit,
//! plus the per-track facts a container needs to describe the stream it
//! belongs to.

use crate::settings::{AudioCodec, ColorInfo, VideoCodec};

/// One encoded access unit — a video frame or an audio frame — ready to
/// be muxed.
///
/// `pts` and `duration` are in the track's own integer timebase
/// (`timescale` ticks per second), never in seconds or milliseconds.
/// That's deliberate: a GBA frame is 280896/16777216 s, which is exact
/// in ticks of 1/16777216 s and *inexact* in any decimal unit, so
/// keeping integers here means the video clock neither drifts nor
/// jitters however long an export runs. Each container rounds to its own
/// resolution at write time, always from the exact tick value rather
/// than from a running total.
///
/// There is no separate DTS: backends are configured to produce streams
/// without frame reordering, so presentation order is storage order.
#[derive(Clone, Debug)]
pub struct Packet {
    pub pts: u64,
    pub duration: u64,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

/// What a container needs to know about the video track.
///
/// Produced once the encoder has revealed its codec configuration — for
/// H.264 the SPS and PPS, which only exist after the first frame — which
/// is why muxing can't begin until the first packet has been parsed.
#[derive(Clone, Debug)]
pub struct VideoTrackInfo {
    pub codec: VideoCodec,
    /// Encoded frame size: the input size times the scale.
    pub width: u32,
    pub height: u32,
    pub timescale: u32,
    pub frame_duration: u64,
    pub color: Option<ColorInfo>,
    /// Codec-private data as the container wants it: an `avcC`
    /// configuration record for H.264, empty for VP8/VP9, which carry
    /// their configuration in the bitstream.
    pub codec_private: Vec<u8>,
}

/// What a container needs to know about one audio track.
#[derive(Clone, Debug)]
pub struct AudioTrackInfo {
    pub codec: AudioCodec,
    pub sample_rate: u32,
    pub channels: u8,
    /// `AudioSpecificConfig` for AAC, `fLaC` magic plus metadata blocks
    /// for FLAC.
    pub codec_private: Vec<u8>,
    /// Encoder priming: samples the decoder emits before the real audio
    /// starts, which a player must discard to stay in sync.
    ///
    /// Every lossy audio encoder has some — AAC's MDCT costs a full
    /// 1024-sample frame — and left unsignalled it becomes a fixed
    /// audio lag against the video
    /// (21 ms for AAC at 48 kHz, enough to notice). Containers signal it
    /// differently (Matroska `CodecDelay`, MP4 edit list), so it travels
    /// as samples and each muxer converts.
    pub codec_delay_samples: u64,
}

impl AudioTrackInfo {
    /// The priming delay in nanoseconds, for containers that want it as a
    /// duration.
    pub fn codec_delay_ns(&self) -> u64 {
        if self.sample_rate == 0 {
            return 0;
        }
        (self.codec_delay_samples as u128 * 1_000_000_000 / self.sample_rate as u128) as u64
    }
}

/// Convert a tick count in `timescale` units to nanoseconds, rounding to
/// nearest. Done in 128-bit so a long export can't overflow.
pub(crate) fn ticks_to_ns(ticks: u64, timescale: u32) -> u64 {
    if timescale == 0 {
        return 0;
    }
    let num = ticks as u128 * 1_000_000_000;
    let den = timescale as u128;
    ((num + den / 2) / den) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The GBA frame clock is the case that matters: 16777216 ticks/s
    /// with a 280896-tick frame. Timestamps must stay exact over a long
    /// export instead of accumulating rounding error.
    #[test]
    fn gba_frame_clock_does_not_drift() {
        const TIMESCALE: u32 = 16_777_216;
        const FRAME: u64 = 280_896;
        // One hour of frames: the tick count is exact, so the derived
        // nanosecond time is within half a nanosecond of the true one no
        // matter how far in we are.
        let frames = 215_000u64;
        let ns = ticks_to_ns(frames * FRAME, TIMESCALE);
        let exact = frames as u128 * FRAME as u128 * 1_000_000_000 / TIMESCALE as u128;
        assert!((ns as i128 - exact as i128).abs() <= 1, "ns {ns} vs exact {exact}");
    }

    #[test]
    fn aac_priming_is_one_frame_at_48k() {
        let info = AudioTrackInfo {
            codec: AudioCodec::Aac { bitrate: 384_000 },
            sample_rate: 48_000,
            channels: 2,
            codec_private: vec![],
            codec_delay_samples: crate::settings::AAC_SAMPLES_PER_FRAME,
        };
        // 1024/48000 s = 21.333 ms.
        assert_eq!(info.codec_delay_ns(), 21_333_333);
    }
}

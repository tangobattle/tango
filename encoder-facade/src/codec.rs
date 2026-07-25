//! Reading a compressed audio packet's length out of the packet itself.
//!
//! Both backends need this and neither is told it: an elementary stream
//! carries no timestamps, and WebCodecs doesn't report how many samples
//! a chunk decodes to. Every codec here states it in its own first
//! bytes, so the length is read from the packet rather than guessed —
//! which is what keeps a track's timeline exact rather than merely
//! plausible.

use crate::AudioCodec;

/// Samples per AAC frame with `frameLengthFlag` clear, which is what
/// every encoder here produces.
pub(crate) const AAC_SAMPLES_PER_FRAME: u64 = 1024;

/// Samples in one encoded audio packet, or `None` when the codec doesn't
/// say and the caller should fall back to what the stream declared.
///
/// The native path doesn't need this — its parsers already know each
/// unit's length from the framing they read — but the tests below do, so
/// it stays compiled rather than gated out of the build that can test it.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn audio_packet_samples(codec: AudioCodec, data: &[u8]) -> Option<u64> {
    match codec {
        AudioCodec::Aac => Some(AAC_SAMPLES_PER_FRAME),
        AudioCodec::Opus => Some(opus_packet_samples(data)),
        AudioCodec::Flac => flac_frame_blocksize(data),
        // PCM is timed by the caller's sample count; it never reaches an
        // encoder to be asked.
        AudioCodec::PcmS16Le => None,
    }
}

/// Samples in an Opus packet, from its TOC byte (RFC 6716 §3.1). Exact
/// in 48 kHz samples: 2.5 ms is 120 samples.
pub(crate) fn opus_packet_samples(data: &[u8]) -> u64 {
    let Some(&toc) = data.first() else {
        return 0;
    };
    let config = toc >> 3;
    let frame = if config < 12 {
        // SILK modes: 10, 20, 40 or 60 ms.
        [480, 960, 1920, 2880][(config & 0b11) as usize]
    } else if config < 16 {
        // Hybrid modes: 10 or 20 ms.
        [480, 960][(config & 0b1) as usize]
    } else {
        // CELT modes: 2.5, 5, 10 or 20 ms.
        [120, 240, 480, 960][(config & 0b11) as usize]
    };
    let frames = match toc & 0b11 {
        0 => 1,
        1 | 2 => 2,
        // Arbitrary count, in the low 6 bits of the frame-count byte.
        _ => data.get(1).map(|b| (b & 0b0011_1111) as u64).max(Some(1)).unwrap_or(1),
    };
    frame * frames
}

/// Block size from a FLAC frame header's high nibble of byte 2.
///
/// Codes 6 and 7 store the size after the frame's coded sample number
/// rather than in the header, and reading it would mean walking a
/// UTF-8-coded integer first; those return `None`. Encoders here use a
/// fixed 4096-sample block (code 12), so the fallback applies to the
/// final short frame at most, where a slightly long duration only
/// affects the container's declared length.
pub(crate) fn flac_frame_blocksize(data: &[u8]) -> Option<u64> {
    match data.get(2)? >> 4 {
        0 | 6 | 7 => None,
        1 => Some(192),
        code @ 2..=5 => Some(576 << (code - 2)),
        code => Some(256 << (code - 8)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The digit groups below are the TOC byte's fields: 5 bits of
    // configuration, 3 of frame count.
    #[allow(clippy::unusual_byte_groupings)]
    #[test]
    fn opus_frame_durations_cover_every_mode() {
        // config 0 (10 ms), config 16 (2.5 ms), config 3 (60 ms).
        assert_eq!(opus_packet_samples(&[0b00000_000]), 480);
        assert_eq!(opus_packet_samples(&[0b10000_000]), 120);
        assert_eq!(opus_packet_samples(&[0b00011_000]), 2880);
        // Two-frame codes double it; an arbitrary count reads the
        // frame-count byte.
        assert_eq!(opus_packet_samples(&[0b00001_001]), 1920);
        assert_eq!(opus_packet_samples(&[0b00001_011, 5]), 4800);
    }

    #[test]
    fn flac_block_sizes() {
        assert_eq!(flac_frame_blocksize(&[0xFF, 0xF8, 0x19]), Some(192));
        assert_eq!(flac_frame_blocksize(&[0xFF, 0xF8, 0x29]), Some(576));
        assert_eq!(flac_frame_blocksize(&[0xFF, 0xF8, 0xC9]), Some(4096));
        assert_eq!(flac_frame_blocksize(&[0xFF, 0xF8, 0x69]), None, "out-of-line size");
    }

    #[test]
    fn aac_frames_are_a_fixed_length() {
        assert_eq!(audio_packet_samples(AudioCodec::Aac, &[]), Some(1024));
    }

    #[test]
    fn pcm_is_timed_by_the_caller() {
        assert_eq!(audio_packet_samples(AudioCodec::PcmS16Le, &[0, 0]), None);
    }
}

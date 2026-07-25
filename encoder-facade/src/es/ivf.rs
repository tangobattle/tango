//! IVF → VP8/VP9 frames.
//!
//! IVF is the one framed container ffmpeg will write for VP8/VP9
//! without muxing them into WebM: a 32-byte file header followed by a
//! 12-byte length-and-timestamp header per frame. Nothing here needs
//! the file header's own timing fields — frame timing comes from the
//! caller's constant frame rate — so it is checked and skipped.

use super::{EsPacket, EsParser};
use crate::VideoCodec;

const FILE_HEADER: usize = 32;
const FRAME_HEADER: usize = 12;

pub struct IvfParser {
    codec: VideoCodec,
    buf: Vec<u8>,
    saw_file_header: bool,
    packets: Vec<EsPacket>,
}

impl IvfParser {
    pub fn new(codec: VideoCodec) -> Self {
        Self {
            codec,
            buf: Vec::new(),
            saw_file_header: false,
            packets: Vec::new(),
        }
    }

}

impl EsParser for IvfParser {
    fn push(&mut self, bytes: &[u8]) -> crate::Result<()> {
        self.buf.extend_from_slice(bytes);
        if !self.saw_file_header {
            if self.buf.len() < FILE_HEADER {
                return Ok(());
            }
            if &self.buf[..4] != b"DKIF" {
                return Err(crate::Error::bitstream("IVF", "the stream does not start with DKIF"));
            }
            let header_len = u16::from_le_bytes([self.buf[6], self.buf[7]]) as usize;
            if header_len < FILE_HEADER || self.buf.len() < header_len {
                return Err(crate::Error::bitstream(
                    "IVF",
                    format!("declared header length {header_len} is out of range"),
                ));
            }
            self.buf.drain(..header_len);
            self.saw_file_header = true;
        }
        let mut consumed = 0;
        while self.buf.len() - consumed >= FRAME_HEADER {
            let head = &self.buf[consumed..consumed + FRAME_HEADER];
            let size = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
            if self.buf.len() - consumed - FRAME_HEADER < size {
                break;
            }
            let start = consumed + FRAME_HEADER;
            let data = self.buf[start..start + size].to_vec();
            self.packets.push(EsPacket {
                keyframe: is_keyframe(self.codec, &data),
                data,
                duration: 1,
            });
            consumed = start + size;
        }
        self.buf.drain(..consumed);
        Ok(())
    }

    fn take_packets(&mut self) -> Vec<EsPacket> {
        std::mem::take(&mut self.packets)
    }

    /// VP8 and VP9 need none: their configuration is in the bitstream.
    fn codec_private(&self) -> Option<Vec<u8>> {
        Some(Vec::new())
    }
}

/// Keyframe flags from the codecs' uncompressed frame headers.
///
/// VP8 (RFC 6386 §9.1) puts an inverted key-frame bit at the bottom of
/// byte 0. VP9 (§6.2) starts with a 2-bit frame marker, two profile
/// bits, a `show_existing_frame` bit and then the frame type, all
/// most-significant-bit first; a shown existing frame carries no type
/// of its own and is never a keyframe.
fn is_keyframe(codec: VideoCodec, data: &[u8]) -> bool {
    let Some(&first) = data.first() else {
        return false;
    };
    match codec {
        VideoCodec::Vp8 => first & 0b1 == 0,
        VideoCodec::Vp9 => first & 0b0000_1000 == 0 && first & 0b0000_0100 == 0,
        VideoCodec::H264 => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_header() -> Vec<u8> {
        let mut h = b"DKIF".to_vec();
        h.extend_from_slice(&0u16.to_le_bytes()); // version
        h.extend_from_slice(&(FILE_HEADER as u16).to_le_bytes());
        h.extend_from_slice(b"VP90");
        h.resize(FILE_HEADER, 0);
        h
    }

    fn frame(payload: &[u8], ts: u64) -> Vec<u8> {
        let mut f = (payload.len() as u32).to_le_bytes().to_vec();
        f.extend_from_slice(&ts.to_le_bytes());
        f.extend_from_slice(payload);
        f
    }

    #[test]
    fn frames_come_out_without_their_headers() {
        let mut stream = file_header();
        // VP9 profile 0: marker 0b10, profile 00, show_existing 0,
        // frame_type 0 -> keyframe; then frame_type 1 -> not.
        stream.extend_from_slice(&frame(&[0b1000_0000, 0xaa], 0));
        stream.extend_from_slice(&frame(&[0b1000_0100, 0xbb, 0xcc], 1));
        let mut p = IvfParser::new(VideoCodec::Vp9);
        p.push(&stream).unwrap();
        assert_eq!(p.packets.len(), 2);
        assert_eq!(p.packets[0].data, vec![0b1000_0000, 0xaa]);
        assert!(p.packets[0].keyframe);
        assert!(!p.packets[1].keyframe);
        assert!(p.packets.iter().all(|pkt| pkt.duration == 1));
    }

    #[test]
    fn vp8_keyframe_bit_is_inverted() {
        assert!(is_keyframe(VideoCodec::Vp8, &[0b0000_0000]));
        assert!(!is_keyframe(VideoCodec::Vp8, &[0b0000_0001]));
    }

    #[test]
    fn a_shown_existing_frame_is_not_a_keyframe() {
        assert!(!is_keyframe(VideoCodec::Vp9, &[0b1000_1000]));
    }

    #[test]
    fn chunking_does_not_change_the_result() {
        let mut stream = file_header();
        stream.extend_from_slice(&frame(&[0b1000_0000, 0xaa], 0));
        stream.extend_from_slice(&frame(&[0b1000_0100, 0xbb], 1));
        for chunk in 1..=stream.len() {
            let mut p = IvfParser::new(VideoCodec::Vp9);
            for part in stream.chunks(chunk) {
                p.push(part).unwrap();
            }
            assert_eq!(p.packets.len(), 2, "chunk size {chunk}");
        }
    }

    #[test]
    fn a_non_ivf_stream_is_an_error() {
        let mut p = IvfParser::new(VideoCodec::Vp9);
        assert!(p.push(&[0u8; 64]).is_err());
    }
}

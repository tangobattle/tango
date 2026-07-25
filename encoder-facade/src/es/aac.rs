//! ADTS AAC → raw AAC frames plus the `AudioSpecificConfig` that MP4's
//! `esds` and Matroska's `CodecPrivate` both carry.
//!
//! ADTS repeats a 7-byte header on every frame; the containers want
//! neither the headers nor the sync words, just the payloads and one
//! out-of-band config. [`adts_reader`] does the framing and validation
//! (sync word, declared length, optional CRC); the three fields the
//! config needs are read straight from the header bits.

use adts_reader::{AdtsHeader, AdtsHeaderError};

use super::{EsPacket, EsParser};
use crate::codec::AAC_SAMPLES_PER_FRAME;

pub struct AacParser {
    buf: Vec<u8>,
    packets: Vec<EsPacket>,
    config: Option<Vec<u8>>,
}

impl AacParser {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            packets: Vec::new(),
            config: None,
        }
    }

}

impl EsParser for AacParser {
    fn push(&mut self, bytes: &[u8]) -> crate::Result<()> {
        self.buf.extend_from_slice(bytes);
        let mut consumed = 0;
        while consumed < self.buf.len() {
            let rest = &self.buf[consumed..];
            let header = match AdtsHeader::from_bytes(rest) {
                Ok(h) => h,
                // A partial header at the tail: wait for more bytes.
                Err(AdtsHeaderError::NotEnoughData { .. }) => break,
                Err(e) => return Err(crate::Error::bitstream("ADTS", format!("{e:?}"))),
            };
            let frame_length = header.frame_length() as usize;
            if rest.len() < frame_length {
                break;
            }
            let payload = header
                .payload()
                .map_err(|e| crate::Error::bitstream("ADTS", format!("{e:?}")))?;
            if self.config.is_none() {
                self.config = Some(audio_specific_config(rest));
            }
            // A frame may carry several raw data blocks; they decode as
            // one unit, so the packet is as long as all of them.
            let blocks = (rest[6] & 0b11) as u64 + 1;
            self.packets.push(EsPacket {
                data: payload.to_vec(),
                // Every AAC frame is independently decodable.
                keyframe: true,
                duration: AAC_SAMPLES_PER_FRAME * blocks,
            });
            consumed += frame_length;
        }
        self.buf.drain(..consumed);
        Ok(())
    }

    fn take_packets(&mut self) -> Vec<EsPacket> {
        std::mem::take(&mut self.packets)
    }

    fn codec_private(&self) -> Option<Vec<u8>> {
        self.config.clone()
    }
}

/// Build the 2-byte `AudioSpecificConfig` from an ADTS header
/// (ISO/IEC 14496-3): 5 bits of audio object type, 4 of sampling
/// frequency index, 4 of channel configuration, then a cleared
/// `frameLengthFlag`, `dependsOnCoreCoder` and `extensionFlag`.
fn audio_specific_config(header: &[u8]) -> Vec<u8> {
    // ADTS stores the profile as `audioObjectType - 1`.
    let object_type = (header[2] >> 6) + 1;
    let frequency_index = (header[2] >> 2) & 0b1111;
    let channel_config = ((header[2] & 0b1) << 2) | (header[3] >> 6);
    vec![
        (object_type << 3) | (frequency_index >> 1),
        ((frequency_index & 0b1) << 7) | (channel_config << 3),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One ADTS frame, no CRC: AAC-LC (profile 1), 48 kHz (index 3),
    /// stereo (config 2), with `len` total bytes.
    fn frame(len: u16, payload_byte: u8) -> Vec<u8> {
        let mut f = vec![
            0xff,
            0xf1, // sync + MPEG-4, no CRC
            (1 << 6) | (3 << 2) | 0,
            (2 << 6) | ((len >> 11) as u8 & 0b11),
            (len >> 3) as u8,
            (((len & 0b111) as u8) << 5) | 0b11111,
            0xfc, // buffer fullness tail, 1 raw data block
        ];
        f.resize(len as usize, payload_byte);
        f
    }

    #[test]
    fn frames_out_headers_off() {
        let mut p = AacParser::new();
        p.push(&frame(20, 0xaa)).unwrap();
        p.push(&frame(15, 0xbb)).unwrap();
        assert_eq!(p.packets.len(), 2);
        assert_eq!(p.packets[0].data, vec![0xaa; 13], "20 bytes minus a 7-byte header");
        assert_eq!(p.packets[1].data, vec![0xbb; 8]);
        assert!(p.packets.iter().all(|pkt| pkt.keyframe && pkt.duration == 1024));
    }

    // The digit groups below are the config's bit fields, not byte
    // halves: 5 bits of object type, 4 of frequency index, 4 of channel
    // configuration.
    #[allow(clippy::unusual_byte_groupings)]
    #[test]
    fn config_is_lc_48k_stereo() {
        let mut p = AacParser::new();
        p.push(&frame(20, 0)).unwrap();
        // object type 2, frequency index 3, channel config 2.
        assert_eq!(p.codec_private().unwrap(), vec![0b00010_001, 0b1_0010_000]);
    }

    #[test]
    fn a_split_frame_waits_for_its_tail() {
        let f = frame(20, 0xcc);
        let mut p = AacParser::new();
        p.push(&f[..4]).unwrap();
        assert!(p.packets.is_empty(), "a partial header yields nothing");
        p.push(&f[4..19]).unwrap();
        assert!(p.packets.is_empty(), "a header without its payload yields nothing");
        p.push(&f[19..]).unwrap();
        assert_eq!(p.packets.len(), 1);
    }

    #[test]
    fn chunking_does_not_change_the_result() {
        let mut stream = frame(20, 0xaa);
        stream.extend_from_slice(&frame(31, 0xbb));
        for chunk in 1..=stream.len() {
            let mut p = AacParser::new();
            for part in stream.chunks(chunk) {
                p.push(part).unwrap();
            }
            assert_eq!(p.packets.len(), 2, "chunk size {chunk}");
            assert_eq!(p.packets[1].data, vec![0xbb; 24], "chunk size {chunk}");
        }
    }

    #[test]
    fn garbage_is_an_error_not_a_hang() {
        let mut p = AacParser::new();
        assert!(p.push(&[0x00; 32]).is_err());
    }
}

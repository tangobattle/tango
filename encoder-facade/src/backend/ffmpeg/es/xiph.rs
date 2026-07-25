//! Ogg-encapsulated Opus and FLAC → packets plus codec-private data.
//!
//! Neither codec has a usable bare-stream form for muxing: Opus has no
//! self-delimiting framing at all, and a raw FLAC stream only marks
//! frame boundaries with a sync code that has to be scanned for and
//! CRC-checked. Ogg already carries exact packet boundaries, and
//! ffmpeg will emit either codec into it (`-f opus`, `-f ogg`), so the
//! encoder's output arrives pre-framed and the framing never has to be
//! guessed. [`ogg`] does the page and lacing work; this reads the
//! codec-level headers out of the resulting packet stream.

use ogg::reading::{BasePacketReader, OggPage, PageParser};

use super::{EsPacket, EsParser};
use crate::codec::{flac_frame_blocksize, opus_packet_samples};

const OGG_PAGE_HEADER: usize = 27;

/// A FLAC frame starts with a 14-bit sync code (`0b11111111111110`),
/// which leaves the second byte as `0xF8`–`0xFB` once the reserved and
/// blocking-strategy bits are masked off. Anything else in this stream is
/// a metadata block.
fn is_flac_frame(data: &[u8]) -> bool {
    data.len() >= 4 && data[0] == 0xFF && data[1] & 0xFC == 0xF8
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mapping {
    Opus,
    Flac,
}

pub struct XiphParser {
    mapping: Mapping,
    buf: Vec<u8>,
    state: State,
    reader: BasePacketReader,
    packets: Vec<EsPacket>,
    codec_private: Option<Vec<u8>>,
    pre_skip: Option<u64>,
    /// FLAC's `STREAMINFO` maximum block size, used as the packet
    /// duration when a frame header stores its block size out of line
    /// (block-size codes 6 and 7) instead of in the header nibble.
    flac_max_blocksize: u64,
    saw_flac_mapping_header: bool,
}

/// Ogg pages arrive in three reads — fixed header, segment table, then
/// the segments' bytes — and the parser has to remember which it is
/// waiting for across `push` calls.
enum State {
    Header,
    Segments(PageParser, usize),
    Body(PageParser, usize),
}

impl XiphParser {
    pub fn new(mapping: Mapping) -> Self {
        Self {
            mapping,
            buf: Vec::new(),
            state: State::Header,
            reader: BasePacketReader::new(),
            packets: Vec::new(),
            codec_private: None,
            pre_skip: None,
            flac_max_blocksize: 4096,
            saw_flac_mapping_header: false,
        }
    }

}

impl EsParser for XiphParser {
    fn push(&mut self, bytes: &[u8]) -> crate::Result<()> {
        self.buf.extend_from_slice(bytes);
        loop {
            match std::mem::replace(&mut self.state, State::Header) {
                State::Header => {
                    if self.buf.len() < OGG_PAGE_HEADER {
                        break;
                    }
                    let mut header = [0u8; OGG_PAGE_HEADER];
                    header.copy_from_slice(&self.buf[..OGG_PAGE_HEADER]);
                    self.buf.drain(..OGG_PAGE_HEADER);
                    let (parser, needed) =
                        PageParser::new(header).map_err(|e| crate::Error::bitstream("Ogg", format!("{e:?}")))?;
                    self.state = State::Segments(parser, needed);
                }
                State::Segments(mut parser, needed) => {
                    if self.buf.len() < needed {
                        self.state = State::Segments(parser, needed);
                        break;
                    }
                    let segments = self.buf.drain(..needed).collect();
                    let body = parser.parse_segments(segments);
                    self.state = State::Body(parser, body);
                }
                State::Body(parser, needed) => {
                    if self.buf.len() < needed {
                        self.state = State::Body(parser, needed);
                        break;
                    }
                    let body = self.buf.drain(..needed).collect();
                    let page: OggPage = parser
                        .parse_packet_data(body)
                        .map_err(|e| crate::Error::bitstream("Ogg", format!("{e:?}")))?;
                    self.reader
                        .push_page(page)
                        .map_err(|e| crate::Error::bitstream("Ogg", format!("{e:?}")))?;
                    while let Some(packet) = self.reader.read_packet() {
                        self.consume(packet.data);
                    }
                }
            }
        }
        Ok(())
    }

    fn take_packets(&mut self) -> Vec<EsPacket> {
        std::mem::take(&mut self.packets)
    }

    fn codec_private(&self) -> Option<Vec<u8>> {
        self.codec_private.clone()
    }

    /// Opus states its own encoder priming in the `OpusHead`.
    fn declared_codec_delay(&self) -> Option<u64> {
        self.pre_skip
    }
}

impl XiphParser {
    fn consume(&mut self, data: Vec<u8>) {
        match self.mapping {
            Mapping::Opus => self.consume_opus(data),
            Mapping::Flac => self.consume_flac(data),
        }
    }

    fn consume_opus(&mut self, data: Vec<u8>) {
        if data.starts_with(b"OpusHead") {
            // Pre-skip is a little-endian u16 of 48 kHz samples at
            // offset 10 (RFC 7845 §5.1).
            self.pre_skip = data
                .get(10..12)
                .map(|b| u16::from_le_bytes([b[0], b[1]]) as u64);
            self.codec_private = Some(data);
            return;
        }
        if data.starts_with(b"OpusTags") {
            return;
        }
        let duration = opus_packet_samples(&data);
        self.packets.push(EsPacket {
            data,
            keyframe: true,
            duration,
        });
    }

    fn consume_flac(&mut self, data: Vec<u8>) {
        if !self.saw_flac_mapping_header {
            // The Ogg mapping's first packet is a 9-byte wrapper
            // (0x7F "FLAC" plus versions and a header count) followed
            // by the native `fLaC` magic and the STREAMINFO block —
            // which is exactly what a container wants as codec-private
            // data, so hand back everything from the magic on.
            if data.len() > 9 && data[0] == 0x7F && &data[1..5] == b"FLAC" {
                self.saw_flac_mapping_header = true;
                let mut private = data[9..].to_vec();
                // STREAMINFO is followed by more metadata blocks in the
                // Ogg stream but not in what we carry, so mark it as the
                // last one or a decoder will read into the audio.
                if private.len() > 4 {
                    private[4] |= 0x80;
                }
                if let Some(max) = private.get(10..12) {
                    let max = u16::from_be_bytes([max[0], max[1]]) as u64;
                    if max > 0 {
                        self.flac_max_blocksize = max;
                    }
                }
                self.codec_private = Some(private);
                return;
            }
            log::warn!("flac: first Ogg packet is not the FLAC mapping header");
        }
        if !is_flac_frame(&data) {
            // A further metadata block (seektable, vorbis comment).
            // Only STREAMINFO travels in codec-private data.
            return;
        }
        let duration = flac_frame_blocksize(&data).unwrap_or(self.flac_max_blocksize);
        self.packets.push(EsPacket {
            data,
            keyframe: true,
            duration,
        });
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use ogg::{PacketWriteEndInfo, PacketWriter};

    /// Wrap packets into a real Ogg stream, so the parser is tested
    /// against pages with correct lacing and checksums rather than
    /// against a hand-built approximation.
    fn oggify(packets: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut writer = PacketWriter::new(&mut out);
            for (i, packet) in packets.iter().enumerate() {
                let end = if i + 1 == packets.len() {
                    PacketWriteEndInfo::EndStream
                } else {
                    PacketWriteEndInfo::EndPage
                };
                writer.write_packet(packet.clone(), 0x1234_5678, end, i as u64).unwrap();
            }
        }
        out
    }

    fn opus_head(pre_skip: u16) -> Vec<u8> {
        let mut h = b"OpusHead".to_vec();
        h.push(1); // version
        h.push(2); // channels
        h.extend_from_slice(&pre_skip.to_le_bytes());
        h.extend_from_slice(&48_000u32.to_le_bytes());
        h.extend_from_slice(&0i16.to_le_bytes()); // output gain
        h.push(0); // channel mapping family
        h
    }

    #[test]
    fn opus_headers_and_packets() {
        // TOC 0x78: config 15 (hybrid, 20 ms), one frame.
        let audio = vec![0x78u8, 1, 2, 3];
        let stream = oggify(&[opus_head(312), b"OpusTags\0\0\0\0\0\0\0\0".to_vec(), audio.clone()]);
        let mut p = XiphParser::new(Mapping::Opus);
        p.push(&stream).unwrap();
        assert_eq!(p.declared_codec_delay(), Some(312), "pre-skip comes out of the OpusHead");
        assert!(p.codec_private().unwrap().starts_with(b"OpusHead"));
        assert_eq!(p.packets.len(), 1, "headers are not audio packets");
        assert_eq!(p.packets[0].data, audio);
        assert_eq!(p.packets[0].duration, 960, "20 ms at 48 kHz");
    }

    fn flac_mapping_header() -> Vec<u8> {
        let mut h = vec![0x7F];
        h.extend_from_slice(b"FLAC");
        h.extend_from_slice(&[1, 0]); // mapping version
        h.extend_from_slice(&[0, 1]); // header packet count
        h.extend_from_slice(b"fLaC");
        // STREAMINFO: type 0, not last, 34-byte body.
        h.extend_from_slice(&[0x00, 0, 0, 34]);
        h.extend_from_slice(&4096u16.to_be_bytes()); // min block size
        h.extend_from_slice(&4096u16.to_be_bytes()); // max block size
        h.resize(9 + 4 + 4 + 34, 0);
        h
    }

    #[test]
    fn flac_codec_private_is_marked_last() {
        // Block-size code 12 in the high nibble of byte 2 is 4096.
        let frame = vec![0xFF, 0xF8, 0xC9, 0x08, 0x00, 0x11];
        let stream = oggify(&[flac_mapping_header(), frame.clone()]);
        let mut p = XiphParser::new(Mapping::Flac);
        p.push(&stream).unwrap();
        let private = p.codec_private().expect("STREAMINFO");
        assert!(private.starts_with(b"fLaC"), "the mapping wrapper is stripped");
        assert_eq!(private[4] & 0x80, 0x80, "STREAMINFO must be the last metadata block");
        assert_eq!(p.packets.len(), 1);
        assert_eq!(p.packets[0].duration, 4096);
    }

    #[test]
    fn chunking_does_not_change_the_result() {
        let stream = oggify(&[opus_head(312), vec![0x78, 1, 2, 3], vec![0x78, 4, 5, 6]]);
        for chunk in [1usize, 7, 13, 64, stream.len()] {
            let mut p = XiphParser::new(Mapping::Opus);
            for part in stream.chunks(chunk) {
                p.push(part).unwrap();
            }
            assert_eq!(p.packets.len(), 2, "chunk size {chunk}");
            assert_eq!(p.declared_codec_delay(), Some(312), "chunk size {chunk}");
        }
    }
}

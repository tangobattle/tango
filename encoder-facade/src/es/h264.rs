//! H.264 Annex-B → access units in the length-prefixed form both MP4
//! and Matroska store (`avcC` sample format), plus the `avcC`
//! configuration record built from the stream's SPS/PPS.

use std::io::Read;

use h264_reader::annexb::AnnexBReader;
use h264_reader::nal::{Nal, RefNal, UnitType};
use h264_reader::push::{AccumulatedNalHandler, NalInterest};
use mp4_atom::{Atom, Avcc};

use super::{EsPacket, EsParser};

/// NAL length prefix width. `avcC` records it as `length_size`, and
/// [`Avcc::new`] always says 4, so samples must agree.
const LENGTH_PREFIX: usize = 4;

pub struct H264Parser {
    reader: AnnexBReader<h264_reader::push::NalAccumulator<NalSink>>,
    /// NALs of the access unit being assembled, each already length-
    /// prefixed and ready to concatenate.
    pending: Vec<u8>,
    /// Whether `pending` has a coded slice in it yet. Leading
    /// parameter sets and SEI belong to the access unit that follows
    /// them, so a slice is what makes the unit real.
    pending_has_slice: bool,
    pending_keyframe: bool,
    packets: Vec<EsPacket>,
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
}

impl H264Parser {
    pub fn new() -> Self {
        Self {
            reader: AnnexBReader::accumulate(NalSink::default()),
            pending: Vec::new(),
            pending_has_slice: false,
            pending_keyframe: false,
            packets: Vec::new(),
            sps: None,
            pps: None,
        }
    }

    fn consume_nal(&mut self, nal: ParsedNal) {
        match nal.kind {
            // Parameter sets travel out-of-band in `avcC`. Keeping
            // them in the samples as well is legal but redundant, and
            // some players trust one source over the other.
            UnitType::SeqParameterSet => {
                self.sps = Some(nal.bytes);
                return;
            }
            UnitType::PicParameterSet => {
                self.pps = Some(nal.bytes);
                return;
            }
            _ => {}
        }
        let is_slice = matches!(
            nal.kind,
            UnitType::SliceLayerWithoutPartitioningNonIdr
                | UnitType::SliceLayerWithoutPartitioningIdr
                | UnitType::SliceDataPartitionALayer
                | UnitType::SliceDataPartitionBLayer
                | UnitType::SliceDataPartitionCLayer
        );
        // A slice whose first macroblock is 0 starts a new picture, so
        // it closes the one being assembled.
        if is_slice && nal.first_mb_is_zero && self.pending_has_slice {
            self.close_access_unit();
        }
        if is_slice {
            self.pending_has_slice = true;
            if nal.kind == UnitType::SliceLayerWithoutPartitioningIdr {
                self.pending_keyframe = true;
            }
        }
        self.pending
            .extend_from_slice(&(nal.bytes.len() as u32).to_be_bytes()[4 - LENGTH_PREFIX..]);
        self.pending.extend_from_slice(&nal.bytes);
    }

    fn close_access_unit(&mut self) {
        if !self.pending_has_slice {
            // Trailing non-slice NALs (an end-of-stream marker, say)
            // aren't a picture; drop them rather than emit an empty
            // frame the muxer would have to time.
            self.pending.clear();
            return;
        }
        self.packets.push(EsPacket {
            data: std::mem::take(&mut self.pending),
            keyframe: self.pending_keyframe,
            duration: 1,
        });
        self.pending_has_slice = false;
        self.pending_keyframe = false;
    }
}

impl EsParser for H264Parser {
    fn push(&mut self, bytes: &[u8]) -> crate::Result<()> {
        self.reader.push(bytes);
        for nal in std::mem::take(&mut self.reader.nal_handler_mut().nals) {
            self.consume_nal(nal);
        }
        Ok(())
    }

    /// The last access unit has no successor to close it, so the end of
    /// the stream closes it.
    fn flush(&mut self) -> crate::Result<()> {
        // A NAL is only complete once the next start code arrives; this
        // is what tells the reader there won't be one.
        self.reader.reset();
        for nal in std::mem::take(&mut self.reader.nal_handler_mut().nals) {
            self.consume_nal(nal);
        }
        self.close_access_unit();
        Ok(())
    }

    fn take_packets(&mut self) -> Vec<EsPacket> {
        std::mem::take(&mut self.packets)
    }

    fn codec_private(&self) -> Option<Vec<u8>> {
        let (sps, pps) = (self.sps.as_ref()?, self.pps.as_ref()?);
        let avcc = Avcc::new(sps, pps).ok()?;
        let mut out = Vec::new();
        // The bare record, without the `avcC` box header: that's what
        // Matroska's CodecPrivate holds and what the `avc1` sample entry
        // is built from.
        avcc.encode_body(&mut out).ok()?;
        Some(out)
    }
}

struct ParsedNal {
    kind: UnitType,
    /// `true` if this is a slice whose `first_mb_in_slice` is 0. The
    /// field is the slice header's leading `ue(v)`, and a `ue(v)` of 0
    /// is the single bit `1`, so the top bit of the first RBSP byte
    /// answers it without a bit reader.
    first_mb_is_zero: bool,
    /// The NAL in storage form: header byte and emulation-prevention
    /// bytes included, start code excluded.
    bytes: Vec<u8>,
}

#[derive(Default)]
struct NalSink {
    nals: Vec<ParsedNal>,
}

impl AccumulatedNalHandler for NalSink {
    fn nal(&mut self, nal: RefNal<'_>) -> NalInterest {
        if !nal.is_complete() {
            // Keep accumulating; we only ever emit whole NALs.
            return NalInterest::Buffer;
        }
        let Ok(header) = nal.header() else {
            log::warn!("h264: dropping NAL with a corrupt header");
            return NalInterest::Ignore;
        };
        let mut bytes = Vec::new();
        if let Err(e) = nal.reader().read_to_end(&mut bytes) {
            log::warn!("h264: dropping unreadable NAL: {e}");
            return NalInterest::Ignore;
        }
        let mut first = [0u8; 1];
        let first_mb_is_zero = nal.rbsp_bytes().read_exact(&mut first).is_ok() && first[0] & 0x80 != 0;
        self.nals.push(ParsedNal {
            kind: header.nal_unit_type(),
            first_mb_is_zero,
            bytes,
        });
        NalInterest::Buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream of three tiny NALs: SPS, PPS, IDR slice. Built by hand
    /// so the test states exactly what the parser should make of it.
    fn stream() -> Vec<u8> {
        let mut s = Vec::new();
        // SPS (type 7) with plausible profile/level bytes.
        s.extend_from_slice(&[0, 0, 0, 1, 0x67, 0x64, 0x00, 0x0d, 0xac]);
        // PPS (type 8), 3-byte start code this time.
        s.extend_from_slice(&[0, 0, 1, 0x68, 0xeb, 0xc0]);
        // IDR slice (type 5); 0x88 has the top bit set, so
        // first_mb_in_slice is 0.
        s.extend_from_slice(&[0, 0, 0, 1, 0x65, 0x88, 0x11, 0x22]);
        s
    }

    #[test]
    fn splits_access_units_and_length_prefixes_them() {
        let mut p = H264Parser::new();
        p.push(&stream()).unwrap();
        p.flush().unwrap();
        let packets = p.take_packets();
        assert_eq!(packets.len(), 1);
        assert!(packets[0].keyframe, "an IDR slice is a keyframe");
        // Only the slice is in the sample, prefixed with its length —
        // the parameter sets went to avcC instead.
        assert_eq!(packets[0].data, vec![0, 0, 0, 4, 0x65, 0x88, 0x11, 0x22]);
    }

    #[test]
    fn two_pictures_become_two_packets() {
        let mut p = H264Parser::new();
        p.push(&stream()).unwrap();
        // A second, non-IDR picture.
        p.push(&[0, 0, 0, 1, 0x41, 0x9a, 0x33]).unwrap();
        p.flush().unwrap();
        let packets = p.take_packets();
        assert_eq!(packets.len(), 2);
        assert!(packets[0].keyframe);
        assert!(!packets[1].keyframe, "a non-IDR picture is not a keyframe");
    }

    /// The parser sees whatever chunks a pipe read hands it, so the
    /// same stream split at every possible boundary must parse the
    /// same way.
    #[test]
    fn chunking_does_not_change_the_result() {
        let s = stream();
        let whole = {
            let mut p = H264Parser::new();
            p.push(&s).unwrap();
            p.flush().unwrap();
            (p.take_packets(), p.codec_private())
        };
        for chunk in 1..=s.len() {
            let mut p = H264Parser::new();
            for part in s.chunks(chunk) {
                p.push(part).unwrap();
            }
            p.flush().unwrap();
            let packets = p.take_packets();
            assert_eq!(packets.len(), whole.0.len(), "chunk size {chunk}");
            assert_eq!(packets[0].data, whole.0[0].data, "chunk size {chunk}");
            assert_eq!(p.codec_private(), whole.1, "chunk size {chunk}");
        }
    }

    #[test]
    fn codec_private_is_a_bare_avcc_record() {
        let mut p = H264Parser::new();
        p.push(&stream()).unwrap();
        p.flush().unwrap();
        let avcc = p.codec_private().expect("SPS and PPS were both present");
        // configurationVersion, then the profile/compat/level copied
        // out of the SPS, then the length-size byte.
        assert_eq!(&avcc[..4], &[1, 0x64, 0x00, 0x0d]);
        assert_eq!(avcc[4] & 0b11, 3, "length_size 4 encodes as 3");
    }

    #[test]
    fn no_codec_private_until_both_parameter_sets_arrive() {
        let mut p = H264Parser::new();
        p.push(&[0, 0, 0, 1, 0x67, 0x64, 0x00, 0x0d, 0xac]).unwrap();
        assert!(p.codec_private().is_none(), "SPS alone is not enough");
    }
}

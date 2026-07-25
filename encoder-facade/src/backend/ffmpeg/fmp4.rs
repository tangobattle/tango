//! Reading the encoders back: fragmented MP4 in, samples out.
//!
//! Each ffmpeg child writes a fragmented MP4 carrying its one stream —
//! `ftyp`, a `moov` describing the track, then a `moof`/`mdat` pair per
//! fragment — and this turns that byte stream back into discrete
//! samples. Every codec arrives this way, so there is one reader here
//! rather than one per bitstream.
//!
//! The format hands over exactly what a container needs and what a bare
//! elementary stream doesn't have:
//!
//!   * **Framing.** `trun` states each sample's size, so nothing has to
//!     be recovered by scanning for start codes or sync words.
//!   * **Timing.** `trun` states each sample's duration in the track's
//!     timescale, which is the timescale we asked ffmpeg for — exact
//!     ticks, not a rate rounded into some other unit.
//!   * **Sync points.** A sample's flags say whether it is one, instead
//!     of a slice header having to be parsed to find out.
//!   * **Codec configuration.** The `moov` carries the `avcC`,
//!     `AudioSpecificConfig` or FLAC `STREAMINFO` verbatim, and arrives
//!     before the first fragment.
//!
//! It is a push reader: bytes arrive in whatever sizes a pipe read
//! produced, and whole samples come out. Bytes are held only until the
//! box they belong to is complete, so a reader's memory is one fragment,
//! not one export.

use mp4_atom::{Atom, DecodeMaybe, Encode, FourCC, Header, Mdat, Moof, Moov, Trun};

use super::Sample;
use crate::Error;

/// `sample_flags` bit 16: this sample is *not* a sync sample (ISO/IEC
/// 14496-12 §8.8.3.1). Everything else in the word is about dependency
/// and degradation, which a muxer here doesn't carry.
const NON_SYNC_SAMPLE: u32 = 0x0001_0000;

/// What the `moov` said about the single track a stream carries.
struct Track {
    timescale: u32,
    codec_private: Vec<u8>,
    /// `trex` defaults, the last resort for a sample field that neither
    /// the `trun` nor the `tfhd` states.
    defaults: Defaults,
}

/// Per-sample values a fragment may state once instead of per sample.
#[derive(Clone, Copy, Default)]
struct Defaults {
    duration: Option<u32>,
    size: Option<u32>,
    flags: Option<u32>,
}

/// A sample the `moof` announced, waiting for the `mdat` holding it.
struct Announced {
    /// Where the sample's first byte is in the stream.
    at: u64,
    size: u32,
    duration: u64,
    keyframe: bool,
}

pub struct Reader {
    /// Bytes not yet consumed as whole atoms.
    buf: Vec<u8>,
    /// Stream position of `buf[0]`, which is how a `trun`'s offsets —
    /// stated against the file — are matched to the bytes as they go
    /// past.
    pos: u64,
    track: Option<Track>,
    announced: Vec<Announced>,
    samples: Vec<Sample>,
}

impl Reader {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            pos: 0,
            track: None,
            announced: Vec::new(),
            samples: Vec::new(),
        }
    }

    /// Take the next slice of encoder output.
    pub fn push(&mut self, bytes: &[u8]) -> crate::Result<()> {
        // Held apart from `self` for the pass so a body can be borrowed
        // out of it while samples are pushed back.
        let mut buf = std::mem::take(&mut self.buf);
        buf.extend_from_slice(bytes);
        let result = self.consume(&mut buf);
        self.buf = buf;
        result
    }

    pub fn take_samples(&mut self) -> Vec<Sample> {
        std::mem::take(&mut self.samples)
    }

    /// The track's timescale: the unit every sample's duration is in.
    /// `None` until the `moov` has arrived.
    pub fn timescale(&self) -> Option<u32> {
        self.track.as_ref().map(|t| t.timescale)
    }

    /// Codec configuration in the form containers want it, or `None`
    /// while the `moov` hasn't arrived.
    pub fn codec_private(&self) -> Option<Vec<u8>> {
        self.track.as_ref().map(|t| t.codec_private.clone())
    }

    /// Consume every whole atom in `buf`, leaving any partial tail.
    fn consume(&mut self, buf: &mut Vec<u8>) -> crate::Result<()> {
        while let Some((kind, header_len, body_len)) = peek(buf)? {
            let total = header_len + body_len;
            if buf.len() < total {
                break;
            }
            let body = &buf[header_len..total];
            match kind {
                Moov::KIND => self.track = Some(read_moov(body)?),
                Moof::KIND => self.announce(self.pos, body)?,
                // Everything the fragment before it announced lives in
                // here.
                Mdat::KIND => self.cut(self.pos + header_len as u64, body)?,
                // `ftyp`, the trailing `mfra`, anything else ffmpeg
                // decides to write: none of it describes a sample.
                _ => {}
            }
            buf.drain(..total);
            self.pos += total as u64;
        }
        Ok(())
    }

    /// Work out where a fragment's samples will be and how long each is.
    fn announce(&mut self, moof_at: u64, body: &[u8]) -> crate::Result<()> {
        let moof = Moof::decode_body(&mut &body[..]).map_err(|e| bitstream(format!("unreadable moof: {e}")))?;
        let stream_defaults = self.track.as_ref().map(|t| t.defaults).unwrap_or_default();
        for traf in &moof.traf {
            let tfhd = &traf.tfhd;
            // We ask ffmpeg for `default_base_moof`, so a fragment's
            // data is addressed from the start of its own `moof` unless
            // it says otherwise.
            let base = tfhd.base_data_offset.unwrap_or(moof_at);
            let defaults = Defaults {
                duration: tfhd.default_sample_duration.or(stream_defaults.duration),
                size: tfhd.default_sample_size.or(stream_defaults.size),
                flags: tfhd.default_sample_flags.or(stream_defaults.flags),
            };
            for trun in &traf.trun {
                self.announce_run(base, trun, defaults)?;
            }
        }
        Ok(())
    }

    fn announce_run(&mut self, base: u64, trun: &Trun, defaults: Defaults) -> crate::Result<()> {
        let mut at = base.saturating_add_signed(trun.data_offset.unwrap_or(0) as i64);
        for entry in &trun.entries {
            // A value is the sample's own, else the fragment's default,
            // else the stream's — the chain ISO/IEC 14496-12 §8.8
            // defines, which is why a run of identical samples costs
            // nothing per sample.
            let size = entry
                .size
                .or(defaults.size)
                .ok_or_else(|| bitstream("a sample with no size"))?;
            let duration = entry
                .duration
                .or(defaults.duration)
                .ok_or_else(|| bitstream("a sample with no duration"))?;
            let flags = entry.flags.or(defaults.flags).unwrap_or(0);
            self.announced.push(Announced {
                at,
                size,
                duration: duration as u64,
                keyframe: flags & NON_SYNC_SAMPLE == 0,
            });
            at += size as u64;
        }
        Ok(())
    }

    /// Cut the announced samples out of the media that just arrived.
    fn cut(&mut self, media_at: u64, media: &[u8]) -> crate::Result<()> {
        for sample in self.announced.drain(..) {
            let from = sample
                .at
                .checked_sub(media_at)
                .and_then(|from| usize::try_from(from).ok())
                .filter(|from| from.checked_add(sample.size as usize).is_some_and(|end| end <= media.len()))
                .ok_or_else(|| {
                    bitstream(format!(
                        "a fragment placed a {}-byte sample at {}, outside the {}-byte mdat at {media_at}",
                        sample.size,
                        sample.at,
                        media.len()
                    ))
                })?;
            self.samples.push(Sample {
                data: media[from..from + sample.size as usize].to_vec(),
                keyframe: sample.keyframe,
                duration: sample.duration,
            });
        }
        Ok(())
    }
}

/// The kind and lengths of the atom at the head of `buf`, or `None`
/// while there isn't a whole header there yet.
fn peek(buf: &[u8]) -> crate::Result<Option<(FourCC, usize, usize)>> {
    let mut cursor: &[u8] = buf;
    let Some(header) = Header::decode_maybe(&mut cursor).map_err(|e| bitstream(format!("unreadable atom: {e}")))?
    else {
        return Ok(None);
    };
    let header_len = buf.len() - cursor.len();
    // Size 0 means "to the end of the file", which a stream that is
    // still being written has no way to answer.
    let body_len = header
        .size
        .ok_or_else(|| bitstream(format!("a {} atom with no stated size", header.kind)))?;
    Ok(Some((header.kind, header_len, body_len)))
}

/// Read the track description: its timescale, its codec configuration,
/// and the sample defaults that apply to every fragment.
fn read_moov(body: &[u8]) -> crate::Result<Track> {
    let moov = Moov::decode_body(&mut &body[..]).map_err(|e| bitstream(format!("unreadable moov: {e}")))?;
    // One stream per encoder, so one track per moov.
    let trak = moov
        .trak
        .first()
        .ok_or_else(|| bitstream("a moov describing no tracks"))?;
    let codec = trak
        .mdia
        .minf
        .stbl
        .stsd
        .codecs
        .first()
        .ok_or_else(|| bitstream("a track with no sample entry"))?;
    let trex = moov.mvex.as_ref().and_then(|mvex| mvex.trex.first());
    Ok(Track {
        timescale: trak.mdia.mdhd.timescale,
        codec_private: codec_private(codec)?,
        defaults: Defaults {
            // trex states its defaults unconditionally; zero is how it
            // says it has none.
            duration: trex.map(|t| t.default_sample_duration).filter(|&d| d != 0),
            size: trex.map(|t| t.default_sample_size).filter(|&s| s != 0),
            flags: trex.map(|t| t.default_sample_flags).filter(|&f| f != 0),
        },
    })
}

/// A sample entry's codec configuration, in the form the muxers want:
/// the bare `avcC` record for H.264, the `AudioSpecificConfig` for AAC,
/// and `fLaC` magic with its metadata blocks for FLAC.
fn codec_private(codec: &mp4_atom::Codec) -> crate::Result<Vec<u8>> {
    let mut out = Vec::new();
    match codec {
        mp4_atom::Codec::Avc1(avc1) => {
            avc1.avcc
                .encode_body(&mut out)
                .map_err(|e| bitstream(format!("unusable avcC record: {e}")))?;
        }
        mp4_atom::Codec::Mp4a(mp4a) => {
            // mp4-atom models the config by its fields rather than as a
            // blob; re-encoding gives back the two bytes of audio object
            // type, sampling frequency index and channel configuration
            // that both containers carry.
            mp4a.esds
                .es_desc
                .dec_config
                .dec_specific
                .encode(&mut out)
                .map_err(|e| bitstream(format!("unusable AudioSpecificConfig: {e}")))?;
        }
        mp4_atom::Codec::Flac(flac) => {
            out.extend_from_slice(b"fLaC");
            // `dfLa` is a full box: four bytes of version and flags,
            // then the metadata blocks. Containers want the blocks
            // behind the native magic, so the header comes back off.
            let mut dfla = Vec::new();
            flac.dfla
                .encode_body(&mut dfla)
                .map_err(|e| bitstream(format!("unusable FLAC STREAMINFO: {e}")))?;
            out.extend_from_slice(dfla.get(4..).unwrap_or_default());
        }
        other => return Err(bitstream(format!("an unexpected sample entry: {other:?}"))),
    }
    Ok(out)
}

fn bitstream(detail: impl std::fmt::Display) -> Error {
    Error::bitstream("fragmented MP4", detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp4_atom::TrunEntry;

    /// Wrap a body in its atom header.
    fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    /// A `moof` for one track: `tfhd` defaults plus one `trun`.
    fn moof(defaults: mp4_atom::Tfhd, trun: Trun) -> Vec<u8> {
        let mut traf = Vec::new();
        defaults.encode(&mut traf).unwrap();
        trun.encode(&mut traf).unwrap();
        let mut body = Vec::new();
        mp4_atom::Mfhd { sequence_number: 1 }.encode(&mut body).unwrap();
        body.extend_from_slice(&atom(b"traf", &traf));
        atom(b"moof", &body)
    }

    fn tfhd(duration: Option<u32>, size: Option<u32>, flags: Option<u32>) -> mp4_atom::Tfhd {
        mp4_atom::Tfhd {
            track_id: 1,
            base_data_offset: None,
            sample_description_index: None,
            default_sample_duration: duration,
            default_sample_size: size,
            default_sample_flags: flags,
            duration_is_empty: false,
            default_base_is_moof: true,
        }
    }

    /// A fragment whose samples are the given byte runs, laid out the
    /// way ffmpeg lays them out: `moof` then `mdat`, addressed from the
    /// start of the `moof`.
    fn fragment(samples: &[&[u8]], defaults: mp4_atom::Tfhd, entries: Vec<TrunEntry>) -> Vec<u8> {
        let media: Vec<u8> = samples.concat();
        // The data offset counts from the moof, so it has to know how
        // long the moof turned out to be: build once to measure it, then
        // again with the answer.
        let measure = moof(defaults.clone(), Trun {
            data_offset: Some(0),
            entries: entries.clone(),
        });
        let mut out = moof(defaults, Trun {
            data_offset: Some((measure.len() + 8) as i32),
            entries,
        });
        out.extend_from_slice(&atom(b"mdat", &media));
        out
    }

    fn entry(size: u32) -> TrunEntry {
        TrunEntry {
            duration: None,
            size: Some(size),
            flags: None,
            cts: None,
        }
    }

    #[test]
    fn samples_come_out_whole_with_their_durations() {
        let mut reader = Reader::new();
        reader
            .push(&fragment(
                &[&[1, 2, 3], &[4, 5], &[6]],
                tfhd(Some(1024), None, None),
                vec![entry(3), entry(2), entry(1)],
            ))
            .unwrap();
        let samples = reader.take_samples();
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0].data, vec![1, 2, 3]);
        assert_eq!(samples[1].data, vec![4, 5]);
        assert_eq!(samples[2].data, vec![6]);
        assert!(
            samples.iter().all(|s| s.duration == 1024),
            "the fragment's default duration applies to every sample"
        );
    }

    /// The reader sees whatever chunks a pipe read hands it, so the same
    /// stream split at every possible boundary must read the same way.
    #[test]
    fn chunking_does_not_change_the_result() {
        let stream = fragment(
            &[&[1, 2, 3], &[4, 5], &[6]],
            tfhd(Some(1024), None, None),
            vec![entry(3), entry(2), entry(1)],
        );
        for chunk in 1..=stream.len() {
            let mut reader = Reader::new();
            let mut samples = Vec::new();
            for part in stream.chunks(chunk) {
                reader.push(part).unwrap();
                samples.extend(reader.take_samples());
            }
            assert_eq!(samples.len(), 3, "chunk size {chunk}");
            assert_eq!(samples[0].data, vec![1, 2, 3], "chunk size {chunk}");
            assert_eq!(samples[2].data, vec![6], "chunk size {chunk}");
        }
    }

    /// Sync flags decide where a container may put a seek point, and
    /// they arrive three ways: per sample, per fragment, or as the first
    /// sample's own flags (how ffmpeg marks the keyframe that opens a
    /// fragment).
    #[test]
    fn keyframes_come_from_whichever_level_states_them() {
        let mut first = entry(1);
        first.flags = Some(0);
        let mut reader = Reader::new();
        reader
            .push(&fragment(
                &[&[1], &[2], &[3]],
                // The fragment's default marks samples as non-sync...
                tfhd(Some(1), None, Some(NON_SYNC_SAMPLE)),
                // ...which the first sample overrides for itself.
                vec![first, entry(1), entry(1)],
            ))
            .unwrap();
        let samples = reader.take_samples();
        assert_eq!(
            samples.iter().map(|s| s.keyframe).collect::<Vec<_>>(),
            vec![true, false, false]
        );
    }

    /// Fragments arrive back to back for the length of an export, so
    /// each one's samples have to be cut from its own media.
    #[test]
    fn later_fragments_are_addressed_from_their_own_moof() {
        let mut reader = Reader::new();
        let mut stream = fragment(&[&[1, 1]], tfhd(Some(1), None, None), vec![entry(2)]);
        stream.extend_from_slice(&fragment(&[&[2, 2]], tfhd(Some(1), None, None), vec![entry(2)]));
        stream.extend_from_slice(&fragment(&[&[3, 3]], tfhd(Some(1), None, None), vec![entry(2)]));
        reader.push(&stream).unwrap();
        let samples = reader.take_samples();
        assert_eq!(
            samples.iter().map(|s| s.data.clone()).collect::<Vec<_>>(),
            vec![vec![1, 1], vec![2, 2], vec![3, 3]]
        );
    }

    /// A sample addressed outside the media that arrived is a misread,
    /// and has to be refused rather than silently truncated.
    #[test]
    fn a_sample_that_does_not_fit_its_media_is_refused() {
        let mut reader = Reader::new();
        let stream = fragment(&[&[1, 2]], tfhd(Some(1), None, None), vec![entry(64)]);
        assert!(reader.push(&stream).is_err());
    }

    /// Atoms that describe no samples are skipped, not misread as ones
    /// that do.
    #[test]
    fn unknown_atoms_are_stepped_over() {
        let mut reader = Reader::new();
        let mut stream = atom(b"ftyp", b"isom\0\0\x02\0mp41");
        stream.extend_from_slice(&fragment(&[&[7]], tfhd(Some(1), None, None), vec![entry(1)]));
        stream.extend_from_slice(&atom(b"mfra", &[0; 16]));
        reader.push(&stream).unwrap();
        assert_eq!(reader.take_samples().len(), 1);
    }
}

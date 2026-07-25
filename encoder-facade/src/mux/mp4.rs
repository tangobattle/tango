//! MP4 (ISO base media file format), written with [`mp4_atom`]'s typed
//! atoms.
//!
//! Layout:
//!
//! ```text
//!   ftyp
//!   mdat (64-bit size placeholder, patched at finish)
//!     ...samples, interleaved as they arrive...
//!   moov (written at the close, once the sample tables are complete)
//! ```
//!
//! `moov` goes last because its tables describe every sample's size and
//! position, which is only known when the stream ends. That rules out
//! `faststart` layout (where `moov` precedes `mdat` so a player can
//! begin before the whole file arrives) — reaching it would mean
//! rewriting the media afterwards. Local playback and every editor are
//! unaffected; only progressive playback straight off a web server
//! notices.
//!
//! Sample *metadata* is buffered, not sample data: an hour-long export
//! holds a few hundred kilobytes of tables while the media streams
//! straight through to the output.

use mp4_atom::{Any, Atom, Encode, FourCC};

use super::{Chapter, Container, MuxConfig, Muxer, Patch};
use crate::packet::ticks_to_ns;
use crate::{AudioCodec, AudioTrackInfo, VideoCodec, VideoTrackInfo};



/// `mvhd`/chapter timescale. Milliseconds is the conventional choice
/// for the movie clock; each track keeps its own exact timescale.
const MOVIE_TIMESCALE: u32 = 1_000;

pub struct Mp4Muxer {
    config: MuxConfig,
    out: Vec<u8>,
    pos: u64,
    /// Position of `mdat`'s 64-bit size field.
    mdat_size_pos: u64,
    mdat_start: u64,
    tracks: Vec<Track>,
}

/// Accumulated sample tables for one track.
struct Track {
    timescale: u32,
    /// (offset, size) per sample, in file order.
    samples: Vec<(u64, u32)>,
    /// Run-length-encoded sample durations, ready for `stts`.
    durations: Vec<(u32, u32)>,
    /// 1-based indices of sync samples, for `stss`. Left empty for
    /// tracks where every sample is a sync sample.
    sync: Vec<u32>,
    all_sync: bool,
    /// Chunks as (first sample index, sample count, offset).
    chunks: Vec<Chunk>,
    total_duration: u64,
    kind: TrackKind,
}

struct Chunk {
    offset: u64,
    samples: u32,
}

/// Which kind of track this is, and everything its sample entry is built
/// from — which the codec does, not this module.
enum TrackKind {
    Video(VideoTrackInfo),
    Audio(AudioTrackInfo),
}

impl Mp4Muxer {
    pub fn new(config: MuxConfig) -> crate::Result<Self> {
        // Refuse a codec MP4 can't carry before writing a byte, rather
        // than at the close when the sample entry can't be built.
        for audio in &config.audio {
            Container::Mp4.accepts(config.video.codec, audio.codec)?;
        }
        let mut tracks = vec![Track::new(
            config.video.timescale,
            false,
            TrackKind::Video(config.video.clone()),
        )];
        for audio in &config.audio {
            tracks.push(Track::new(audio.sample_rate, true, TrackKind::Audio(audio.clone())));
        }
        let mut muxer = Self {
            config,
            out: Vec::new(),
            pos: 0,
            mdat_size_pos: 0,
            mdat_start: 0,
            tracks,
        };
        muxer.write_head()?;
        Ok(muxer)
    }

    fn write_head(&mut self) -> crate::Result<()> {
        let ftyp = mp4_atom::Ftyp {
            major_brand: b"isom".into(),
            minor_version: 512,
            compatible_brands: vec![b"isom".into(), b"iso2".into(), b"avc1".into(), b"mp41".into()],
        };
        let mut buf = Vec::new();
        ftyp.encode(&mut buf)?;
        self.emit(&buf);

        // `mdat` in its 64-bit form: a 32-bit size of 1 says the real
        // size follows the type as a u64. Written by hand because the
        // media is streamed into it rather than handed to the library
        // as one buffer, and because the size is only known at the end.
        self.mdat_size_pos = self.pos;
        let mut header = 1u32.to_be_bytes().to_vec();
        header.extend_from_slice(b"mdat");
        header.extend_from_slice(&0u64.to_be_bytes());
        self.emit(&header);
        self.mdat_start = self.mdat_size_pos;
        Ok(())
    }

    fn emit(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
        self.pos += bytes.len() as u64;
    }
}

impl Muxer for Mp4Muxer {
    fn write(&mut self, track: usize, packet: &crate::Packet) -> crate::Result<()> {
        if track >= self.tracks.len() {
            return Err(crate::Error::internal(format!("no track {track}")));
        }
        let offset = self.pos;
        self.out.extend_from_slice(&packet.data);
        self.pos += packet.data.len() as u64;

        let last_chunk_ends_here = self.tracks[track]
            .chunks
            .last()
            .zip(self.tracks[track].samples.last())
            .is_some_and(|(chunk, &(last_offset, last_size))| {
                chunk.offset <= last_offset && last_offset + last_size as u64 == offset
            });
        let t = &mut self.tracks[track];
        t.samples.push((offset, packet.data.len() as u32));
        // Samples that landed back to back belong to the same chunk;
        // anything else starts a new one. Interleaving two tracks
        // naturally produces one chunk per run.
        if last_chunk_ends_here {
            if let Some(chunk) = t.chunks.last_mut() {
                chunk.samples += 1;
            }
        } else {
            t.chunks.push(Chunk { offset, samples: 1 });
        }
        let duration = packet.duration as u32;
        match t.durations.last_mut() {
            Some((count, d)) if *d == duration => *count += 1,
            _ => t.durations.push((1, duration)),
        }
        t.total_duration += packet.duration;
        if !t.all_sync && packet.keyframe {
            t.sync.push(t.samples.len() as u32);
        }
        Ok(())
    }

    fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }

    fn finish(&mut self, chapters: &[Chapter]) -> crate::Result<Vec<Patch>> {
        let mdat_size = self.pos - self.mdat_start;
        let movie_duration = self
            .tracks
            .iter()
            .map(|t| ticks_to_ns(t.total_duration, t.timescale) / 1_000_000)
            .max()
            .unwrap_or(0);

        // moov is assembled as a container atom holding
        // library-encoded children, which is what lets the Nero `chpl`
        // chapter box ride along: mp4-atom models `udta` with a fixed
        // set of children, and `chpl` isn't one of them.
        let mut children = Vec::new();
        mp4_atom::Mvhd {
            creation_time: 0,
            modification_time: 0,
            timescale: MOVIE_TIMESCALE,
            duration: movie_duration,
            rate: mp4_atom::FixedPoint::new(1, 0),
            volume: mp4_atom::FixedPoint::new(1, 0),
            matrix: mp4_atom::Matrix::default(),
            next_track_id: self.tracks.len() as u32 + 1,
        }
        .encode(&mut children)?;
        for (i, track) in self.tracks.iter().enumerate() {
            track.encode_trak(i as u32 + 1, movie_duration, &mut children)?;
        }
        if !chapters.is_empty() {
            let chpl = chpl_atom(chapters, &self.config);
            let udta = Any::Unknown(FourCC::new(b"udta"), chpl);
            udta.encode(&mut children)?;
        }
        let moov = Any::Unknown(FourCC::new(b"moov"), children);
        let mut buf = Vec::new();
        moov.encode(&mut buf)?;
        self.emit(&buf);

        Ok(vec![Patch {
            position: self.mdat_size_pos + 8,
            bytes: mdat_size.to_be_bytes().to_vec(),
        }])
    }
}

impl Track {
    fn new(timescale: u32, all_sync: bool, kind: TrackKind) -> Self {
        Self {
            timescale,
            samples: Vec::new(),
            durations: Vec::new(),
            sync: Vec::new(),
            all_sync,
            chunks: Vec::new(),
            total_duration: 0,
            kind,
        }
    }

    fn encode_trak(&self, id: u32, movie_duration: u64, out: &mut Vec<u8>) -> crate::Result<()> {
        let is_video = matches!(self.kind, TrackKind::Video { .. });
        let duration_ms = ticks_to_ns(self.total_duration, self.timescale) / 1_000_000;

        let mut trak = Vec::new();
        mp4_atom::Tkhd {
            creation_time: 0,
            modification_time: 0,
            track_id: id,
            duration: duration_ms,
            layer: 0,
            alternate_group: 0,
            volume: mp4_atom::FixedPoint::new(if is_video { 0 } else { 1 }, 0),
            matrix: mp4_atom::Matrix::default(),
            width: mp4_atom::FixedPoint::new(
                match &self.kind {
                    TrackKind::Video(video) => video.width as u16,
                    TrackKind::Audio(_) => 0,
                },
                0,
            ),
            height: mp4_atom::FixedPoint::new(
                match &self.kind {
                    TrackKind::Video(video) => video.height as u16,
                    TrackKind::Audio(_) => 0,
                },
                0,
            ),
            enabled: true,
            in_movie: true,
            size_is_aspect_ratio: false,
        }
        .encode(&mut trak)?;

        // An audio encoder's priming samples decode before the real
        // audio does; an edit list that starts the track's presentation
        // at the end of the priming is how MP4 says "discard these".
        // Without it the audio plays that much late against the video.
        if let TrackKind::Audio(audio) = &self.kind {
            let codec_delay_samples = audio.codec_delay_samples;
            if codec_delay_samples > 0 {
                mp4_atom::Edts {
                    elst: Some(mp4_atom::Elst {
                        entries: vec![mp4_atom::ElstEntry {
                            segment_duration: movie_duration.saturating_sub(
                                ticks_to_ns(codec_delay_samples, self.timescale) / 1_000_000,
                            ),
                            media_time: Some(codec_delay_samples),
                            media_rate: 1.into(),
                        }],
                    }),
                }
                .encode(&mut trak)?;
            }
        }

        let mut mdia = Vec::new();
        mp4_atom::Mdhd {
            creation_time: 0,
            modification_time: 0,
            timescale: self.timescale,
            duration: self.total_duration,
            language: "und".parse().unwrap_or_default(),
        }
        .encode(&mut mdia)?;
        mp4_atom::Hdlr {
            handler: if is_video { b"vide".into() } else { b"soun".into() },
            name: if is_video { "VideoHandler" } else { "SoundHandler" }.into(),
        }
        .encode(&mut mdia)?;

        let mut minf = Vec::new();
        if is_video {
            mp4_atom::Vmhd::default().encode(&mut minf)?;
        } else {
            mp4_atom::Smhd::default().encode(&mut minf)?;
        }
        mp4_atom::Dinf {
            dref: mp4_atom::Dref {
                urls: vec![mp4_atom::Url::default()],
            },
        }
        .encode(&mut minf)?;
        self.encode_stbl(&mut minf)?;
        Any::Unknown(FourCC::new(b"minf"), minf).encode(&mut mdia)?;
        Any::Unknown(FourCC::new(b"mdia"), mdia).encode(&mut trak)?;
        Any::Unknown(FourCC::new(b"trak"), trak).encode(out)?;
        Ok(())
    }

    fn encode_stbl(&self, out: &mut Vec<u8>) -> crate::Result<()> {
        let mut stbl = Vec::new();
        self.encode_stsd(&mut stbl)?;

        mp4_atom::Stts {
            entries: self
                .durations
                .iter()
                .map(|&(count, delta)| mp4_atom::SttsEntry {
                    sample_count: count,
                    sample_delta: delta,
                })
                .collect(),
        }
        .encode(&mut stbl)?;

        if !self.all_sync {
            mp4_atom::Stss {
                entries: self.sync.clone(),
            }
            .encode(&mut stbl)?;
        }

        // `stsc` runs together consecutive chunks holding the same
        // number of samples.
        let mut stsc_entries: Vec<mp4_atom::StscEntry> = Vec::new();
        for (i, chunk) in self.chunks.iter().enumerate() {
            let repeats = stsc_entries
                .last()
                .is_some_and(|last| last.samples_per_chunk == chunk.samples);
            if !repeats {
                stsc_entries.push(mp4_atom::StscEntry {
                    first_chunk: i as u32 + 1,
                    samples_per_chunk: chunk.samples,
                    sample_description_index: 1,
                });
            }
        }
        mp4_atom::Stsc {
            entries: stsc_entries,
        }
        .encode(&mut stbl)?;

        mp4_atom::Stsz {
            samples: mp4_atom::StszSamples::Different {
                sizes: self.samples.iter().map(|&(_, size)| size).collect(),
            },
        }
        .encode(&mut stbl)?;

        // 32-bit chunk offsets while the file fits in 4 GiB, which is
        // the more widely understood form; past that they have to be
        // 64-bit.
        let offsets: Vec<u64> = self.chunks.iter().map(|c| c.offset).collect();
        if offsets.iter().any(|&o| o > u32::MAX as u64) {
            mp4_atom::Co64 { entries: offsets }.encode(&mut stbl)?;
        } else {
            mp4_atom::Stco {
                entries: offsets.iter().map(|&o| o as u32).collect(),
            }
            .encode(&mut stbl)?;
        }
        Any::Unknown(FourCC::new(b"stbl"), stbl).encode(out)?;
        Ok(())
    }

    fn encode_stsd(&self, out: &mut Vec<u8>) -> crate::Result<()> {
        let codec = match &self.kind {
            TrackKind::Video(video) => video_sample_entry(video)?,
            TrackKind::Audio(audio) => audio_sample_entry(audio)?,
        };
        mp4_atom::Stsd { codecs: vec![codec] }.encode(out)?;
        Ok(())
    }
}

/// The sample entry describing a video track.
fn video_sample_entry(video: &VideoTrackInfo) -> crate::Result<mp4_atom::Codec> {
    if video.codec != VideoCodec::H264 {
        return Err(crate::Error::internal(format!(
            "MP4 has no sample entry here for {:?}",
            video.codec
        )));
    }
    // The bare record the H.264 parser produced, no box header around it.
    let avcc = mp4_atom::Avcc::decode_body(&mut video.codec_private.as_slice())
        .map_err(|e| crate::Error::bitstream("H.264", format!("unusable avcC record: {e}")))?;
    Ok(mp4_atom::Avc1 {
        visual: mp4_atom::Visual {
            data_reference_index: 1,
            width: video.width as u16,
            height: video.height as u16,
            horizresolution: mp4_atom::FixedPoint::new(72, 0),
            vertresolution: mp4_atom::FixedPoint::new(72, 0),
            frame_count: 1,
            compressor: Default::default(),
            depth: 24,
        },
        avcc,
        colr: video.color.map(|c| mp4_atom::Colr::Nclx {
            colour_primaries: c.primaries as u16,
            transfer_characteristics: c.transfer as u16,
            matrix_coefficients: c.matrix as u16,
            full_range_flag: c.full_range,
        }),
        pasp: None,
        btrt: None,
        taic: None,
        fiel: None,
    }
    .into())
}

/// The sample entry describing an audio track, with the `esds`
/// descriptor chain built from the `AudioSpecificConfig` the stream
/// reported.
///
/// mp4-atom models that config by its fields rather than as a blob and
/// re-encodes it, so the two bytes are unpacked here: 5 bits of audio
/// object type, 4 of sampling frequency index, 4 of channel configuration
/// (ISO/IEC 14496-3).
fn audio_sample_entry(audio: &AudioTrackInfo) -> crate::Result<mp4_atom::Codec> {
    if audio.codec != AudioCodec::Aac {
        return Err(crate::Error::internal(format!(
            "MP4 has no sample entry here for {:?}",
            audio.codec
        )));
    }
    let (a, b) = match audio.codec_private.as_slice() {
        [a, b, ..] => (*a, *b),
        other => {
            return Err(crate::Error::bitstream(
                "AAC",
                format!("expected a 2-byte AudioSpecificConfig, got {other:?}"),
            ))
        }
    };
    Ok(mp4_atom::Mp4a {
        audio: mp4_atom::Audio {
            data_reference_index: 1,
            channel_count: audio.channels as u16,
            sample_size: 16,
            sample_rate: mp4_atom::FixedPoint::new(audio.sample_rate as u16, 0),
        },
        esds: mp4_atom::Esds {
            es_desc: mp4_atom::esds::EsDescriptor {
                es_id: 1,
                dec_config: mp4_atom::esds::DecoderConfig {
                    // 0x40 is MPEG-4 audio; stream type 5 is audio.
                    object_type_indication: 0x40,
                    stream_type: 5,
                    up_stream: 0,
                    buffer_size_db: [0, 0, 0].into(),
                    max_bitrate: 0,
                    avg_bitrate: 0,
                    dec_specific: mp4_atom::esds::DecoderSpecific {
                        profile: a >> 3,
                        freq_index: ((a & 0b111) << 1) | (b >> 7),
                        chan_conf: (b >> 3) & 0b1111,
                    },
                },
                sl_config: mp4_atom::esds::SLConfig::default(),
            },
        },
        btrt: None,
        taic: None,
    }
    .into())
}


/// A Nero `chpl` box: the chapter form ffmpeg writes into `moov/udta`
/// and that ffmpeg, VLC and mpv all read back.
///
/// Layout, matching ffmpeg's `mov_write_chpl_tag` byte for byte: version
/// 1 and empty flags, four unused bytes, a *single-byte* chapter count,
/// then per chapter a 64-bit start time in 100-nanosecond units and a
/// length-prefixed UTF-8 title. The one-byte count is why the format
/// carries at most 255 chapters.
fn chpl_atom(chapters: &[Chapter], config: &MuxConfig) -> Vec<u8> {
    let count = chapters.len().min(u8::MAX as usize);
    if count < chapters.len() {
        log::warn!("chpl carries at most {count} chapters; dropping the rest");
    }
    let mut body = vec![0x01, 0x00, 0x00, 0x00];
    body.extend_from_slice(&[0x00; 4]);
    body.push(count as u8);
    for chapter in chapters.iter().take(count) {
        let (start_ns, _) = super::chapter_bounds_ns(chapter, &config.video);
        body.extend_from_slice(&(start_ns / 100).to_be_bytes());
        let mut title = chapter.title.as_bytes().to_vec();
        title.truncate(u8::MAX as usize);
        body.push(title.len() as u8);
        body.extend_from_slice(&title);
    }
    let mut out = Vec::new();
    // `chpl` is a full box: the version and flags are the first four
    // bytes of the body above.
    let _ = Any::Unknown(FourCC::new(b"chpl"), body).encode(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AudioTrackInfo, ColorInfo, Container, Packet, VideoTrackInfo, VIDEO_TRACK};

    /// A minimal but real avcC record: one SPS and one PPS.
    fn avcc() -> Vec<u8> {
        vec![
            1, 0x64, 0x00, 0x0d, 0xff, 0xe1, 0x00, 0x04, 0x67, 0x64, 0x00, 0x0d, 0x01, 0x00, 0x02, 0x68, 0xee,
        ]
    }

    fn config() -> MuxConfig {
        MuxConfig {
            container: Container::Mp4,
            video: VideoTrackInfo {
                codec: VideoCodec::H264,
                width: 720,
                height: 480,
                timescale: 16_777_216,
                frame_duration: 280_896,
                color: Some(ColorInfo::SRGB_FULL),
                codec_private: avcc(),
            },
            audio: vec![AudioTrackInfo {
                codec: AudioCodec::Aac,
                sample_rate: 48_000,
                channels: 2,
                codec_private: vec![0x11, 0x90],
                codec_delay_samples: 1024,
            }],
            writing_app: "encoder-facade-test".into(),
        }
    }

    fn mux(chapters: &[Chapter]) -> Vec<u8> {
        let mut muxer = Mp4Muxer::new(config()).unwrap();
        let mut file = Vec::new();
        for frame in 0..10u64 {
            muxer
                .write(
                    VIDEO_TRACK,
                    &Packet {
                        pts: frame * 280_896,
                        duration: 280_896,
                        keyframe: frame % 5 == 0,
                        data: vec![0u8; 40],
                    },
                )
                .unwrap();
            muxer
                .write(
                    VIDEO_TRACK + 1,
                    &Packet {
                        pts: frame * 1024,
                        duration: 1024,
                        keyframe: true,
                        data: vec![0u8; 20],
                    },
                )
                .unwrap();
            file.extend_from_slice(&muxer.take_output());
        }
        let patches = muxer.finish(chapters).unwrap();
        file.extend_from_slice(&muxer.take_output());
        for patch in patches {
            let at = patch.position as usize;
            file[at..at + patch.bytes.len()].copy_from_slice(&patch.bytes);
        }
        file
    }

    fn sample_sizes(samples: &mp4_atom::StszSamples) -> Vec<u32> {
        match samples {
            mp4_atom::StszSamples::Different { sizes } => sizes.clone(),
            mp4_atom::StszSamples::Identical { size, count } => vec![*size; *count as usize],
        }
    }

    /// Walk the top-level atoms and check they tile the file exactly —
    /// the failure mode a wrong size field produces.
    fn top_level_atoms(file: &[u8]) -> Vec<(String, u64)> {
        let mut atoms = Vec::new();
        let mut at = 0usize;
        while at + 8 <= file.len() {
            let size32 = u32::from_be_bytes(file[at..at + 4].try_into().unwrap()) as u64;
            let kind = String::from_utf8_lossy(&file[at + 4..at + 8]).to_string();
            let (size, _header) = if size32 == 1 {
                (
                    u64::from_be_bytes(file[at + 8..at + 16].try_into().unwrap()),
                    16,
                )
            } else {
                (size32, 8)
            };
            assert!(size >= 8, "atom {kind} has an impossible size {size}");
            atoms.push((kind, size));
            at += size as usize;
        }
        assert_eq!(at, file.len(), "atoms must tile the file exactly");
        atoms
    }

    #[test]
    fn file_is_ftyp_mdat_moov() {
        let file = mux(&[]);
        let atoms = top_level_atoms(&file);
        let kinds: Vec<&str> = atoms.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(kinds, vec!["ftyp", "mdat", "moov"]);
        // 10 video samples of 40 bytes and 10 audio of 20, plus the
        // 16-byte 64-bit mdat header.
        assert_eq!(atoms[1].1, 16 + 10 * 40 + 10 * 20);
    }

    /// Parse the moov the muxer wrote. `decode_body` starts at an atom's
    /// payload, so the 8-byte header is skipped.
    fn parse_moov(file: &[u8]) -> mp4_atom::Moov {
        let moov_size = top_level_atoms(file)
            .last()
            .map(|(_, size)| *size as usize)
            .expect("a moov must be present");
        let body = &file[file.len() - moov_size + 8..];
        mp4_atom::Moov::decode_body(&mut &body[..]).expect("moov must parse")
    }

    #[test]
    fn moov_is_parseable_and_describes_both_tracks() {
        let file = mux(&[]);
        let moov = parse_moov(&file);
        assert_eq!(moov.trak.len(), 2);
        assert_eq!(moov.mvhd.timescale, MOVIE_TIMESCALE);

        let video = &moov.trak[0];
        assert_eq!(video.mdia.mdhd.timescale, 16_777_216);
        let stbl = &video.mdia.minf.stbl;
        assert_eq!(sample_sizes(&stbl.stsz.samples).len(), 10);
        assert_eq!(
            stbl.stss.as_ref().map(|s| s.entries.clone()),
            Some(vec![1, 6]),
            "keyframes at samples 1 and 6"
        );
        // Ten frames of one duration collapse to a single stts run.
        assert_eq!(stbl.stts.entries.len(), 1);
        assert_eq!(stbl.stts.entries[0].sample_count, 10);
        assert_eq!(stbl.stts.entries[0].sample_delta, 280_896);

        let audio = &moov.trak[1];
        assert!(audio.mdia.minf.stbl.stss.is_none(), "every AAC frame is a sync sample");
        assert!(
            audio.edts.is_some(),
            "the AAC track needs an edit list to trim its priming"
        );
    }

    #[test]
    fn every_sample_offset_lands_inside_mdat_at_the_right_size() {
        let file = mux(&[]);
        let atoms = top_level_atoms(&file);
        let mdat_start = atoms[0].1;
        let mdat_end = mdat_start + atoms[1].1;
        let moov = parse_moov(&file);
        for trak in &moov.trak {
            let stbl = &trak.mdia.minf.stbl;
            let sizes = sample_sizes(&stbl.stsz.samples);
            let offsets: Vec<u64> = match (&stbl.stco, &stbl.co64) {
                (Some(stco), _) => stco.entries.iter().map(|&o| o as u64).collect(),
                (_, Some(co64)) => co64.entries.clone(),
                _ => panic!("a track needs chunk offsets"),
            };
            // Walk chunks through stsc and confirm each sample's bytes
            // sit within mdat.
            let mut sample = 0usize;
            for (chunk_index, &offset) in offsets.iter().enumerate() {
                let per_chunk = stbl
                    .stsc
                    .entries
                    .iter()
                    .take_while(|e| e.first_chunk as usize <= chunk_index + 1)
                    .last()
                    .map(|e| e.samples_per_chunk)
                    .expect("stsc must cover every chunk");
                let mut at = offset;
                for _ in 0..per_chunk {
                    let size = sizes[sample] as u64;
                    assert!(
                        at >= mdat_start && at + size <= mdat_end,
                        "sample {sample} at {at}+{size} is outside mdat {mdat_start}..{mdat_end}"
                    );
                    at += size;
                    sample += 1;
                }
            }
            assert_eq!(sample, sizes.len(), "stsc must account for every sample");
        }
    }

    #[test]
    fn chapters_land_in_a_chpl_box() {
        let file = mux(&[
            Chapter {
                title: "Round 1".into(),
                start_frame: 0,
                end_frame: 5,
            },
            Chapter {
                title: "Round 2".into(),
                start_frame: 5,
                end_frame: 10,
            },
        ]);
        let chpl_at = file
            .windows(4)
            .position(|w| w == b"chpl")
            .expect("a chpl box must be present");
        // Past the type: version and flags, four unused bytes, then the
        // one-byte chapter count.
        let body = &file[chpl_at + 4..];
        assert_eq!(body[8], 2, "two chapters");
        // First chapter starts at zero, second at five frames in.
        assert_eq!(u64::from_be_bytes(body[9..17].try_into().unwrap()), 0);
        let title_len = body[17] as usize;
        assert_eq!(&body[18..18 + title_len], b"Round 1");
        let second = &body[18 + title_len..];
        assert_eq!(
            u64::from_be_bytes(second[..8].try_into().unwrap()),
            837_135 // five frames in, in 100 ns units
        );
    }

    #[test]
    fn non_h264_video_is_refused_up_front() {
        let mut config = config();
        config.video.codec = VideoCodec::Vp9;
        assert!(Mp4Muxer::new(config).is_err());
    }
}

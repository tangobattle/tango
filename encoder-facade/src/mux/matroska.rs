//! Matroska and WebM, written with [`mkv_element`]'s typed element
//! tree.
//!
//! Layout, in the order bytes reach the output:
//!
//! ```text
//!   EBML head
//!   Segment (8-byte size placeholder, patched at finish)
//!     Void  ── reserved; patched at finish with Info + SeekHead
//!     Tracks
//!     Cluster...
//!     Cues
//!     Chapters
//! ```
//!
//! Two things can only be known once the stream ends: the Segment's size
//! and the Duration. Rather than leave the Segment unsized (legal, but
//! it costs seekability) both are handled by reserving a region up front
//! and patching a complete Info + SeekHead into it at the close — which
//! also puts a seek index at the head of the file, where players look
//! for one.

use bytes::Bytes;
use mkv_element::prelude::*;
use mkv_element::io::blocking_impl::WriteTo;
use mkv_element::ClusterBlock;

use super::{Chapter, Fixup, MuxConfig, Muxer};
use crate::packet::ticks_to_ns;
use crate::{AudioCodec, Container, Packet, VideoCodec, VIDEO_TRACK};



/// Matroska timestamps are counted in `TimestampScale` nanoseconds. One
/// millisecond is what every muxer uses and what keeps a SimpleBlock's
/// 16-bit relative timestamp comfortable.
const TIMESTAMP_SCALE_NS: u64 = 1_000_000;

/// Bytes held for the SeekHead + Info that get patched in at the close.
/// Both are small and fixed in shape; 1 KiB is many times over.
const RESERVED_HEAD: usize = 1024;

/// Cap on how much time one cluster may span. Keyframes normally close
/// clusters long before this; it's a backstop so a stream with sparse
/// keyframes can't overflow a block's 16-bit relative timestamp.
const MAX_CLUSTER_SPAN_MS: i64 = 5_000;

/// Opus decoders need audio before a seek point to converge, and 80 ms is
/// what RFC 7845 §4.2 recommends a muxer declare.
const OPUS_SEEK_PREROLL_NS: u64 = 80_000_000;

pub struct MatroskaMuxer {
    config: MuxConfig,
    out: Vec<u8>,
    /// Absolute output position, counting bytes already drained.
    pos: u64,
    segment_size_pos: u64,
    segment_data_start: u64,
    reserved_pos: u64,
    tracks_pos: u64,
    cluster: Vec<ClusterBlock>,
    cluster_start_ms: i64,
    cluster_open: bool,
    /// Cue for the cluster being assembled: its keyframe's timestamp,
    /// filled in when a cluster opens on a video keyframe.
    cluster_cue_ms: Option<u64>,
    cues: Vec<(u64, u64)>,
    last_ms: i64,
    video_frames: u64,
}

impl MatroskaMuxer {
    pub fn new(config: MuxConfig) -> crate::Result<Self> {
        let mut muxer = Self {
            config,
            out: Vec::new(),
            pos: 0,
            segment_size_pos: 0,
            segment_data_start: 0,
            reserved_pos: 0,
            tracks_pos: 0,
            cluster: Vec::new(),
            cluster_start_ms: 0,
            cluster_open: false,
            cluster_cue_ms: None,
            cues: Vec::new(),
            last_ms: 0,
            video_frames: 0,
        };
        muxer.write_head()?;
        Ok(muxer)
    }

    fn write_head(&mut self) -> crate::Result<()> {
        let webm = self.config.container == Container::WebM;
        let ebml = Ebml {
            ebml_version: Some(EbmlVersion(1)),
            ebml_read_version: Some(EbmlReadVersion(1)),
            ebml_max_id_length: EbmlMaxIdLength(4),
            ebml_max_size_length: EbmlMaxSizeLength(8),
            doc_type: Some(DocType(if webm { "webm".into() } else { "matroska".into() })),
            doc_type_version: Some(DocTypeVersion(4)),
            doc_type_read_version: Some(DocTypeReadVersion(2)),
            ..Default::default()
        };
        self.emit_element(&ebml)?;

        // The Segment header is the one element written by hand: its
        // size has to be patched later, which means reserving a
        // fixed-width VINT, and the library always encodes a size in
        // its shortest form. An 8-byte all-ones VINT is the "unknown
        // size" encoding, so the file is valid even if an export dies
        // before the patch lands.
        let id = Segment::ID.as_encoded().to_be_bytes();
        self.emit(&id[id.len() - 4..]);
        self.segment_size_pos = self.pos;
        self.emit([0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF].as_ref());
        self.segment_data_start = self.pos;

        self.reserved_pos = self.pos;
        self.emit(&void_padding(RESERVED_HEAD)?);

        self.tracks_pos = self.pos;
        let tracks = self.build_tracks()?;
        self.emit_element(&tracks)?;
        Ok(())
    }

    fn build_tracks(&self) -> crate::Result<Tracks> {
        let video = &self.config.video;
        let mut entries = vec![TrackEntry {
            track_number: TrackNumber(1),
            track_uid: TrackUid(1),
            track_type: TrackType(1), // video
            flag_lacing: FlagLacing(0),
            codec_id: CodecId(
                match video.codec {
                    VideoCodec::H264 => "V_MPEG4/ISO/AVC",
                    VideoCodec::Vp8 => "V_VP8",
                    VideoCodec::Vp9 => "V_VP9",
                }
                .into(),
            ),
            codec_private: (!video.codec_private.is_empty())
                .then(|| CodecPrivate(Bytes::from(video.codec_private.clone()))),
            default_duration: Some(DefaultDuration(ticks_to_ns(video.frame_duration, video.timescale))),
            video: Some(Video {
                pixel_width: PixelWidth(video.width as u64),
                pixel_height: PixelHeight(video.height as u64),
                colour: video.color.map(|c| Colour {
                    primaries: Primaries(c.primaries as u64),
                    transfer_characteristics: TransferCharacteristics(c.transfer as u64),
                    matrix_coefficients: MatrixCoefficients(c.matrix as u64),
                    range: Range(if c.full_range { 2 } else { 1 }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }];

        for (i, audio) in self.config.audio.iter().enumerate() {
            let number = 2 + i as u64;
            entries.push(TrackEntry {
                track_number: TrackNumber(number),
                track_uid: TrackUid(number),
                track_type: TrackType(2), // audio
                flag_lacing: FlagLacing(0),
                codec_id: CodecId(
                    match audio.codec {
                        AudioCodec::Aac => "A_AAC",
                        AudioCodec::Opus => "A_OPUS",
                        AudioCodec::Flac => "A_FLAC",
                        AudioCodec::PcmS16Le => "A_PCM/INT/LIT",
                    }
                    .into(),
                ),
                codec_private: (!audio.codec_private.is_empty())
                    .then(|| CodecPrivate(Bytes::from(audio.codec_private.clone()))),
                // What a player must discard so the audio it plays
                // lines up with the video.
                codec_delay: CodecDelay(audio.codec_delay_ns()),
                seek_pre_roll: SeekPreRoll(if audio.codec == AudioCodec::Opus {
                    OPUS_SEEK_PREROLL_NS
                } else {
                    0
                }),
                audio: Some(Audio {
                    sampling_frequency: SamplingFrequency(audio.sample_rate as f64),
                    channels: Channels(audio.channels as u64),
                    // PCM's codec ID doesn't imply a sample width, so
                    // Matroska wants it stated.
                    bit_depth: (audio.codec == AudioCodec::PcmS16Le).then_some(BitDepth(16)),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
        Ok(Tracks {
            track_entry: entries,
            ..Default::default()
        })
    }
}

impl Muxer for MatroskaMuxer {
    fn write(&mut self, track: usize, packet: &Packet) -> crate::Result<()> {
        let is_video = track == VIDEO_TRACK;
        let timescale = if is_video {
            self.config.video.timescale
        } else {
            let audio = self
                .config
                .audio
                .get(track - 1)
                .ok_or_else(|| crate::Error::internal(format!("no audio track {}", track - 1)))?;
            audio.sample_rate
        };
        let ms = ms_from_ticks(packet.pts, timescale);

        // Clusters break on video keyframes so that every cluster after
        // the first starts at a seek point.
        let split = !self.cluster_open
            || (is_video && packet.keyframe && !self.cluster.is_empty() && ms > self.cluster_start_ms)
            || ms - self.cluster_start_ms > MAX_CLUSTER_SPAN_MS;
        if split {
            self.flush_cluster()?;
            self.cluster_start_ms = ms;
            self.cluster_open = true;
            self.cluster_cue_ms = (is_video && packet.keyframe).then_some(ms.max(0) as u64);
        }

        let relative = (ms - self.cluster_start_ms).clamp(i16::MIN as i64, i16::MAX as i64) as i16;
        self.cluster.push(ClusterBlock::Simple(simple_block(
            track as u64 + 1,
            relative,
            packet.keyframe,
            &packet.data,
        )?));
        self.last_ms = self.last_ms.max(ms);
        if is_video {
            self.video_frames += 1;
        }
        Ok(())
    }

    fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }

    fn finish(&mut self, chapters: &[Chapter]) -> crate::Result<Vec<Fixup>> {
        self.flush_cluster()?;
        self.cluster_open = false;

        let cues_pos = self.pos;
        let has_cues = !self.cues.is_empty();
        if has_cues {
            let cues = Cues {
                cue_point: self
                    .cues
                    .iter()
                    .map(|&(ms, position)| CuePoint {
                        cue_time: CueTime(ms),
                        cue_track_positions: vec![CueTrackPositions {
                            cue_track: CueTrack(1),
                            cue_cluster_position: CueClusterPosition(position),
                            ..Default::default()
                        }],
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            };
            self.emit_element(&cues)?;
        }

        let chapters_pos = self.pos;
        if !chapters.is_empty() {
            let chapters = self.build_chapters(chapters);
            self.emit_element(&chapters)?;
        }

        // Now that every top-level element's position is known, fill the
        // reserved region with the Info carrying the final duration and
        // a seek index pointing at everything.
        //
        // Info goes first so that its own position is known before the
        // SeekHead that has to point at it — the other way round the two
        // sizes would depend on each other.
        let mut head = Vec::new();
        Info {
            timestamp_scale: TimestampScale(TIMESTAMP_SCALE_NS),
            duration: Some(Duration(self.duration_ms())),
            muxing_app: MuxingApp(self.config.writing_app.clone()),
            writing_app: WritingApp(self.config.writing_app.clone()),
            ..Default::default()
        }
        .write_to(&mut head)?;
        let info_len = head.len();
        let mut seeks = vec![
            seek_entry(Info::ID, self.reserved_pos - self.segment_data_start),
            seek_entry(Tracks::ID, self.tracks_pos - self.segment_data_start),
        ];
        if has_cues {
            seeks.push(seek_entry(Cues::ID, cues_pos - self.segment_data_start));
        }
        if !chapters.is_empty() {
            seeks.push(seek_entry(Chapters::ID, chapters_pos - self.segment_data_start));
        }
        SeekHead {
            seek: seeks,
            ..Default::default()
        }
        .write_to(&mut head)?;
        if head.len() + 2 > RESERVED_HEAD {
            return Err(crate::Error::internal(format!(
                "reserved {RESERVED_HEAD} bytes for Info ({info_len}) + SeekHead ({}), which does not fit",
                head.len() - info_len
            )));
        }
        // Whatever is left of the region becomes padding.
        head.extend_from_slice(&void_padding(RESERVED_HEAD - head.len())?);

        let segment_size = self.pos - self.segment_data_start;
        Ok(vec![
            Fixup::Overwrite {
                position: self.segment_size_pos,
                bytes: fixed_width_size(segment_size),
            },
            Fixup::Overwrite {
                position: self.reserved_pos,
                bytes: head,
            },
        ])
    }
}

impl MatroskaMuxer {
    /// Write out the cluster being assembled, recording a cue for it if
    /// it opened on a keyframe.
    fn flush_cluster(&mut self) -> crate::Result<()> {
        if !self.cluster_open || self.cluster.is_empty() {
            self.cluster.clear();
            return Ok(());
        }
        if let Some(cue_ms) = self.cluster_cue_ms.take() {
            self.cues.push((cue_ms, self.pos - self.segment_data_start));
        }
        let cluster = Cluster {
            timestamp: Timestamp(self.cluster_start_ms.max(0) as u64),
            blocks: std::mem::take(&mut self.cluster),
            ..Default::default()
        };
        self.emit_element(&cluster)?;
        Ok(())
    }

    fn build_chapters(&self, chapters: &[Chapter]) -> Chapters {
        Chapters {
            edition_entry: vec![EditionEntry {
                edition_uid: Some(EditionUid(1)),
                edition_flag_default: EditionFlagDefault(1),
                chapter_atom: chapters
                    .iter()
                    .enumerate()
                    .map(|(i, chapter)| {
                        let (start, end) = super::chapter_bounds_ns(chapter, &self.config.video);
                        ChapterAtom {
                            // Matroska chapter times are nanoseconds,
                            // independent of TimestampScale.
                            chapter_uid: ChapterUid(i as u64 + 1),
                            chapter_time_start: ChapterTimeStart(start),
                            chapter_time_end: Some(ChapterTimeEnd(end)),
                            chapter_display: vec![ChapterDisplay {
                                chap_string: ChapString(chapter.title.clone()),
                                chap_language: vec![ChapLanguage("und".into())],
                                ..Default::default()
                            }],
                            ..Default::default()
                        }
                    })
                    .collect(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    /// Duration in TimestampScale units. Taken from the video frame
    /// count rather than the last block's timestamp: the frame count is
    /// exact, and the audio tail usually runs a little past the last
    /// frame after an encoder pads its final packet.
    fn duration_ms(&self) -> f64 {
        let video = &self.config.video;
        let ticks = self.video_frames.saturating_mul(video.frame_duration);
        let ns = ticks_to_ns(ticks, video.timescale);
        (ns.max(self.last_ms.max(0) as u64 * TIMESTAMP_SCALE_NS) as f64) / TIMESTAMP_SCALE_NS as f64
    }

    fn emit_element<E: WriteTo>(&mut self, element: &E) -> crate::Result<()> {
        let mut buf = Vec::new();
        element.write_to(&mut buf)?;
        self.emit(&buf);
        Ok(())
    }

    fn emit(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
        self.pos += bytes.len() as u64;
    }
}

/// Round a tick count to whole milliseconds — the resolution Matroska
/// block timestamps have. Always computed from the packet's exact tick
/// value, so the error stays under half a millisecond instead of
/// accumulating across an export.
fn ms_from_ticks(ticks: u64, timescale: u32) -> i64 {
    (ticks_to_ns(ticks, timescale) / TIMESTAMP_SCALE_NS) as i64
}

/// A SimpleBlock body: track number as a VINT, a signed 16-bit
/// timestamp relative to the cluster, a flags byte, then the frame.
fn simple_block(track: u64, relative: i16, keyframe: bool, data: &[u8]) -> crate::Result<SimpleBlock> {
    let mut body = Vec::with_capacity(4 + data.len());
    VInt64::new(track).write_to(&mut body)?;
    body.extend_from_slice(&relative.to_be_bytes());
    body.push(if keyframe { 0x80 } else { 0x00 });
    body.extend_from_slice(data);
    Ok(SimpleBlock(Bytes::from(body)))
}

fn seek_entry(id: VInt64, position: u64) -> Seek {
    let encoded = id.as_encoded().to_be_bytes();
    let first = encoded.iter().position(|&b| b != 0).unwrap_or(encoded.len() - 1);
    Seek {
        crc32: None,
        void: None,
        seek_id: SeekId(Bytes::copy_from_slice(&encoded[first..])),
        seek_position: SeekPosition(position),
    }
}

/// The always-8-byte form of an element size, for patching over the
/// placeholder written at the head of the Segment.
fn fixed_width_size(size: u64) -> Vec<u8> {
    debug_assert!(size < 1 << 56, "segment too large for an 8-byte size");
    let mut bytes = size.to_be_bytes();
    bytes[0] = 0x01; // 8-byte length marker; the value rides in the rest.
    bytes.to_vec()
}

/// Padding that occupies exactly `total` bytes, as one Void element or
/// two.
///
/// A Void is a 1-byte ID, a size VINT and the padding itself, so the
/// size VINT's own width counts toward the total — and because the
/// library always writes a size in its shortest form, some totals are
/// unreachable with a single element (129 is: the candidates around it
/// come out 128 and 130). A second Void covers the gap, which is
/// ordinary practice; a run of Void elements is as good as one.
fn void_padding(total: usize) -> crate::Result<Vec<u8>> {
    if total == 0 {
        return Ok(Vec::new());
    }
    if total < 2 {
        return Err(crate::Error::internal(format!(
            "padding needs at least 2 bytes for a Void element, not {total}"
        )));
    }
    if let Some(exact) = void_element(total)? {
        return Ok(exact);
    }
    // Shave off a minimal Void and pad the rest; `total - 2` is
    // reachable whenever `total` isn't.
    let mut out = void_element(2)?.ok_or_else(|| crate::Error::internal("a 2-byte Void must exist"))?;
    out.extend_from_slice(&void_padding(total - 2)?);
    if out.len() != total {
        return Err(crate::Error::internal(format!(
            "padding came to {} bytes, not {total}",
            out.len()
        )));
    }
    Ok(out)
}

/// One Void element of exactly `total` bytes, or `None` if no single
/// element is that long.
fn void_element(total: usize) -> crate::Result<Option<Vec<u8>>> {
    for size_width in 1..=8usize {
        let Some(payload) = total.checked_sub(1 + size_width) else {
            break;
        };
        let mut buf = Vec::with_capacity(total);
        Void { size: payload as u64 }.write_to(&mut buf)?;
        if buf.len() == total {
            return Ok(Some(buf));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::apply;
    use crate::{AudioTrackInfo, ColorInfo, VideoTrackInfo};

    fn config(container: Container) -> MuxConfig {
        MuxConfig {
            container,
            video: VideoTrackInfo {
                codec: VideoCodec::H264,
                width: 240,
                height: 160,
                timescale: 16_777_216,
                frame_duration: 280_896,
                color: Some(ColorInfo::SRGB_FULL),
                codec_private: vec![1, 0x64, 0, 0x0d, 0xff, 0xe1, 0, 2, 0x67, 0x64, 0x01, 0, 4, 0x68, 0xee],
            },
            audio: vec![AudioTrackInfo {
                codec: AudioCodec::Aac,
                sample_rate: 48_000,
                channels: 2,
                codec_private: vec![0x11, 0x90],
                codec_delay_samples: 1024,
            }],
            writing_app: "encoder-facade-test".into(),
            faststart: false,
        }
    }

    /// Drop the zero padding EBML permits on string elements.
    fn trim(s: &str) -> &str {
        s.trim_end_matches('\0')
    }

    /// Mux a few frames and hand the bytes back, as a finished file
    /// would look on disk.
    fn mux(container: Container, chapters: &[Chapter]) -> Vec<u8> {
        let mut muxer = MatroskaMuxer::new(config(container)).unwrap();
        let mut file = Vec::new();
        for frame in 0..10u64 {
            muxer
                .write(
                    VIDEO_TRACK,
                    &Packet {
                        pts: frame * 280_896,
                        duration: 280_896,
                        keyframe: frame % 5 == 0,
                        data: vec![0u8; 32],
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
                        data: vec![0u8; 16],
                    },
                )
                .unwrap();
            file.extend_from_slice(&muxer.take_output());
        }
        let fixups = muxer.finish(chapters).unwrap();
        file.extend_from_slice(&muxer.take_output());
        apply(file, &fixups)
    }

    /// The real test of a muxer: a demuxer nobody here wrote has to
    /// make sense of the file.
    #[test]
    fn output_satisfies_an_independent_demuxer() {
        let file = mux(Container::Matroska, &[]);
        let mut mkv = matroska_demuxer::MatroskaFile::open(std::io::Cursor::new(file)).unwrap();
        let tracks = mkv.tracks();
        assert_eq!(tracks.len(), 2);
        // mkv-element zero-pads string elements, which EBML allows and
        // C-string readers (ffmpeg, VLC, mpv) stop at; this demuxer
        // hands the padding back, so trim it before comparing.
        assert_eq!(trim(tracks[0].codec_id()), "V_MPEG4/ISO/AVC");
        assert_eq!(trim(tracks[1].codec_id()), "A_AAC");
        assert_eq!(tracks[1].codec_delay(), Some(21_333_333), "AAC priming is declared");
        let video = tracks[0].video().unwrap();
        assert_eq!((video.pixel_width().get(), video.pixel_height().get()), (240, 160));
        assert_eq!(mkv.info().timestamp_scale().get(), TIMESTAMP_SCALE_NS);

        let mut frames = 0;
        let mut frame = matroska_demuxer::Frame::default();
        while mkv.next_frame(&mut frame).unwrap() {
            frames += 1;
        }
        assert_eq!(frames, 20, "10 video frames and 10 audio frames come back");
    }

    #[test]
    fn chapters_survive_the_round_trip() {
        let chapters = vec![
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
        ];
        let file = mux(Container::Matroska, &chapters);
        let mkv = matroska_demuxer::MatroskaFile::open(std::io::Cursor::new(file)).unwrap();
        let editions = mkv.chapters().expect("the file must carry chapters");
        assert_eq!(editions.len(), 1);
        let atoms = editions[0].chapter_atoms();
        assert_eq!(atoms.len(), 2);
        assert_eq!(trim(atoms[0].displays()[0].string()), "Round 1");
        assert_eq!(trim(atoms[1].displays()[0].string()), "Round 2");
        // Five frames of 280896 ticks at 16777216 Hz: 1404480/16777216 s.
        assert_eq!(atoms[0].time_start(), 0);
        assert_eq!(atoms[1].time_start(), 83_713_531);
    }

    #[test]
    fn webm_rejects_h264() {
        let mut config = config(Container::WebM);
        config.audio[0].codec = AudioCodec::Opus;
        assert!(super::super::open(config).is_err());
    }

    #[test]
    fn padding_is_exactly_as_long_as_asked() {
        for total in 2..=2000usize {
            let padding = void_padding(total).unwrap();
            assert_eq!(padding.len(), total, "padding of {total} bytes");
            assert_eq!(padding[0], 0xEC, "starts with a Void element");
        }
        // 129 is the size no single Void element can be, so it takes
        // two.
        assert!(void_element(129).unwrap().is_none());
        assert_eq!(void_padding(129).unwrap().len(), 129);
    }

    #[test]
    fn segment_size_placeholder_and_patch_are_the_same_width() {
        assert_eq!(fixed_width_size(1234).len(), 8);
        assert_eq!(fixed_width_size(0)[0], 0x01);
    }
}

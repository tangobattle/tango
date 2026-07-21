//! A minimal streaming WebM (Matroska) muxer for the replay exporter:
//! one VP8/VP9 video track + one Opus audio track, written incrementally
//! as the WebCodecs encoders emit — memory holds only the current
//! cluster plus a small two-track reorder queue, never the whole file.
//! The Segment is opened with an unknown size and its true size (plus
//! the Info Duration) is patched by seeking back once the stream ends.

use std::collections::VecDeque;

/// Where the muxed bytes go. Sequential `write`s carry the stream;
/// `patch` seeks back over already-written bytes (the caller seeks
/// forward again by continuing to `write`); `close` finalizes.
/// Abstract so the muxer is testable off-browser.
pub trait Sink {
    async fn write(&mut self, bytes: &[u8]) -> anyhow::Result<()>;
    async fn patch(&mut self, position: u64, bytes: &[u8]) -> anyhow::Result<()>;
    async fn close(&mut self) -> anyhow::Result<()>;
}

/// One encoded frame queued for muxing. `timestamp_us` is the encoder's
/// microsecond timestamp; Matroska gets milliseconds (TimestampScale
/// 1_000_000 ns).
pub struct Chunk {
    pub timestamp_us: f64,
    pub key: bool,
    pub data: Vec<u8>,
}

pub struct MuxConfig {
    /// Matroska codec ID, e.g. "V_VP9" / "V_VP8".
    pub video_codec_id: &'static str,
    pub width: u32,
    pub height: u32,
    pub audio_sample_rate: f64,
    pub audio_channels: u8,
}

// ---- EBML primitives ----

/// Element ID bytes are written verbatim (they carry their own length
/// marker); sizes get the shortest VINT that fits.
fn vint_size(mut n: u64) -> Vec<u8> {
    let mut len = 1;
    while len < 8 && n >= (1u64 << (7 * len)) - 1 {
        len += 1;
    }
    let mut out = vec![0u8; len];
    for i in (0..len).rev() {
        out[i] = (n & 0xff) as u8;
        n >>= 8;
    }
    out[0] |= 1 << (8 - len as u32);
    out
}

fn element(id: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(id.len() + 8 + payload.len());
    out.extend_from_slice(id);
    out.extend_from_slice(&vint_size(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

fn uint_payload(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        out.insert(0, (v & 0xff) as u8);
        v >>= 8;
        if v == 0 {
            break;
        }
    }
    out
}

fn uint(id: &[u8], v: u64) -> Vec<u8> {
    element(id, &uint_payload(v))
}

fn float(id: &[u8], v: f64) -> Vec<u8> {
    element(id, &v.to_be_bytes())
}

fn string(id: &[u8], s: &str) -> Vec<u8> {
    element(id, s.as_bytes())
}

/// The always-8-byte VINT form of `n` (must fit 56 bits) — used where a
/// placeholder is patched in place regardless of the final value.
fn vint8(n: u64) -> [u8; 8] {
    debug_assert!(n < (1 << 56));
    let mut out = [0u8; 8];
    out[0] = 0x01; // 8-byte length marker; payload rides in out[1..].
    let mut v = n;
    for i in (1..8).rev() {
        out[i] = (v & 0xff) as u8;
        v >>= 8;
    }
    out
}

/// Split clusters at ~4s so SimpleBlock's i16 relative timestamp (and
/// player expectations) stay comfortable.
const CLUSTER_SPAN_MS: i64 = 4_000;

pub struct StreamingMuxer<S: Sink> {
    sink: S,
    config: MuxConfig,
    /// Bytes written so far (the sink's forward position).
    pos: u64,
    /// Position of the Segment's 8-byte size VINT, patched at finish.
    segment_size_pos: u64,
    /// First byte of the Segment payload — the size base.
    segment_payload_start: u64,
    /// Position of the Duration float's 8-byte payload.
    duration_pos: u64,
    /// Tracks can only be written once the Opus CodecPrivate (the
    /// encoder's OpusHead) is known; queued chunks wait behind it.
    tracks_written: bool,
    video: VecDeque<Chunk>,
    audio: VecDeque<Chunk>,
    cluster: Vec<u8>,
    cluster_start_ms: i64,
    cluster_open: bool,
}

impl<S: Sink> StreamingMuxer<S> {
    /// Write the stream head: EBML header, the unknown-size Segment,
    /// and Info with a Duration placeholder.
    pub async fn begin(mut sink: S, config: MuxConfig) -> anyhow::Result<Self> {
        let ebml = element(
            &[0x1A, 0x45, 0xDF, 0xA3],
            &[
                uint(&[0x42, 0x86], 1),        // EBMLVersion
                uint(&[0x42, 0xF7], 1),        // EBMLReadVersion
                uint(&[0x42, 0xF2], 4),        // EBMLMaxIDLength
                uint(&[0x42, 0xF3], 8),        // EBMLMaxSizeLength
                string(&[0x42, 0x82], "webm"), // DocType
                uint(&[0x42, 0x87], 4),        // DocTypeVersion
                uint(&[0x42, 0x85], 2),        // DocTypeReadVersion
            ]
            .concat(),
        );
        let mut pos = 0u64;
        sink.write(&ebml).await?;
        pos += ebml.len() as u64;

        // Segment, size unknown while streaming (all-ones 8-byte VINT).
        sink.write(&[0x18, 0x53, 0x80, 0x67]).await?;
        pos += 4;
        let segment_size_pos = pos;
        sink.write(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]).await?;
        pos += 8;
        let segment_payload_start = pos;

        // Info. The Duration payload position is derived from the
        // element layout below: it's the last 8 bytes of `info`.
        let info = element(
            &[0x15, 0x49, 0xA9, 0x66],
            &[
                uint(&[0x2A, 0xD7, 0xB1], 1_000_000), // TimestampScale: 1 ms
                string(&[0x4D, 0x80], "tango-web"),   // MuxingApp
                string(&[0x57, 0x41], "tango-web"),   // WritingApp
                float(&[0x44, 0x89], 0.0),            // Duration, patched at finish
            ]
            .concat(),
        );
        sink.write(&info).await?;
        pos += info.len() as u64;
        let duration_pos = pos - 8;

        Ok(Self {
            sink,
            config,
            pos,
            segment_size_pos,
            segment_payload_start,
            duration_pos,
            tracks_written: false,
            video: VecDeque::new(),
            audio: VecDeque::new(),
            cluster: Vec::new(),
            cluster_start_ms: 0,
            cluster_open: false,
        })
    }

    /// Write the Tracks element once the audio encoder has surfaced its
    /// OpusHead (`decoderConfig.description` on the first chunk).
    pub async fn write_tracks(&mut self, opus_codec_private: &[u8]) -> anyhow::Result<()> {
        debug_assert!(!self.tracks_written);
        let video_track = element(
            &[0xAE],
            &[
                uint(&[0xD7], 1),       // TrackNumber
                uint(&[0x73, 0xC5], 1), // TrackUID
                uint(&[0x83], 1),       // TrackType: video
                string(&[0x86], self.config.video_codec_id),
                element(
                    &[0xE0], // Video
                    &[
                        uint(&[0xB0], self.config.width as u64),
                        uint(&[0xBA], self.config.height as u64),
                    ]
                    .concat(),
                ),
            ]
            .concat(),
        );
        // Opus pre-skip (samples at 48 kHz) lives in the OpusHead at
        // bytes 10..12 (LE); Matroska wants it echoed as CodecDelay
        // in ns.
        let pre_skip = opus_codec_private
            .get(10..12)
            .map(|b| u16::from_le_bytes([b[0], b[1]]) as u64)
            .unwrap_or(0);
        let audio_track = element(
            &[0xAE],
            &[
                uint(&[0xD7], 2),
                uint(&[0x73, 0xC5], 2),
                uint(&[0x83], 2), // TrackType: audio
                string(&[0x86], "A_OPUS"),
                uint(&[0x56, 0xAA], pre_skip * 1_000_000_000 / 48_000), // CodecDelay
                uint(&[0x56, 0xBB], 80_000_000),                        // SeekPreRoll
                element(&[0x63, 0xA2], opus_codec_private),             // CodecPrivate
                element(
                    &[0xE1], // Audio
                    &[
                        float(&[0xB5], self.config.audio_sample_rate),
                        uint(&[0x9F], self.config.audio_channels as u64),
                    ]
                    .concat(),
                ),
            ]
            .concat(),
        );
        let tracks = element(&[0x16, 0x54, 0xAE, 0x6B], &[video_track, audio_track].concat());
        self.sink.write(&tracks).await?;
        self.pos += tracks.len() as u64;
        self.tracks_written = true;
        Ok(())
    }

    /// Whether [`Self::write_tracks`] has run — chunks stream only
    /// after it.
    pub fn tracks_written(&self) -> bool {
        self.tracks_written
    }

    pub fn push_video(&mut self, chunk: Chunk) {
        self.video.push_back(chunk);
    }

    pub fn push_audio(&mut self, chunk: Chunk) {
        self.audio.push_back(chunk);
    }

    /// Interleave + flush everything that's safely ordered: while both
    /// queues have chunks, the earlier head is appended (finished
    /// clusters stream out). With `drain` set (the end of the stream),
    /// order is decided against whatever remains.
    pub async fn pump(&mut self, drain: bool) -> anyhow::Result<()> {
        if !self.tracks_written {
            return Ok(());
        }
        loop {
            let track = match (self.video.front(), self.audio.front()) {
                (Some(v), Some(a)) => {
                    if v.timestamp_us <= a.timestamp_us {
                        Track::Video
                    } else {
                        Track::Audio
                    }
                }
                (Some(_), None) if drain => Track::Video,
                (None, Some(_)) if drain => Track::Audio,
                _ => break,
            };
            let chunk = match track {
                Track::Video => self.video.pop_front().unwrap(),
                Track::Audio => self.audio.pop_front().unwrap(),
            };
            self.append_block(track, &chunk).await?;
        }
        Ok(())
    }

    async fn append_block(&mut self, track: Track, chunk: &Chunk) -> anyhow::Result<()> {
        let ts_ms = (chunk.timestamp_us / 1000.0).round() as i64;
        // Split on video keyframes (once the cluster has content) or on
        // span overflow.
        let split = !self.cluster_open
            || (track == Track::Video && chunk.key && ts_ms > self.cluster_start_ms)
            || ts_ms - self.cluster_start_ms > CLUSTER_SPAN_MS;
        if split {
            self.flush_cluster().await?;
            self.cluster_start_ms = ts_ms;
            self.cluster_open = true;
        }
        let mut block = Vec::with_capacity(4 + chunk.data.len());
        block.push(match track {
            Track::Video => 0x81,
            Track::Audio => 0x82,
        });
        let rel = (ts_ms - self.cluster_start_ms).clamp(i16::MIN as i64, i16::MAX as i64) as i16;
        block.extend_from_slice(&rel.to_be_bytes());
        block.push(if chunk.key { 0x80 } else { 0x00 });
        block.extend_from_slice(&chunk.data);
        let simple_block = element(&[0xA3], &block);
        self.cluster.extend_from_slice(&simple_block);
        Ok(())
    }

    async fn flush_cluster(&mut self) -> anyhow::Result<()> {
        if !self.cluster_open || self.cluster.is_empty() {
            self.cluster.clear();
            return Ok(());
        }
        let mut body = uint(&[0xE7], self.cluster_start_ms.max(0) as u64); // Timestamp
        body.extend_from_slice(&self.cluster);
        let cluster = element(&[0x1F, 0x43, 0xB6, 0x75], &body);
        self.sink.write(&cluster).await?;
        self.pos += cluster.len() as u64;
        self.cluster.clear();
        Ok(())
    }

    /// Drain the queues, close the last cluster, patch the Segment size
    /// + Duration, and finalize the sink.
    pub async fn finish(mut self, duration_ms: f64) -> anyhow::Result<()> {
        self.pump(true).await?;
        self.flush_cluster().await?;
        let segment_size = self.pos - self.segment_payload_start;
        self.sink
            .patch(self.segment_size_pos, &vint8(segment_size))
            .await?;
        self.sink
            .patch(self.duration_pos, &duration_ms.to_be_bytes())
            .await?;
        self.sink.close().await?;
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Track {
    Video,
    Audio,
}

//! A minimal WebM (Matroska) muxer for the replay exporter: one VP8/VP9
//! video track + one Opus audio track, whole-file-in-memory (a replay's
//! worth of encoded chunks is tens of MB). Only what the exporter needs
//! — EBML header, Info, Tracks, and timestamp-sorted Clusters of
//! SimpleBlocks — nothing else of the spec.

/// One encoded frame queued for muxing. `timestamp_us` is the encoder's
/// microsecond timestamp; Matroska gets milliseconds (TimestampScale
/// 1_000_000 ns).
pub struct Chunk {
    pub track: Track,
    pub timestamp_us: f64,
    pub key: bool,
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Track {
    Video,
    Audio,
}

pub struct MuxConfig {
    /// Matroska codec ID, e.g. "V_VP9" / "V_VP8".
    pub video_codec_id: &'static str,
    pub width: u32,
    pub height: u32,
    /// The Opus `decoderConfig.description` (an OpusHead blob) captured
    /// off the audio encoder's first chunk metadata; without it most
    /// players refuse the track.
    pub opus_codec_private: Vec<u8>,
    pub audio_sample_rate: f64,
    pub audio_channels: u8,
    /// Total duration in ms, for the header.
    pub duration_ms: f64,
}

// ---- EBML primitives ----

/// Element ID bytes are written verbatim (they carry their own length
/// marker); sizes get the shortest VINT that fits.
fn vint_size(mut n: u64) -> Vec<u8> {
    // Shortest length descriptor: 1 byte carries 7 bits, 2 carry 14, ...
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

// ---- The file ----

/// Assemble the complete `.webm`. `chunks` may arrive in per-track
/// order; they're interleaved by timestamp here.
pub fn mux(config: &MuxConfig, mut chunks: Vec<Chunk>) -> Vec<u8> {
    chunks.sort_by(|a, b| a.timestamp_us.total_cmp(&b.timestamp_us));

    let ebml = element(
        &[0x1A, 0x45, 0xDF, 0xA3],
        &[
            uint(&[0x42, 0x86], 1),               // EBMLVersion
            uint(&[0x42, 0xF7], 1),               // EBMLReadVersion
            uint(&[0x42, 0xF2], 4),               // EBMLMaxIDLength
            uint(&[0x42, 0xF3], 8),               // EBMLMaxSizeLength
            string(&[0x42, 0x82], "webm"),        // DocType
            uint(&[0x42, 0x87], 4),               // DocTypeVersion
            uint(&[0x42, 0x85], 2),               // DocTypeReadVersion
        ]
        .concat(),
    );

    let info = element(
        &[0x15, 0x49, 0xA9, 0x66],
        &[
            uint(&[0x2A, 0xD7, 0xB1], 1_000_000), // TimestampScale: 1 ms
            float(&[0x44, 0x89], config.duration_ms),
            string(&[0x4D, 0x80], "tango-web"), // MuxingApp
            string(&[0x57, 0x41], "tango-web"), // WritingApp
        ]
        .concat(),
    );

    let video_track = element(
        &[0xAE],
        &[
            uint(&[0xD7], 1),         // TrackNumber
            uint(&[0x73, 0xC5], 1),   // TrackUID
            uint(&[0x83], 1),         // TrackType: video
            string(&[0x86], config.video_codec_id),
            element(
                &[0xE0], // Video
                &[
                    uint(&[0xB0], config.width as u64),
                    uint(&[0xBA], config.height as u64),
                ]
                .concat(),
            ),
        ]
        .concat(),
    );
    // Opus pre-skip (samples at 48 kHz) lives in the OpusHead at bytes
    // 10..12 (LE); Matroska wants it echoed as CodecDelay in ns.
    let pre_skip = config
        .opus_codec_private
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
            element(&[0x63, 0xA2], &config.opus_codec_private),     // CodecPrivate
            element(
                &[0xE1], // Audio
                &[
                    float(&[0xB5], config.audio_sample_rate),
                    uint(&[0x9F], config.audio_channels as u64),
                ]
                .concat(),
            ),
        ]
        .concat(),
    );
    let tracks = element(&[0x16, 0x54, 0xAE, 0x6B], &[video_track, audio_track].concat());

    // Clusters: split on video keyframes or ~4s, whichever first, so
    // SimpleBlock's i16 relative timestamp never overflows.
    let mut clusters: Vec<u8> = Vec::new();
    let mut cluster_body: Vec<u8> = Vec::new();
    let mut cluster_start_ms: i64 = 0;
    let mut started = false;
    for chunk in &chunks {
        let ts_ms = (chunk.timestamp_us / 1000.0).round() as i64;
        let split = !started
            || (chunk.track == Track::Video && chunk.key && ts_ms > cluster_start_ms)
            || ts_ms - cluster_start_ms > 4_000;
        if split {
            if started {
                let mut body = uint(&[0xE7], cluster_start_ms as u64); // Timestamp
                body.extend_from_slice(&cluster_body);
                clusters.extend_from_slice(&element(&[0x1F, 0x43, 0xB6, 0x75], &body));
            }
            cluster_body = Vec::new();
            cluster_start_ms = ts_ms;
            started = true;
        }
        // SimpleBlock: track vint, i16 relative timestamp, flags, data.
        let mut block = Vec::with_capacity(4 + chunk.data.len());
        block.push(match chunk.track {
            Track::Video => 0x81,
            Track::Audio => 0x82,
        });
        let rel = (ts_ms - cluster_start_ms).clamp(i16::MIN as i64, i16::MAX as i64) as i16;
        block.extend_from_slice(&rel.to_be_bytes());
        block.push(if chunk.key { 0x80 } else { 0x00 });
        block.extend_from_slice(&chunk.data);
        cluster_body.extend_from_slice(&element(&[0xA3], &block));
    }
    if started {
        let mut body = uint(&[0xE7], cluster_start_ms as u64);
        body.extend_from_slice(&cluster_body);
        clusters.extend_from_slice(&element(&[0x1F, 0x43, 0xB6, 0x75], &body));
    }

    let segment = element(
        &[0x18, 0x53, 0x80, 0x67],
        &[info, tracks, clusters].concat(),
    );

    [ebml, segment].concat()
}

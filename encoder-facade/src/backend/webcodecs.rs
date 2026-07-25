//! The browser encoder backend: WebCodecs `VideoEncoder`/`AudioEncoder`.
//!
//! Bindings are hand-rolled because web-sys keeps WebCodecs behind the
//! `web_sys_unstable_apis` cfg — a global RUSTFLAGS knob every
//! downstream build would have to set — and only a sliver of the API is
//! needed: configure, encode, flush, and the chunk callback.
//!
//! Three differences from the ffmpeg backend shape the code:
//!
//!   * **Scaling happens here.** WebCodecs has no scaler, so frames are
//!     expanded in Rust before they reach the encoder. (ffmpeg does it in
//!     its filtergraph, which is why the [`Backend`] contract has callers
//!     push frames at input size either way.)
//!   * **Flushing is a promise.** `flush()` can't be awaited from a
//!     synchronous call, so [`Backend::begin_flush`] spawns a task per
//!     encoder that marks it done and [`Backend::poll_flush`] reports
//!     whether all of them have — the reason that part of the trait is
//!     two-phase.
//!   * **Timing is counted, not read.** A chunk says nothing about how
//!     long it is, so a video unit is one frame and an audio unit's
//!     length comes out of its own framing. (An ffmpeg child is asked
//!     for a container, which states both.)
//!
//! Codec configuration arrives the same way as on native, just from a
//! different place: the first chunk's `decoderConfig.description` holds
//! the `avcC` record for H.264 and the `AudioSpecificConfig` for AAC.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::settings::AAC_SAMPLES_PER_FRAME;
use crate::{AudioCodec, Backend, Error, H264Quality, Packet, Settings, VideoCodec, VideoSettings};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = VideoEncoder)]
    type VideoEncoder;
    #[wasm_bindgen(constructor, js_class = "VideoEncoder")]
    fn new(init: &js_sys::Object) -> VideoEncoder;
    #[wasm_bindgen(method, catch)]
    fn configure(this: &VideoEncoder, config: &js_sys::Object) -> Result<(), JsValue>;
    #[wasm_bindgen(method, js_name = encode, catch)]
    fn encode_with_options(this: &VideoEncoder, frame: &VideoFrame, options: &js_sys::Object) -> Result<(), JsValue>;
    #[wasm_bindgen(method)]
    fn flush(this: &VideoEncoder) -> js_sys::Promise;
    #[wasm_bindgen(method)]
    fn close(this: &VideoEncoder);
    #[wasm_bindgen(method, getter, js_name = encodeQueueSize)]
    fn encode_queue_size(this: &VideoEncoder) -> u32;

    #[wasm_bindgen(js_name = VideoFrame)]
    type VideoFrame;
    #[wasm_bindgen(constructor, js_class = "VideoFrame", catch)]
    fn new_with_u8_array(data: &js_sys::Uint8Array, init: &js_sys::Object) -> Result<VideoFrame, JsValue>;
    #[wasm_bindgen(method)]
    fn close(this: &VideoFrame);

    #[wasm_bindgen(js_name = AudioEncoder)]
    type AudioEncoder;
    #[wasm_bindgen(constructor, js_class = "AudioEncoder")]
    fn new(init: &js_sys::Object) -> AudioEncoder;
    #[wasm_bindgen(method, catch)]
    fn configure(this: &AudioEncoder, config: &js_sys::Object) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch)]
    fn encode(this: &AudioEncoder, data: &AudioData) -> Result<(), JsValue>;
    #[wasm_bindgen(method)]
    fn flush(this: &AudioEncoder) -> js_sys::Promise;
    #[wasm_bindgen(method)]
    fn close(this: &AudioEncoder);

    #[wasm_bindgen(js_name = AudioData)]
    type AudioData;
    #[wasm_bindgen(constructor, js_class = "AudioData", catch)]
    fn new(init: &js_sys::Object) -> Result<AudioData, JsValue>;
    #[wasm_bindgen(method)]
    fn close(this: &AudioData);

    /// One encoded chunk. Video and audio expose the same surface.
    #[wasm_bindgen(js_name = EncodedVideoChunk)]
    type EncodedChunk;
    #[wasm_bindgen(method, getter, js_name = type)]
    fn type_(this: &EncodedChunk) -> String;
    #[wasm_bindgen(method, getter, js_name = byteLength)]
    fn byte_length(this: &EncodedChunk) -> u32;
    #[wasm_bindgen(method, js_name = copyTo)]
    fn copy_to(this: &EncodedChunk, dest: &js_sys::Uint8Array);
}

/// `{ key: value, ... }` for the config dictionaries.
fn obj(entries: &[(&str, JsValue)]) -> js_sys::Object {
    let object = js_sys::Object::new();
    for (key, value) in entries {
        let _ = js_sys::Reflect::set(&object, &JsValue::from_str(key), value);
    }
    object
}

fn js_error(context: &str, value: JsValue) -> Error {
    Error::WebCodecs(format!("{context}: {value:?}"))
}

/// What a callback fills in as chunks arrive: the encoded units, and the
/// codec configuration the first chunk's metadata carries.
#[derive(Default)]
struct Incoming {
    /// Encoded units in arrival order, without timestamps — those are
    /// assigned from the ordinal, as on native, so they stay exact.
    units: Vec<(bool, Vec<u8>)>,
    codec_private: Option<Vec<u8>>,
}

type Shared = Rc<RefCell<Incoming>>;

struct Stream {
    incoming: Shared,
    /// Kept alive for as long as the encoder can call them.
    _callbacks: Vec<Closure<dyn FnMut(JsValue, JsValue)>>,
    flushed: Rc<RefCell<bool>>,
    next_pts: u64,
    /// How to read a unit's length: video units are one frame each,
    /// audio units state their own sample count.
    length: UnitLength,
}

enum UnitLength {
    /// Ticks per frame, in the video track's timebase.
    Frame(u64),
    /// Samples, read out of each packet.
    Audio(AudioCodec),
}

pub struct WebCodecsBackend {
    video: VideoEncoder,
    audio: Vec<AudioEncoder>,
    streams: Vec<Stream>,
    error: Rc<RefCell<Option<String>>>,
    settings: Settings,
    /// Scratch for the upscaled frame, reused every frame.
    scaled: Vec<u8>,
    frame_index: u64,
    audio_frames_sent: Vec<u64>,
}

impl WebCodecsBackend {
    /// Configure the browser's encoders for `settings`.
    ///
    /// Which codec strings to ask for is worked out from the settings —
    /// the browser needs a fully qualified `avc1.PPCCLL` triplet rather
    /// than a codec name, and the level in it depends on the geometry
    /// being encoded. Callers that want to know whether a browser has
    /// the encoder at all should ask `VideoEncoder.isConfigSupported`
    /// first; a configure the browser rejects surfaces here.
    pub fn new(settings: &Settings) -> crate::Result<Self> {
        settings.validate()?;
        check_encodable(settings)?;
        let error: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let mut streams = Vec::with_capacity(settings.audio_tracks + 1);

        let (video, video_stream) = Self::make_video(settings, &error)?;
        streams.push(video_stream);
        let mut audio = Vec::with_capacity(settings.audio_tracks);
        for _ in 0..settings.audio_tracks {
            let (encoder, stream) = Self::make_audio(settings, &error)?;
            audio.push(encoder);
            streams.push(stream);
        }

        Ok(Self {
            video,
            audio,
            streams,
            error,
            settings: settings.clone(),
            scaled: vec![0u8; (settings.video.output_width() * settings.video.output_height() * 4) as usize],
            frame_index: 0,
            audio_frames_sent: vec![0; settings.audio_tracks],
        })
    }

    fn make_video(settings: &Settings, error: &Rc<RefCell<Option<String>>>) -> crate::Result<(VideoEncoder, Stream)> {
        let incoming: Shared = Rc::new(RefCell::new(Incoming::default()));
        let output = chunk_callback(incoming.clone());
        let failed = error_callback(error.clone());
        let encoder = VideoEncoder::new(&obj(&[
            ("output", output.as_ref().clone()),
            ("error", failed.as_ref().clone()),
        ]));
        let video = &settings.video;
        let bitrate = match video.codec {
            VideoCodec::H264 {
                quality: H264Quality::Bitrate(bits),
            } => bits,
            // WebCodecs has no quality-targeted mode, so a lossless or
            // CRF request becomes a rate that suits a small screen
            // capture.
            VideoCodec::H264 { .. } => 4_000_000,
        };
        encoder
            .configure(&obj(&[
                ("codec", JsValue::from_str(&video_codec_string(video))),
                ("width", JsValue::from_f64(video.output_width() as f64)),
                ("height", JsValue::from_f64(video.output_height() as f64)),
                ("bitrate", JsValue::from_f64(bitrate as f64)),
                (
                    "framerate",
                    JsValue::from_f64(video.timescale as f64 / video.frame_duration as f64),
                ),
                // Length-prefixed samples with the parameter sets out of
                // band, which is the form both muxers store and the
                // `description` the chunk metadata carries. The default
                // is this, but a container's whole sample layout hangs
                // on it, so it's stated.
                ("avc", obj(&[("format", JsValue::from_str("avc"))]).into()),
            ]))
            .map_err(|e| js_error("configuring the video encoder", e))?;
        Ok((
            encoder,
            Stream {
                incoming,
                _callbacks: vec![output, failed],
                flushed: Rc::new(RefCell::new(false)),
                next_pts: 0,
                length: UnitLength::Frame(video.frame_duration),
            },
        ))
    }

    fn make_audio(settings: &Settings, error: &Rc<RefCell<Option<String>>>) -> crate::Result<(AudioEncoder, Stream)> {
        let incoming: Shared = Rc::new(RefCell::new(Incoming::default()));
        let output = chunk_callback(incoming.clone());
        let failed = error_callback(error.clone());
        let encoder = AudioEncoder::new(&obj(&[
            ("output", output.as_ref().clone()),
            ("error", failed.as_ref().clone()),
        ]));
        let audio = &settings.audio;
        let codec = match audio.codec {
            AudioCodec::Aac { .. } => "mp4a.40.2",
            AudioCodec::Flac => "flac",
        };
        let mut config = vec![
            ("codec", JsValue::from_str(codec)),
            ("sampleRate", JsValue::from_f64(audio.sample_rate as f64)),
            ("numberOfChannels", JsValue::from_f64(audio.channels as f64)),
        ];
        // Only the lossy codec takes a rate, and a browser is entitled
        // to reject a `bitrate` on one that doesn't — so FLAC is
        // configured without the key rather than with a zero.
        if let AudioCodec::Aac { bitrate } = audio.codec {
            config.push(("bitrate", JsValue::from_f64(bitrate as f64)));
        }
        encoder
            .configure(&obj(&config))
            .map_err(|e| js_error("configuring the audio encoder", e))?;
        Ok((
            encoder,
            Stream {
                incoming,
                _callbacks: vec![output, failed],
                flushed: Rc::new(RefCell::new(false)),
                next_pts: 0,
                length: UnitLength::Audio(audio.codec),
            },
        ))
    }

    /// Anything an encoder reported through its error callback.
    fn check_error(&self) -> crate::Result<()> {
        if let Some(message) = self.error.borrow_mut().take() {
            return Err(Error::WebCodecs(message));
        }
        Ok(())
    }

    /// Nearest-neighbor upscale into the scratch buffer. Kept in Rust
    /// because WebCodecs won't scale, and nearest-neighbor because
    /// smooth scaling would blur pixel art.
    fn scale_into_scratch(&mut self, rgba: &[u8]) {
        let scale = self.settings.video.scale as usize;
        let (width, height) = (
            self.settings.video.width as usize,
            self.settings.video.height as usize,
        );
        if scale == 1 {
            self.scaled.copy_from_slice(rgba);
            return;
        }
        let out_width = width * scale;
        for y in 0..height {
            for x in 0..width {
                let from = (y * width + x) * 4;
                let pixel = &rgba[from..from + 4];
                for row in 0..scale {
                    let start = ((y * scale + row) * out_width + x * scale) * 4;
                    for column in 0..scale {
                        let at = start + column * 4;
                        self.scaled[at..at + 4].copy_from_slice(pixel);
                    }
                }
            }
        }
    }
}

/// The `output` callback: copy the chunk's bytes out, and pick up the
/// codec configuration the first one's metadata carries.
fn chunk_callback(incoming: Shared) -> Closure<dyn FnMut(JsValue, JsValue)> {
    Closure::new(move |chunk: JsValue, metadata: JsValue| {
        let chunk: EncodedChunk = chunk.unchecked_into();
        let mut data = vec![0u8; chunk.byte_length() as usize];
        let array = js_sys::Uint8Array::new_with_length(data.len() as u32);
        chunk.copy_to(&array);
        array.copy_to(&mut data);
        let keyframe = chunk.type_() == "key";

        let mut incoming = incoming.borrow_mut();
        if incoming.codec_private.is_none() {
            if let Some(description) = decoder_description(&metadata) {
                incoming.codec_private = Some(description);
            }
        }
        incoming.units.push((keyframe, data));
    })
}

/// `metadata.decoderConfig.description`, as bytes.
fn decoder_description(metadata: &JsValue) -> Option<Vec<u8>> {
    let config = js_sys::Reflect::get(metadata, &JsValue::from_str("decoderConfig")).ok()?;
    let description = js_sys::Reflect::get(&config, &JsValue::from_str("description")).ok()?;
    if let Some(buffer) = description.dyn_ref::<js_sys::ArrayBuffer>() {
        return Some(js_sys::Uint8Array::new(buffer).to_vec());
    }
    if description.is_instance_of::<js_sys::Uint8Array>() {
        return Some(js_sys::Uint8Array::new(&description).to_vec());
    }
    None
}

fn error_callback(slot: Rc<RefCell<Option<String>>>) -> Closure<dyn FnMut(JsValue, JsValue)> {
    Closure::new(move |value: JsValue, _unused: JsValue| {
        let message = js_sys::Reflect::get(&value, &JsValue::from_str("message"))
            .ok()
            .and_then(|m| m.as_string())
            .unwrap_or_else(|| format!("{value:?}"));
        *slot.borrow_mut() = Some(message);
    })
}

impl Backend for WebCodecsBackend {
    fn submit_video(&mut self, rgba: &[u8]) -> crate::Result<()> {
        self.check_error()?;
        self.scale_into_scratch(rgba);
        let video = &self.settings.video;
        let array = js_sys::Uint8Array::new_with_length(self.scaled.len() as u32);
        array.copy_from(&self.scaled);
        // WebCodecs timestamps are microseconds; the muxers never see
        // these, since packets are timed from the ordinal instead.
        let timestamp =
            (self.frame_index as f64 * video.frame_duration as f64 * 1_000_000.0) / video.timescale as f64;
        let frame = VideoFrame::new_with_u8_array(
            &array,
            &obj(&[
                ("format", JsValue::from_str("RGBA")),
                ("codedWidth", JsValue::from_f64(video.output_width() as f64)),
                ("codedHeight", JsValue::from_f64(video.output_height() as f64)),
                ("timestamp", JsValue::from_f64(timestamp)),
            ]),
        )
        .map_err(|e| js_error("building a VideoFrame", e))?;
        let keyframe = self.frame_index.is_multiple_of(video.keyframe_interval.max(1) as u64);
        let result = self
            .video
            .encode_with_options(&frame, &obj(&[("keyFrame", JsValue::from_bool(keyframe))]));
        frame.close();
        result.map_err(|e| js_error("encoding a frame", e))?;
        self.frame_index += 1;
        Ok(())
    }

    fn submit_audio(&mut self, track: usize, samples: &[i16]) -> crate::Result<()> {
        self.check_error()?;
        let encoder = self
            .audio
            .get(track)
            .ok_or_else(|| Error::internal(format!("no audio track {track}")))?;
        let channels = self.settings.audio.channels as usize;
        let frames = samples.len() / channels.max(1);
        if frames == 0 {
            return Ok(());
        }
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let array = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
        array.copy_from(&bytes);
        let timestamp = self.audio_frames_sent[track] as f64 * 1_000_000.0 / self.settings.audio.sample_rate as f64;
        let data = AudioData::new(&obj(&[
            ("format", JsValue::from_str("s16")),
            ("sampleRate", JsValue::from_f64(self.settings.audio.sample_rate as f64)),
            ("numberOfFrames", JsValue::from_f64(frames as f64)),
            ("numberOfChannels", JsValue::from_f64(channels as f64)),
            ("timestamp", JsValue::from_f64(timestamp)),
            ("data", array.into()),
        ]))
        .map_err(|e| js_error("building AudioData", e))?;
        let result = encoder.encode(&data);
        data.close();
        result.map_err(|e| js_error("encoding samples", e))?;
        self.audio_frames_sent[track] += frames as u64;
        Ok(())
    }

    fn poll(&mut self) -> crate::Result<Vec<(usize, Packet)>> {
        self.check_error()?;
        let mut out = Vec::new();
        for (track, stream) in self.streams.iter_mut().enumerate() {
            let units = std::mem::take(&mut stream.incoming.borrow_mut().units);
            for (keyframe, data) in units {
                // WebCodecs doesn't report how long a chunk is, so an
                // audio unit's length is read out of the packet itself.
                let duration = match stream.length {
                    UnitLength::Frame(ticks) => ticks,
                    UnitLength::Audio(codec) => audio_packet_samples(codec, &data),
                };
                let pts = stream.next_pts;
                stream.next_pts += duration;
                out.push((
                    track,
                    Packet {
                        pts,
                        duration,
                        keyframe,
                        data,
                    },
                ));
            }
        }
        Ok(out)
    }

    /// WebCodecs accepts frames as fast as they're pushed and buffers
    /// the excess, so this is what stops an export from running ahead
    /// of the browser's encoder and into memory.
    fn queue_depth(&self) -> u32 {
        self.video.encode_queue_size()
    }

    fn codec_private(&self, track: usize) -> Option<Vec<u8>> {
        self.streams.get(track)?.incoming.borrow().codec_private.clone()
    }

    /// Undeclared: neither codec here gives a browser a way to ask what
    /// its encoder primed with, and guessing a delay that isn't there
    /// would shift the audio the wrong way. (The ffmpeg backend knows
    /// its own encoder, so it can state AAC's.)
    fn codec_delay_samples(&self, _track: usize) -> u64 {
        0
    }

    fn begin_flush(&mut self) -> crate::Result<()> {
        // `flush()` resolves on the event loop, which a synchronous call
        // can't wait for: each task marks its own stream done and
        // `poll_flush` reports when all of them have.
        let promises: Vec<js_sys::Promise> = std::iter::once(self.video.flush())
            .chain(self.audio.iter().map(|encoder| encoder.flush()))
            .collect();
        for (promise, stream) in promises.into_iter().zip(self.streams.iter()) {
            let flushed = stream.flushed.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                *flushed.borrow_mut() = true;
            });
        }
        Ok(())
    }

    fn poll_flush(&mut self) -> crate::Result<bool> {
        self.check_error()?;
        let done = self.streams.iter().all(|stream| *stream.flushed.borrow());
        if done {
            self.video.close();
            for encoder in &self.audio {
                encoder.close();
            }
        }
        Ok(done)
    }
}

/// Refuse settings this backend can't honour, rather than quietly
/// producing something else.
///
/// [`crate::Settings::validate`] has already checked what any backend
/// would reject; these are the browser's own limits.
fn check_encodable(settings: &Settings) -> crate::Result<()> {
    let video = &settings.video;
    if matches!(
        video.codec,
        VideoCodec::H264 {
            quality: H264Quality::Lossless
        }
    ) {
        // The native path reaches lossless with an RGB 4:4:4 encoder at
        // QP 0. WebCodecs has no quality-targeted mode at all, so the
        // request can only be met with a lossy rate — which is not the
        // thing that was asked for.
        return Err(Error::invalid(
            "WebCodecs has no lossless H.264 mode; ask for a bitrate instead",
        ));
    }
    // 4:2:0 chroma is subsampled by two in each direction, so an odd
    // dimension has no representation. Browsers refuse it, generally
    // without saying which of the two numbers is the problem.
    let (width, height) = (video.output_width(), video.output_height());
    if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err(Error::invalid(format!(
            "WebCodecs encodes in 4:2:0, which needs even dimensions; \
             {}x{} at {}x scale is {width}x{height}",
            video.width, video.height, video.scale
        )));
    }
    Ok(())
}

/// The WebCodecs codec string for these video settings: the
/// `avc1.PPCCLL` triplet of profile, constraint flags and level that
/// `VideoEncoder.configure` wants, rather than a codec name.
///
/// The level is the lowest one that covers the frame size and rate being
/// asked for. It has to: a level too low for the geometry is a
/// configuration a browser is entitled to reject, and one needlessly
/// high can be refused by a decoder that would otherwise have played the
/// result.
fn video_codec_string(video: &VideoSettings) -> String {
    /// High profile, which every browser with an H.264 encoder has.
    const PROFILE: u8 = 0x64;
    /// `level_idc` with the frame and rate limits from ISO/IEC 14496-10
    /// Table A-1, in macroblocks and macroblocks per second.
    const LEVELS: &[(u8, u64, u64)] = &[
        (0x1E, 1_620, 40_500),        // 3.0
        (0x1F, 3_600, 108_000),       // 3.1
        (0x20, 5_120, 216_000),       // 3.2
        (0x28, 8_192, 245_760),       // 4.0
        (0x2A, 8_704, 522_240),       // 4.2
        (0x32, 22_080, 589_824),      // 5.0
        (0x33, 36_864, 983_040),      // 5.1
        (0x34, 36_864, 2_073_600),    // 5.2
        (0x3C, 139_264, 4_177_920),   // 6.0
        (0x3D, 139_264, 8_355_840),   // 6.1
        (0x3E, 139_264, 16_711_680),  // 6.2
    ];

    let macroblocks = video.output_width().div_ceil(16) as u64 * video.output_height().div_ceil(16) as u64;
    let per_second = macroblocks * video.timescale.max(1) as u64 / video.frame_duration.max(1);
    let level = LEVELS
        .iter()
        .find(|&&(_, max_frame, max_rate)| macroblocks <= max_frame && per_second <= max_rate)
        // Past the last level there is nothing left to ask for; let the
        // browser be the one to say no.
        .unwrap_or(LEVELS.last().expect("the table is not empty"))
        .0;
    format!("avc1.{PROFILE:02x}00{level:02x}")
}

/// Samples in one encoded audio packet.
///
/// WebCodecs hands back chunks without saying how long they are, and
/// the muxers time audio in samples — so each codec's own framing has to
/// answer it. AAC's frames are a fixed length; a FLAC frame states its
/// block size in the high nibble of byte 2 of its header (codes 6 and 7
/// put it after the coded sample number instead, which would mean
/// walking a UTF-8-coded integer, so those fall back — encoders use a
/// fixed 4096-sample block, leaving at most the final short frame
/// slightly long in the container's declared length).
fn audio_packet_samples(codec: AudioCodec, data: &[u8]) -> u64 {
    match codec {
        AudioCodec::Aac { .. } => AAC_SAMPLES_PER_FRAME,
        AudioCodec::Flac => flac_frame_blocksize(data).unwrap_or(FLAC_FALLBACK_BLOCKSIZE),
    }
}

/// The block size FLAC encoders use, and so the best guess for a frame
/// that states its own out of line.
const FLAC_FALLBACK_BLOCKSIZE: u64 = 4096;

/// Block size from a FLAC frame header's high nibble of byte 2, or
/// `None` for the two codes that store it after the frame's coded sample
/// number instead of in the header.
fn flac_frame_blocksize(data: &[u8]) -> Option<u64> {
    match data.get(2)? >> 4 {
        0 | 6 | 7 => None,
        1 => Some(192),
        code @ 2..=5 => Some(576 << (code - 2)),
        code => Some(256 << (code - 8)),
    }
}

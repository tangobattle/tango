//! The browser encoder backend: WebCodecs `VideoEncoder`/`AudioEncoder`.
//!
//! Bindings are hand-rolled because web-sys keeps WebCodecs behind the
//! `web_sys_unstable_apis` cfg — a global RUSTFLAGS knob every
//! downstream build would have to set — and only a sliver of the API is
//! needed: configure, encode, flush, and the chunk callback.
//!
//! Two differences from the ffmpeg backend shape the code:
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
//!
//! Codec configuration arrives the same way as on native, just from a
//! different place: the first chunk's `decoderConfig.description` holds
//! the `avcC` record for H.264, the `OpusHead` for Opus, the
//! `AudioSpecificConfig` for AAC. VP8 and VP9 need none.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::{AudioCodec, Backend, Error, Packet, Settings, VideoCodec, VideoQuality};

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
    /// The video codec string follows WebCodecs' registry: `vp8`,
    /// `vp09.00.10.08`, or an `avc1.*` triplet. Callers that care
    /// whether a codec exists should ask `VideoEncoder.isConfigSupported`
    /// first — a configure that the browser rejects surfaces here.
    pub fn new(settings: &Settings, video_codec_string: &str) -> crate::Result<Self> {
        settings.validate()?;
        let error: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let mut streams = Vec::with_capacity(settings.audio_tracks + 1);

        let (video, video_stream) = Self::make_video(settings, video_codec_string, &error)?;
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

    fn make_video(
        settings: &Settings,
        codec: &str,
        error: &Rc<RefCell<Option<String>>>,
    ) -> crate::Result<(VideoEncoder, Stream)> {
        let incoming: Shared = Rc::new(RefCell::new(Incoming::default()));
        let output = chunk_callback(incoming.clone());
        let failed = error_callback(error.clone());
        let encoder = VideoEncoder::new(&obj(&[
            ("output", output.as_ref().clone()),
            ("error", failed.as_ref().clone()),
        ]));
        let video = &settings.video;
        let bitrate = match video.quality {
            VideoQuality::Bitrate(bits) => bits,
            // WebCodecs has no quality-targeted mode, so a CRF request
            // becomes a rate that suits a small screen capture.
            _ => 4_000_000,
        };
        encoder
            .configure(&obj(&[
                ("codec", JsValue::from_str(codec)),
                ("width", JsValue::from_f64(video.output_width() as f64)),
                ("height", JsValue::from_f64(video.output_height() as f64)),
                ("bitrate", JsValue::from_f64(bitrate as f64)),
                (
                    "framerate",
                    JsValue::from_f64(video.timescale as f64 / video.frame_duration as f64),
                ),
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
            AudioCodec::Opus => "opus",
            AudioCodec::Aac => "mp4a.40.2",
            AudioCodec::Flac => "flac",
            AudioCodec::PcmS16Le => {
                return Err(Error::invalid("PCM audio needs no encoder; don't ask WebCodecs for one"))
            }
        };
        encoder
            .configure(&obj(&[
                ("codec", JsValue::from_str(codec)),
                ("sampleRate", JsValue::from_f64(audio.sample_rate as f64)),
                ("numberOfChannels", JsValue::from_f64(audio.channels as f64)),
                ("bitrate", JsValue::from_f64(audio.bitrate as f64)),
            ]))
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

    /// How deep the video encoder's queue is, for callers pacing their
    /// own loop: WebCodecs accepts frames as fast as they're pushed and
    /// buffers the excess, so a cooperative export should stall while
    /// this is large rather than run ahead into memory.
    pub fn video_queue_depth(&self) -> u32 {
        self.video.encode_queue_size()
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
                    UnitLength::Audio(codec) => crate::codec::audio_packet_samples(codec, &data)
                        .unwrap_or(crate::codec::AAC_SAMPLES_PER_FRAME),
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

    fn codec_private(&self, track: usize) -> Option<Vec<u8>> {
        let stream = self.streams.get(track)?;
        if track == crate::VIDEO_TRACK && matches!(self.settings.video.codec, VideoCodec::Vp8 | VideoCodec::Vp9) {
            // VP8 and VP9 carry their configuration in the bitstream.
            return Some(Vec::new());
        }
        stream.incoming.borrow().codec_private.clone()
    }

    fn codec_delay_samples(&self, track: usize) -> u64 {
        if track == crate::VIDEO_TRACK {
            return 0;
        }
        // Opus states its priming in the OpusHead; the other codecs give
        // a browser no way to ask, so they go undeclared.
        let private = self.streams.get(track).and_then(|s| s.incoming.borrow().codec_private.clone());
        match (self.settings.audio.codec, private) {
            (AudioCodec::Opus, Some(head)) if head.len() >= 12 => {
                u16::from_le_bytes([head[10], head[11]]) as u64
            }
            _ => 0,
        }
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

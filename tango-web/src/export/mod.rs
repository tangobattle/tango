//! In-browser replay → video export: the desktop's render pipeline
//! rebuilt on WebCodecs. The same playback pair the viewer runs is
//! stepped headless and faster than realtime (yielding to the event
//! loop every few frames so the UI stays live), each frame's BGR555
//! framebuffer expanded + integer-upscaled into a `VideoFrame`, the
//! cores' native-rate audio resampled to 48 kHz `AudioData`, both fed
//! through `VideoEncoder` / `AudioEncoder` (VP9, falling back to VP8;
//! Opus), and the chunks **streamed** through the WebM muxer as they
//! arrive — memory never holds more than the current cluster. The
//! stream lands in the user's own file via `showSaveFilePicker` where
//! the browser has it, else in an OPFS temp handed to the downloader.
//! No lossless mode — WebCodecs doesn't offer one — so the desktop's
//! lossless render stays desktop-only.

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{FileSystemFileHandle, FileSystemGetFileOptions};

use crate::platform::video::{SCREEN_HEIGHT, SCREEN_WIDTH};

mod sink;
mod webcodecs;
mod webm;

pub use sink::{pick_save_file, save_picker_available};

use sink::FileSink;
use webcodecs::obj;

/// Nearest-neighbor integer upscale baked into the encoded frames, so
/// players that smooth-scale don't blur the pixel art (the desktop
/// exporter scales for the same reason).
const SCALE: usize = 3;
const OUT_W: usize = SCREEN_WIDTH * SCALE;
const OUT_H: usize = SCREEN_HEIGHT * SCALE;

/// One GBA frame in microseconds (280896 cycles at 2^24 Hz) — the
/// exact tick the audio production also follows, so A/V stay aligned.
const FRAME_US: f64 = 280_896.0 * 1_000_000.0 / 16_777_216.0;

const OPUS_RATE: f64 = 48_000.0;
const VIDEO_BITRATE: f64 = 4_000_000.0;
const AUDIO_BITRATE: f64 = 128_000.0;
/// Request a keyframe every ~2s: bounds cluster spans + seek granularity.
const KEYFRAME_INTERVAL: usize = 120;

/// The OPFS temp the fallback path streams into (outside the scanned
/// subdirectories).
const TEMP_FILE: &str = "export-tmp.webm";

/// Where the export streams to.
pub enum ExportTarget {
    /// The user's own file, picked up front (Chromium): the video
    /// never materializes in memory or OPFS at all.
    Picked(FileSystemFileHandle),
    /// Browsers without the picker: an OPFS temp, handed to the
    /// downloader once complete, then deleted.
    OpfsTemp(crate::storage::Storage),
}

/// Live progress of the running export, for the Replays tab's status
/// line. `None` = no export running.
#[derive(Clone, Copy, PartialEq)]
pub struct Progress {
    pub frame: usize,
    pub total: usize,
}

pub static EXPORT_PROGRESS: GlobalSignal<Option<Progress>> = Signal::global(|| None);
/// Set by the UI's cancel button; the export loop checks it at every
/// yield point.
pub static EXPORT_CANCEL: GlobalSignal<bool> = Signal::global(|| false);

/// Render `replay` into `target` as a WebM. Runs on the main thread in
/// cooperative slices; returns once the file is finalized (and, for the
/// OPFS-temp path, the download kicked off). With `range` set, only
/// the `[start, end)` tick span is encoded (the clip strip's export) —
/// the run to `start` fast-skips under frameskip.
pub async fn export_replay(
    replay: tango_pvp::replay::Replay,
    local_rom: Vec<u8>,
    remote_rom: Vec<u8>,
    file_stem: String,
    target: ExportTarget,
    range: Option<(u32, u32)>,
) -> anyhow::Result<()> {
    // Pick the codec: VP9 where supported, else VP8 (both mux into the
    // same WebM); neither → no WebCodecs worth using in this browser.
    let (codec, codec_id) = if webcodecs::video_codec_supported("vp09.00.10.08", OUT_W as u32, OUT_H as u32).await {
        ("vp09.00.10.08", "V_VP9")
    } else if webcodecs::video_codec_supported("vp8", OUT_W as u32, OUT_H as u32).await {
        ("vp8", "V_VP8")
    } else {
        anyhow::bail!("this browser has no WebCodecs VP8/VP9 encoder");
    };

    let stream_len = replay.inputs.len();
    let (start, end) = match range {
        Some((s, e)) => (
            (s as usize).min(stream_len),
            (e as usize).clamp(s as usize, stream_len),
        ),
        None => (0, stream_len),
    };
    let total = end - start;
    if total == 0 {
        anyhow::bail!("empty export range");
    }
    *EXPORT_CANCEL.write() = false;
    *EXPORT_PROGRESS.write() = Some(Progress { frame: 0, total });
    // Everything below must clear the progress line on every exit path.
    let result = run(
        replay, local_rom, remote_rom, &file_stem, &target, codec, codec_id, start, end,
    )
    .await;
    *EXPORT_PROGRESS.write() = None;
    if result.is_err() {
        // Best-effort: don't leave a half-streamed temp behind.
        if let ExportTarget::OpfsTemp(storage) = &target {
            let _ = crate::storage::delete(storage.root(), TEMP_FILE).await;
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn run(
    replay: tango_pvp::replay::Replay,
    local_rom: Vec<u8>,
    remote_rom: Vec<u8>,
    file_stem: &str,
    target: &ExportTarget,
    codec: &str,
    codec_id: &'static str,
    start: usize,
    end: usize,
) -> anyhow::Result<()> {
    let total = end - start;
    // ---- the sink + muxer ----
    let handle = match target {
        ExportTarget::Picked(handle) => handle.clone(),
        ExportTarget::OpfsTemp(storage) => {
            let opts = FileSystemGetFileOptions::new();
            opts.set_create(true);
            JsFuture::from(storage.root().get_file_handle_with_options(TEMP_FILE, &opts))
                .await
                .map_err(|e| anyhow::anyhow!("temp file: {e:?}"))?
                .dyn_into()
                .map_err(|_| anyhow::anyhow!("expected a file handle"))?
        }
    };
    let sink = FileSink::open(&handle).await?;
    let mut muxer = webm::StreamingMuxer::begin(
        sink,
        webm::MuxConfig {
            video_codec_id: codec_id,
            width: OUT_W as u32,
            height: OUT_H as u32,
            audio_sample_rate: OPUS_RATE,
            audio_channels: 2,
        },
    )
    .await?;

    // ---- the encoders ----
    // Callbacks are synchronous: they only copy the chunk bytes into
    // these queues; the driver loop moves them into the muxer and does
    // the (async) sink writes with no RefCell borrow held.
    let video_q: Rc<RefCell<Vec<webm::Chunk>>> = Rc::new(RefCell::new(Vec::new()));
    let audio_q: Rc<RefCell<Vec<webm::Chunk>>> = Rc::new(RefCell::new(Vec::new()));
    let error: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let opus_private: Rc<RefCell<Option<Vec<u8>>>> = Rc::new(RefCell::new(None));

    let read_chunk = |chunk: JsValue| -> webm::Chunk {
        let chunk: webcodecs::EncodedChunk = chunk.unchecked_into();
        let mut data = vec![0u8; chunk.byte_length() as usize];
        let array = js_sys::Uint8Array::new_with_length(data.len() as u32);
        chunk.copy_to(&array);
        array.copy_to(&mut data);
        webm::Chunk {
            timestamp_us: chunk.timestamp(),
            key: chunk.type_() == "key",
            data,
        }
    };

    let video_output = {
        let video_q = video_q.clone();
        Closure::<dyn FnMut(JsValue, JsValue)>::new(move |chunk: JsValue, _meta: JsValue| {
            video_q.borrow_mut().push(read_chunk(chunk));
        })
    };
    let audio_output = {
        let audio_q = audio_q.clone();
        let opus_private = opus_private.clone();
        Closure::<dyn FnMut(JsValue, JsValue)>::new(move |chunk: JsValue, meta: JsValue| {
            // The first chunk's metadata carries the OpusHead the WebM
            // needs as CodecPrivate.
            if opus_private.borrow().is_none() {
                if let Ok(desc) = js_sys::Reflect::get(&meta, &JsValue::from_str("decoderConfig"))
                    .and_then(|dc| js_sys::Reflect::get(&dc, &JsValue::from_str("description")))
                {
                    let bytes = if let Some(buf) = desc.dyn_ref::<js_sys::ArrayBuffer>() {
                        Some(js_sys::Uint8Array::new(buf).to_vec())
                    } else if desc.is_instance_of::<js_sys::Uint8Array>() {
                        Some(js_sys::Uint8Array::new(&desc).to_vec())
                    } else {
                        None
                    };
                    if let Some(bytes) = bytes {
                        *opus_private.borrow_mut() = Some(bytes);
                    }
                }
            }
            audio_q.borrow_mut().push(read_chunk(chunk));
        })
    };
    let make_error_cb = |slot: Rc<RefCell<Option<String>>>| {
        Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
            let msg = js_sys::Reflect::get(&e, &JsValue::from_str("message"))
                .ok()
                .and_then(|m| m.as_string())
                .unwrap_or_else(|| "encoder error".to_string());
            *slot.borrow_mut() = Some(msg);
        })
    };
    let video_error = make_error_cb(error.clone());
    let audio_error = make_error_cb(error.clone());

    let video_encoder = webcodecs::VideoEncoder::new(&obj(&[
        ("output", video_output.as_ref().clone()),
        ("error", video_error.as_ref().clone()),
    ]));
    video_encoder.configure(&obj(&[
        ("codec", JsValue::from_str(codec)),
        ("width", JsValue::from_f64(OUT_W as f64)),
        ("height", JsValue::from_f64(OUT_H as f64)),
        ("bitrate", JsValue::from_f64(VIDEO_BITRATE)),
        ("framerate", JsValue::from_f64(1_000_000.0 / FRAME_US)),
    ]));
    let audio_encoder = webcodecs::AudioEncoder::new(&obj(&[
        ("output", audio_output.as_ref().clone()),
        ("error", audio_error.as_ref().clone()),
    ]));
    audio_encoder.configure(&obj(&[
        ("codec", JsValue::from_str("opus")),
        ("sampleRate", JsValue::from_f64(OPUS_RATE)),
        ("numberOfChannels", JsValue::from_f64(2.0)),
        ("bitrate", JsValue::from_f64(AUDIO_BITRATE)),
    ]));

    // ---- the pair ----
    // The same boot the viewer uses, minus an audio sink or canvas.
    let (mut playback, local_player, _inputs) = crate::session::replay::boot(&replay, local_rom, remote_rom, false)?;

    // Clip export: fast-skip to the range start under frameskip, then
    // drop the skip-produced audio so the clip opens clean.
    if start > 0 {
        playback.pair_mut().set_frameskip(0, i32::MAX);
        playback.pair_mut().set_frameskip(1, i32::MAX);
        while (playback.cursor() as usize) < start {
            if *EXPORT_CANCEL.peek() {
                video_encoder.close();
                audio_encoder.close();
                anyhow::bail!("cancelled");
            }
            if !playback.step() {
                break;
            }
            if playback.cursor() % 600 == 0 {
                // The skip generates audio nobody wants — cap the
                // buffers' growth while yielding to the UI.
                for i in 0..2 {
                    playback.pair_mut().core_mut(i).audio_buffer().clear();
                }
                gloo_timers::future::TimeoutFuture::new(0).await;
            }
        }
        playback.pair_mut().set_frameskip(0, 0);
        playback.pair_mut().set_frameskip(1, 0);
        for i in 0..2 {
            playback.pair_mut().core_mut(i).audio_buffer().clear();
        }
    }

    // Audio replumbing: native-rate core output → 48 kHz s16 via the
    // mgba resampler (the realtime LinkStream's servo/faux-clock logic
    // doesn't apply — export wants every sample, 1:1).
    let mut resampler = mgba::audio::AudioResampler::new();
    let dest_capacity = OPUS_RATE as usize;
    let mut dest_buffer = mgba::audio::OwnedAudioBuffer::new(dest_capacity, 2);
    let mut pending_audio: Vec<i16> = Vec::new();
    let mut audio_samples_sent: u64 = 0;

    let mut rgba = vec![0u8; OUT_W * OUT_H * 4];
    let mut frame_idx: usize = 0;

    // Move callback-queued chunks into the muxer and stream what's
    // ordered. Sync borrows drop before any await.
    macro_rules! pump_muxer {
        ($drain:expr) => {{
            if !muxer.tracks_written() {
                let head = opus_private.borrow().clone();
                if let Some(head) = head {
                    muxer.write_tracks(&head).await?;
                }
            }
            for c in video_q.borrow_mut().drain(..) {
                muxer.push_video(c);
            }
            for c in audio_q.borrow_mut().drain(..) {
                muxer.push_audio(c);
            }
            muxer.pump($drain).await?;
        }};
    }

    loop {
        if *EXPORT_CANCEL.peek() {
            video_encoder.close();
            audio_encoder.close();
            anyhow::bail!("cancelled");
        }
        if let Some(msg) = error.borrow_mut().take() {
            anyhow::bail!("encoder: {msg}");
        }

        if playback.cursor() as usize >= end || !playback.step() {
            break;
        }

        let link = playback.pair_mut();

        // ---- video ----
        if let Some(vbuf) = link.video_buffer(local_player) {
            expand_and_scale(vbuf, &mut rgba);
            let array = js_sys::Uint8Array::new_with_length(rgba.len() as u32);
            array.copy_from(&rgba);
            let ts = frame_idx as f64 * FRAME_US;
            let frame = webcodecs::VideoFrame::new_with_u8_array(
                &array,
                &obj(&[
                    ("format", JsValue::from_str("RGBA")),
                    ("codedWidth", JsValue::from_f64(OUT_W as f64)),
                    ("codedHeight", JsValue::from_f64(OUT_H as f64)),
                    ("timestamp", JsValue::from_f64(ts)),
                ]),
            );
            let key = frame_idx % KEYFRAME_INTERVAL == 0;
            video_encoder.encode_with_options(&frame, &obj(&[("keyFrame", JsValue::from_bool(key))]));
            frame.close();
        }

        // ---- audio ----
        {
            let rate = link.core(local_player).audio_sample_rate() as f64;
            let core = link.core_mut(local_player);
            let mut source = core.audio_buffer();
            resampler.set_source(&mut source, rate, true);
            resampler.set_destination(&mut dest_buffer, OPUS_RATE);
            resampler.process();
            let available = dest_buffer.available().min(dest_capacity);
            if available > 0 {
                let start = pending_audio.len();
                pending_audio.resize(start + available * 2, 0);
                dest_buffer.read(&mut pending_audio[start..], available);
            }
        }
        // Feed ~100ms batches; the encoder does its own Opus framing.
        if pending_audio.len() >= (OPUS_RATE as usize / 10) * 2 {
            flush_audio(&audio_encoder, &mut pending_audio, &mut audio_samples_sent);
        }

        frame_idx += 1;
        if frame_idx % 30 == 0 {
            *EXPORT_PROGRESS.write() = Some(Progress {
                frame: frame_idx,
                total,
            });
            // Stream out what the encoders have produced, then yield so
            // the UI paints; stall while the encoder queues run deep so
            // unencoded frames don't pile up in memory.
            pump_muxer!(false);
            gloo_timers::future::TimeoutFuture::new(0).await;
            while video_encoder.encode_queue_size() > 60 || audio_encoder.audio_encode_queue_size() > 60 {
                if *EXPORT_CANCEL.peek() || error.borrow().is_some() {
                    break;
                }
                gloo_timers::future::TimeoutFuture::new(10).await;
                pump_muxer!(false);
            }
        }
    }

    if !pending_audio.is_empty() {
        flush_audio(&audio_encoder, &mut pending_audio, &mut audio_samples_sent);
    }
    let _ = JsFuture::from(video_encoder.flush()).await;
    let _ = JsFuture::from(audio_encoder.flush()).await;
    video_encoder.close();
    audio_encoder.close();
    if let Some(msg) = error.borrow_mut().take() {
        anyhow::bail!("encoder: {msg}");
    }
    // The callbacks are done for good once the encoders are closed.
    pump_muxer!(true);
    drop((video_output, audio_output, video_error, audio_error));

    muxer.finish(frame_idx as f64 * FRAME_US / 1000.0).await?;

    // The picker path streamed straight into the user's file; the OPFS
    // temp still needs handing to the downloader.
    if let ExportTarget::OpfsTemp(storage) = target {
        let bytes = crate::storage::read(storage.root(), TEMP_FILE)
            .await
            .map_err(|e| anyhow::anyhow!("read temp: {e}"))?
            .ok_or_else(|| anyhow::anyhow!("export temp disappeared"))?;
        crate::web::download_bytes(&format!("{file_stem}.webm"), &bytes);
        let _ = crate::storage::delete(storage.root(), TEMP_FILE).await;
    }
    Ok(())
}

/// Send the accumulated interleaved s16 stereo samples as one
/// `AudioData`, timestamped by the running sample count.
fn flush_audio(encoder: &webcodecs::AudioEncoder, pending: &mut Vec<i16>, samples_sent: &mut u64) {
    let frames = pending.len() / 2;
    if frames == 0 {
        return;
    }
    let bytes: &[u8] = bytemuck::cast_slice(pending.as_slice());
    let array = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
    array.copy_from(bytes);
    let ts = *samples_sent as f64 * 1_000_000.0 / OPUS_RATE;
    let data = webcodecs::AudioData::new(&obj(&[
        ("format", JsValue::from_str("s16")),
        ("sampleRate", JsValue::from_f64(OPUS_RATE)),
        ("numberOfFrames", JsValue::from_f64(frames as f64)),
        ("numberOfChannels", JsValue::from_f64(2.0)),
        ("timestamp", JsValue::from_f64(ts)),
        ("data", array.into()),
    ]));
    encoder.encode(&data);
    data.close();
    *samples_sent += frames as u64;
    pending.clear();
}

/// mGBA's little-endian BGR555 → RGBA8, nearest-neighbor upscaled by
/// [`SCALE`]. 5-bit channels expand as `(c << 3) | (c >> 2)` so white
/// maps to 255.
fn expand_and_scale(vbuf: &[u8], out: &mut [u8]) {
    for y in 0..SCREEN_HEIGHT {
        for x in 0..SCREEN_WIDTH {
            let i = (y * SCREEN_WIDTH + x) * 2;
            let v = u16::from_le_bytes([vbuf[i], vbuf[i + 1]]);
            let r = (v & 0x1f) as u8;
            let g = ((v >> 5) & 0x1f) as u8;
            let b = ((v >> 10) & 0x1f) as u8;
            let px = [(r << 3) | (r >> 2), (g << 3) | (g >> 2), (b << 3) | (b >> 2), 0xff];
            for sy in 0..SCALE {
                let row = ((y * SCALE + sy) * OUT_W + x * SCALE) * 4;
                for sx in 0..SCALE {
                    let o = row + sx * 4;
                    out[o..o + 4].copy_from_slice(&px);
                }
            }
        }
    }
}

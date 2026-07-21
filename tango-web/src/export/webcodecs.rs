//! Hand-rolled WebCodecs bindings. web-sys gates its WebCodecs API
//! behind the `web_sys_unstable_apis` cfg (a global RUSTFLAGS knob we'd
//! rather not thread through dx + CI), and the exporter only needs a
//! sliver of the surface: configure/encode/flush on the two encoders,
//! frame/data construction from raw buffers, and the chunk readback.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = VideoEncoder)]
    pub type VideoEncoder;
    #[wasm_bindgen(constructor, js_class = "VideoEncoder")]
    pub fn new(init: &js_sys::Object) -> VideoEncoder;
    #[wasm_bindgen(method)]
    pub fn configure(this: &VideoEncoder, config: &js_sys::Object);
    #[wasm_bindgen(method, js_name = encode)]
    pub fn encode_with_options(this: &VideoEncoder, frame: &VideoFrame, options: &js_sys::Object);
    #[wasm_bindgen(method)]
    pub fn flush(this: &VideoEncoder) -> js_sys::Promise;
    #[wasm_bindgen(method)]
    pub fn close(this: &VideoEncoder);
    #[wasm_bindgen(method, getter, js_name = encodeQueueSize)]
    pub fn encode_queue_size(this: &VideoEncoder) -> u32;
    #[wasm_bindgen(static_method_of = VideoEncoder, js_name = isConfigSupported)]
    pub fn is_config_supported(config: &js_sys::Object) -> js_sys::Promise;

    #[wasm_bindgen(js_name = VideoFrame)]
    pub type VideoFrame;
    #[wasm_bindgen(constructor, js_class = "VideoFrame")]
    pub fn new_with_u8_array(data: &js_sys::Uint8Array, init: &js_sys::Object) -> VideoFrame;
    #[wasm_bindgen(method)]
    pub fn close(this: &VideoFrame);

    #[wasm_bindgen(js_name = AudioEncoder)]
    pub type AudioEncoder;
    #[wasm_bindgen(constructor, js_class = "AudioEncoder")]
    pub fn new(init: &js_sys::Object) -> AudioEncoder;
    #[wasm_bindgen(method)]
    pub fn configure(this: &AudioEncoder, config: &js_sys::Object);
    #[wasm_bindgen(method)]
    pub fn encode(this: &AudioEncoder, data: &AudioData);
    #[wasm_bindgen(method)]
    pub fn flush(this: &AudioEncoder) -> js_sys::Promise;
    #[wasm_bindgen(method)]
    pub fn close(this: &AudioEncoder);
    #[wasm_bindgen(method, getter, js_name = encodeQueueSize)]
    pub fn audio_encode_queue_size(this: &AudioEncoder) -> u32;

    #[wasm_bindgen(js_name = AudioData)]
    pub type AudioData;
    #[wasm_bindgen(constructor, js_class = "AudioData")]
    pub fn new(init: &js_sys::Object) -> AudioData;
    #[wasm_bindgen(method)]
    pub fn close(this: &AudioData);

    /// One encoded chunk, video and audio alike — both expose the same
    /// `type` / `timestamp` / `byteLength` / `copyTo` surface.
    #[wasm_bindgen(js_name = EncodedVideoChunk)]
    pub type EncodedChunk;
    #[wasm_bindgen(method, getter, js_name = type)]
    pub fn type_(this: &EncodedChunk) -> String;
    #[wasm_bindgen(method, getter)]
    pub fn timestamp(this: &EncodedChunk) -> f64;
    #[wasm_bindgen(method, getter, js_name = byteLength)]
    pub fn byte_length(this: &EncodedChunk) -> u32;
    #[wasm_bindgen(method, js_name = copyTo)]
    pub fn copy_to(this: &EncodedChunk, dest: &js_sys::Uint8Array);
}

/// `{ key: value, ... }` object builder for the various config dicts.
pub fn obj(entries: &[(&str, JsValue)]) -> js_sys::Object {
    let o = js_sys::Object::new();
    for (k, v) in entries {
        let _ = js_sys::Reflect::set(&o, &JsValue::from_str(k), v);
    }
    o
}

/// Whether `VideoEncoder.isConfigSupported` reports `codec` usable at
/// `width`×`height`. `false` on any error (including WebCodecs being
/// absent entirely).
pub async fn video_codec_supported(codec: &str, width: u32, height: u32) -> bool {
    if js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("VideoEncoder"))
        .map(|v| v.is_undefined())
        .unwrap_or(true)
    {
        return false;
    }
    let config = obj(&[
        ("codec", JsValue::from_str(codec)),
        ("width", JsValue::from_f64(width as f64)),
        ("height", JsValue::from_f64(height as f64)),
    ]);
    let Ok(result) = wasm_bindgen_futures::JsFuture::from(VideoEncoder::is_config_supported(&config)).await else {
        return false;
    };
    js_sys::Reflect::get(&result, &JsValue::from_str("supported"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

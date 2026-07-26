//! Sound out: one `AudioWorkletNode` for the page, fed by a push pump.
//!
//! A session hands its host a [`CoreStream`] — the same dynamic-rate-control
//! stream cpal plays on the desktop, which resamples a live core's queue
//! to the device rate and servos the queue level against the pace the
//! drive loop publishes. All this module does is get its samples to the
//! speakers.
//!
//! The worklet processor runs on the audio rendering thread and cannot
//! reach this wasm module, so the flow inverts relative to a native
//! callback backend: instead of the device pulling, [`Sink::pump`]
//! estimates how far below the latency target the worklet's ring has
//! fallen, pulls exactly that many frames out of the stream, and posts
//! them over. The worklet reports its depth back roughly every 10ms,
//! and that report is also a tick source for [`crate::engine`] — it
//! keeps arriving when the tab is hidden and `requestAnimationFrame`
//! stops, which is what lets a netplay match survive being
//! backgrounded.
//!
//! One context and one node for the page, not one per session: creating
//! an `AudioContext` costs a user gesture to un-suspend and iOS caps how
//! many a page may have. Sessions come and go by swapping which stream
//! `pump` is reading and flushing the ring in between.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use tango_session::audio::{CoreStream, Stream as _, NUM_CHANNELS};

/// The processor. Registered from a blob URL rather than a file so the
/// build stays "one wasm, one js, one html" with no path config — a
/// worklet module has to come from a URL, but nothing says it has to be
/// a served file.
const WORKLET_JS: &str = include_str!("../assets/audio-worklet.js");
const PROCESSOR_NAME: &str = "tango-lite-sink";

/// Fixed output rate. Pinning it means the session's resampler is built
/// against a rate that can't change under it when the OS swaps the
/// default device (bluetooth earbuds connecting mid-match).
const SAMPLE_RATE: u32 = 48_000;

/// Steady-state ring depth, in frames: ~64ms at 48kHz. Enough to absorb
/// a dropped animation frame plus a catch-up tick burst plus message
/// jitter. The stream's own 50ms queue sits upstream of this; the two
/// add up to the total latency.
const TARGET_QUEUED_FRAMES: u32 = 3072;

/// Don't post a chunk shorter than one render quantum — the message
/// overhead would outweigh the frames.
const MIN_CHUNK_FRAMES: u32 = 128;

/// Silence pushed when a session starts. The deficit pump alone never
/// builds a cushion: in steady state the stream produces exactly what
/// the device consumes, so the ring's floor would sit whereever the
/// first pump left it, and every scheduling hiccup would be audible.
const PRIME_FRAMES: usize = 2048;

/// 50ms of 8-bit mono silence as a WAV data URI — the pre-16.4 iOS
/// workaround below needs real, unmuted media to play.
const SILENT_WAV: &str = "data:audio/wav;base64,UklGRrQBAABXQVZFZm10IBAAAAABAAEAQB8AAEAfAAABAAgAZGF0YZABAACAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICA";

thread_local! {
    static SINK: RefCell<Option<Rc<RefCell<Sink>>>> = const { RefCell::new(None) };
}

/// The rate sessions must resample to.
pub fn sample_rate() -> u32 {
    SAMPLE_RATE
}

/// The page's sink, building it on first call.
///
/// Call it from a user gesture: a context created outside one starts
/// suspended on every mobile browser, and the worklet module load wants
/// to have already happened by the time a match starts.
pub async fn sink() -> Option<Rc<RefCell<Sink>>> {
    if let Some(sink) = SINK.with(|s| s.borrow().clone()) {
        sink.borrow().resume_if_suspended();
        return Some(sink);
    }
    match Sink::create().await {
        Ok(sink) => {
            let sink = Rc::new(RefCell::new(sink));
            SINK.with(|s| *s.borrow_mut() = Some(sink.clone()));
            Some(sink)
        }
        Err(e) => {
            log::warn!("audio: sink unavailable, playing silently: {e:?}");
            None
        }
    }
}

/// iOS's ringer switch mutes pages in the default "ambient" audio
/// session category. Games want "playback" — the category native games
/// and video use, which the switch doesn't touch. Where the Audio
/// Session API exists (iOS 16.4+) that is one assignment, done through
/// `Reflect` because web-sys still gates `AudioSession` behind its
/// unstable-APIs cfg. Returns whether the API was there at all.
fn claim_playback_audio_session() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let navigator: JsValue = window.navigator().into();
    let Ok(session) = js_sys::Reflect::get(&navigator, &"audioSession".into()) else {
        return false;
    };
    if session.is_undefined() || session.is_null() {
        return false;
    }
    js_sys::Reflect::set(&session, &"type".into(), &"playback".into()).is_ok()
}

/// Pre-16.4 iOS fallback: an unmuted, genuinely-playing, looping media
/// element flips the page into the playback category, and Web Audio
/// rides along. Has to start inside the user gesture that built the
/// sink, which is why it happens here rather than lazily.
fn start_silent_loop() -> Option<web_sys::HtmlAudioElement> {
    let el = web_sys::HtmlAudioElement::new_with_src(SILENT_WAV).ok()?;
    el.set_loop(true);
    let _ = el.play();
    Some(el)
}

fn is_ios() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let ua = window.navigator().user_agent().unwrap_or_default();
    // iPadOS 13+ reports itself as a Mac; the touch-point count is what
    // still tells them apart.
    ua.contains("iPhone")
        || ua.contains("iPad")
        || ua.contains("iPod")
        || (ua.contains("Macintosh") && window.navigator().max_touch_points() > 1)
}

pub struct Sink {
    ctx: web_sys::AudioContext,
    node: web_sys::AudioWorkletNode,
    /// Frames in the worklet's ring as of its last report.
    reported: Rc<Cell<u32>>,
    /// Frames posted since that report — the other half of the estimate.
    /// Zeroed by the report handler, since a report already accounts for
    /// everything sent before it.
    sent_since_report: Rc<Cell<u32>>,
    scratch: Vec<[i16; NUM_CHANNELS]>,
    /// The pre-16.4 iOS ringer workaround, kept playing for the page's
    /// lifetime (see [`start_silent_loop`]).
    silent_loop: Option<web_sys::HtmlAudioElement>,
    /// Keeps the depth-report closure alive for the node's lifetime.
    _onmessage: Closure<dyn FnMut(web_sys::MessageEvent)>,
}

impl Sink {
    async fn create() -> Result<Sink, JsValue> {
        // Before the context: the ringer switch must not mute the game.
        let silent_loop = if !claim_playback_audio_session() && is_ios() {
            start_silent_loop()
        } else {
            None
        };

        let options = web_sys::AudioContextOptions::new();
        options.set_sample_rate(SAMPLE_RATE as f32);
        let ctx = web_sys::AudioContext::new_with_context_options(&options)?;

        let module_url = worklet_url()?;
        let added = JsFuture::from(ctx.audio_worklet()?.add_module(&module_url)?).await;
        // The blob is only needed until the module is parsed.
        let _ = web_sys::Url::revoke_object_url(&module_url);
        added?;

        // Without an explicit outputChannelCount a worklet node with no
        // connected input computes a MONO output — which silently drops
        // one side of every pan.
        let node_options = web_sys::AudioWorkletNodeOptions::new();
        node_options.set_output_channel_count(&js_sys::Array::of1(&JsValue::from_f64(NUM_CHANNELS as f64)));
        let node = web_sys::AudioWorkletNode::new_with_options(&ctx, PROCESSOR_NAME, &node_options)?;
        node.connect_with_audio_node(&ctx.destination())?;

        let reported = Rc::new(Cell::new(0u32));
        let sent_since_report = Rc::new(Cell::new(0u32));
        let onmessage = {
            let reported = reported.clone();
            let sent_since_report = sent_since_report.clone();
            Closure::new(move |e: web_sys::MessageEvent| {
                if let Some(n) = e.data().as_f64() {
                    reported.set(n as u32);
                    sent_since_report.set(0);
                }
                // The hidden-tab tick source: rAF has stopped, but this
                // hasn't, so the session keeps advancing.
                crate::engine::pump_now();
            })
        };
        node.port()?.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        Ok(Sink {
            ctx,
            node,
            reported,
            sent_since_report,
            scratch: Vec::new(),
            silent_loop,
            _onmessage: onmessage,
        })
    }

    /// Top the ring back up to the latency target.
    ///
    /// The estimate errs high on purpose — frames the worklet consumed
    /// since it last reported still count as queued — because
    /// overestimating costs a slightly shallower ring and
    /// underestimating costs an overrun that drops samples.
    pub fn pump(&mut self, stream: &mut CoreStream) {
        let estimate = self.reported.get() + self.sent_since_report.get();
        if estimate + MIN_CHUNK_FRAMES > TARGET_QUEUED_FRAMES {
            return;
        }
        let deficit = (TARGET_QUEUED_FRAMES - estimate) as usize;
        self.scratch.clear();
        self.scratch.resize(deficit, [0; NUM_CHANNELS]);
        // Short delivery is normal: a priming or stalled session has
        // nothing to give, and the ring simply doesn't get topped up.
        let filled = stream.fill(&mut self.scratch[..deficit]);
        if filled == 0 {
            return;
        }
        self.post(filled);
    }

    /// Push silence so the ring starts at a usable depth. See
    /// [`PRIME_FRAMES`] for why the pump can't do this on its own.
    pub fn prime(&mut self) {
        self.scratch.clear();
        self.scratch.resize(PRIME_FRAMES, [0; NUM_CHANNELS]);
        self.post(PRIME_FRAMES);
    }

    /// Drop whatever is queued. Called at session boundaries so the
    /// previous match's tail doesn't play under the next one.
    pub fn flush(&mut self) {
        if let Ok(port) = self.node.port() {
            let _ = port.post_message(&JsValue::NULL);
        }
        self.reported.set(0);
        self.sent_since_report.set(0);
    }

    /// Post the first `frames` frames of the scratch buffer, handing the
    /// backing store over rather than copying it across the boundary.
    fn post(&mut self, frames: usize) {
        let flat: &[i16] = bytemuck::cast_slice(&self.scratch[..frames]);
        let chunk = js_sys::Int16Array::from(flat);
        if let Ok(port) = self.node.port() {
            let transfer = js_sys::Array::of1(&chunk.buffer());
            let _ = port.post_message_with_transferable(&chunk, &transfer);
        }
        self.sent_since_report.set(self.sent_since_report.get() + frames as u32);
    }

    /// The context auto-suspends without a gesture and on some
    /// backgrounding paths; poke it whenever we're about to play. The
    /// same paths pause the ringer workaround's loop, so it gets the
    /// same poke.
    pub fn resume_if_suspended(&self) {
        if self.ctx.state() == web_sys::AudioContextState::Suspended {
            let _ = self.ctx.resume();
        }
        if let Some(el) = &self.silent_loop {
            if el.paused() {
                let _ = el.play();
            }
        }
    }
}

/// The processor source as a blob URL.
fn worklet_url() -> Result<String, JsValue> {
    let parts = js_sys::Array::of1(&JsValue::from_str(WORKLET_JS));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("text/javascript");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &options)?;
    web_sys::Url::create_object_url_with_blob(&blob)
}

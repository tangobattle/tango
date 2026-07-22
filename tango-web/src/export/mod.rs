//! Replay → video export, one backend per target: WebCodecs + the
//! streaming WebM muxer in the browser, ffmpeg subprocesses (the
//! desktop client's pipeline) on native. Both step the same headless
//! playback pair and share the progress/cancel surface the Replays tab
//! and clip strip drive.

use dioxus::prelude::*;

use crate::platform::video::{SCREEN_HEIGHT, SCREEN_WIDTH};

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod sink;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
mod webcodecs;
#[cfg(target_arch = "wasm32")]
mod webm;

#[cfg(not(target_arch = "wasm32"))]
pub use native::{export_replay, pick_save_file, save_picker_available, ExportTarget};
#[cfg(target_arch = "wasm32")]
pub use sink::{pick_save_file, save_picker_available};
#[cfg(target_arch = "wasm32")]
pub use web::{export_replay, ExportTarget};

/// Nearest-neighbor integer upscale baked into the encoded frames, so
/// players that smooth-scale don't blur the pixel art (the desktop
/// exporter scales for the same reason).
pub(crate) const SCALE: usize = 3;
pub(crate) const OUT_W: usize = SCREEN_WIDTH * SCALE;
pub(crate) const OUT_H: usize = SCREEN_HEIGHT * SCALE;

/// One GBA frame in microseconds (280896 cycles at 2^24 Hz) — the
/// exact tick the audio production also follows, so A/V stay aligned.
/// (The native pipeline states the same rate as ffmpeg's `-framerate
/// 16777216/280896` instead of stamping timestamps.)
#[cfg(target_arch = "wasm32")]
pub(crate) const FRAME_US: f64 = 280_896.0 * 1_000_000.0 / 16_777_216.0;

pub(crate) const OPUS_RATE: f64 = 48_000.0;

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

/// mGBA's little-endian BGR555 → RGBA8, nearest-neighbor upscaled by
/// [`SCALE`]. 5-bit channels expand as `(c << 3) | (c >> 2)` so white
/// maps to 255.
pub(crate) fn expand_and_scale(vbuf: &[u8], out: &mut [u8]) {
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

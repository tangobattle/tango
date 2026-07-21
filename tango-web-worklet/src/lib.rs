//! The audio sink's DSP half: an interleaved-i16 ring buffer drained
//! into planar f32 render quanta. Compiled to a dependency-free wasm
//! module by gbaroll's build script and instantiated inside the
//! AudioWorkletGlobalScope (assets/audio-worklet.js is the thin
//! registration shell around it — a worklet can't be JS-free, but all
//! the logic lives here).
//!
//! No wasm-bindgen: the worklet scope lacks the DOM and text codecs
//! the generated glue would want, so the surface is raw exports over
//! linear memory. The shim copies chunks into `push_ptr`, calls
//! [`push`], and bulk-copies [`render`]'s output views out.

#![cfg_attr(target_arch = "wasm32", no_std)]

use core::cell::UnsafeCell;

/// Ring capacity in frames (~341ms at 48kHz) — matches the JS
/// implementation this replaced; the pump targets ~64ms so the
/// headroom only matters for drop-oldest bursts.
const CAPACITY: usize = 16384;
/// Frames a single `push` can take; the shim slices bigger chunks.
const PUSH_CAPACITY: usize = 4096;
/// Frames a single `render` can produce. The render quantum is 128
/// today; sized for the spec's future configurable quanta.
const QUANTUM_CAPACITY: usize = 1024;

/// One wasm instance per processor and one audio thread per instance:
/// nothing is actually shared, the wrapper only satisfies `static`.
#[repr(transparent)]
struct Racefree<T>(UnsafeCell<T>);
unsafe impl<T> Sync for Racefree<T> {}

static RING: Racefree<[i16; CAPACITY * 2]> = Racefree(UnsafeCell::new([0; CAPACITY * 2]));
static PUSH: Racefree<[i16; PUSH_CAPACITY * 2]> = Racefree(UnsafeCell::new([0; PUSH_CAPACITY * 2]));
static OUT_L: Racefree<[f32; QUANTUM_CAPACITY]> = Racefree(UnsafeCell::new([0.0; QUANTUM_CAPACITY]));
static OUT_R: Racefree<[f32; QUANTUM_CAPACITY]> = Racefree(UnsafeCell::new([0.0; QUANTUM_CAPACITY]));
static READ_POS: Racefree<usize> = Racefree(UnsafeCell::new(0));
static LEN: Racefree<usize> = Racefree(UnsafeCell::new(0));

#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    // Unreachable by construction: every index below is bounded.
    core::arch::wasm32::unreachable()
}

/// Where the shim writes incoming interleaved-i16 chunks.
#[no_mangle]
pub extern "C" fn push_ptr() -> *mut i16 {
    PUSH.0.get() as *mut i16
}

#[no_mangle]
pub extern "C" fn push_capacity() -> usize {
    PUSH_CAPACITY
}

#[no_mangle]
pub extern "C" fn out_l_ptr() -> *const f32 {
    OUT_L.0.get() as *const f32
}

#[no_mangle]
pub extern "C" fn out_r_ptr() -> *const f32 {
    OUT_R.0.get() as *const f32
}

#[no_mangle]
pub extern "C" fn quantum_capacity() -> usize {
    QUANTUM_CAPACITY
}

#[no_mangle]
pub extern "C" fn queue_len() -> usize {
    unsafe { *LEN.0.get() }
}

/// Take `frames` interleaved stereo frames from the push buffer.
/// Overflow drops oldest so latency stays bounded.
#[no_mangle]
pub extern "C" fn push(frames: usize) {
    let frames = frames.min(PUSH_CAPACITY);
    unsafe {
        let ring = &mut *RING.0.get();
        let chunk = &*PUSH.0.get();
        let read_pos = &mut *READ_POS.0.get();
        let len = &mut *LEN.0.get();

        let overflow = (*len + frames).saturating_sub(CAPACITY);
        if overflow > 0 {
            *read_pos = (*read_pos + overflow) % CAPACITY;
            *len -= overflow;
        }
        let mut write_pos = (*read_pos + *len) % CAPACITY;
        for i in 0..frames {
            ring[write_pos * 2] = chunk[i * 2];
            ring[write_pos * 2 + 1] = chunk[i * 2 + 1];
            write_pos = (write_pos + 1) % CAPACITY;
        }
        *len += frames;
    }
}

/// Fill `n` frames of output; short queues pad with silence. Stereo
/// fills both planes; mono downmixes into the left plane rather than
/// dropping one side of every pan.
#[no_mangle]
pub extern "C" fn render(n: usize, stereo: i32) {
    let n = n.min(QUANTUM_CAPACITY);
    unsafe {
        let ring = &*RING.0.get();
        let read_pos = &mut *READ_POS.0.get();
        let len = &mut *LEN.0.get();
        let out_l = &mut *OUT_L.0.get();
        let out_r = &mut *OUT_R.0.get();

        for i in 0..n {
            if *len > 0 {
                let r = *read_pos * 2;
                let left = ring[r] as f32 / 32768.0;
                let right = ring[r + 1] as f32 / 32768.0;
                if stereo != 0 {
                    out_l[i] = left;
                    out_r[i] = right;
                } else {
                    out_l[i] = (left + right) / 2.0;
                }
                *read_pos = (*read_pos + 1) % CAPACITY;
                *len -= 1;
            } else {
                out_l[i] = 0.0;
                out_r[i] = 0.0;
            }
        }
    }
}

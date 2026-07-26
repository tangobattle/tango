// The sink that actually feeds the speakers.
//
// It runs on the browser's audio rendering thread, in its own realm: it
// cannot fetch, cannot see the app's wasm module, and cannot call back
// into Rust. So the flow is inverted relative to a native callback
// backend — the main thread *pushes* interleaved-i16 chunks over
// `port.postMessage` and this holds them in a ring until the hardware
// asks for them.
//
// The queue depth goes back the other way every 4th render quantum
// (~10.7ms at 48kHz). The Rust side uses it two ways: as the deficit to
// top the ring back up to, and as a tick source — it keeps firing when
// the tab is hidden and `requestAnimationFrame` stops, which is what
// lets a netplay match survive being backgrounded.
//
// No SharedArrayBuffer anywhere, so this needs no cross-origin
// isolation headers: the app is a static file drop.

// ~340ms at 48kHz. Far above the latency target the producer aims for —
// this is headroom for a main thread that stalled, not the operating
// depth.
const CAPACITY = 16384;

// Render quanta between depth reports. 4 keeps the report rate near
// 100Hz without making the message traffic itself the bottleneck.
const REPORT_EVERY = 4;

class TangoLiteSink extends AudioWorkletProcessor {
    constructor() {
        super();
        // Planar, so `process` is a straight copy per channel.
        this.left = new Float32Array(CAPACITY);
        this.right = new Float32Array(CAPACITY);
        this.head = 0;
        this.queued = 0;
        this.sinceReport = 0;
        this.port.onmessage = (e) => {
            // `null` is the session-boundary flush: the previous
            // session's tail must not play under the new one.
            if (e.data === null) {
                this.head = 0;
                this.queued = 0;
                return;
            }
            this.push(e.data);
        };
    }

    // chunk: Int16Array of interleaved stereo frames.
    push(chunk) {
        const frames = chunk.length >> 1;
        for (let i = 0; i < frames; i++) {
            // Overrun means the producer misjudged the depth. Dropping
            // the newest is the lesser evil: the ring still holds a
            // contiguous, in-order stretch to play.
            if (this.queued >= CAPACITY) break;
            let w = this.head + this.queued;
            if (w >= CAPACITY) w -= CAPACITY;
            this.left[w] = chunk[i * 2] / 32768;
            this.right[w] = chunk[i * 2 + 1] / 32768;
            this.queued++;
        }
    }

    process(inputs, outputs) {
        const out = outputs[0];
        const left = out[0];
        // A mono output shouldn't happen (the node declares stereo), but
        // if it does, play the left channel rather than crashing.
        const right = out[1] || null;
        const n = left.length;
        const take = Math.min(n, this.queued);
        for (let i = 0; i < take; i++) {
            let idx = this.head + i;
            if (idx >= CAPACITY) idx -= CAPACITY;
            left[i] = this.left[idx];
            if (right) right[i] = this.right[idx];
        }
        // Underrun: silence, not stale samples.
        for (let i = take; i < n; i++) {
            left[i] = 0;
            if (right) right[i] = 0;
        }
        this.head += take;
        if (this.head >= CAPACITY) this.head -= CAPACITY;
        this.queued -= take;

        if (++this.sinceReport >= REPORT_EVERY) {
            this.sinceReport = 0;
            this.port.postMessage(this.queued);
        }
        // Never false: the node outlives any one session, and returning
        // false would let the browser collect it between matches.
        return true;
    }
}

registerProcessor("tango-lite-sink", TangoLiteSink);

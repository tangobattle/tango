// The tango-web audio sink's shell. The ring buffer and mixdown live in
// tango-web-worklet's wasm module, shipped in via processorOptions (this
// scope can't fetch); a worklet can't be JS-free, so this class is
// just the registration, the copies across the wasm boundary, and the
// queue-depth report every 4th render quantum (~10.7ms at 48kHz) —
// which the main thread also uses as a tick source when the tab is
// hidden and requestAnimationFrame stops. Interleaved-i16 chunks
// arrive via port.postMessage (no SharedArrayBuffer anywhere).
class GbarollSink extends AudioWorkletProcessor {
    constructor(options) {
        super();
        // Sync compile: a few KB of dependency-free code, and worklet
        // scopes allow it (the main thread's 4KB limit doesn't apply).
        const module = new WebAssembly.Module(options.processorOptions.wasm);
        this.wasm = new WebAssembly.Instance(module, {}).exports;
        // All state is static — the memory never grows, so these views
        // stay valid for the processor's lifetime.
        const memory = this.wasm.memory.buffer;
        this.pushBuf = new Int16Array(
            memory,
            this.wasm.push_ptr(),
            this.wasm.push_capacity() * 2
        );
        this.outL = new Float32Array(
            memory,
            this.wasm.out_l_ptr(),
            this.wasm.quantum_capacity()
        );
        this.outR = new Float32Array(
            memory,
            this.wasm.out_r_ptr(),
            this.wasm.quantum_capacity()
        );
        this.sinceReport = 0;
        this.port.onmessage = (e) => this.push(e.data);
    }

    // chunk: Int16Array of interleaved stereo frames, any length.
    push(chunk) {
        for (let done = 0; done < chunk.length; done += this.pushBuf.length) {
            const part = chunk.subarray(done, done + this.pushBuf.length);
            this.pushBuf.set(part);
            this.wasm.push(part.length >> 1);
        }
    }

    process(inputs, outputs) {
        const out = outputs[0];
        const left = out[0];
        // A mono output (shouldn't happen — the node declares stereo)
        // gets a proper downmix rather than one side of every pan.
        const right = out[1] || null;
        const n = left.length;
        this.wasm.render(n, right ? 1 : 0);
        left.set(this.outL.subarray(0, n));
        if (right) {
            right.set(this.outR.subarray(0, n));
        }
        if (++this.sinceReport >= 4) {
            this.sinceReport = 0;
            this.port.postMessage(this.wasm.queue_len());
        }
        return true;
    }
}

registerProcessor("tango-web-sink", GbarollSink);

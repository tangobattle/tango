// The audio sink: a ring the page pushes into and the browser's audio
// thread drains. It runs on that thread, which can't call into wasm, so
// the flow is inverted relative to a desktop callback backend — the
// page pulls samples out of the session and posts them here, and this
// reports back how much is left. That report is also what keeps the
// session ticking when the tab is hidden and rAF stops.

const CAPACITY = 1 << 15; // frames; plenty above the page's target

class SinkProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.left = new Float32Array(CAPACITY);
    this.right = new Float32Array(CAPACITY);
    this.read = 0;
    this.write = 0;
    this.port.onmessage = (e) => this.push(e.data);
  }

  get queued() {
    return (this.write - this.read + CAPACITY) % CAPACITY;
  }

  // Interleaved stereo in, deinterleaved into the ring. An overrun
  // drops the newest frames rather than wrapping over unplayed ones.
  push(samples) {
    for (let i = 0; i + 1 < samples.length; i += 2) {
      const next = (this.write + 1) % CAPACITY;
      if (next === this.read) break;
      this.left[this.write] = samples[i];
      this.right[this.write] = samples[i + 1];
      this.write = next;
    }
  }

  process(_inputs, outputs) {
    const [left, right] = outputs[0];
    for (let i = 0; i < left.length; i++) {
      if (this.read === this.write) {
        // Underrun: silence beats repeating stale samples.
        left[i] = 0;
        right[i] = 0;
        continue;
      }
      left[i] = this.left[this.read];
      right[i] = this.right[this.read];
      this.read = (this.read + 1) % CAPACITY;
    }
    this.port.postMessage(this.queued);
    return true;
  }
}

registerProcessor('tango-demo-sink', SinkProcessor);

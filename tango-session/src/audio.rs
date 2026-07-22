//! The session-side audio contract: a `Stream` produces interleaved
//! stereo i16 on demand. Each session's [`CoreStream`] impl pulls
//! samples out of its live core(s) and resamples to the host rate;
//! how a `Stream` reaches the speakers is entirely the host's business
//! (the app routes it through its own late-binding mux into SDL).
//!
//! [`CoreStream`]: crate::core_stream::CoreStream

pub const NUM_CHANNELS: usize = 2;
pub const SAMPLES: usize = 512;

pub trait Stream {
    fn fill(&mut self, buf: &mut [[i16; NUM_CHANNELS]]) -> usize;
}


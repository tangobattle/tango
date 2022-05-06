#[derive(Clone)]
pub struct MixHandle(std::sync::Arc<InnerMixHandle>);

impl Drop for InnerMixHandle {
    fn drop(&mut self) {
        let mut mix = self.mix.lock();
        mix.mixees.remove(&self.id);
    }
}

struct InnerMixHandle {
    id: usize,
    mix: std::sync::Arc<parking_lot::Mutex<InnerMixStream>>,
}

#[derive(Clone)]
pub struct MixStream(std::sync::Arc<parking_lot::Mutex<InnerMixStream>>);

struct Mixee {
    stream: Box<dyn super::Stream + Send + 'static>,
}

struct InnerMixStream {
    buf: std::sync::Arc<parking_lot::Mutex<Vec<i16>>>,
    mixees: std::collections::HashMap<usize, Mixee>,
    next_id: usize,
}

impl MixStream {
    pub fn new() -> MixStream {
        MixStream(std::sync::Arc::new(parking_lot::Mutex::new(
            InnerMixStream {
                buf: std::sync::Arc::new(parking_lot::Mutex::new(vec![])),
                mixees: std::collections::HashMap::new(),
                next_id: 0,
            },
        )))
    }

    pub fn open_stream(&self, stream: impl super::Stream + Send + 'static) -> MixHandle {
        let mut mix = self.0.lock();
        let id = mix.next_id;
        mix.mixees.insert(
            id,
            Mixee {
                stream: Box::new(stream),
            },
        );
        mix.next_id += 1;
        MixHandle(std::sync::Arc::new(InnerMixHandle {
            id,
            mix: self.0.clone(),
        }))
    }
}

impl super::Stream for MixStream {
    fn fill(&mut self, buf: &mut [i16]) -> usize {
        let mut mix = self.0.lock();
        let mix_buf = mix.buf.clone();
        let mut mix_buf = mix_buf.lock();
        if buf.len() > mix_buf.len() {
            *mix_buf = vec![0i16; buf.len()];
        }
        for (_, mixee) in mix.mixees.iter_mut() {
            mixee.stream.fill(&mut mix_buf);
            for (i, v) in mix_buf.iter().enumerate() {
                buf[i] += v;
            }
        }
        buf.len()
    }
}

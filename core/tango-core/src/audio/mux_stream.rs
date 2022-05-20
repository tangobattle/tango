#[derive(Clone)]
pub struct MuxHandle(std::sync::Arc<InnerMuxHandle>);

impl MuxHandle {
    pub fn switch(&self) {
        let mut mux = self.0.mux.lock();
        mux.current_id = self.0.id;
    }
}

impl Drop for InnerMuxHandle {
    fn drop(&mut self) {
        let mut mux = self.mux.lock();
        mux.streams.remove(&self.id);
        if mux.current_id == self.id {
            mux.current_id = 0;
        }
    }
}

struct InnerMuxHandle {
    id: usize,
    mux: std::sync::Arc<parking_lot::Mutex<InnerMuxStream>>,
}

#[derive(Clone)]
pub struct MuxStream(std::sync::Arc<parking_lot::Mutex<InnerMuxStream>>);

struct InnerMuxStream {
    streams: std::collections::HashMap<usize, Box<dyn super::Stream + Send + 'static>>,
    current_id: usize,
    next_id: usize,
}

impl MuxStream {
    pub fn new() -> MuxStream {
        MuxStream(std::sync::Arc::new(parking_lot::Mutex::new(
            InnerMuxStream {
                streams: std::collections::HashMap::new(),
                current_id: 0,
                next_id: 0,
            },
        )))
    }

    pub fn open_stream(&self, stream: impl super::Stream + Send + 'static) -> MuxHandle {
        let mut mux = self.0.lock();
        let id = mux.next_id;
        mux.streams.insert(id, Box::new(stream));
        mux.next_id += 1;
        MuxHandle(std::sync::Arc::new(InnerMuxHandle {
            id,
            mux: self.0.clone(),
        }))
    }
}

impl super::Stream for MuxStream {
    fn fill(&mut self, buf: &mut [i16]) -> usize {
        let mut mux = self.0.lock();
        let current_id = mux.current_id;
        for (id, stream) in mux.streams.iter_mut() {
            if *id == current_id {
                continue;
            }
            stream.fill(buf);
        }
        if let Some(stream) = mux.streams.get_mut(&current_id) {
            stream.fill(buf)
        } else {
            0
        }
    }
}

//! [`ReplayStore`] for a host with no filesystem.
//!
//! A recording accumulates in memory and lands in storage when the
//! match lets go of it. That is later than it sounds and it is the only
//! moment available: [`tango_replay::Writer`] takes a
//! `Box<dyn Write + Send>` and writes into it for the whole match, so
//! there is no completion callback to hook — but there is a `Drop`, and
//! it runs on both paths that end a match. A finished match drops the
//! writer from `PvpDriver::finish` after the tail is flushed; an
//! abandoned one drops it with the session, leaving the
//! truncated-but-parseable recording the format is designed to tolerate.
//!
//! Buffering the whole thing is fine at the sizes involved — a long
//! match is a few hundred KB of input pairs — and it is what lets the
//! write be one atomic put rather than a stream of appends into a store
//! that has no append.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tango_session::pvp::{Recording, ReplayStore};

pub struct BrowserReplayStore;

impl ReplayStore for BrowserReplayStore {
    fn create(&self, name: &str) -> std::io::Result<Recording> {
        let key = crate::library::replays_path().join(format!("{name}.{}", tango_replay::EXTENSION));
        log::info!("pvp: recording to {}", key.display());
        Ok(Recording {
            sink: Box::new(Sink {
                key: key.clone(),
                buffer: Arc::new(Mutex::new(Vec::new())),
            }),
            key,
        })
    }
}

/// The in-memory recording. `Arc<Mutex<…>>` rather than a plain `Vec`
/// only to satisfy the `Send` the writer's box demands — wasm is
/// single-threaded and the lock is never contended.
struct Sink {
    key: PathBuf,
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        let bytes = std::mem::take(&mut *self.buffer.lock().unwrap());
        // A match that ended before the header was written has nothing
        // worth a row.
        if bytes.len() <= tango_replay::HEADER.len() {
            return;
        }
        log::info!("pvp: saved recording {} ({} bytes)", self.key.display(), bytes.len());
        crate::library::write_replay(&self.key, &bytes);
    }
}

//! Caller-side cancellation for a running export.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A cancel handle, cloneable and callable from any thread.
///
/// [`Canceller::kill`] does two things:
///
///   * Sets a flag the export checks at every loop iteration and at
///     each encoder-free boundary, so a cancel lands promptly whatever
///     phase the export is in — before any encoder exists, between
///     replays, or mid-loop during a stretch that isn't being written.
///   * Terminates every encoder subprocess registered with it, so a
///     blocked pipe write (the encode loop's likely parking spot when
///     an encoder falls behind) and the post-loop wait on each child
///     both return an error immediately.
///
/// Either signal alone would unblock the export; both fire from one
/// `kill()` so neither has to cover the other's gap.
#[derive(Clone, Default)]
pub struct Canceller {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Canceller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Canceller")
            .field("cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct Inner {
    cancelled: AtomicBool,
    #[cfg(not(target_arch = "wasm32"))]
    children: std::sync::Mutex<Vec<ChildSlot>>,
}

/// A spawned encoder, shared between the wrapper that writes to it and
/// the canceller that may have to kill it.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) type ChildSlot = Arc<std::sync::Mutex<Option<std::process::Child>>>;

impl Canceller {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark this canceller cancelled and kill every encoder registered
    /// with it. Safe to call from any thread, any number of times.
    pub fn kill(&self) {
        self.inner.cancelled.store(true, Ordering::Relaxed);
        #[cfg(not(target_arch = "wasm32"))]
        for slot in self.inner.children.lock().unwrap().iter() {
            if let Some(child) = slot.lock().unwrap().as_mut() {
                let _ = child.kill();
            }
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Relaxed)
    }

    /// `Err` if this canceller has been killed — the shape the export
    /// loop's frequent checks want.
    pub fn check(&self) -> crate::Result<()> {
        if self.is_cancelled() {
            return Err(crate::Error::Cancelled);
        }
        Ok(())
    }

    /// Put a freshly spawned encoder under this canceller's control.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn register(&self, child: std::process::Child) -> ChildSlot {
        let slot: ChildSlot = Arc::new(std::sync::Mutex::new(Some(child)));
        self.inner.children.lock().unwrap().push(slot.clone());
        // Race guard: if kill() fired between the spawn and this
        // registration, kill the newcomer on arrival so it can't slip
        // through the net and keep running.
        if self.is_cancelled() {
            if let Some(child) = slot.lock().unwrap().as_mut() {
                let _ = child.kill();
            }
        }
        slot
    }
}

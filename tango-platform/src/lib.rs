//! What differs between a desktop and a browser.
//!
//! Two things, and only two: whether a bound is `Send` (a browser's
//! handles are JS values pinned to their thread), and how a task gets
//! started (tokio's runtime, or the microtask queue). Everything else in
//! this crate is written once.

/// `Send`, except on wasm32 where it is no constraint at all.
#[cfg(not(target_arch = "wasm32"))]
pub trait WasmNotSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + ?Sized> WasmNotSend for T {}

#[cfg(target_arch = "wasm32")]
pub trait WasmNotSend {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> WasmNotSend for T {}

/// `Sync`, except on wasm32 where it is no constraint at all.
#[cfg(not(target_arch = "wasm32"))]
pub trait WasmNotSync: Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync + ?Sized> WasmNotSync for T {}

#[cfg(target_arch = "wasm32")]
pub trait WasmNotSync {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> WasmNotSync for T {}

/// Start `future` and let it run on its own.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn(future: impl std::future::Future<Output = ()> + Send + 'static) {
    tokio::spawn(future);
}

#[cfg(target_arch = "wasm32")]
pub fn spawn(future: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(future);
}

/// Sleep for `duration`.
///
/// Natively this is the runtime's timer, which is why a drive thread
/// runs inside a runtime context. A browser's timers are the page's,
/// and don't need one.
#[cfg(not(target_arch = "wasm32"))]
pub async fn sleep(duration: std::time::Duration) {
    tokio::time::sleep(duration).await;
}

#[cfg(target_arch = "wasm32")]
pub async fn sleep(duration: std::time::Duration) {
    gloo_timers::future::sleep(duration).await;
}

/// A deadline passed before the future finished.
#[derive(Debug, thiserror::Error)]
#[error("deadline elapsed")]
pub struct Elapsed;

/// Run `future` with a deadline.
#[cfg(not(target_arch = "wasm32"))]
pub async fn timeout<F: std::future::Future>(duration: std::time::Duration, future: F) -> Result<F::Output, Elapsed> {
    tokio::time::timeout(duration, future).await.map_err(|_| Elapsed)
}

#[cfg(target_arch = "wasm32")]
pub async fn timeout<F: std::future::Future>(duration: std::time::Duration, future: F) -> Result<F::Output, Elapsed> {
    use futures::future::Either;
    futures::pin_mut!(future);
    match futures::future::select(future, Box::pin(sleep(duration))).await {
        Either::Left((output, _)) => Ok(output),
        Either::Right(_) => Err(Elapsed),
    }
}

/// A fixed-period tick, for the loops that ping on a cadence.
///
/// Late ticks are dropped rather than queued: a loop that was blocked
/// past several periods wants the next one on schedule, not a burst
/// catching up.
pub struct Ticker {
    #[cfg(target_arch = "wasm32")]
    period: std::time::Duration,
    #[cfg(not(target_arch = "wasm32"))]
    inner: tokio::time::Interval,
    /// Whether the next tick is the immediate one.
    #[cfg(target_arch = "wasm32")]
    first: bool,
}

impl Ticker {
    /// Tick every `period`, the first one immediately.
    pub fn immediate(period: std::time::Duration) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut inner = tokio::time::interval(period);
            inner.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            Self { inner }
        }
        #[cfg(target_arch = "wasm32")]
        Self { period, first: true }
    }

    /// Tick every `period`, starting one period from now.
    pub fn every(period: std::time::Duration) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut inner = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
            inner.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            Self { inner }
        }
        #[cfg(target_arch = "wasm32")]
        Self { period, first: false }
    }

    pub async fn tick(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.inner.tick().await;
        }
        #[cfg(target_arch = "wasm32")]
        if std::mem::take(&mut self.first) {
        } else {
            sleep(self.period).await;
        }
    }
}

//! Target compatibility shims for the handful of host services the
//! shared code needs: async sleeps, fire-and-forget local task spawns,
//! and wall-clock time. The web build maps these onto gloo/wasm-bindgen
//! and `Date.now()`; the native build onto a timer thread, the Dioxus
//! executor, and `SystemTime`.
//!
//! Everything spawned here is `!Send` and runs on the main thread on
//! both targets — the crate's single-thread session/runtime model
//! (`Rc`/`RefCell`/`thread_local!` throughout) depends on that.

#[cfg(target_arch = "wasm32")]
pub async fn sleep_ms(ms: u32) {
    gloo_timers::future::TimeoutFuture::new(ms).await;
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn sleep_ms(ms: u32) {
    futures_timer::Delay::new(std::time::Duration::from_millis(ms as u64)).await;
}

/// Spawn a `!Send` fire-and-forget task on the main-thread executor.
///
/// Native builds delegate to the Dioxus runtime's own task arena, so
/// this must be called from within a Dioxus runtime context (component
/// scope, event handler, or a task that was itself spawned there) —
/// which is where every call site in this crate already lives.
pub fn spawn_local(fut: impl std::future::Future<Output = ()> + 'static) {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(fut);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = dioxus::core::spawn_forever(fut);
}

/// Milliseconds since the Unix epoch, as `Date.now()` reports it.
pub fn now_unix_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0)
    }
}

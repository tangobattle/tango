//! The tokio runtime the native net backends spawn their I/O tasks on.
//!
//! dioxus-native enters a tokio runtime around its event loop, so
//! usually `Handle::try_current()` from the main thread just works;
//! the lazily-built fallback runtime covers headless/test contexts.
//! Socket I/O lives on these worker threads and talks to the
//! main-thread (`!Send`) world through channels — the same shape the
//! browser backends have, with the tokio side standing in for the JS
//! event loop.

use std::sync::OnceLock;

static FALLBACK: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

pub fn handle() -> tokio::runtime::Handle {
    if let Ok(h) = tokio::runtime::Handle::try_current() {
        return h;
    }
    FALLBACK
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("tokio runtime")
        })
        .handle()
        .clone()
}

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

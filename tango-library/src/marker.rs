//! `Send`/`Sync` bounds that evaporate on wasm.
//!
//! The seam traits ([`crate::storage::Storage`], [`crate::http::Http`])
//! want thread-safety bounds on native, where a scan runs on a worker
//! thread — and can't have them on wasm32, where the backing handles
//! (`FileSystemSyncAccessHandle`, `fetch` futures) are JS values bound
//! to their thread and are neither `Send` nor `Sync`.
//!
//! Writing each trait twice under `#[cfg]` would mean maintaining two
//! copies of every signature. These blanket markers say it once instead:
//! they alias the real bound off wasm and mean nothing on it, so a trait
//! writes `WasmNotSend + WasmNotSync` and gets the right thing on both.

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

/// A future carrying the same conditional `Send`.
///
/// `dyn Future + WasmNotSend` would be rejected — only *auto* traits may
/// be listed as extra bounds on a trait object, and these blankets
/// aren't auto traits. Naming the combination as one trait sidesteps
/// that: `dyn WasmNotSendFuture` has a single principal trait, and off
/// wasm the `Send` supertrait elaborates through it, so the boxed form
/// below is still `Send` where it needs to be.
pub trait WasmNotSendFuture: std::future::Future + WasmNotSend {}
impl<F: std::future::Future + WasmNotSend + ?Sized> WasmNotSendFuture for F {}

/// Boxed [`WasmNotSendFuture`] — the return type of every async method
/// on a seam trait. One alias covering both targets, in place of the
/// `BoxFuture` / `LocalBoxFuture` `#[cfg]` pair it replaces.
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn WasmNotSendFuture<Output = T> + 'a>>;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// The whole point of routing the boxed futures through
    /// [`WasmNotSendFuture`] rather than a plain `dyn Future` is that off
    /// wasm they stay `Send`, so a scan can still be driven from a
    /// worker thread. That relies on the `Send` supertrait elaborating
    /// through the trait object, which is worth pinning down.
    #[test]
    fn boxed_future_is_send_off_wasm() {
        fn assert_send<T: Send>() {}
        assert_send::<BoxFuture<'static, u32>>();
    }
}

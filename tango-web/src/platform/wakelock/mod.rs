//! Screen wake lock while a session runs. Browser backend uses
//! `navigator.wakeLock`; there is no native backend — desktop OSes
//! don't blank the screen under an app that's presenting frames the
//! way a mobile browser does, so the native impl is a no-op.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
#[cfg(target_arch = "wasm32")]
pub use web::*;

//! Gamepad input, one backend per target: the browser Gamepad API
//! snapshot on wasm, SDL3 events on native. Both fold into
//! [`HeldState`](super::input::HeldState) via [`poll_into`], called
//! once per runtime pump.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
#[cfg(target_arch = "wasm32")]
pub use web::*;

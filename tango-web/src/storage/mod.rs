//! File storage, one backend per target with the same API surface:
//! OPFS in the browser (files are imported/copied in), the desktop
//! client's real directory layout on native — `~/Documents/Tango/
//! {roms,saves,replays,patches}`, shared with the desktop app, scanned
//! in place. Handle types differ per target (`FileSystemDirectoryHandle`
//! vs. path newtypes) but flow through the same named functions, so
//! callers don't care.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
#[cfg(target_arch = "wasm32")]
pub use web::*;

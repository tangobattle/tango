//! The on-disk game library, as this frontend sees it.
//!
//! The library itself lives in the [`tango_library`] crate, which knows
//! nothing about a UI toolkit and reaches storage and the network only
//! through its `Storage` / `Http` traits — so a browser build can reuse
//! the registry, the scanners, and the patch catalog over OPFS and
//! `fetch`. This module re-exports that surface (so `crate::library::*`
//! keeps resolving), binds the native implementations of the two seams,
//! and adds the parts that are genuinely host-side:
//!
//! * [`replays`]: the crate's replay index, plus the re-simulation that
//!   produces match stats — that one needs an emulator core and the
//!   analysis engine, so it stays out of the library.
//! * [`autoupdate`]: the background index refresher, which owns a tokio
//!   task and so belongs to whoever owns the runtime.

pub mod autoupdate;
pub mod replays;

pub use tango_library::{bnlc, game, patch, rom, rom_overrides, save, storage};

use tango_library::http::Http;
use tango_library::storage::Storage;

/// The filesystem, as the library sees it. Stateless, so this is a
/// plain static.
pub fn storage() -> &'static dyn Storage {
    &tango_library::storage::StdStorage
}

/// The app's HTTP client. One instance for the process so the index
/// poll and any package downloads share a connection pool.
pub fn http() -> &'static dyn Http {
    static HTTP: std::sync::LazyLock<tango_library::http::ReqwestHttp> =
        std::sync::LazyLock::new(tango_library::http::ReqwestHttp::new);
    &*HTTP
}

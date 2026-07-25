//! Video/audio encoding behind one API.
//!
//! An encoder [`Backend`] feeds one [`Session`], which hands its packets
//! to a [`mux::Muxer`]. Only the encoding is platform-specific; timing,
//! interleaving, containers and chapters are shared:
//!
//! ```text
//!   frames + samples ──► Backend ──► Packet ──► Session ──► Muxer ──► bytes
//! ```
//!
//! The modules follow that pipeline:
//!
//!   * [`settings`] — what to produce: codecs, quality, geometry,
//!     timebase. The caller's choices, all checked before anything runs.
//!   * [`codec`] — the codecs themselves, each answering for its own
//!     container IDs, sample entries, encoder flags and stream parsing.
//!   * [`backend`] — the encoders. `FfmpegBackend` runs a subprocess per
//!     stream and parses back the elementary streams they write;
//!     `WebCodecsBackend` drives the browser's encoders on wasm32.
//!   * [`packet`] — what comes out of them and goes into a container.
//!   * [`mux`] — the containers: MP4, Matroska, WebM.
//!   * [`Session`] — the pipeline: interleaves packets and drives the
//!     muxer.
//!
//! Two seams make that composable. [`Packet`] is one encoded access unit
//! timestamped in its track's own integer timebase, so a new encoder
//! plugs in by implementing [`Backend`] and a new container by
//! implementing [`mux::Muxer`], with nothing else changing.
//!
//! Nothing here does I/O. A session produces bytes to append and, at the
//! close, [`mux::Patch`]es to write back over positions already passed —
//! so a caller holding a [`std::fs::File`] and one awaiting a browser's
//! file stream drive the same session the same way. Native callers can
//! hand both to [`Output`].

pub mod backend;
pub mod codec;
pub mod mux;
pub mod packet;
pub mod settings;

mod cancel;
mod error;
mod session;

pub use backend::{Backend, VIDEO_TRACK};
pub use cancel::Canceller;
pub use error::{Error, Result};
pub use codec::{AudioCodec, VideoCodec};
pub use mux::{Chapter, Container};
pub use packet::{AudioTrackInfo, Packet, VideoTrackInfo};
pub use session::Session;
pub use settings::{AudioSettings, ColorInfo, Settings, VideoQuality, VideoSettings};

#[cfg(not(target_arch = "wasm32"))]
mod output;

#[cfg(not(target_arch = "wasm32"))]
pub use backend::FfmpegBackend;
#[cfg(not(target_arch = "wasm32"))]
pub use output::Output;

#[cfg(target_arch = "wasm32")]
pub use backend::WebCodecsBackend;

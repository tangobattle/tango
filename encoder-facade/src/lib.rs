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
//!   * [`backend`] — the encoders. `FfmpegBackend` runs a subprocess per
//!     stream and reads back the fragmented MP4 each one writes;
//!     `WebCodecsBackend` drives the browser's encoders on wasm32.
//!   * [`packet`] — what comes out of them and goes into a container.
//!   * [`mux`] — the containers: MP4 and Matroska.
//!   * [`Session`] — the pipeline: interleaves packets and drives the
//!     muxer.
//!
//! Two seams make that composable. [`Packet`] is one encoded access unit
//! timestamped in its track's own integer timebase, so a new encoder
//! plugs in by implementing [`Backend`] and a new container by
//! implementing [`mux::Muxer`], with nothing else changing.
//!
//! Nothing here does I/O. A session produces bytes to append and, at the
//! close, [`mux::Fixup`]s that finish the parts written earlier — so a
//! caller holding a [`std::fs::File`] and one awaiting a browser's file
//! stream drive the same session the same way. Native callers can hand
//! both to [`Output`].

pub mod backend;
pub mod mux;
pub mod packet;
pub mod settings;

mod cancel;
mod error;
mod output;
mod session;

pub use backend::{Backend, VIDEO_TRACK};
pub use cancel::Canceller;
pub use error::{Error, Result};
pub use mux::{Chapter, Container};
pub use output::Output;
pub use packet::{AudioTrackInfo, Packet, VideoTrackInfo};
pub use session::Session;
pub use settings::{AudioCodec, AudioSettings, ColorInfo, H264Quality, Settings, VideoCodec, VideoSettings};

#[cfg(not(target_arch = "wasm32"))]
pub use backend::FfmpegBackend;

#[cfg(target_arch = "wasm32")]
pub use backend::WebCodecsBackend;

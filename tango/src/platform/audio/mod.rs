//! The app's audio surface: the toolkit-agnostic core (the `Stream`
//! trait + the `LateBinder` mux sessions bind into) lives in
//! [`tango_session::audio`]; this module re-exports it and adds the
//! SDL host backend that actually drives the speakers.

pub mod sdl;

pub use tango_session::audio::*;

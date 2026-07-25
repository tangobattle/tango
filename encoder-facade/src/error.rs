//! What can go wrong, as something a caller can match on.

use crate::{AudioCodec, Container, VideoCodec};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The canceller was killed. Distinct from a failure — a caller
    /// reports a cancelled export differently from a broken one.
    #[error("the export was cancelled")]
    Cancelled,

    /// Settings or arguments that can't produce a valid file, caught
    /// before anything is encoded.
    #[error("{0}")]
    Invalid(String),

    /// An invariant inside the crate didn't hold: a bug here rather than
    /// bad input.
    #[error("{0}")]
    Internal(String),

    /// A codec the chosen container has no way to carry.
    #[error("{container:?} cannot carry {video:?} video with {audio:?} audio")]
    CodecNotInContainer {
        container: Container,
        video: VideoCodec,
        audio: AudioCodec,
    },

    /// An encoder's output didn't match the format it was asked for.
    #[error("malformed {format} stream from the encoder: {detail}")]
    Bitstream { format: &'static str, detail: String },

    /// The encoders finished without producing anything to mux.
    #[error("the encoders produced nothing to mux — the export was empty")]
    Empty,

    /// ffmpeg couldn't be started at all.
    #[error("couldn't start ffmpeg at {path} (is it installed, or beside the executable?): {source}")]
    FfmpegSpawn {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// This ffmpeg build has no muxer for an elementary-stream format
    /// the export needs — the usual shape of a sidecar build trimmed
    /// down to muxing duties.
    #[error(
        "this ffmpeg build cannot write the {formats} output format(s) this export needs; \
         rebuild it with --enable-muxer={formats}"
    )]
    FfmpegMissingFormats { formats: String },

    /// ffmpeg failed while encoding. `stderr` is its own last words,
    /// which say far more than we could.
    #[error("{context}{}", format_stderr(stderr))]
    Ffmpeg { context: String, stderr: Vec<String> },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("writing MP4: {0}")]
    Mp4(#[from] mp4_atom::Error),

    #[error("writing Matroska: {0}")]
    Matroska(#[from] mkv_element::Error),

    /// A WebCodecs encoder reported an error, or the browser has none.
    #[cfg(target_arch = "wasm32")]
    #[error("WebCodecs: {0}")]
    WebCodecs(String),
}

impl Error {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Error::Invalid(message.into())
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Error::Internal(message.into())
    }

    pub(crate) fn bitstream(format: &'static str, detail: impl std::fmt::Display) -> Self {
        Error::Bitstream {
            format,
            detail: detail.to_string(),
        }
    }
}

fn format_stderr(stderr: &[String]) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!("\nffmpeg said:\n{}", stderr.join("\n"))
    }
}

/// `return Err(Error::Invalid(...))` unless the condition holds.
macro_rules! check {
    ($cond:expr, $($arg:tt)+) => {
        if !$cond {
            return Err($crate::Error::invalid(format!($($arg)+)));
        }
    };
}

pub(crate) use check;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffmpeg_errors_carry_the_encoders_own_words() {
        let error = Error::Ffmpeg {
            context: "couldn't feed ffmpeg".into(),
            stderr: vec!["Unknown encoder 'libx264rgb'".into()],
        };
        let shown = error.to_string();
        assert!(shown.contains("couldn't feed ffmpeg"), "{shown}");
        assert!(shown.contains("Unknown encoder 'libx264rgb'"), "{shown}");
    }

    #[test]
    fn a_quiet_failure_says_only_what_it_knows() {
        let error = Error::Ffmpeg {
            context: "ffmpeg exited with code 1".into(),
            stderr: vec![],
        };
        assert_eq!(error.to_string(), "ffmpeg exited with code 1");
    }
}

//! The Tango patch package format: `.tangopatch` files, their manifests,
//! and the repo index that lets a client find and fetch them on demand.
//!
//! # The unit is one patch version
//!
//! A `.tangopatch` is a zip holding exactly one version of one patch —
//! its metadata, its BPS patches, its save templates, and its README —
//! and it is the same artifact everywhere: what an author builds, what
//! the repo stores and serves, and what the app keeps on disk. There is
//! no loose-file layout to scan and no shared per-patch file that every
//! version has to agree about.
//!
//! # How a client uses this
//!
//! A repo publishes an [`index::Index`] (small, cacheable) listing every
//! package with its URL, hash, and everything the UI and the netplay
//! compatibility check need. Clients poll only that, and download a
//! package when something actually calls for it — the player picks the
//! patch, a peer shows up using it, or a replay needs it. Nobody mirrors
//! the whole repo.
//!
//! # Modules
//!
//! Always available: [`manifest`] (`manifest.toml`, including the
//! [`Compatibility`] declaration), [`overrides`] (the `[rom_overrides]`
//! schema), [`layout`] (what lives where inside a package), and [`tag`]
//! (resolving netplay compatibility).
//!
//! The three that do I/O are separately gated, since no single consumer
//! wants all of them — the app reads packages and the index but never
//! writes one, and the bundler writes them without needing the reader:
//!
//! | feature | module | for |
//! |---|---|---|
//! | `package` | [`package`] | reading a `.tangopatch` |
//! | `bundle` | [`bundle`] | writing one |
//! | `index` | [`index`] | the repo index (building it also needs the other two) |
//! | `cli` | — | the `tango-patch` bundler binary; enables all three |

#[cfg(feature = "bundle")]
pub mod bundle;
#[cfg(feature = "index")]
pub mod index;
pub mod layout;
pub mod manifest;
pub mod overrides;
#[cfg(feature = "package")]
pub mod package;
pub mod tag;

#[cfg(feature = "index")]
pub use index::Index;
pub use layout::RomTarget;
pub use manifest::{Compatibility, Manifest};
pub use overrides::Overrides;
#[cfg(feature = "package")]
pub use package::Package;
pub use tag::Tag;

/// The package file extension, without the dot.
pub const EXTENSION: &str = "tangopatch";

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[cfg(any(feature = "package", feature = "bundle"))]
    #[error("{0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("malformed manifest: {0}")]
    ManifestSyntax(#[from] toml::de::Error),
    #[error("could not write manifest: {0}")]
    ManifestSerialize(#[from] toml::ser::Error),
    #[cfg(feature = "index")]
    #[error("malformed index: {0}")]
    IndexSyntax(#[from] serde_json::Error),
    #[error("format {0} is not supported (this build reads format {})", manifest::FORMAT)]
    UnsupportedFormat(u32),
    #[error("{0}")]
    Invalid(String),
    #[error("this package does not patch {0}")]
    NoSuchTarget(RomTarget),
    #[error("{path}: {source}")]
    At {
        path: std::path::PathBuf,
        #[source]
        source: Box<Error>,
    },
}

impl Error {
    /// Attach the file a failure came from. Without this, a bad package
    /// in a directory of 250 reports only "malformed manifest".
    pub fn at(self, path: impl Into<std::path::PathBuf>) -> Error {
        match self {
            // Don't stack paths when an inner call already attributed it.
            Error::At { .. } => self,
            source => Error::At {
                path: path.into(),
                source: Box::new(source),
            },
        }
    }
}

/// Accept a patch name, netplay group, or save template name.
///
/// The charset is deliberately narrow — ASCII alphanumerics, `_` and `-`,
/// starting alphanumeric. These strings become file names, URL path
/// segments, and (for groups) parts of a [`Tag`], so restricting them
/// once here is what lets everything downstream skip escaping, path
/// traversal checks, and separator ambiguity.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("must not be empty".into());
    }
    if name.len() > manifest::MAX_NAME_LEN {
        return Err(format!("must be at most {} characters", manifest::MAX_NAME_LEN));
    }
    if !name.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        return Err(format!("{name:?} must start with a letter or digit"));
    }
    if let Some(c) = name
        .chars()
        .find(|c| !matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-'))
    {
        return Err(format!("{name:?} contains disallowed character {c:?}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_restricted_to_safe_characters() {
        for ok in ["bn6_allstars", "bn3_re--mod", "bbbn3_Espanol", "a", "9lives"] {
            assert!(validate_name(ok).is_ok(), "{ok:?} should be accepted");
        }
        for bad in [
            "",
            "_leading",
            "-leading",
            "has space",
            "has/slash",
            "..",
            "dot.ted",
            "emoji✨",
        ] {
            assert!(validate_name(bad).is_err(), "{bad:?} should be rejected");
        }
        assert!(validate_name(&"a".repeat(manifest::MAX_NAME_LEN + 1)).is_err());
    }
}

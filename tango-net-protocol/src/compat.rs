//! What a host knows about its own side of a pairing, resolved against
//! its library: the input to the lobby's compatibility verdict.
//!
//! Lives here, below both the library (which resolves it) and the lobby
//! (which judges it), because neither may depend on the other. Never
//! crosses the wire.

/// Resolved local availability for a pairing of two peers' Settings.
/// Contains neither locks nor ROM bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// We hold the ROM the peer picked — the shadow core runs their
    /// side of the match from it.
    pub remote_rom_available: bool,
    /// Both picks resolve to the same compatibility tag.
    pub matching_tags: bool,
    /// A patch either side picked that isn't installed here yet.
    pub missing_patch: Option<(String, semver::Version)>,
}

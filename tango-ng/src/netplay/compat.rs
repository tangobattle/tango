//! Netplay compatibility check between two peers' Settings packets.
//! Copied from `tango/src/netplay/compat.rs` verbatim (tango-ng's
//! `patch::PatchMap` / `rom::GameRef` line up with what it needs).
//! Both sides must:
//! - have a game_info,
//! - each have the other's chosen game in their `available_games`,
//! - each have the other's chosen patch (if any) in their
//!   `available_patches`,
//! - resolve to the same `netplay_compatibility` tag (the patch
//!   info's tag, or the rom family for unpatched games),
//! - agree on `match_type`.
//!
//! Used by the lobby pane to gate the Ready button + the green
//! "compatible" indicator.

use crate::net::protocol;
use crate::patch::PatchMap;

/// Resolve the netplay_compatibility tag for a `(game, patch)`
/// pair. For patched games it's the patch's
/// `netplay_compatibility` string; for unpatched ones it's the rom
/// family ("bn6", "exe6", etc). Returns None when the patch info
/// references a name + version that isn't in our scanner cache.
pub fn netplay_compatibility(
    game: crate::rom::GameRef,
    patch: Option<(&str, &semver::Version)>,
    patches: &PatchMap,
) -> Option<String> {
    if let Some((name, version)) = patch {
        patches
            .get(name)
            .and_then(|p| p.versions.get(version).map(|v| v.netplay_compatibility.clone()))
    } else {
        Some(game.family_and_variant().0.to_string())
    }
}

/// Same as `netplay_compatibility` but starting from a
/// `protocol::GameInfo` (what we receive from the peer).
pub fn netplay_compatibility_from_game_info(g: &protocol::GameInfo, patches: &PatchMap) -> Option<String> {
    let game = crate::game::find_by_family_and_variant(g.family_and_variant.0.as_str(), g.family_and_variant.1)?;
    netplay_compatibility(game, g.patch.as_ref().map(|p| (p.name.as_str(), &p.version)), patches)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Both sides agree on a netplay-compatible game + patch +
    /// match type. Ready button can go primary.
    Compatible,
    /// One or both sides are missing a game selection.
    MissingGame,
    /// Games + patches resolve but to different netplay_compatibility
    /// tags. Cross-version play not allowed.
    DifferentVersions,
    /// Compatibility tags agree but the picked match types diverge.
    DifferentMatchTypes,
}

/// Are these two peers ready to play together? Mirrors
/// `are_settings_compatible` from the legacy app but returns a
/// structured Verdict so the UI can show the specific reason
/// instead of just "incompatible".
pub fn check(local: &protocol::Settings, remote: &protocol::Settings, patches: &PatchMap) -> Verdict {
    let (Some(local_gi), Some(remote_gi)) = (local.game_info.as_ref(), remote.game_info.as_ref()) else {
        return Verdict::MissingGame;
    };

    let local_tag = netplay_compatibility_from_game_info(local_gi, patches);
    let remote_tag = netplay_compatibility_from_game_info(remote_gi, patches);
    if local_tag.is_none() || remote_tag.is_none() || local_tag != remote_tag {
        return Verdict::DifferentVersions;
    }

    if local.match_type != remote.match_type {
        return Verdict::DifferentMatchTypes;
    }

    Verdict::Compatible
}

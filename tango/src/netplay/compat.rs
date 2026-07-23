//! Netplay compatibility check between two peers' Settings packets.
//!
//! To play, both sides must:
//! - have a game_info,
//! - have the *other's* chosen ROM (the match runs the peer's game here:
//!   the shadow core re-simulates their side from their rom + save),
//! - resolve to the same [`tango_patch::Tag`],
//! - have both sides' patch packages installed,
//! - agree on `match_type`.
//!
//! Possession is checked from our side only — the legacy app exchanged
//! `available_games` / `available_patches` lists over the wire, but the
//! peer runs this same check against *our* game_info, so an un-runnable
//! pairing can't ready up from either end without any lists crossing the
//! wire.
//!
//! # Tags resolve without downloading
//!
//! A patch's compatibility comes from the catalog, which merges what's
//! installed with the repo index — so a peer can turn up using a patch
//! we've never downloaded and we can still tell whether it would match.
//! When it would, the missing package is a [`Verdict::MissingPatch`] the
//! app resolves by fetching it, rather than a dead end.

use crate::library::patch::Catalog;
use tango_net_protocol::control as protocol;

/// Resolve the netplay tag of a `protocol::GameInfo` (what we receive
/// from the peer) against the catalog. `None` when the patch is one the
/// catalog has never heard of — neither installed nor indexed — which
/// reads as "can't vouch for this".
pub fn tag_from_game_info(g: &protocol::GameInfo, catalog: &Catalog) -> Option<tango_patch::Tag> {
    let game =
        crate::library::game::find_by_family_and_variant(g.family_and_variant.0.as_str(), g.family_and_variant.1)?;
    catalog.tag(game, g.patch.as_ref().map(|p| (p.name.as_str(), &p.version)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Both sides agree on a netplay-compatible game + patch + match
    /// type, and everything needed is on disk. Ready button can go
    /// primary.
    Compatible,
    /// One or both sides are missing a game selection.
    MissingGame,
    /// We don't have the peer's game rom, so we couldn't run their side
    /// of the match. Without this gate the failure only surfaces after
    /// both sides commit, as a "remote rom not scanned" error at match
    /// spawn. Unlike a patch, a ROM isn't something we can go get.
    MissingRom,
    /// Everything agrees, but a patch package isn't installed here yet.
    /// Fixable: the app downloads it and the verdict clears itself.
    MissingPatch { name: String, version: semver::Version },
    /// Games + patches resolve but to different tags. Cross-version play
    /// not allowed.
    DifferentVersions,
    /// Tags agree but the picked match types diverge.
    DifferentMatchTypes,
}

/// Are these two peers ready to play together? `roms` is the local ROM
/// scanner's map, for the possession check (see the module docs).
pub fn check(
    local: &protocol::Settings,
    remote: &protocol::Settings,
    roms: &std::collections::HashMap<crate::library::rom::GameRef, Vec<u8>>,
    catalog: &Catalog,
) -> Verdict {
    let (Some(local_gi), Some(remote_gi)) = (local.game_info.as_ref(), remote.game_info.as_ref()) else {
        return Verdict::MissingGame;
    };

    // The match runs the peer's game locally (their patch is applied to
    // our copy of their rom at spawn), so their rom must be scanned. An
    // unknown family/variant reads as "not installed" too.
    let Some(remote_game) = crate::library::game::find_by_family_and_variant(
        remote_gi.family_and_variant.0.as_str(),
        remote_gi.family_and_variant.1,
    ) else {
        return Verdict::MissingRom;
    };
    if !roms.contains_key(&remote_game) {
        return Verdict::MissingRom;
    }

    // Identity before possession-of-patch: there's no point fetching a
    // package for a matchup that wouldn't be playable anyway.
    let local_tag = tag_from_game_info(local_gi, catalog);
    let remote_tag = tag_from_game_info(remote_gi, catalog);
    if local_tag.is_none() || remote_tag.is_none() || local_tag != remote_tag {
        return Verdict::DifferentVersions;
    }

    if let Some(missing) = missing_patch(local, remote, catalog) {
        return missing;
    }

    if local.match_type != remote.match_type {
        return Verdict::DifferentMatchTypes;
    }

    Verdict::Compatible
}

/// The first patch either side needs that isn't installed here. Both
/// sides matter: we apply our own patch to run our game, and the peer's
/// to run theirs in the shadow core.
fn missing_patch(local: &protocol::Settings, remote: &protocol::Settings, catalog: &Catalog) -> Option<Verdict> {
    [local, remote]
        .iter()
        .filter_map(|s| s.game_info.as_ref()?.patch.as_ref())
        .find(|p| !catalog.is_installed(&p.name, &p.version))
        .map(|p| Verdict::MissingPatch {
            name: p.name.clone(),
            version: p.version.clone(),
        })
}

impl Verdict {
    /// Is this a state the app can clear on its own by downloading?
    pub fn fetchable(&self) -> Option<(&str, &semver::Version)> {
        match self {
            Verdict::MissingPatch { name, version } => Some((name.as_str(), version)),
            _ => None,
        }
    }
}

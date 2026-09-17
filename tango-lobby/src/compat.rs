//! Netplay compatibility check between two peers' Settings packets.
//!
//! To play, both sides must:
//! - have a game_info,
//! - have the *other's* chosen ROM (the match runs the peer's game here:
//!   the shadow core re-simulates their side from their rom + save),
//! - resolve to the same [`tango_patch::Tag`],
//! - have both sides' patch packages installed,
//! - be builds that simulate the game the same way (`sim_version`),
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

use tango_net_protocol::control as protocol;

/// Resolved local availability. The host obtains these facts from its catalog.
pub struct Facts {
    pub remote_rom_available: bool,
    pub matching_tags: bool,
    pub missing_patch: Option<(String, semver::Version)>,
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
    /// The same game, but the peer's build simulates it the way an
    /// older one of ours did — its engine support for this game
    /// predates a change here. Not fixable from this end: they have to
    /// update. See `tango_match::Backend::sim_version`.
    SimVersionTooOld,
    /// The same game, simulated the way a build newer than ours does —
    /// this game's engine support has changed since our release, so
    /// we're the ones who have to update.
    SimVersionTooNew,
    /// Tags agree but the picked match types diverge.
    DifferentMatchTypes,
}

/// Are these peers ready to play together, given the host's availability facts?
pub fn check(local: &protocol::Settings, remote: &protocol::Settings, facts: Facts) -> Verdict {
    let (Some(local_gi), Some(remote_gi)) = (local.game_info.as_ref(), remote.game_info.as_ref()) else {
        return Verdict::MissingGame;
    };

    if !facts.remote_rom_available {
        return Verdict::MissingRom;
    }
    if !facts.matching_tags {
        return Verdict::DifferentVersions;
    }

    // Same game, different simulations of it: one build's engine
    // support for this game has moved since the other's. The peer's
    // number is theirs to report — we can't derive it, because what it
    // describes is their build, not their game. Which side is behind is
    // worth saying: only one of the two can act on it.
    match remote_gi.sim_version.cmp(&local_gi.sim_version) {
        std::cmp::Ordering::Less => return Verdict::SimVersionTooOld,
        std::cmp::Ordering::Greater => return Verdict::SimVersionTooNew,
        std::cmp::Ordering::Equal => {}
    }

    if let Some((name, version)) = facts.missing_patch {
        return Verdict::MissingPatch { name, version };
    }

    if local.match_type != remote.match_type {
        return Verdict::DifferentMatchTypes;
    }

    Verdict::Compatible
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

#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> protocol::Settings {
        protocol::Settings {
            nickname: String::new(),
            match_type: (0, 0),
            blind_setup: false,
            game_info: Some(protocol::GameInfo {
                family_and_variant: ("example".into(), 0),
                patch: None,
                sim_version: 3,
            }),
        }
    }
    fn facts() -> Facts {
        Facts {
            remote_rom_available: true,
            matching_tags: true,
            missing_patch: None,
        }
    }
    #[test]
    fn compatibility_uses_resolved_facts_without_a_registry_or_roms() {
        let local = settings();
        let mut remote = settings();
        assert_eq!(check(&local, &remote, facts()), Verdict::Compatible);
        assert_eq!(
            check(
                &local,
                &remote,
                Facts {
                    remote_rom_available: false,
                    ..facts()
                }
            ),
            Verdict::MissingRom
        );
        let missing = Some(("patch".into(), semver::Version::new(1, 0, 0)));
        assert_eq!(
            check(
                &local,
                &remote,
                Facts {
                    matching_tags: false,
                    missing_patch: missing.clone(),
                    ..facts()
                }
            ),
            Verdict::DifferentVersions
        );
        assert!(matches!(
            check(
                &local,
                &remote,
                Facts {
                    missing_patch: missing,
                    ..facts()
                }
            ),
            Verdict::MissingPatch { .. }
        ));
        remote.game_info.as_mut().unwrap().sim_version = 2;
        assert_eq!(check(&local, &remote, facts()), Verdict::SimVersionTooOld);
        remote.game_info.as_mut().unwrap().sim_version = 4;
        assert_eq!(check(&local, &remote, facts()), Verdict::SimVersionTooNew);
        remote.game_info.as_mut().unwrap().sim_version = 3;
        remote.match_type = (1, 0);
        assert_eq!(check(&local, &remote, facts()), Verdict::DifferentMatchTypes);
        remote.game_info = None;
        assert_eq!(check(&local, &remote, facts()), Verdict::MissingGame);
    }
}

//! The follow-up every host runs after anything moves — a report from the
//! connection, a change of pick, a match-type tap — and the match-type
//! policy it applies. Host-agnostic: the host hands over its facts as
//! plain data, and does whatever it does with the patch to fetch.

use tango_net_protocol::control as protocol;

use super::{compat, LobbyState, Phase, State};

impl State {
    /// Bring the lobby in line with the host's current state. Idempotent,
    /// which is what lets a host hang it off every transition rather than
    /// threading it through each one. Outside the lobby it does nothing.
    ///
    /// * Makes sure the peer has our current Settings, built by
    ///   `local_settings` from the lobby's own picks. This is also how
    ///   they get sent on lobby entry, where there is no user action to
    ///   hang it off. Deduped, and a material change drops our commit
    ///   (see [`State::send_local_settings`]).
    /// * Once both sides' Settings are on hand, judges them with the
    ///   host's `facts` and drops our commit if they're no longer
    ///   [`compat::Verdict::Compatible`] — the peer changed their game,
    ///   patch or match type, or a patch we held went away, and the
    ///   commit mustn't outlive the agreement it was based on.
    /// * Returns the patch package to fetch when that is all that stands
    ///   in the way. The verdict resolves from the index, so we know the
    ///   matchup is playable before the package is on this device.
    pub fn reconcile(
        &mut self,
        local_settings: impl FnOnce(&LobbyState) -> protocol::Settings,
        facts: impl FnOnce(&protocol::Settings, &protocol::Settings) -> compat::Facts,
    ) -> Option<(String, semver::Version)> {
        if !matches!(self.phase, Phase::Lobby { .. }) {
            return None;
        }
        let settings = local_settings(&self.lobby);
        self.send_local_settings(settings);
        let verdict = self.verdict(facts)?;
        if verdict != compat::Verdict::Compatible && self.local_ready() {
            self.uncommit();
        }
        verdict
            .fetchable()
            .map(|(name, version)| (name.to_owned(), version.clone()))
    }

    /// The compatibility verdict between the two sides' Settings, once
    /// both are on hand in the lobby. `facts` resolves the host's side
    /// of it from its library.
    pub fn verdict(
        &self,
        facts: impl FnOnce(&protocol::Settings, &protocol::Settings) -> compat::Facts,
    ) -> Option<compat::Verdict> {
        if !matches!(self.phase, Phase::Lobby { .. }) {
            return None;
        }
        let (local, remote) = (self.lobby.local.as_ref()?, self.lobby.remote.as_ref()?);
        Some(compat::check(local, remote, facts(local, remote)))
    }

    /// Default match-type policy for the game the host is bringing:
    /// `family`, whose `match_types` table has one entry per mode giving
    /// its subtype count (mode 1 is Triple).
    ///
    /// * Family just changed (or first selection in this lobby): the
    ///   mode this family was last picked in (`remembered`, from the
    ///   host's config) if the game still offers it, or failing that
    ///   Triple where the game has one, else Single. Triple is what
    ///   people actually play, and both sides have to agree before
    ///   either can ready up.
    /// * Same family, current value out of range for this game: the
    ///   same fallback (the versions of a family can differ).
    /// * Same family, valid value: left alone — a sticky user pick.
    ///
    /// Keyed off [`LobbyState::default_mt_for_family`], so it fires once
    /// per (lobby, family) pair.
    pub fn apply_default_match_type(&mut self, family: &str, match_types: &[usize], remembered: Option<(u8, u8)>) {
        let offered = |(mode, subtype): (u8, u8)| {
            match_types
                .get(mode as usize)
                .is_some_and(|subtypes| (subtype as usize) < *subtypes)
        };
        let family_changed = self.lobby.default_mt_for_family.as_deref() != Some(family);
        if !family_changed && offered(self.lobby.match_type) {
            return;
        }
        // A remembered pick outranks the built-in default; a stale one
        // (the table shrank under a patch) falls through to it.
        self.lobby.match_type = remembered.filter(|&mt| offered(mt)).unwrap_or_else(|| {
            if match_types.get(1).copied().unwrap_or(0) > 0 {
                (1, 0) // Triple
            } else {
                (0, 0) // Single
            }
        });
        self.lobby.default_mt_for_family = Some(family.to_owned());
    }

    /// The user picked a match type while bringing a game of `family`.
    /// Stamps the family as already defaulted, so the default policy
    /// doesn't clobber a pick made before it ever ran (say, before the
    /// lobby came up). The host remembers the pick per family in its
    /// config and follows this with [`State::reconcile`], whose
    /// material-diff check does the unready — so that's deliberately not
    /// done here.
    pub fn pick_match_type(&mut self, family: Option<&str>, match_type: (u8, u8)) {
        self.lobby.match_type = match_type;
        if let Some(family) = family {
            self.lobby.default_mt_for_family = Some(family.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lobby::Command;
    use crate::{Inbound, Incoming, LinkIdent};

    fn settings(match_type: (u8, u8)) -> protocol::Settings {
        protocol::Settings {
            nickname: String::new(),
            match_type,
            blind_setup: false,
            game_info: Some(protocol::GameInfo {
                family_and_variant: ("example".into(), 0),
                patch: None,
                sim_version: 1,
            }),
        }
    }

    fn facts(_: &protocol::Settings, _: &protocol::Settings) -> compat::Facts {
        compat::Facts {
            remote_rom_available: true,
            matching_tags: true,
            missing_patch: None,
        }
    }

    /// A lobby-phase state whose wire sends land in the returned queue.
    fn lobby() -> (State, futures::channel::mpsc::UnboundedReceiver<Command>) {
        let mut state = State::new();
        state.phase = Phase::Lobby {
            ident: LinkIdent::Matchmaking("code".into()),
        };
        let (tx, rx) = futures::channel::mpsc::unbounded();
        state.commands = Some(tx);
        (state, rx)
    }

    fn drain(rx: &mut futures::channel::mpsc::UnboundedReceiver<Command>) -> Vec<Command> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    fn local(lobby: &LobbyState) -> protocol::Settings {
        settings(lobby.match_type)
    }

    #[test]
    fn settings_resend_is_deduped() {
        let (mut state, mut rx) = lobby();
        state.reconcile(local, facts);
        state.reconcile(local, facts);
        let sent = drain(&mut rx);
        assert_eq!(sent.len(), 1);
        assert!(matches!(sent[0], Command::Settings(_)));

        state.pick_match_type(Some("example"), (1, 0));
        state.reconcile(local, facts);
        assert!(matches!(drain(&mut rx).as_slice(), [Command::Settings(_)]));
    }

    #[test]
    fn incompatible_peer_change_uncommits() {
        let (mut state, mut rx) = lobby();
        state.apply(Incoming(Inbound::RemoteSettings(Box::new(settings((0, 0))))));
        assert_eq!(state.reconcile(local, facts), None);
        state.commit(vec![1, 2, 3]);
        assert!(state.local_ready());

        // Still compatible: the commit stands.
        state.reconcile(local, facts);
        assert!(state.local_ready());
        drain(&mut rx);

        // The peer switches match type out from under our commit.
        state.apply(Incoming(Inbound::RemoteSettings(Box::new(settings((1, 0))))));
        state.reconcile(local, facts);
        assert!(!state.local_ready());
        assert!(matches!(drain(&mut rx).as_slice(), [Command::Uncommit]));
    }

    #[test]
    fn missing_patch_is_returned_for_fetching() {
        let (mut state, _rx) = lobby();
        state.apply(Incoming(Inbound::RemoteSettings(Box::new(settings((0, 0))))));
        let missing = ("patch".to_owned(), semver::Version::new(1, 0, 0));
        let fetch = state.reconcile(local, |_, _| compat::Facts {
            missing_patch: Some(missing.clone()),
            ..facts(&settings((0, 0)), &settings((0, 0)))
        });
        assert_eq!(fetch, Some(missing));
    }

    #[test]
    fn default_match_type_prefers_memory_then_triple() {
        let mut state = State::new();
        state.apply_default_match_type("a", &[1, 1], None);
        assert_eq!(state.lobby.match_type, (1, 0));
        // A pick within the family sticks.
        state.pick_match_type(Some("a"), (0, 0));
        state.apply_default_match_type("a", &[1, 1], None);
        assert_eq!(state.lobby.match_type, (0, 0));
        // A family change re-defaults: memory first, if still offered.
        state.apply_default_match_type("b", &[1, 1], Some((0, 0)));
        assert_eq!(state.lobby.match_type, (0, 0));
        state.apply_default_match_type("c", &[1], Some((1, 0)));
        assert_eq!(state.lobby.match_type, (0, 0));
    }
}

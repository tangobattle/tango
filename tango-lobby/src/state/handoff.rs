//! The handoff from a ready lobby to a live match: draining the
//! connection into a [`PreMatchData`], and settling the host's session
//! build against the attempt it was started for.

use crate::handshake::{LocalReady, RemoteReady};
use crate::*;

/// The attempt a handoff belongs to, issued by [`State::take_pre_match`].
///
/// Leaving the lobby is always allowed, even while the host is still
/// building the match, and starts a new attempt. A build that resolves
/// under an older ticket belongs to a lobby the user already left and
/// must not be installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoffTicket(u64);

impl State {
    /// Drain everything the PvP session needs to take over the data
    /// channel, with the [`HandoffTicket`] its build settles against.
    /// Returns `None` if either we're not at the handoff point yet, or
    /// it's already been drained. After this call the netplay subsystem
    /// retains no live handles — the cancellation token fires (which
    /// tears down the lobby pump), and the host owns sender / receiver /
    /// peer_conn / negotiated state.
    ///
    /// `phase` and `lobby` are deliberately NOT cleared here — the lobby
    /// UI keeps rendering its post-ready snapshot while the host builds
    /// the live session in the background, so the user doesn't see the
    /// bottom strip flash back to the singleplayer chrome. The host
    /// reports the build's outcome to [`complete_handoff`], which clears
    /// the snapshot or parks the failure in the lobby's sticky Failed
    /// status.
    ///
    /// [`complete_handoff`]: State::complete_handoff
    pub fn take_pre_match(&mut self) -> Option<(HandoffTicket, PreMatchData)> {
        if !(self.handshake.local.match_ready() && self.handshake.remote.start_match()) {
            return None;
        }
        let handles = self.conn.take()?;
        // Drain our commit off the ladder; the `HandedOff` rung keeps
        // the lobby chrome rendering ready-state until the build settles.
        let local_commit = match std::mem::replace(&mut self.handshake.local, LocalReady::HandedOff) {
            LocalReady::StartMatchSent(commit) => commit,
            // Already drained (match_ready() also covers HandedOff) —
            // but conn.take() above already returned None in that case.
            _ => return None,
        };
        let RemoteReady::Committed {
            chunks: remote_chunks, ..
        } = &self.handshake.remote
        else {
            return None;
        };
        let local_settings = self.lobby.local.clone()?;
        let remote_settings = self.lobby.remote.clone()?;
        // Decompress + decode peer's NegotiatedState — we already
        // verified its hash in maybe_finish_handshake; this is just
        // to recover the nonce + save_data.
        let peer_state_bytes = match zstd::stream::decode_all(std::io::Cursor::new(remote_chunks)) {
            Ok(b) => b,
            Err(e) => {
                return self.fail_handoff(Error::DecodePeerState {
                    stage: "zstd decode",
                    message: e.to_string(),
                })
            }
        };
        let peer_state = match tango_net_protocol::control::NegotiatedState::deserialize(&peer_state_bytes) {
            Ok(s) => s,
            Err(e) => {
                return self.fail_handoff(Error::DecodePeerState {
                    stage: "decode peer state",
                    message: e.to_string(),
                })
            }
        };
        // Direct codes carry no remote-discoverable identity, so the
        // replay metadata's `link_code` slot is left empty for them — the
        // replay filename and view substitute their own placeholder.
        // Matchmaking codes round-trip verbatim so a recorded match can be
        // cross-referenced with the matchmaking-server logs.
        let link_code = match &self.phase {
            Phase::Lobby { ident } => ident.matchmaking_code().unwrap_or_default().to_owned(),
            _ => return None,
        };
        // RNG seed for the in-match shared RNG (XOR of the two nonces)
        // and the match clock (the offerer's commit-time wall clock, so
        // both peers pin every core's cart RTC to the same instant —
        // see PreMatchData::match_ts). Both are determinism-critical,
        // so the constructions live in the shared protocol crate.
        let rng_seed = tango_net_protocol::derive::derive_rng_seed(&local_commit.state.nonce, &peer_state.nonce);
        let match_ts =
            tango_net_protocol::derive::pick_match_ts(handles.is_offerer, local_commit.state.ts, peer_state.ts);
        // Cancel the pump so it returns ownership of the receiver via the
        // handles' oneshot. It sends the receiver down it on cancel-exit;
        // `Link::bring_up` awaits it.
        self.cancel.cancel();
        // Build the mid-match reconnect recipe. The direct path carries its
        // recipe on ConnectionHandles; the matchmaking path combines the params
        // stashed at connect time with a session_id derived from the shared RNG
        // seed (now known), so both peers re-rendezvous on the same secret id.
        let recipe = if let Some(role) = handles.reconnect {
            Some(ReconnectRecipe::Direct(role))
        } else {
            self.matchmaking_reconnect
                .take()
                .map(|mm| ReconnectRecipe::Matchmaking {
                    endpoint: mm.endpoint,
                    session_id: tango_net::link::derive_reconnect_session_id(
                        &rng_seed,
                        &handles.local_dtls_fingerprint,
                        &handles.peer_dtls_fingerprint,
                    ),
                    use_relay: mm.use_relay,
                })
        };
        let pre_match = PreMatchData {
            link_parts: LinkParts {
                control_sender: handles.sender,
                control_receiver_rx: handles.post_lobby_rx,
                in_match_sender: handles.in_match_sender,
                in_match_receiver: handles.in_match_receiver,
                peer_conn: handles.peer_conn,
                recipe,
                rng_seed,
            },
            terms: tango_net::handoff::MatchTerms {
                is_offerer: handles.is_offerer,
                rng_seed,
                match_ts,
                local_save_data: local_commit.state.save_data,
                remote_save_data: peer_state.save_data,
                local_settings,
                remote_settings,
                link_code,
                match_type: self.lobby.match_type,
            },
        };
        Some((HandoffTicket(self.session_id), pre_match))
    }

    /// A handoff-time decode failure: the peer's revealed state won't parse
    /// even though its hash matched the commitment (checked back in
    /// `maybe_finish_handshake`). By this point `take_pre_match` has already
    /// consumed the connection handles, so the session can't proceed — tear it
    /// down into a visible Failed banner. Returning a bare `None` instead
    /// would read as "already drained" to the host, leaving the lobby stuck on
    /// its "Starting match…" chrome with no error.
    fn fail_handoff<T>(&mut self, error: Error) -> Option<T> {
        self.cancel_and_renew();
        self.phase = Phase::Failed { error };
        None
    }

    /// Whether `ticket`'s attempt is still the live one. A host building
    /// the match can check this between steps to stop early once the
    /// user has left the lobby.
    pub fn is_current(&self, ticket: HandoffTicket) -> bool {
        ticket.0 == self.session_id
    }

    /// Settle the session build `ticket` started. Returns the built
    /// session when the host should install it: the build succeeded for
    /// the attempt still in the lobby, whose snapshot is cleared now that
    /// the match takes over the screen.
    ///
    /// A failed build parks the failure in the same sticky Failed status
    /// every other netplay failure lands in — the lobby chrome is still
    /// on screen at this point ([`handoff_pending`] kept it up while the
    /// session was built), so the failure shows in the status line the
    /// user is already looking at. A build for an abandoned attempt is
    /// dropped here, whatever its outcome: dropping a built session
    /// winds its workers down.
    ///
    /// [`handoff_pending`]: State::handoff_pending
    pub fn complete_handoff<T>(&mut self, ticket: HandoffTicket, result: Result<T, String>) -> Option<T> {
        if !self.is_current(ticket) {
            if let Err(e) = result {
                log::info!("netplay: match build failed after the lobby was left: {e}");
            }
            return None;
        }
        match result {
            Ok(session) => {
                self.phase = Phase::Idle;
                self.lobby = LobbyState::default();
                self.handshake = Default::default();
                Some(session)
            }
            Err(e) => {
                log::error!("netplay: match build failed: {e}");
                self.fail_keeping_lobby(Error::SessionBuild(e));
                None
            }
        }
    }

    /// True once both sides have exchanged StartMatch and the
    /// connection handles have been drained into a PreMatchData,
    /// but before [`complete_handoff`](State::complete_handoff) settles
    /// the build. The lobby UI uses this to lock the committed controls
    /// (save / game selection, match type, blind setup) and show a
    /// "Starting match…" placeholder while the session is built —
    /// leaving the lobby stays possible throughout (the ticket tells
    /// the abandoned build apart when it lands).
    pub fn handoff_pending(&self) -> bool {
        matches!(self.handshake.local, LocalReady::HandedOff) && self.handshake.remote.start_match()
    }
}

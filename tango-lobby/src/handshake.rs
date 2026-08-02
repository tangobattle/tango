//! The lobby ready/commitment exchange: both sides commit to a hash
//! of their (zstd'd) NegotiatedState, stream the state across in
//! chunks once both have committed, verify the reveal against the
//! commitment, and exchange StartMatch. Commit-then-reveal keeps
//! either side from picking their save in response to the opponent's.
//!
//! Each peer's progress is one explicit ladder ([`LocalReady`] for
//! ours, [`RemoteReady`] for what we've observed of theirs) instead of
//! loose booleans, so "how ready are we" has a single source of truth
//! and the transitions live next to the states they connect. The UI
//! reads a derived [`ReadyView`] projection.

use subtle::ConstantTimeEq;

use tango_net_protocol::control::make_commitment;

use super::{Command, Error, Event, Phase, State};

#[derive(Clone)]
pub(super) struct LocalCommit {
    /// Pre-`StartMatch` view of our negotiated state. Used to
    /// (a) derive the post-handshake RNG seed (`local.nonce XOR
    /// remote.nonce`) and (b) pass our save bytes into the PvP
    /// session once the match starts.
    pub(super) state: tango_net_protocol::control::NegotiatedState,
    /// `zstd(bincode(state))` — the bytes we hash for our
    /// commitment and slice into the Chunk packets.
    pub(super) compressed: Vec<u8>,
}

/// Our side of the ready ladder. Strictly monotone within one commit
/// pairing — `NotReady → Committed → ChunksSent → StartMatchSent →
/// HandedOff` — and reset back down by Uncommit / material settings
/// changes / session boundaries (`StartMatchSent` also regresses one
/// rung when the peer's reveal is voided; see
/// [`revoke_start_match`](Self::revoke_start_match)).
#[derive(Default)]
pub(super) enum LocalReady {
    #[default]
    NotReady,
    /// We sent Commit — nonce/save picked, commitment on the wire.
    Committed(LocalCommit),
    /// Both sides have committed and our chunk-stream task is spawned.
    /// The rung doubles as the spawn guard: the kick only fires from
    /// `Committed`, so it can't double-send within one pairing.
    ChunksSent(LocalCommit),
    /// We verified the peer's reveal against their commitment and sent
    /// StartMatch — our half of the handoff condition.
    StartMatchSent(LocalCommit),
    /// `take_pre_match` drained the commit into the PvP handoff. The
    /// lobby chrome keeps rendering its ready-state snapshot until
    /// `finish_handoff` resets the ladder.
    HandedOff,
}

impl LocalReady {
    /// "You: ready" — we've committed (any rung past `NotReady`).
    pub(super) fn is_ready(&self) -> bool {
        !matches!(self, LocalReady::NotReady)
    }

    /// Our half of the handoff condition: peer's reveal verified +
    /// StartMatch sent (or already handed off).
    pub(super) fn match_ready(&self) -> bool {
        matches!(self, LocalReady::StartMatchSent(_) | LocalReady::HandedOff)
    }

    /// Undo the StartMatch rung when the peer's reveal it was
    /// predicated on is voided (their Uncommit, their re-Commit, or our
    /// blind-setup flip dropping their commit). Our own commit + sent
    /// chunks stay valid, so this only steps back to `ChunksSent` —
    /// where the replacement reveal completing re-verifies and re-sends.
    pub(super) fn revoke_start_match(&mut self) {
        if matches!(self, LocalReady::StartMatchSent(_)) {
            let LocalReady::StartMatchSent(commit) = std::mem::take(self) else {
                unreachable!();
            };
            *self = LocalReady::ChunksSent(commit);
        }
    }
}

/// The peer's side of the ladder, as observed from received packets.
/// Their reveal progress (`chunks` / `revealed`) and their StartMatch
/// are carried on the `Committed` rung they belong to, so voiding the
/// commitment (Uncommit) drops everything derived from it at once.
#[derive(Default)]
pub(super) enum RemoteReady {
    #[default]
    NotReady,
    /// Peer's Commit arrived.
    Committed {
        commitment: [u8; 16],
        /// Total reveal length their ChunkStart announced, once it
        /// has arrived. Chunks before it are strays and get dropped.
        expected: Option<u64>,
        /// Their reveal, accumulating until `expected` bytes are here.
        chunks: Vec<u8>,
        /// The announced length fully arrived — `chunks` is the
        /// complete reveal. Latched (not just an event) so a
        /// re-commit on our side can re-verify the held reveal
        /// without the peer re-sending it.
        revealed: bool,
        /// Peer sent StartMatch — they verified *our* reveal. Cleared
        /// again if our own commitment changes under it, since that
        /// makes them revoke it on their side
        /// ([`RemoteReady::revoke_start_match`]).
        start_match: bool,
    },
}

impl RemoteReady {
    /// "Opponent: ready" — their commitment is on hand.
    pub(super) fn is_ready(&self) -> bool {
        matches!(self, RemoteReady::Committed { .. })
    }

    pub(super) fn start_match(&self) -> bool {
        matches!(self, RemoteReady::Committed { start_match: true, .. })
    }

    /// Forget that the peer sent StartMatch, because our own commitment
    /// just changed under it (Commit or Uncommit). Their StartMatch was
    /// their half of a pairing our packet supersedes: the moment it
    /// lands they revoke it too — [`LocalReady::revoke_start_match`] on
    /// our Uncommit, the same on our Commit — and re-send once they've
    /// verified our new reveal. Keeping the old one latched would let a
    /// re-Ready mint an [`Event::MatchReady`](super::Event::MatchReady)
    /// out of a StartMatch the peer no longer stands behind, and we'd
    /// hand off into a match they aren't in.
    ///
    /// Their commitment and reveal are untouched — those they don't
    /// re-send, and the latched reveal is what lets a re-commit verify
    /// without another round trip.
    pub(super) fn revoke_start_match(&mut self) {
        if let RemoteReady::Committed { start_match, .. } = self {
            *start_match = false;
        }
    }
}

/// The ready/commitment exchange between the two lobby peers. Bundled
/// out of [`State`] because the two ladders move as a unit: every
/// session boundary (`State::cancel_and_renew`, peer-disconnect,
/// handoff finish) wipes them together via `Handshake::default()`.
#[derive(Default)]
pub(super) struct Handshake {
    pub(super) local: LocalReady,
    pub(super) remote: RemoteReady,
}

/// UI projection of the two ladders, derived per frame (and frozen
/// into the lobby's exit snapshot so the band's exit animation renders
/// the last live ready-state). Read-only — the ladders in
/// [`Handshake`] are the source of truth.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadyView {
    /// We've committed ("you: ready").
    pub local_ready: bool,
    /// Peer has committed ("opponent: ready").
    pub remote_ready: bool,
    /// We verified their reveal + sent StartMatch — the Ready button
    /// flips to its "match starting" state. (The peer's StartMatch
    /// isn't projected: the UI never renders it — it only feeds the
    /// handoff gate, which the ladders own internally.)
    pub match_ready: bool,
}

impl State {
    /// Derived ready-state for the UI. See [`ReadyView`].
    pub fn ready_view(&self) -> ReadyView {
        ReadyView {
            local_ready: self.handshake.local.is_ready(),
            remote_ready: self.handshake.remote.is_ready(),
            match_ready: self.handshake.local.match_ready(),
        }
    }

    /// Whether we've committed — the host's re-commit / uncommit
    /// triggers key off this.
    pub fn local_ready(&self) -> bool {
        self.handshake.local.is_ready()
    }

    /// The user un-pressed Ready. Drops the local commitment (ladder back
    /// to `NotReady`) and, if we'd already sent a Commit, fires an
    /// Uncommit so the peer doesn't sit waiting for our chunks.
    pub fn uncommit(&mut self) {
        self.invalidate_local_commit();
    }

    pub(super) fn invalidate_local_commit(&mut self) {
        let had_commit = self.handshake.local.is_ready();
        self.handshake.local = LocalReady::NotReady;
        if had_commit {
            // Our Uncommit walks the peer back down their own ladder —
            // mirror the half of that we can see (see
            // [`RemoteReady::revoke_start_match`]).
            self.handshake.remote.revoke_start_match();
            self.send(Command::Uncommit);
        }
    }

    /// The user pressed Ready. Builds a NegotiatedState from a fresh
    /// nonce + the local save's SRAM, zstd-compresses it, hashes it for
    /// the commitment and sends the Commit packet — then kicks the reveal
    /// if the peer has already committed, and re-verifies their reveal if
    /// it's already complete (a re-commit after our Uncommit: the peer
    /// won't re-send what we already hold).
    pub fn commit(&mut self, save_sram: Vec<u8>) -> Option<Event> {
        if !matches!(self.phase, Phase::Lobby { .. }) {
            return None;
        }
        let mut nonce = [0u8; 16];
        rand::Rng::fill(&mut rand::thread_rng(), &mut nonce);
        let state = tango_net_protocol::control::NegotiatedState {
            nonce,
            ts: web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            save_data: save_sram,
        };
        let bin = match state.serialize() {
            Ok(b) => b,
            Err(e) => {
                self.fail(Error::Other(format!("serialize state: {e}")));
                return None;
            }
        };
        let compressed = match zstd::stream::encode_all(std::io::Cursor::new(&bin), 3) {
            Ok(c) => c,
            Err(e) => {
                self.fail(Error::Other(format!("zstd encode: {e}")));
                return None;
            }
        };
        let commitment = make_commitment(&compressed);
        self.handshake.local = LocalReady::Committed(LocalCommit { state, compressed });
        // A fresh commitment supersedes whatever pairing the peer's
        // StartMatch belonged to; they'll send another once they've
        // verified the reveal that follows this Commit (see
        // [`RemoteReady::revoke_start_match`]).
        self.handshake.remote.revoke_start_match();
        self.send(Command::Commit(commitment));
        self.maybe_kick_reveal();
        self.maybe_finish_handshake()
    }

    /// If both sides have committed and we haven't sent our reveal yet
    /// (local ladder at `Committed`), queue the reveal stream and advance
    /// to `ChunksSent`. Idempotent: called from both the local commit and
    /// the peer's, and fires exactly once per commit pairing — the rung
    /// itself is the guard.
    pub(super) fn maybe_kick_reveal(&mut self) {
        if !matches!(self.handshake.local, LocalReady::Committed(_)) || !self.handshake.remote.is_ready() {
            return;
        }
        let LocalReady::Committed(commit) = std::mem::take(&mut self.handshake.local) else {
            unreachable!();
        };
        self.send(Command::Reveal(commit.compressed.clone()));
        self.handshake.local = LocalReady::ChunksSent(commit);
    }

    /// If the peer's reveal is complete and we've sent ours, verify theirs
    /// against their commitment, advance to `StartMatchSent`, and fire
    /// StartMatch. No-op from any other rung pairing: before their reveal
    /// completes it just waits; after `StartMatchSent` it's a duplicate
    /// trip; before our commit the reveal is held (`revealed` stays
    /// latched) until we commit.
    pub(super) fn maybe_finish_handshake(&mut self) -> Option<Event> {
        let RemoteReady::Committed {
            commitment,
            chunks,
            revealed: true,
            ..
        } = &self.handshake.remote
        else {
            return None;
        };
        if !matches!(self.handshake.local, LocalReady::ChunksSent(_)) {
            return None;
        }
        let actual = make_commitment(chunks);
        if !bool::from(actual.ct_eq(commitment)) {
            self.fail(Error::Other("peer commitment mismatch".to_string()));
            return None;
        }
        // Decompress + decode the peer's NegotiatedState. We don't use it
        // for anything until the PvP session handoff, but verifying that
        // it parses now means we catch wire-format breakage before the
        // user hits Play.
        let peer_state_bytes = match zstd::stream::decode_all(std::io::Cursor::new(chunks)) {
            Ok(b) => b,
            Err(e) => {
                self.fail(Error::Other(format!("zstd decode: {e}")));
                return None;
            }
        };
        if let Err(e) = tango_net_protocol::control::NegotiatedState::deserialize(&peer_state_bytes) {
            self.fail(Error::Other(format!("decode peer state: {e}")));
            return None;
        }
        let LocalReady::ChunksSent(commit) = std::mem::take(&mut self.handshake.local) else {
            unreachable!();
        };
        self.handshake.local = LocalReady::StartMatchSent(commit);
        self.send(Command::StartMatch);
        self.match_ready_event()
    }

    /// Both sides have sent + received StartMatch — the host's cue to spin
    /// up the live match. `None` until both halves are present.
    pub(super) fn match_ready_event(&self) -> Option<Event> {
        (self.handshake.local.match_ready() && self.handshake.remote.start_match()).then_some(Event::MatchReady)
    }
}

/// Both tests drive the two ladders through a supersession — one side's
/// commitment being replaced while the other's StartMatch is already in
/// hand — because that's where the handoff can go asymmetric: mint a
/// `MatchReady` from a StartMatch the peer has since revoked and we walk
/// into a match they aren't in, leaving them in the lobby with no packet
/// left that could get them out.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Inbound, Incoming, LinkIdent, Phase};

    /// A State parked where the ready exchange happens. No connection
    /// behind it: the outbound commands drop on the floor (see
    /// [`State::send`]), which is exactly what a test wants — the ladders
    /// are the subject.
    fn lobby() -> State {
        State {
            phase: Phase::Lobby {
                ident: LinkIdent::Matchmaking("test".to_string()),
            },
            ..State::new()
        }
    }

    /// A peer's reveal and the commitment that goes with it, built the
    /// same way [`State::commit`] builds ours.
    fn peer_reveal(nonce: u8) -> ([u8; 16], Vec<u8>) {
        let state = tango_net_protocol::control::NegotiatedState {
            nonce: [nonce; 16],
            ts: 1,
            save_data: vec![0xab; 64],
        };
        let compressed = zstd::stream::encode_all(std::io::Cursor::new(state.serialize().unwrap()), 3).unwrap();
        (make_commitment(&compressed), compressed)
    }

    /// Deliver a whole reveal in one chunk — the split is a sender-side
    /// concern, and the accumulator only counts bytes.
    fn reveal(state: &mut State, compressed: &[u8]) -> Option<Event> {
        state.apply(Incoming(Inbound::RemoteChunkStart(compressed.len() as u64)));
        state.apply(Incoming(Inbound::RemoteChunk(compressed.to_vec())))
    }

    /// Our Uncommit makes the peer revoke the StartMatch they'd already
    /// sent, so a later re-Ready must wait for a fresh one rather than
    /// starting on the stale flag.
    #[test]
    fn re_ready_waits_for_a_fresh_start_match() {
        let (commitment, compressed) = peer_reveal(7);
        let mut state = lobby();

        assert!(state.apply(Incoming(Inbound::RemoteCommit(commitment))).is_none());
        assert!(state.commit(vec![1, 2, 3]).is_none());
        // Their StartMatch lands ahead of their reveal finishing.
        assert!(state.apply(Incoming(Inbound::RemoteStartMatch)).is_none());

        // We drop out of ready before verifying them; the rest of their
        // reveal arrives anyway and sits latched.
        state.uncommit();
        assert!(reveal(&mut state, &compressed).is_none());

        // Ready again: their latched reveal verifies without a re-send,
        // so our StartMatch goes back out — but theirs was for the
        // pairing we just replaced, and the match waits on the new one.
        assert!(state.commit(vec![1, 2, 3]).is_none());
        assert!(state.ready_view().match_ready);
        assert!(matches!(
            state.apply(Incoming(Inbound::RemoteStartMatch)),
            Some(Event::MatchReady)
        ));
    }

    /// The mirror: their re-Commit supersedes the reveal our StartMatch
    /// was predicated on, so we re-verify the replacement before the
    /// match can start — and never hand off on half-arrived chunks.
    #[test]
    fn peer_recommit_re_verifies_before_starting() {
        let (first, first_reveal) = peer_reveal(7);
        let mut state = lobby();

        assert!(state.apply(Incoming(Inbound::RemoteCommit(first))).is_none());
        assert!(state.commit(vec![1, 2, 3]).is_none());
        assert!(reveal(&mut state, &first_reveal).is_none());
        assert!(state.ready_view().match_ready);

        // They re-commit (an edited save, say). Our StartMatch regresses
        // with the reveal it was for.
        let (second, second_reveal) = peer_reveal(9);
        assert!(state.apply(Incoming(Inbound::RemoteCommit(second))).is_none());
        assert!(!state.ready_view().match_ready);

        // Their StartMatch for the new pairing alone doesn't start it:
        // the replacement reveal has to land and verify first.
        assert!(state.apply(Incoming(Inbound::RemoteStartMatch)).is_none());
        assert!(matches!(reveal(&mut state, &second_reveal), Some(Event::MatchReady)));
    }
}

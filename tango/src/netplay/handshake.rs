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

use crate::net::protocol::make_commitment;

use super::{map_async_err, AsyncError, Message, Phase, State};

#[derive(Clone)]
pub(super) struct LocalCommit {
    /// Pre-`StartMatch` view of our negotiated state. Used to
    /// (a) derive the post-handshake RNG seed (`local.nonce XOR
    /// remote.nonce`) and (b) pass our save bytes into the PvP
    /// session once the match starts.
    pub(super) state: crate::net::protocol::NegotiatedState,
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
    /// predicated on is voided (their Uncommit, or our blind-setup
    /// flip dropping their commit). Our own commit + sent chunks stay
    /// valid, so this only steps back to `ChunksSent`.
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
        /// Their reveal, accumulating until the empty-sentinel Chunk.
        chunks: Vec<u8>,
        /// The sentinel arrived — `chunks` is the complete reveal.
        /// Latched (not just an event) so a re-commit on our side can
        /// re-verify the held reveal without the peer re-sending it.
        revealed: bool,
        /// Peer sent StartMatch — they verified *our* reveal.
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

    /// Whether we've committed — the App's re-commit / uncommit
    /// triggers key off this.
    pub fn local_ready(&self) -> bool {
        self.handshake.local.is_ready()
    }

    /// Drop the local commitment (ladder back to `NotReady`).
    /// If we had previously sent a Commit, also fires an Uncommit
    /// packet so the peer doesn't sit waiting for our chunks.
    pub(super) fn invalidate_local_commit(&mut self) -> iced::Task<Message> {
        let had_commit = self.handshake.local.is_ready();
        self.handshake.local = LocalReady::NotReady;
        if !had_commit {
            return iced::Task::none();
        }
        let Some(sender) = self.conn.as_ref().map(|c| c.sender.clone()) else {
            return iced::Task::none();
        };
        iced::Task::perform(
            async move {
                sender
                    .lock()
                    .await
                    .send_uncommit()
                    .await
                    .map_err(|e| super::Error::Other(format!("send_uncommit: {e}")))
            },
            |r| match r {
                Ok(()) => Message::WireOpDone,
                Err(e) => Message::Failed(e),
            },
        )
    }

    /// Build a NegotiatedState from a fresh nonce + the local
    /// save's SRAM, zstd-compress it, hash it for the commitment,
    /// send the Commit packet, then kick the chunk exchange if
    /// the peer has already committed — and re-verify their reveal
    /// if it's already complete (a re-commit after our Uncommit:
    /// the peer won't re-send what we already hold).
    pub(super) fn commit_local(&mut self, save_sram: Vec<u8>) -> iced::Task<Message> {
        if !matches!(self.phase, Phase::Lobby { .. }) {
            return iced::Task::none();
        }
        let Some(sender) = self.conn.as_ref().map(|c| c.sender.clone()) else {
            return iced::Task::none();
        };
        let mut nonce = [0u8; 16];
        rand::Rng::fill(&mut rand::thread_rng(), &mut nonce);
        let state = crate::net::protocol::NegotiatedState {
            nonce,
            ts: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            save_data: save_sram,
        };
        let bin = match state.serialize() {
            Ok(b) => b,
            Err(e) => {
                return iced::Task::done(Message::Failed(super::Error::Other(format!("serialize state: {e}"))));
            }
        };
        let compressed = match zstd::stream::encode_all(std::io::Cursor::new(&bin), 3) {
            Ok(c) => c,
            Err(e) => {
                return iced::Task::done(Message::Failed(super::Error::Other(format!("zstd encode: {e}"))));
            }
        };
        let commitment = make_commitment(&compressed);
        self.handshake.local = LocalReady::Committed(LocalCommit { state, compressed });

        let send_commit = iced::Task::perform(
            async move {
                sender
                    .lock()
                    .await
                    .send_commit(commitment)
                    .await
                    .map_err(|e| super::Error::Other(format!("send_commit: {e}")))
            },
            |r| match r {
                Ok(()) => Message::WireOpDone,
                Err(e) => Message::Failed(e),
            },
        );
        iced::Task::batch([send_commit, self.maybe_kick_chunk_exchange(), self.maybe_finish_handshake()])
    }

    /// If both sides have committed and we haven't sent our
    /// chunks yet (local ladder at `Committed`), spawn the
    /// chunk-streaming task and advance to `ChunksSent`. Idempotent:
    /// called from both Commit and RemoteCommit handlers, fires
    /// the task exactly once per commit pairing.
    pub(super) fn maybe_kick_chunk_exchange(&mut self) -> iced::Task<Message> {
        if !matches!(self.handshake.local, LocalReady::Committed(_)) || !self.handshake.remote.is_ready() {
            return iced::Task::none();
        }
        let Some(sender) = self.conn.as_ref().map(|c| c.sender.clone()) else {
            return iced::Task::none();
        };
        let LocalReady::Committed(commit) = std::mem::take(&mut self.handshake.local) else {
            unreachable!();
        };
        let compressed = commit.compressed.clone();
        self.handshake.local = LocalReady::ChunksSent(commit);
        let cancel = self.cancel.clone();
        iced::Task::perform(
            async move {
                // bincode-framed Packet caps at 64 KB; 32 KB
                // payload leaves room for the discriminant +
                // length prefix.
                const CHUNK_SIZE: usize = 32 * 1024;
                for chunk in compressed.chunks(CHUNK_SIZE) {
                    let buf = chunk.to_vec();
                    let sender = sender.clone();
                    let result: std::io::Result<()> = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => return Err(AsyncError::Cancelled),
                        r = async move { sender.lock().await.send_chunk(buf).await } => r,
                    };
                    result.map_err(|e| AsyncError::Failed(super::Error::Other(format!("send_chunk: {e}"))))?;
                }
                // Empty sentinel = end-of-stream.
                sender
                    .lock()
                    .await
                    .send_chunk(Vec::new())
                    .await
                    .map_err(|e| AsyncError::Failed(super::Error::Other(format!("send_chunk-end: {e}"))))?;
                Ok::<(), AsyncError>(())
            },
            |r| match r {
                Ok(()) => Message::WireOpDone,
                Err(e) => map_async_err(e),
            },
        )
    }

    /// If the peer's reveal is complete and we've sent our chunks,
    /// verify their reveal against their commitment, advance to
    /// `StartMatchSent`, and fire StartMatch. No-op from any other
    /// rung pairing: before their sentinel it just waits; after
    /// `StartMatchSent` it's a duplicate trip; before our commit the
    /// reveal is held (`revealed` stays latched) until we commit.
    pub(super) fn maybe_finish_handshake(&mut self) -> iced::Task<Message> {
        let RemoteReady::Committed {
            commitment,
            chunks,
            revealed: true,
            ..
        } = &self.handshake.remote
        else {
            return iced::Task::none();
        };
        if !matches!(self.handshake.local, LocalReady::ChunksSent(_)) {
            return iced::Task::none();
        }
        let actual = make_commitment(chunks);
        if !bool::from(actual.ct_eq(commitment)) {
            return iced::Task::done(Message::Failed(super::Error::Other(
                "peer commitment mismatch".to_string(),
            )));
        }
        // Decompress + decode the peer's NegotiatedState. We
        // don't use it for anything until the PvP session
        // handoff, but verifying that it parses now means we
        // catch wire-format breakage before the user hits Play.
        let peer_state_bytes = match zstd::stream::decode_all(std::io::Cursor::new(chunks)) {
            Ok(b) => b,
            Err(e) => {
                return iced::Task::done(Message::Failed(super::Error::Other(format!("zstd decode: {e}"))));
            }
        };
        if let Err(e) = crate::net::protocol::NegotiatedState::deserialize(&peer_state_bytes) {
            return iced::Task::done(Message::Failed(super::Error::Other(format!("decode peer state: {e}"))));
        }
        let LocalReady::ChunksSent(commit) = std::mem::take(&mut self.handshake.local) else {
            unreachable!();
        };
        self.handshake.local = LocalReady::StartMatchSent(commit);

        let Some(sender) = self.conn.as_ref().map(|c| c.sender.clone()) else {
            return iced::Task::none();
        };
        let send_sm = iced::Task::perform(
            async move {
                sender
                    .lock()
                    .await
                    .send_start_match()
                    .await
                    .map_err(|e| super::Error::Other(format!("send_start_match: {e}")))
            },
            |r| match r {
                Ok(()) => Message::WireOpDone,
                Err(e) => Message::Failed(e),
            },
        );
        iced::Task::batch([send_sm, self.maybe_signal_pvp_handoff()])
    }

    /// Both sides have sent + received StartMatch — emit the
    /// signal the App listens for to spin up the live match.
    /// No-op until both halves are present.
    pub(super) fn maybe_signal_pvp_handoff(&mut self) -> iced::Task<Message> {
        if self.handshake.local.match_ready() && self.handshake.remote.start_match() {
            iced::Task::done(Message::MatchHandoffReady)
        } else {
            iced::Task::none()
        }
    }
}

//! The lobby ready/commitment exchange: both sides commit to a hash
//! of their (zstd'd) NegotiatedState, stream the state across in
//! chunks once both have committed, verify the reveal against the
//! commitment, and exchange StartMatch. Commit-then-reveal keeps
//! either side from picking their save in response to the opponent's.
//!
//! Copied from `tango/src/netplay/handshake.rs`, transformed: handlers
//! return `()`, wire sends go through `State::perform` (runtime spawn +
//! event-channel send) instead of `iced::Task::perform`, and immediate
//! `Task::done` dispatches become `State::emit` (applied next tick).
//! The commitment logic — zstd level 3, 32KB chunks, empty-chunk EOS
//! sentinel, constant-time compare — is verbatim.

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

/// The ready/commitment exchange between the two lobby peers. Bundled
/// out of [`State`] because the four fields move as a unit: every
/// session boundary ([`State::cancel_and_renew`], peer-disconnect,
/// handoff finish) wipes them together.
#[derive(Default)]
pub(super) struct Handshake {
    /// Local ready handshake data: the random nonce we picked, the
    /// zstd-compressed serialized NegotiatedState we committed to, and
    /// the commitment we sent. Cleared on Uncommit + on every settings
    /// change.
    pub(super) local_commit: Option<LocalCommit>,
    /// Peer's most recently received Commit hash.
    pub(super) remote_commitment: Option<[u8; 16]>,
    /// Reassembled peer chunks (zstd-compressed NegotiatedState).
    /// Cleared whenever either side uncommits / disconnects / fails.
    /// Finalized once an empty-sentinel `Chunk` arrives — see the
    /// `Message::RemoteChunk` handler.
    pub(super) remote_chunks: Vec<u8>,
    /// Guards `maybe_kick_chunk_exchange` so it spawns the chunk-sender
    /// task at most once per commit pairing. Cleared on Uncommit /
    /// Disconnect / Failed.
    pub(super) local_chunks_sent: bool,
}

impl State {
    /// Drop the local commitment + reset the related lobby flags.
    /// If we had previously sent a Commit, also fires an Uncommit
    /// packet so the peer doesn't sit waiting for our chunks.
    pub(super) fn invalidate_local_commit(&mut self) {
        let had_commit = self.handshake.local_commit.is_some();
        self.handshake.local_commit = None;
        self.handshake.local_chunks_sent = false;
        self.lobby.local_ready = false;
        self.lobby.match_ready = false;
        if !had_commit {
            return;
        }
        let Some(sender) = self.conn.as_ref().map(|c| c.sender.clone()) else {
            return;
        };
        self.perform(
            async move {
                sender
                    .lock()
                    .await
                    .send_uncommit()
                    .await
                    .map_err(|e| format!("send_uncommit: {e}"))
            },
            |r| match r {
                Ok(()) => Message::WireOpDone,
                Err(e) => Message::Failed(e),
            },
        );
    }

    /// Build a NegotiatedState from a fresh nonce + the local
    /// save's SRAM, zstd-compress it, hash it for the commitment,
    /// send the Commit packet, then kick the chunk exchange if
    /// the peer has already committed.
    pub(super) fn commit_local(&mut self, save_sram: Vec<u8>) {
        if !matches!(self.phase, Phase::Lobby { .. }) {
            return;
        }
        let Some(sender) = self.conn.as_ref().map(|c| c.sender.clone()) else {
            return;
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
                self.emit(Message::Failed(format!("serialize state: {e}")));
                return;
            }
        };
        let compressed = match zstd::stream::encode_all(std::io::Cursor::new(&bin), 3) {
            Ok(c) => c,
            Err(e) => {
                self.emit(Message::Failed(format!("zstd encode: {e}")));
                return;
            }
        };
        let commitment = make_commitment(&compressed);
        self.handshake.local_commit = Some(LocalCommit { state, compressed });
        self.handshake.local_chunks_sent = false;
        self.lobby.local_ready = true;

        self.perform(
            async move {
                sender
                    .lock()
                    .await
                    .send_commit(commitment)
                    .await
                    .map_err(|e| format!("send_commit: {e}"))
            },
            |r| match r {
                Ok(()) => Message::WireOpDone,
                Err(e) => Message::Failed(e),
            },
        );
        self.maybe_kick_chunk_exchange();
    }

    /// If both sides have committed and we haven't sent our
    /// chunks yet, spawn the chunk-streaming task. Idempotent:
    /// called from both Commit and RemoteCommit handlers, fires
    /// the task exactly once per commit pairing.
    pub(super) fn maybe_kick_chunk_exchange(&mut self) {
        if self.handshake.local_chunks_sent
            || self.handshake.local_commit.is_none()
            || self.handshake.remote_commitment.is_none()
        {
            return;
        }
        let Some(sender) = self.conn.as_ref().map(|c| c.sender.clone()) else {
            return;
        };
        let compressed = self.handshake.local_commit.as_ref().unwrap().compressed.clone();
        self.handshake.local_chunks_sent = true;
        let cancel = self.cancel.clone();
        self.perform(
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
                    result.map_err(|e| AsyncError::Failed(format!("send_chunk: {e}")))?;
                }
                // Empty sentinel = end-of-stream.
                sender
                    .lock()
                    .await
                    .send_chunk(Vec::new())
                    .await
                    .map_err(|e| AsyncError::Failed(format!("send_chunk-end: {e}")))?;
                Ok::<(), AsyncError>(())
            },
            |r| match r {
                Ok(()) => Message::WireOpDone,
                Err(e) => map_async_err(e),
            },
        );
    }

    /// Called when the empty-sentinel chunk arrives. Verifies
    /// the peer's commitment matches the hash of the accumulated
    /// chunks, decodes their NegotiatedState (sanity), flips
    /// `match_ready`, then fires StartMatch.
    pub(super) fn try_finish_handshake(&mut self) {
        let Some(remote_commitment) = self.handshake.remote_commitment else {
            self.emit(Message::Failed("peer sent end-of-chunks before Commit".to_string()));
            return;
        };
        if self.handshake.local_commit.is_none() {
            // Their stream is here but we haven't committed yet —
            // just hold the bytes; finalization runs once we
            // commit + their stream re-finalizes via the
            // duplicate trip through this handler. Easier to
            // just bail until both sides are ready.
            return;
        }
        let actual = make_commitment(&self.handshake.remote_chunks);
        if !bool::from(actual.ct_eq(&remote_commitment)) {
            self.emit(Message::Failed("peer commitment mismatch".to_string()));
            return;
        }
        // Decompress + decode the peer's NegotiatedState. We
        // don't use it for anything until round 6 (PvP session
        // handoff), but verifying that it parses now means we
        // catch wire-format breakage before the user hits Play.
        let peer_state_bytes = match zstd::stream::decode_all(std::io::Cursor::new(&self.handshake.remote_chunks)) {
            Ok(b) => b,
            Err(e) => {
                self.emit(Message::Failed(format!("zstd decode: {e}")));
                return;
            }
        };
        if let Err(e) = crate::net::protocol::NegotiatedState::deserialize(&peer_state_bytes) {
            self.emit(Message::Failed(format!("decode peer state: {e}")));
            return;
        }
        self.lobby.match_ready = true;

        let Some(sender) = self.conn.as_ref().map(|c| c.sender.clone()) else {
            return;
        };
        self.perform(
            async move {
                sender
                    .lock()
                    .await
                    .send_start_match()
                    .await
                    .map_err(|e| format!("send_start_match: {e}"))
            },
            |r| match r {
                Ok(()) => Message::WireOpDone,
                Err(e) => Message::Failed(e),
            },
        );
        self.maybe_signal_pvp_handoff();
    }

    /// Both sides have sent + received StartMatch — emit the
    /// signal the App listens for to spin up the live match.
    /// No-op until both halves are present.
    pub(super) fn maybe_signal_pvp_handoff(&mut self) {
        if self.lobby.match_ready && self.lobby.remote_match_ready {
            self.emit(Message::MatchHandoffReady);
        }
    }
}

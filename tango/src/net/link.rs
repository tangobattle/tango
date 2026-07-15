//! The peer link: the one object that owns a connected peer's transport for
//! the lifetime of a match — the peer connection, both channels' send/receive
//! halves, and the transparent mid-match reconnect that can replace all of
//! them without the match noticing.
//!
//! The match's reliability state (the rennet seq/ack streams inside
//! [`InMatchTx`]) is keyed to the session's cancellation token, *not* to any
//! physical transport — so when the link drops, [`Link::reconnect`] can tear
//! the peer connection down, rebuild it from its [`ReconnectRecipe`], and
//! hot-swap the new channel halves underneath. The unacked redundancy window
//! survives the swap and refills the peer's gap, so the lockstep sim above
//! experiences the whole outage as a pause: no state resync, no protocol
//! re-handshake beyond the version `negotiate`.
//!
//! The embedder (the PvP session) stays in charge of *policy* — deciding when
//! a trip is worth reconnecting (that needs match-level knowledge: completion,
//! the peer's EndOfMatch) and freezing/unfreezing the emulator around the
//! attempt. The link owns the *mechanism*: everything from the recipe down.

use std::sync::Arc;

use super::InMatchTx;
use tokio_util::sync::CancellationToken;

/// How long the link keeps trying to rebuild a dropped direct connection
/// before giving up. Generous: the sim is paused throughout, so a long outage
/// costs nothing but the wait.
const RECONNECT_DIRECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Per-attempt cap on a single `host`/`connect` + `negotiate` rebuild — the
/// dialer's `connect` will hang on ICE until the host is listening again, so
/// bound it and retry rather than blocking the whole budget on one attempt.
const RECONNECT_DIRECT_ATTEMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
/// Give-up window for the matchmaking path — longer than direct's, since each
/// attempt re-rendezvouses on the signaling server then re-gathers ICE (and
/// possibly TURN), which is much slower than re-binding a known local port.
const RECONNECT_MATCHMAKING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// Per-attempt cap for a matchmaking rebuild (signaling rendezvous + ICE/TURN
/// gathering + negotiate).
const RECONNECT_MATCHMAKING_ATTEMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// Pause between failed rebuild attempts (e.g. dialer racing ahead of the host
/// re-binding its port).
const RECONNECT_BACKOFF: std::time::Duration = std::time::Duration::from_millis(250);
/// Give-up window for a reconnect triggered by a channel *close* (rather than
/// a stalled input queue). libdatachannel tears connections down gracefully,
/// so a close is either a deliberate quit or the peer's own reconnect dropping
/// its old transport. A quit normally announces itself first (the control
/// channel's `Goodbye`, see [`Link::send_goodbye`]) and ends the match without
/// any window — so a bare close is *probably* the peer's reconnect, but the
/// goodbye is best-effort, so we still wait only briefly: a genuine asymmetric
/// drop has the peer already at the rendezvous and rejoins almost at once,
/// while a quit whose goodbye was lost finds no peer and ends without a long
/// "Reconnecting…". Applies to both transports (the cost of a needless wait is
/// the same either way).
const RECONNECT_CLEAN_CLOSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);
/// Cap on the deliberate-quit `Goodbye` send. Best-effort by design — a
/// wedged transport must not delay the local teardown; the peer just falls
/// back to the clean-close reconnect window.
const GOODBYE_SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);
/// Upper bound on how long [`Link::bring_up`] waits for the lobby loop to
/// observe its cancellation and release the control receiver. The loop
/// typically returns within a few ms; the cap just keeps a wedged loop from
/// hanging the PvP setup forever.
const HANDOFF_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// What tripped the supervisor into a reconnect — sizes the give-up window.
/// The *policy* (whether a trip reconnects at all) stays with the embedder;
/// this only tells the mechanism how patient to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconnectCause {
    /// The input queue stalled: a quiet/dead link. Full per-transport window.
    Stall,
    /// A channel closed cleanly *without* a preceding `Goodbye`: the peer's
    /// own reconnect dropping its old transport, or a quit whose goodbye was
    /// lost. Short window — a real drop's peer is already at the rendezvous,
    /// a quit never shows up. (An announced quit never reaches a reconnect at
    /// all — see [`ControlEnd::Goodbye`].)
    CleanClose,
}

/// What ended [`Link::watch_control`]'s mid-match watch. The distinction is
/// what lets the supervisor end the match at once on a deliberate quit
/// instead of burning the clean-close reconnect window on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlEnd {
    /// The peer announced a deliberate quit ([`Link::send_goodbye`]'s
    /// `Goodbye` packet): it is leaving and will never be at a rendezvous.
    Goodbye,
    /// The channel closed without a goodbye — the peer's own reconnect
    /// dropping its old transport, its transport declaring the link dead, or
    /// a quit whose goodbye was lost.
    Eof,
}

/// Which side of a direct (signaling-free) connection the local
/// instance is. Drives the offer/answer symmetry breaker, and which
/// side pins the UDP port vs. dials it.
#[derive(Debug, Clone)]
pub enum DirectRole {
    /// Pin the given UDP port and accept the first inbound peer.
    Host { port: u16 },
    /// Dial the given `host:port` string.
    Connect { addr: String },
}

/// How to rebuild a dropped connection mid-match. Assembled by
/// `netplay::State::take_pre_match` and consumed by [`Link::reconnect`];
/// a link without one can't be transparently rebuilt.
#[derive(Debug, Clone)]
pub enum ReconnectRecipe {
    /// Signaling-free direct link: re-run the same `host`/`connect`.
    Direct(DirectRole),
    /// Matchmaking link: re-rendezvous on the signaling server. Both peers
    /// reconnect to `session_id` — derived from the shared match RNG seed (see
    /// [`derive_reconnect_session_id`]) so it's unguessable and can't collide
    /// with a stranger on the original link code — and re-exchange fresh SDP.
    /// The server keeps no per-session state once a pair's sockets close, so it
    /// re-pairs them with no server-side changes. `is_offerer`/player index stay
    /// fixed from the original match, so the re-assigned offerer/answerer roles
    /// here don't matter.
    Matchmaking {
        endpoint: String,
        session_id: String,
        use_relay: Option<bool>,
        identity: Option<tango_signaling::ClientIdentity>,
    },
}

/// Derive the matchmaking reconnect `session_id`, the rendezvous code both peers
/// re-dial after a mid-match drop. It must be reproducible by either peer yet
/// unguessable to anyone else (so a stranger can't camp the rendezvous and
/// hijack the reconnect).
///
/// Two independent secrets are mixed in, neither sufficient alone:
///
/// * `rng_seed` — the shared match RNG seed (XOR of both commit nonces, exchanged
///   over the encrypted data channel). The *signaling server* never sees it.
/// * the two DTLS certificate fingerprints — per-connection, high-entropy, and
///   verified during the handshake, but unlike `rng_seed` never written to disk
///   (the seed doubles as the in-match RNG seed, so it lands in replay files). A
///   *replay holder* never sees the fingerprints.
///
/// So no single party outside the two peers can reproduce the id: the server has
/// the fingerprints but not the seed; a replay leaks the seed but not the
/// fingerprints. The two fingerprints are folded together by XOR — commutative,
/// so both peers reach the same value without having to agree on an order (which
/// is "local" vs "remote" is swapped between them).
///
/// Falls back to seed-only (the original construction) when a fingerprint is
/// missing or the two differ in length — e.g. a peer whose signaling stack didn't
/// surface one — so the two ends still agree on an id rather than silently
/// diverging. Domain-separated from the lobby commitment (same `Shake128`, over
/// `"tango:lobby:"`).
///
/// We also prefix it with _ as the client does not allow construction of
/// link codes containing _, but the server does permit them.
pub(crate) fn derive_reconnect_session_id(rng_seed: &[u8; 16], fp_a: &[u8], fp_b: &[u8]) -> String {
    use sha3::digest::{ExtendableOutput, Update, XofReader};
    let mut h = sha3::Shake128::default();
    h.update(b"tango:reconnect:");
    h.update(rng_seed);
    // Both fingerprints are SHA-256 digests (equal length); the empty / unequal
    // guard keeps the two peers in lockstep on the seed-only fallback when one is
    // absent rather than mixing in a lopsided value.
    if !fp_a.is_empty() && fp_a.len() == fp_b.len() {
        let folded: Vec<u8> = fp_a.iter().zip(fp_b).map(|(a, b)| a ^ b).collect();
        h.update(&folded);
    }
    let mut out = [0u8; 16];
    h.finalize_xof().read(&mut out);
    let mut code: String = "_".into();
    code.extend(out.iter().map(|b| format!("{b:02x}")));
    code
}

/// Everything `netplay::State::take_pre_match` drains out of the lobby-era
/// connection handles for [`Link::bring_up`] to assemble. The control
/// receiver arrives late — the lobby loop owns it until it observes its
/// cancellation — so it comes as a oneshot rather than by value.
pub struct LinkParts {
    /// Reliable control/lobby channel sender. In-match it carries only the
    /// deliberate-quit `Goodbye` (all live traffic is on the unreliable
    /// channel), and is held open so its close doesn't surface as a spurious
    /// disconnect on the peer's control-channel watch.
    pub control_sender: Arc<tokio::sync::Mutex<super::Sender>>,
    /// Reliable control receiver, sent by the lobby loop on cancel-exit. The
    /// link watches it only for the peer's deliberate-quit `Goodbye` and the
    /// disconnect signal (the unreliable datagram channel has no clean close
    /// event).
    pub control_receiver_rx: tokio::sync::oneshot::Receiver<super::Receiver>,
    /// Unreliable in-match channel's send half — becomes the [`InMatchTx`] sink.
    pub in_match_sender: super::data::Sender,
    /// Unreliable in-match channel's receive half, parked at negotiate time.
    pub in_match_receiver: super::data::Receiver,
    /// The peer connection; brought up by both transports and kept alive for
    /// the channels' lifetime.
    pub peer_conn: datachannel_wrapper::PeerConnection,
    /// Recipe for transparently rebuilding the connection if it drops
    /// mid-match, or `None` for a transport that can't be rebuilt.
    pub recipe: Option<ReconnectRecipe>,
    /// The shared match RNG seed — the unchanging half of the matchmaking
    /// reconnect `session_id` derivation (the DTLS fingerprints are the
    /// per-connection half, refreshed on every rebuild).
    pub rng_seed: [u8; 16],
}

/// Live link state, published on a watch channel so the UI can draw the
/// "Reconnecting…" overlay and its depleting give-up bar. The `(started,
/// give_up_at)` pair (rather than just the deadline) lets the bar's fraction
/// work across the direct/matchmaking window sizes.
#[derive(Clone, Copy, Debug)]
pub enum LinkHealth {
    /// Steady state: the transport is up (or presumed up).
    Connected,
    /// The transport dropped and [`Link::reconnect`] is rebuilding it. The
    /// emulator is paused for the duration.
    Reconnecting {
        started: std::time::Instant,
        give_up_at: std::time::Instant,
    },
    /// A reconnect gave up (or the recipe was unusable). Terminal.
    Dead,
}

/// A logical connection to the peer that outlives physical transports: owns
/// the peer connection, both channels' halves, and the mid-match reconnect
/// mechanism. See the module docs for the design.
pub struct Link {
    /// The current peer connection. `reconnect` drops the old one and slots
    /// the rebuilt one in; the link keeps it alive for the channels' lifetime,
    /// and its eventual graceful drop (DTLS close_notify) is what hands the
    /// peer a prompt EOF when we leave. libdatachannel has no silent teardown,
    /// so the peer also sees a clean EOF mid-reconnect — which is why a close
    /// arms the peer's own reconnect window (see [`ReconnectCause`]) instead
    /// of ending its match.
    peer_conn: std::sync::Mutex<Option<datachannel_wrapper::PeerConnection>>,
    /// The in-match send handle: rennet out/in streams + retransmit heartbeat.
    /// Its streams are keyed to the session cancellation token, so they (and
    /// the unacked window) persist across a transport swap.
    in_match: InMatchTx,
    /// Reliable control channel sender. See [`LinkParts::control_sender`].
    control_sender: Arc<tokio::sync::Mutex<super::Sender>>,
    /// Reliable control receiver, watched mid-match for the peer's `Goodbye`
    /// and the disconnect signal.
    /// A tokio Mutex: [`watch_control`](Self::watch_control) holds it
    /// across its receive await; `reconnect` replaces it once the watcher has
    /// been dropped (the supervisor's `select!` tears its arms down before
    /// reconnecting).
    control_receiver: tokio::sync::Mutex<super::Receiver>,
    /// The current in-match receive half. Taken by the supervisor to build a
    /// fresh `PvpReceiver` at match start and after every successful
    /// reconnect (the rennet in-stream carries across, so the peer's resent
    /// window fills the gap contiguously).
    match_receiver: std::sync::Mutex<Option<super::data::Receiver>>,
    /// Rebuild recipe. Mutable so a successful matchmaking reconnect can
    /// refresh the rendezvous `session_id` for the next drop; `rng_seed` is
    /// the unchanging half of that derivation.
    recipe: std::sync::Mutex<Option<ReconnectRecipe>>,
    rng_seed: [u8; 16],
    /// Ping tracker shared with the in-match receive adapter (ack-derived RTT
    /// samples land here). `Some` while the link is up; retired to `None` when
    /// the remote drops, which is how the UI retires the instrument panel.
    /// A std Mutex — every guard scope is a plain read or swap, never held
    /// across an await, and the UI reads it from the render thread.
    latency: Arc<std::sync::Mutex<Option<super::LatencyCounter>>>,
    health: tokio::sync::watch::Sender<LinkHealth>,
    cancel: CancellationToken,
}

impl Link {
    /// Assemble the link from the lobby handoff. Awaits the lobby loop
    /// releasing the control receiver (worst case a few ms after the caller's
    /// cancel flipped, capped at [`HANDOFF_TIMEOUT`]) and starts the in-match
    /// retransmit heartbeat (cadence `heartbeat`, lifetime keyed to `cancel` —
    /// and *only* that: a transport error doesn't end it, so the unacked
    /// window keeps flowing across a mid-match reconnect).
    pub async fn bring_up(
        parts: LinkParts,
        heartbeat: std::time::Duration,
        cancel: CancellationToken,
    ) -> anyhow::Result<Self> {
        let control_receiver = tokio::time::timeout(HANDOFF_TIMEOUT, parts.control_receiver_rx)
            .await
            .map_err(|_| anyhow::anyhow!("timed out waiting for lobby loop to release receiver"))?
            .map_err(|_| anyhow::anyhow!("lobby loop dropped without releasing receiver"))?;
        let in_match = InMatchTx::new(parts.in_match_sender, heartbeat, cancel.clone());
        let (health, _) = tokio::sync::watch::channel(LinkHealth::Connected);
        Ok(Self {
            peer_conn: std::sync::Mutex::new(Some(parts.peer_conn)),
            in_match,
            control_sender: parts.control_sender,
            control_receiver: tokio::sync::Mutex::new(control_receiver),
            match_receiver: std::sync::Mutex::new(Some(parts.in_match_receiver)),
            recipe: std::sync::Mutex::new(parts.recipe),
            rng_seed: parts.rng_seed,
            // 5 marks at roughly one ack-confirmed seq per frame ≈ a 5 s
            // median window, matching the lobby's ping counter.
            latency: Arc::new(std::sync::Mutex::new(Some(super::LatencyCounter::new(5)))),
            health,
            cancel,
        })
    }

    /// Whether this link's transport can be transparently rebuilt on a drop.
    pub fn can_reconnect(&self) -> bool {
        self.recipe.lock().unwrap().is_some()
    }

    /// The in-match send handle (cloneable; the sender pump, the EndOfMatch
    /// fire, and the receive adapter all share it).
    pub fn in_match(&self) -> &InMatchTx {
        &self.in_match
    }

    /// Take the current in-match receive half — at match start, and again
    /// after every successful [`reconnect`](Self::reconnect) (which parks the
    /// rebuilt one here). `None` if it was already taken since the last swap.
    pub fn take_match_receiver(&self) -> Option<super::data::Receiver> {
        self.match_receiver.lock().unwrap().take()
    }

    /// Shared handle to the latency counter, for wiring into the in-match
    /// receive adapter (its ack-derived RTT samples are the only writer).
    pub fn latency_handle(&self) -> Arc<std::sync::Mutex<Option<super::LatencyCounter>>> {
        self.latency.clone()
    }

    /// Median ping over the last few seconds — smoothed, for the frame-delay
    /// suggestion. `Some(ZERO)` until the first sample, `None` once the
    /// counter is retired (remote dropped / teardown).
    pub fn latency(&self) -> Option<std::time::Duration> {
        self.latency.lock().unwrap().as_ref().map(|c| c.median())
    }

    /// Raw latest ping — the most recent single measurement, unsmoothed, for
    /// the live telemetry plate. Same `Some`/`None` semantics as
    /// [`latency`](Self::latency).
    pub fn latency_raw(&self) -> Option<std::time::Duration> {
        self.latency
            .lock()
            .unwrap()
            .as_ref()
            .map(|c| c.latest().unwrap_or(std::time::Duration::ZERO))
    }

    /// Retire the latency readout (reads become `None`). Called at teardown so
    /// the instrument panel retires rather than sticking at its last reading.
    pub fn retire_latency(&self) {
        *self.latency.lock().unwrap() = None;
    }

    /// Snapshot of the link's health, for the reconnect overlay.
    pub fn health(&self) -> LinkHealth {
        *self.health.borrow()
    }

    /// Watch the control channel mid-match. Only two things legitimately
    /// happen here: the peer's deliberate-quit `Goodbye`, and the close (a
    /// recv error). Either way the peer *told* us something — a goodbye means
    /// it's leaving and the match ends at once; a bare EOF (graceful drop
    /// sends DTLS close_notify) is its reconnect dropping its old transport,
    /// its transport declaring the link dead, or a quit whose goodbye was
    /// lost. A mere outage delivers nothing (our own reconnect teardown is
    /// silent).
    pub async fn watch_control(&self) -> ControlEnd {
        let mut receiver = self.control_receiver.lock().await;
        loop {
            match receiver.receive().await {
                // The peer announced a deliberate quit before tearing down.
                Ok(super::control::protocol::Packet::Goodbye(_)) => return ControlEnd::Goodbye,
                // Any other packet — nothing else legitimately flows here
                // mid-match, but ignore it and keep watching.
                Ok(_) => {}
                // Undecodable bytes (`InvalidData`) are stray traffic, not a close —
                // ignore and keep watching. Any other error (notably the channel's
                // `UnexpectedEof`) means it actually closed, so stop.
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {}
                Err(_) => return ControlEnd::Eof,
            }
        }
    }

    /// Announce a deliberate local quit to the peer on the (otherwise idle)
    /// reliable control channel, just before teardown. Our teardown's clean
    /// EOF alone is ambiguous to the peer — its own reconnect's transport
    /// drop looks identical — so without this it burns the clean-close
    /// reconnect window on us; the goodbye lets its mid-match watch end the
    /// match at once. Best-effort: on a wedged or torn-down transport the
    /// send fails or times out and the peer falls back to that window.
    pub async fn send_goodbye(&self) {
        let send = async {
            let mut sender = self.control_sender.lock().await;
            sender.send_goodbye().await
        };
        match tokio::time::timeout(GOODBYE_SEND_TIMEOUT, send).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::debug!("goodbye send failed: {e}"),
            Err(_) => log::debug!("goodbye send timed out"),
        }
    }

    /// Transparently rebuild the dropped transport and hot-swap it under the
    /// persistent rennet streams. Returns `true` once the link is carrying
    /// traffic again, `false` on give-up / cancellation (the link is then
    /// [`LinkHealth::Dead`]).
    ///
    /// The caller must have stopped every consumer of the link's receivers
    /// first (the supervisor's `select!` arms are dropped before this is
    /// called) — the swap replaces them. The emulator freeze/unfreeze around
    /// the attempt is also the caller's job; this is transport-only.
    pub async fn reconnect(&self, cause: ReconnectCause) -> bool {
        let Some(recipe) = self.recipe.lock().unwrap().clone() else {
            self.health.send_replace(LinkHealth::Dead);
            return false;
        };

        // Arm the give-up window the UI bar drains over (the sim is paused
        // throughout, so a long wait costs nothing but the wait). A stall
        // gets the full per-transport window — both peers converge on it:
        // whoever trips first goes silent, which stall-trips the other
        // within `RECONNECT_QUEUE_LENGTH` frames. A channel close gets the
        // short window instead (likely a quit — don't hang on it; a real
        // drop's peer is already at the rendezvous). Retire the latency
        // readout for the duration.
        let started = std::time::Instant::now();
        let timeout = match cause {
            ReconnectCause::CleanClose => RECONNECT_CLEAN_CLOSE_TIMEOUT,
            ReconnectCause::Stall => match recipe {
                ReconnectRecipe::Direct(_) => RECONNECT_DIRECT_TIMEOUT,
                ReconnectRecipe::Matchmaking { .. } => RECONNECT_MATCHMAKING_TIMEOUT,
            },
        };
        let give_up_at = started + timeout;
        self.health
            .send_replace(LinkHealth::Reconnecting { started, give_up_at });
        self.retire_latency();

        // Tear the old peer connection down *before* rebuilding so the host's
        // pinned UDP port frees up for the re-bind. libdatachannel has no
        // silent teardown — the drop closes gracefully, handing the peer a
        // clean EOF mid-rebuild — which is exactly why the peer's supervisor
        // treats a close as reconnectable (short window) rather than "the
        // peer left". The socket is released asynchronously as the stack
        // tears down, so a rebuild attempt can race it and see AddrInUse —
        // `rebuild_connection` retries, absorbing that.
        drop(self.peer_conn.lock().unwrap().take());

        let Some(channels) = self.rebuild_connection(&recipe, give_up_at).await else {
            // Timed out or cancelled — give up; the match ends.
            self.health.send_replace(LinkHealth::Dead);
            return false;
        };

        // Hot-swap the rebuilt channels under the persistent streams.
        let super::channel::Channels {
            control: (new_control_sender, new_control_receiver),
            in_match: (new_in_match_sender, new_in_match_receiver),
            peer_conn: new_peer_conn,
            local_dtls_fingerprint,
            peer_dtls_fingerprint,
            // Informational on a reconnect: the rendezvous is already bound to
            // this match by the derived session_id. Logged so a hijack attempt
            // (same session_id, different install) at least leaves a trace.
            peer_client_cert_fingerprint,
        } = channels;

        if !peer_client_cert_fingerprint.is_empty() {
            log::info!(
                "reconnected peer client identity (sha256 fingerprint: {})",
                crate::netplay::identity::hex(&peer_client_cert_fingerprint)
            );
        }

        // Refresh the matchmaking rendezvous so the *next* drop re-dials a
        // fresh, unguessable `session_id` derived from this new connection's
        // DTLS fingerprints (both peers just handshook, so they derive the
        // same one) instead of reusing the original — which the signaling
        // server has already seen. The direct path's recipe (re-run
        // host/connect) needs no fingerprints, so its empty pair leaves the
        // seed-only fallback in place harmlessly.
        if let Some(ReconnectRecipe::Matchmaking { session_id, .. }) = self.recipe.lock().unwrap().as_mut() {
            *session_id = derive_reconnect_session_id(&self.rng_seed, &local_dtls_fingerprint, &peer_dtls_fingerprint);
        }
        *self.peer_conn.lock().unwrap() = Some(new_peer_conn);
        // Retarget the out-stream sink (pump + heartbeat both send through the
        // shared handle); keep the new control sender alive so its channel
        // doesn't half-close under the peer.
        self.in_match.swap_sink(new_in_match_sender).await;
        *self.control_sender.lock().await = new_control_sender;
        *self.control_receiver.lock().await = new_control_receiver;
        // Park the fresh receive half for the supervisor; the rennet in-stream
        // (seq/ack) carries across the swap, so the peer's resent window fills
        // our gap contiguously.
        *self.match_receiver.lock().unwrap() = Some(new_in_match_receiver);
        *self.latency.lock().unwrap() = Some(super::LatencyCounter::new(5));
        self.health.send_replace(LinkHealth::Connected);
        true
    }

    /// Rebuild a dropped connection from its recipe, then run the version
    /// `negotiate` handshake on the rebuilt reliable channel. The bring-up
    /// doubles as a rendezvous barrier — the direct `host`'s first send blocks
    /// until the dialer is back, and matchmaking's `connect` blocks at the
    /// signaling server until the peer rejoins — so both peers only return
    /// (and unpause) once the link is genuinely carrying traffic again.
    /// Retries failed attempts (the peers race each other to re-rendezvous)
    /// until `deadline`, returning `None` on timeout or cancellation.
    ///
    /// Returns the rebuilt [`super::channel::Channels`] bundle regardless of
    /// transport — the matchmaking path funnels the signaling client's
    /// `Connected` through the same [`Channels::from_signaling`] the initial
    /// connect uses, so a rebuild and a fresh build produce the identical
    /// shape (fingerprints and all).
    ///
    /// [`Channels::from_signaling`]: super::channel::Channels::from_signaling
    async fn rebuild_connection(
        &self,
        recipe: &ReconnectRecipe,
        deadline: std::time::Instant,
    ) -> Option<super::channel::Channels> {
        let attempt_timeout = match recipe {
            ReconnectRecipe::Direct(_) => RECONNECT_DIRECT_ATTEMPT_TIMEOUT,
            ReconnectRecipe::Matchmaking { .. } => RECONNECT_MATCHMAKING_ATTEMPT_TIMEOUT,
        };
        loop {
            let now = std::time::Instant::now();
            if self.cancel.is_cancelled() || now >= deadline {
                return None;
            }
            // Cap the attempt by whichever is sooner — the per-attempt limit or the
            // remaining give-up budget — so a short give-up window fires on time
            // instead of overrunning by a whole attempt.
            let this_timeout = attempt_timeout.min(deadline.saturating_duration_since(now));
            let attempt = async {
                let mut channels = match recipe {
                    ReconnectRecipe::Direct(DirectRole::Host { port }) => super::direct_rtc::host(*port).await?,
                    ReconnectRecipe::Direct(DirectRole::Connect { addr }) => super::direct_rtc::connect(addr).await?,
                    ReconnectRecipe::Matchmaking {
                        endpoint,
                        session_id,
                        use_relay,
                        identity,
                    } => {
                        let connecting = tango_signaling::connect(
                            endpoint,
                            session_id,
                            *use_relay,
                            crate::netplay::PROTOCOL_VERSION,
                            vec![super::channel::control_channel(), super::channel::in_match_channel()],
                            identity.clone(),
                        )
                        .await
                        .map_err(|e| std::io::Error::other(format!("signaling: {e}")))?;
                        // Blocks at the server until the peer rejoins the session, then
                        // completes the WebRTC handshake — the matchmaking rendezvous.
                        // The bundle carries this handshake's fingerprints so the link
                        // can re-derive the session_id for the next drop; they don't
                        // affect *this* rendezvous (its id is already fixed).
                        let connected = connecting
                            .await
                            .map_err(|e| std::io::Error::other(format!("webrtc: {e}")))?;
                        super::channel::Channels::from_signaling(connected)?
                    }
                };
                super::negotiate(&mut channels.control.0, &mut channels.control.1)
                    .await
                    .map_err(|e| std::io::Error::other(format!("negotiate: {e:?}")))?;
                Ok::<_, std::io::Error>(channels)
            };
            let outcome = tokio::select! {
                biased;
                _ = self.cancel.cancelled() => return None,
                r = tokio::time::timeout(this_timeout, attempt) => r,
            };
            match outcome {
                Ok(Ok(channels)) => return Some(channels),
                Ok(Err(e)) => log::debug!("pvp reconnect attempt failed: {e}"),
                Err(_) => log::debug!("pvp reconnect attempt timed out"),
            }
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => return None,
                _ = tokio::time::sleep(RECONNECT_BACKOFF) => {}
            }
        }
    }
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Link { .. }")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assemble one side's link from a direct-transport `Channels` bundle,
    /// faking the lobby-loop handback (the control receiver goes straight
    /// down the oneshot).
    async fn link_from_channels(
        channels: crate::net::channel::Channels,
        recipe: ReconnectRecipe,
        cancel: &CancellationToken,
    ) -> Link {
        let crate::net::channel::Channels {
            control: (control_sender, control_receiver),
            in_match: (in_match_sender, in_match_receiver),
            peer_conn,
            ..
        } = channels;
        let (post_tx, post_rx) = tokio::sync::oneshot::channel();
        let _ = post_tx.send(control_receiver);
        Link::bring_up(
            LinkParts {
                control_sender: Arc::new(tokio::sync::Mutex::new(control_sender)),
                control_receiver_rx: post_rx,
                in_match_sender,
                in_match_receiver,
                peer_conn,
                recipe: Some(recipe),
                rng_seed: [0; 16],
            },
            std::time::Duration::from_millis(16),
            cancel.clone(),
        )
        .await
        .expect("bring_up")
    }

    /// Wrap the link's current in-match receive half in the real `PvpReceiver`
    /// adapter (dummy end-of-match hooks — the test only reads inputs).
    fn receiver_for(link: &Link) -> crate::net::PvpReceiver {
        crate::net::PvpReceiver::new(
            link.take_match_receiver().expect("match receiver parked"),
            link.in_match().clone(),
            link.latency_handle(),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(tokio::sync::Notify::new()),
        )
    }

    async fn recv_joyflags(rx: &mut crate::net::PvpReceiver, n: usize) -> Vec<u16> {
        use tango_pvp::net::Receiver as _;
        let mut got = vec![];
        while got.len() < n {
            match rx.receive().await.expect("receive") {
                tango_pvp::net::Event::Input(input) => got.push(input.joyflags),
            }
        }
        got
    }

    /// The transparent-reconnect contract end to end: two links over the
    /// direct transport carry inputs; both peers' `reconnect()` silently tears
    /// the transport down and rebuilds it from the recipe; and the persistent
    /// rennet streams deliver every input exactly once across the swap —
    /// including ones sent into the dead transport mid-outage, which the
    /// heartbeat resends through the fresh sink (and *excluding* re-deliveries
    /// of pre-outage inputs, which the in-stream dedups by seq).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn reconnect_delivers_exactly_once() {
        // A high, unlikely-to-clash loopback port (distinct from the
        // direct_rtc tests' 24987/24988/24990).
        let port = 24991;
        let addr = format!("127.0.0.1:{port}");

        let (host_res, conn_res) = tokio::join!(
            crate::net::direct_rtc::host(port),
            crate::net::direct_rtc::connect(&addr)
        );
        let mut host_ch = host_res.expect("host setup");
        let mut conn_ch = conn_res.expect("connect setup");
        // `negotiate`'s first send blocks until the channel opens, so this
        // drives the whole ICE/DTLS bring-up.
        let handshake = async {
            tokio::try_join!(
                crate::net::negotiate(&mut host_ch.control.0, &mut host_ch.control.1),
                crate::net::negotiate(&mut conn_ch.control.0, &mut conn_ch.control.1),
            )
        };
        tokio::time::timeout(std::time::Duration::from_secs(15), handshake)
            .await
            .expect("handshake timed out — channel never opened")
            .expect("negotiate failed");

        let cancel = CancellationToken::new();
        let host_link =
            Arc::new(link_from_channels(host_ch, ReconnectRecipe::Direct(DirectRole::Host { port }), &cancel).await);
        let conn_link = Arc::new(
            link_from_channels(
                conn_ch,
                ReconnectRecipe::Direct(DirectRole::Connect { addr: addr.clone() }),
                &cancel,
            )
            .await,
        );
        assert!(matches!(host_link.health(), LinkHealth::Connected));

        // Phase 1: inputs flow host → dialer over the original transport.
        let mut conn_rx = receiver_for(&conn_link);
        for joyflags in 1..=5u16 {
            host_link.in_match().send_input(joyflags, 0).await.expect("send");
        }
        let got = tokio::time::timeout(std::time::Duration::from_secs(15), recv_joyflags(&mut conn_rx, 5))
            .await
            .expect("phase-1 recv timed out");
        assert_eq!(got, (1..=5).collect::<Vec<u16>>());
        // The old receive half reads from the transport about to be torn
        // down; drop it before the swap parks a fresh one.
        drop(conn_rx);

        // Phase 2: both sides reconnect. The host goes first — its rebuild
        // blocks waiting for the dialer to re-dial, which lets the test
        // observe the Reconnecting health state before releasing it.
        let host_reconnect = tokio::spawn({
            let link = host_link.clone();
            async move { link.reconnect(ReconnectCause::Stall).await }
        });
        let observe_reconnecting = async {
            while !matches!(host_link.health(), LinkHealth::Reconnecting { .. }) {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), observe_reconnecting)
            .await
            .expect("host never entered Reconnecting");

        // Inputs sent into the outage: the send may fail (dead transport) but
        // each element lands in the out-stream window first, so the heartbeat
        // must deliver them all once a live sink is swapped back in.
        for joyflags in 6..=10u16 {
            let _ = host_link.in_match().send_input(joyflags, 0).await;
        }

        let conn_restored = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            conn_link.reconnect(ReconnectCause::Stall),
        )
        .await
        .expect("dialer reconnect timed out");
        assert!(conn_restored, "dialer reconnect gave up");
        let host_restored = tokio::time::timeout(std::time::Duration::from_secs(30), host_reconnect)
            .await
            .expect("host reconnect timed out")
            .expect("host reconnect task panicked");
        assert!(host_restored, "host reconnect gave up");
        assert!(matches!(host_link.health(), LinkHealth::Connected));
        assert!(matches!(conn_link.health(), LinkHealth::Connected));

        // Phase 3: exactly the mid-outage inputs arrive on the rebuilt link,
        // in order — no duplicates of 1..=5 even though the host's entire
        // unacked window (nothing acked it: the host never read the dialer's
        // heartbeat acks in this test) is resent across the swap.
        let mut conn_rx = receiver_for(&conn_link);
        let got = tokio::time::timeout(std::time::Duration::from_secs(15), recv_joyflags(&mut conn_rx, 5))
            .await
            .expect("phase-3 recv timed out");
        assert_eq!(got, (6..=10).collect::<Vec<u16>>());

        cancel.cancel();
    }

    /// Bidirectional resume: the real match is two-way, and after a mutual
    /// reconnect *both* peers must carry fresh traffic again — not just the
    /// host→dialer direction `reconnect_delivers_exactly_once` checks. Sends
    /// inputs each way before the drop, reconnects both sides, then sends fresh
    /// inputs each way and asserts both arrive. Guards against a swap that
    /// silently re-establishes only one direction (which live shows up as
    /// "reconnects but never resumes": each side stays stalled waiting for the
    /// other's inputs that never arrive).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn reconnect_resumes_both_directions() {
        let port = 24993;
        let addr = format!("127.0.0.1:{port}");

        let (host_res, conn_res) = tokio::join!(
            crate::net::direct_rtc::host(port),
            crate::net::direct_rtc::connect(&addr)
        );
        let mut host_ch = host_res.expect("host setup");
        let mut conn_ch = conn_res.expect("connect setup");
        let handshake = async {
            tokio::try_join!(
                crate::net::negotiate(&mut host_ch.control.0, &mut host_ch.control.1),
                crate::net::negotiate(&mut conn_ch.control.0, &mut conn_ch.control.1),
            )
        };
        tokio::time::timeout(std::time::Duration::from_secs(15), handshake)
            .await
            .expect("handshake timed out — channel never opened")
            .expect("negotiate failed");

        let cancel = CancellationToken::new();
        let host_link =
            Arc::new(link_from_channels(host_ch, ReconnectRecipe::Direct(DirectRole::Host { port }), &cancel).await);
        let conn_link = Arc::new(
            link_from_channels(
                conn_ch,
                ReconnectRecipe::Direct(DirectRole::Connect { addr: addr.clone() }),
                &cancel,
            )
            .await,
        );

        // Phase 1: inputs flow both ways over the original transport.
        let mut host_rx = receiver_for(&host_link);
        let mut conn_rx = receiver_for(&conn_link);
        for joyflags in 1..=3u16 {
            host_link.in_match().send_input(joyflags, 0).await.expect("host send");
            conn_link.in_match().send_input(joyflags, 0).await.expect("conn send");
        }
        let (h1, c1) = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            async { tokio::join!(recv_joyflags(&mut host_rx, 3), recv_joyflags(&mut conn_rx, 3)) },
        )
        .await
        .expect("phase-1 recv timed out");
        assert_eq!(h1, (1..=3).collect::<Vec<u16>>());
        assert_eq!(c1, (1..=3).collect::<Vec<u16>>());
        // Old receive halves read the transport about to be torn down.
        drop(host_rx);
        drop(conn_rx);

        // Phase 2: both sides reconnect (host first, so its rebuild blocks
        // waiting for the dialer to re-dial).
        let host_reconnect = tokio::spawn({
            let link = host_link.clone();
            async move { link.reconnect(ReconnectCause::Stall).await }
        });
        let observe_reconnecting = async {
            while !matches!(host_link.health(), LinkHealth::Reconnecting { .. }) {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), observe_reconnecting)
            .await
            .expect("host never entered Reconnecting");
        let conn_restored = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            conn_link.reconnect(ReconnectCause::Stall),
        )
        .await
        .expect("dialer reconnect timed out");
        assert!(conn_restored, "dialer reconnect gave up");
        let host_restored = tokio::time::timeout(std::time::Duration::from_secs(30), host_reconnect)
            .await
            .expect("host reconnect timed out")
            .expect("host reconnect task panicked");
        assert!(host_restored, "host reconnect gave up");

        // Phase 3: fresh inputs must flow *both* ways over the rebuilt link.
        let mut host_rx = receiver_for(&host_link);
        let mut conn_rx = receiver_for(&conn_link);
        for joyflags in 4..=6u16 {
            host_link.in_match().send_input(joyflags, 0).await.expect("host post-reconnect send");
            conn_link.in_match().send_input(joyflags, 0).await.expect("conn post-reconnect send");
        }
        let (h2, c2) = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            async { tokio::join!(recv_joyflags(&mut host_rx, 3), recv_joyflags(&mut conn_rx, 3)) },
        )
        .await
        .expect("phase-3 recv timed out — a direction never resumed");
        assert_eq!(h2, (4..=6).collect::<Vec<u16>>(), "host did not receive dialer's post-reconnect inputs");
        assert_eq!(c2, (4..=6).collect::<Vec<u16>>(), "dialer did not receive host's post-reconnect inputs");

        cancel.cancel();
    }

    /// The deliberate-quit fast path: `send_goodbye` fired right before the
    /// quitting side's teardown (exactly how the supervisor quits) surfaces on
    /// the peer's `watch_control` as `Goodbye`, not as the close racing along
    /// behind it — the control channel is ordered, so the announce always wins.
    /// The close itself then reads as a plain `Eof`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn goodbye_outruns_the_close() {
        // A high, unlikely-to-clash loopback port (distinct from the
        // direct_rtc tests' 24987/24988/24990 and the reconnect test's 24991).
        let port = 24992;
        let addr = format!("127.0.0.1:{port}");

        let (host_res, conn_res) = tokio::join!(
            crate::net::direct_rtc::host(port),
            crate::net::direct_rtc::connect(&addr)
        );
        let mut host_ch = host_res.expect("host setup");
        let mut conn_ch = conn_res.expect("connect setup");
        let handshake = async {
            tokio::try_join!(
                crate::net::negotiate(&mut host_ch.control.0, &mut host_ch.control.1),
                crate::net::negotiate(&mut conn_ch.control.0, &mut conn_ch.control.1),
            )
        };
        tokio::time::timeout(std::time::Duration::from_secs(15), handshake)
            .await
            .expect("handshake timed out — channel never opened")
            .expect("negotiate failed");

        let cancel = CancellationToken::new();
        let host_link =
            Arc::new(link_from_channels(host_ch, ReconnectRecipe::Direct(DirectRole::Host { port }), &cancel).await);
        let conn_link =
            Arc::new(link_from_channels(conn_ch, ReconnectRecipe::Direct(DirectRole::Connect { addr }), &cancel).await);

        // The dialer quits: announce, then tear the whole link down at once.
        conn_link.send_goodbye().await;
        drop(conn_link);

        let end = tokio::time::timeout(std::time::Duration::from_secs(15), host_link.watch_control())
            .await
            .expect("watch timed out — goodbye never arrived");
        assert_eq!(end, ControlEnd::Goodbye);

        // Watching again rides out the teardown's actual close as a bare EOF.
        let end = tokio::time::timeout(std::time::Duration::from_secs(15), host_link.watch_control())
            .await
            .expect("watch timed out — close never arrived");
        assert_eq!(end, ControlEnd::Eof);

        cancel.cancel();
    }
}

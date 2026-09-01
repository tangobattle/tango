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

use std::sync::atomic::{AtomicBool, Ordering};
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
/// Cap on the deliberate-quit `Goodbye` send. Best-effort by design — a
/// wedged transport must not delay the local teardown; the peer just falls
/// back to treating the unannounced EOF as a recoverable link loss.
const GOODBYE_SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);
/// Upper bound on how long [`Link::bring_up`] waits for the lobby loop to
/// observe its cancellation and release the control receiver. The loop
/// typically returns within a few ms; the cap just keeps a wedged loop from
/// hanging the PvP setup forever.
const HANDOFF_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// What ended [`Link::watch_control`]'s mid-match watch. The distinction is
/// what lets the supervisor end the match at once on a deliberate quit
/// instead of entering reconnect on it.
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
    },
}

fn reconnect_timeout(recipe: &ReconnectRecipe) -> std::time::Duration {
    match recipe {
        ReconnectRecipe::Direct(_) => RECONNECT_DIRECT_TIMEOUT,
        ReconnectRecipe::Matchmaking { .. } => RECONNECT_MATCHMAKING_TIMEOUT,
    }
}

// The reconnect session id is determinism-critical between peers, so
// its construction (and full security rationale) lives in the shared
// protocol crate.
pub use tango_net_protocol::derive::derive_reconnect_session_id;

/// Why [`Link::bring_up`] failed to assemble the transport bundle —
/// both variants are the lobby loop failing to hand its control
/// receiver over (see [`LinkParts::control_receiver_rx`]).
#[derive(Debug, thiserror::Error)]
pub enum BringUpError {
    #[error("timed out waiting for lobby loop to release receiver")]
    ReceiverHandoffTimeout,
    #[error("lobby loop dropped without releasing receiver")]
    ReceiverHandoffDropped,
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
        started: web_time::Instant,
        give_up_at: web_time::Instant,
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
    /// enters the peer's reconnect path instead of ending its match.
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
    /// Latched by [`watch_control`](Self::watch_control) when the peer's
    /// `Primed` lands. The match's ready gate reads it from the drive
    /// loop, so it's an atomic rather than a channel.
    peer_primed: Arc<AtomicBool>,
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
    ) -> Result<Self, BringUpError> {
        let control_receiver = crate::platform::timeout(HANDOFF_TIMEOUT, parts.control_receiver_rx)
            .await
            .map_err(|_| BringUpError::ReceiverHandoffTimeout)?
            .map_err(|_| BringUpError::ReceiverHandoffDropped)?;
        let in_match = InMatchTx::new(parts.in_match_sender, heartbeat, cancel.clone());
        let (health, _) = tokio::sync::watch::channel(LinkHealth::Connected);
        Ok(Self {
            peer_conn: std::sync::Mutex::new(Some(parts.peer_conn)),
            in_match,
            control_sender: parts.control_sender,
            control_receiver: tokio::sync::Mutex::new(control_receiver),
            match_receiver: std::sync::Mutex::new(Some(parts.in_match_receiver)),
            peer_primed: Arc::new(AtomicBool::new(false)),
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

    /// Wall-clock budget the supervisor's independent give-up watchdog uses.
    /// `reconnect` uses the same helper, so the dialog and the watchdog cannot
    /// drift onto different timeout values.
    pub(crate) fn reconnect_timeout(&self) -> Option<std::time::Duration> {
        self.recipe
            .lock()
            .unwrap()
            .as_ref()
            .map(reconnect_timeout)
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
                Ok(tango_net_protocol::control::Packet::Goodbye(_)) => return ControlEnd::Goodbye,
                // The peer's pair reached its link battle. Latch it and keep
                // watching: this opens the match's ready gate rather than
                // ending the watch. It can arrive before we've even started
                // priming (the watch is up from match start), which is why
                // it's a latch and not an event the gate has to be waiting on.
                Ok(tango_net_protocol::control::Packet::Primed(_)) => {
                    self.peer_primed.store(true, Ordering::Release);
                }
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

    /// The peer-primed latch, for the match's ready gate. Survives a
    /// reconnect: it records that the peer *reached* its link battle,
    /// which a transport swap doesn't undo.
    pub fn peer_primed(&self) -> Arc<AtomicBool> {
        self.peer_primed.clone()
    }

    /// Tell the peer our pair is primed (see
    /// [`Packet::Primed`](tango_net_protocol::control::Packet::Primed)).
    /// Unlike the goodbye this is not best-effort — the peer's gate will
    /// not open without it — so a failure is surfaced to the caller,
    /// which retries. It rides the reliable control channel, so a send
    /// that reports success is delivered.
    pub async fn send_primed(&self) -> std::io::Result<()> {
        let mut sender = self.control_sender.lock().await;
        sender.send_primed().await
    }

    /// Announce a deliberate local quit to the peer on the (otherwise idle)
    /// reliable control channel, just before teardown. Our teardown's clean
    /// EOF alone is ambiguous to the peer — its own reconnect's transport
    /// drop looks identical — so without this it enters reconnect on us; the
    /// goodbye lets its mid-match watch end the match at once. Best-effort: on
    /// a wedged or torn-down transport the send fails or times out and the
    /// peer falls back to ordinary reconnect.
    pub async fn send_goodbye(&self) {
        let send = async {
            let mut sender = self.control_sender.lock().await;
            sender.send_goodbye().await
        };
        match crate::platform::timeout(GOODBYE_SEND_TIMEOUT, send).await {
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
    pub async fn reconnect(&self) -> bool {
        let Some(recipe) = self.recipe.lock().unwrap().clone() else {
            self.health.send_replace(LinkHealth::Dead);
            return false;
        };

        // Arm the per-transport give-up window the UI bar drains over (the sim
        // is paused throughout, so a long outage costs nothing but the wait).
        // Both a stalled queue and an unannounced channel close are link loss;
        // deliberate quits take the separate `Goodbye` path and never enter
        // here. Retire the latency readout for the duration.
        let started = web_time::Instant::now();
        let timeout = reconnect_timeout(&recipe);
        let give_up_at = started + timeout;
        self.health
            .send_replace(LinkHealth::Reconnecting { started, give_up_at });
        self.retire_latency();

        // Tear the old peer connection down *before* rebuilding so the host's
        // pinned UDP port frees up for the re-bind. libdatachannel has no
        // silent teardown — the drop closes gracefully, handing the peer a
        // clean EOF mid-rebuild — which is exactly why the peer's supervisor
        // treats an unannounced close as reconnectable rather than "the peer
        // left". The socket is released asynchronously as the stack tears
        // down, so a rebuild attempt can race it and see AddrInUse —
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
        } = channels;

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
    ///
    /// Both transports rebuild in a browser except the direct one, which
    /// re-dials a UDP socket a browser can't open — and can't be holding
    /// a direct link in the first place, so that arm is unreachable there
    /// rather than merely unavailable.
    async fn rebuild_connection(
        &self,
        recipe: &ReconnectRecipe,
        deadline: web_time::Instant,
    ) -> Option<super::channel::Channels> {
        let attempt_timeout = match recipe {
            ReconnectRecipe::Direct(_) => RECONNECT_DIRECT_ATTEMPT_TIMEOUT,
            ReconnectRecipe::Matchmaking { .. } => RECONNECT_MATCHMAKING_ATTEMPT_TIMEOUT,
        };
        loop {
            let now = web_time::Instant::now();
            if self.cancel.is_cancelled() || now >= deadline {
                return None;
            }
            // Cap the attempt by whichever is sooner — the per-attempt limit or the
            // remaining give-up budget, so the deadline fires on time instead
            // of overrunning by a whole attempt.
            let this_timeout = attempt_timeout.min(deadline.saturating_duration_since(now));
            let attempt = async {
                let mut channels = match recipe {
                    #[cfg(not(target_arch = "wasm32"))]
                    ReconnectRecipe::Direct(DirectRole::Host { port }) => super::direct_rtc::host(*port).await?,
                    #[cfg(not(target_arch = "wasm32"))]
                    ReconnectRecipe::Direct(DirectRole::Connect { addr }) => super::direct_rtc::connect(addr).await?,
                    // A browser can't have built a direct link, so it can't
                    // be rebuilding one either.
                    #[cfg(target_arch = "wasm32")]
                    ReconnectRecipe::Direct(_) => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Unsupported,
                            "a direct link can't be rebuilt in a browser",
                        ))
                    }
                    ReconnectRecipe::Matchmaking {
                        endpoint,
                        session_id,
                        use_relay,
                    } => {
                        let connecting = tango_signaling::connect(
                            endpoint,
                            session_id,
                            *use_relay,
                            tango_net_protocol::PROTOCOL_VERSION,
                            vec![super::channel::control_channel(), super::channel::in_match_channel()],
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
                r = crate::platform::timeout(this_timeout, attempt) => r,
            };
            match outcome {
                Ok(Ok(channels)) => return Some(channels),
                Ok(Err(e)) => log::debug!("pvp reconnect attempt failed: {e}"),
                Err(_) => log::debug!("pvp reconnect attempt timed out"),
            }
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => return None,
                _ = crate::platform::sleep(RECONNECT_BACKOFF) => {}
            }
        }
    }
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Link { .. }")
    }
}

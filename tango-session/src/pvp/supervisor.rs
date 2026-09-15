//! Network readiness, receive pumping, reconnection, and orderly shutdown.

use super::{EndState, Metrics};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Session-redraw cadence while reconnecting (~30 fps), so the give-up progress
/// bar drains smoothly even though the paused drive loop emits no frames. Purely
/// cosmetic.
const RECONNECT_UI_TICK: std::time::Duration = std::time::Duration::from_millis(33);

/// Grace window after a successful reconnect during which the stall watchdog
/// is suppressed. On reconnect the local input queue is still pegged at
/// [`crate::net::data::RECONNECT_QUEUE_LENGTH`] — that's *why* we reconnected
/// — and the resumed drive loop only drains it back down as the peer's resent
/// window arrives. Without this grace the still-high `queue_len` would re-trip
/// the stall the instant the supervisor loops back, re-pausing the drive loop
/// before it could ingest a single resend: the transport renegotiates forever
/// while the sim never resumes. The grace ends early the moment the queue
/// recovers (drops below the threshold); the deadline only bounds the wait so
/// a reconnect that genuinely fails to recover still re-trips.
const RECONNECT_DRAIN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

// ---------------------------------------------------------------------------
// The ready gate's announcer.

/// Tell the peer our pair is primed, once the boot says so.
///
/// A task of its own because the boot is synchronous and the host
/// chooses where it runs — a blocking thread on the desktop, the event
/// loop in a browser — so it can't await a send itself.
pub(super) fn spawn_primed_announcer(
    link: Arc<crate::net::link::Link>,
    local_primed: Arc<AtomicBool>,
    announce: Arc<tokio::sync::Notify>,
    end: EndState,
    cancel: tokio_util::sync::CancellationToken,
) {
    crate::platform::spawn(async move {
        while !local_primed.load(Ordering::Acquire) {
            tokio::select! {
                _ = cancel.cancelled() => return,
                // `notify_one` before the first `notified()` still wakes
                // it: Notify holds the permit. Re-checking the flag on
                // each pass is what makes the ordering irrelevant.
                _ = announce.notified() => {}
            }
        }
        match link.send_primed().await {
            Ok(()) => log::info!("pvp: announced primed"),
            Err(e) => {
                // The control channel is reliable and ordered, so this
                // only fails when it is dead or dying — its status
                // settled to Error/Closed, or libdatachannel refused the
                // write. There is nothing to retry into: the peer's gate
                // never opens without this packet, and a gate that never
                // opens is a match that hangs with both sides idle. End
                // it as the disconnect it is.
                log::warn!("pvp: could not announce primed, ending match: {e}");
                end.remote_disconnected.store(true, Ordering::Release);
                cancel.cancel();
            }
        }
    });
}

// ---------------------------------------------------------------------------
// The receive pump + link supervisor.

pub(super) struct SupervisorContext {
    pub(super) link: Arc<crate::net::link::Link>,
    pub(super) in_match: crate::net::InMatchTx,
    pub(super) event_tx: std::sync::mpsc::Sender<crate::net::data::Input>,
    pub(super) end: EndState,
    pub(super) completed: Arc<AtomicBool>,
    pub(super) cancel: tokio_util::sync::CancellationToken,
    pub(super) metrics: Arc<Metrics>,
    pub(super) drive_paused: Arc<crate::PauseGate>,
    pub(super) wake: Arc<tokio::sync::Notify>,
    /// Whether our own pair is primed, so a reconnect can re-announce it
    /// (the rebuild drops anything the old transport hadn't delivered).
    pub(super) local_primed: Arc<AtomicBool>,
    /// Process-exit waiter, cancelled after the bounded quit announcement and
    /// the rest of supervisor teardown have completed.
    pub(super) done: tokio_util::sync::CancellationToken,
}

/// Pump one receiver until error/EOF, forwarding events to the drive
/// thread. Returns when the channel dies (the reconnect decision is the
/// supervisor's).
async fn run_receive_pump(
    mut receiver: crate::net::PvpReceiver,
    event_tx: std::sync::mpsc::Sender<crate::net::data::Input>,
    wake: Arc<tokio::sync::Notify>,
) -> std::io::Error {
    loop {
        match receiver.receive().await {
            Ok(input) => {
                if event_tx.send(input).is_err() {
                    return std::io::Error::new(std::io::ErrorKind::BrokenPipe, "drive thread gone");
                }
                // Remote inputs settle ticks; make sure a paused/idle UI
                // still observes progress.
                wake.notify_one();
            }
            Err(e) => return e,
        }
    }
}

/// Receive loop + link supervisor. Reads peer frames into the drive
/// thread's queue until the match ends (completion / cancel) or, when the
/// link drops, until reconnection gives up. Policy lives here — deciding
/// when a trip is worth reconnecting and freezing/unfreezing the drive
/// loop around the attempt. The transport surgery (silent teardown,
/// rebuild, hot-swap under the persistent rennet streams) is
/// [`crate::net::link::Link::reconnect`]'s; the lockstep sim treats the
/// whole gap as a pause, so no state resync is needed.
pub(super) fn spawn_supervisor(ctx: SupervisorContext) {
    let SupervisorContext {
        link,
        in_match,
        event_tx,
        end,
        completed,
        cancel,
        metrics,
        drive_paused,
        wake,
        local_primed,
        done,
    } = ctx;

    let make_receiver = {
        let link = link.clone();
        let in_match = in_match.clone();
        let end = end.clone();
        let wake = wake.clone();
        move || -> Option<crate::net::PvpReceiver> {
            Some(crate::net::PvpReceiver::new(
                link.take_match_receiver()?,
                in_match.clone(),
                link.latency_handle(),
                end.remote_ended.clone(),
                wake.clone(),
            ))
        }
    };

    crate::platform::spawn(async move {
        // Why the receive loop ended this iteration.
        enum Trip {
            /// Clean local teardown (user closed / cancelled). Announces the
            /// quit to the peer (best-effort `Goodbye`), never reconnects.
            Cancelled,
            /// The peer announced a deliberate quit (the control channel's
            /// `Goodbye`): it is leaving and will never be at a rendezvous,
            /// so the match ends at once — no reconnect window at all.
            PeerQuit,
            /// A channel hit EOF without a goodbye — the peer's reconnect
            /// dropping its old transport (libdatachannel closes gracefully;
            /// there is no silent teardown), its transport declaring the
            /// link dead, or a quit whose goodbye was lost. An unannounced
            /// close is treated as recoverable link loss and gets the normal
            /// per-transport reconnect window.
            Closed,
            /// The local input queue climbed to `RECONNECT_QUEUE_LENGTH`:
            /// the peer stopped matching our inputs, i.e. a quiet/dead link.
            /// Reconnects on the full per-transport window.
            Stalled,
        }

        let mut receiver = make_receiver().expect("bring_up parks the in-match receiver");
        // Set after each successful reconnect to `now + RECONNECT_DRAIN_GRACE`:
        // the stall watch stays quiet until the queue recovers or this passes,
        // so the just-reconnected (still-full) queue can't instantly re-trip.
        let mut drain_until: Option<web_time::Instant> = None;
        loop {
            // The stall watch: poll the drive thread's published queue
            // length. Coarse (10 Hz) is fine — the queue takes seconds to
            // climb to the trip point. A queue below the threshold clears any
            // post-reconnect drain grace (the link recovered); a queue at or
            // above it trips a stall — unless we're still inside the grace,
            // where a high queue is the expected pre-drain state.
            let stall_watch = async {
                loop {
                    let queue_len = metrics.queue_len.load(Ordering::Relaxed) as usize;
                    if queue_len < crate::net::data::RECONNECT_QUEUE_LENGTH {
                        drain_until = None;
                    } else if drain_until.is_none_or(|t| web_time::Instant::now() >= t) {
                        return;
                    }
                    crate::platform::sleep(std::time::Duration::from_millis(100)).await;
                }
            };
            let trip = tokio::select! {
                biased;
                _ = cancel.cancelled() => Trip::Cancelled,
                end = link.watch_control() => match end {
                    crate::net::link::ControlEnd::Goodbye => {
                        log::info!("pvp: peer announced a quit");
                        Trip::PeerQuit
                    }
                    crate::net::link::ControlEnd::Eof => Trip::Closed,
                },
                e = run_receive_pump(receiver, event_tx.clone(), wake.clone()) => {
                    log::info!("pvp in-match channel closed: {e:?}");
                    Trip::Closed
                }
                _ = stall_watch => Trip::Stalled,
            };

            // Our own deliberate close: announce it, then stop. The
            // session's teardown drops the peer connection gracefully, and
            // its DTLS close_notify hands the peer a prompt EOF — but that
            // EOF alone is ambiguous over there (our reconnect's transport
            // drop looks identical), so send the goodbye first to let the
            // peer end at once instead of entering its reconnect path on us.
            // Best-effort: if it's lost, ordinary reconnect is the fallback.
            if matches!(trip, Trip::Cancelled) {
                link.send_goodbye().await;
                break;
            }

            // Reconnect on any mid-match link loss — a stalled input queue
            // *or* a bare channel close — as long as the transport can
            // rebuild and the match isn't ending (our completion, the
            // peer's EndOfMatch, or an in-game abort). An announced quit
            // (`PeerQuit`) never reconnects — the peer told us it isn't
            // coming back; every unannounced loss uses the normal transport
            // budget.
            let reconnectable = matches!(trip, Trip::Stalled | Trip::Closed)
                && link.can_reconnect()
                && !completed.load(Ordering::Acquire)
                && !end.remote_ended.load(Ordering::Acquire)
                && !end.aborted.load(Ordering::Acquire);
            if !reconnectable {
                // An in-game abort ends BOTH sessions from their own
                // simulations at (nearly) the same instant — the peer's
                // teardown racing ours to the wire (its goodbye, its
                // channel's EOF) is expected, not a disconnect, so don't
                // dress the end as one.
                if !end.aborted.load(Ordering::Acquire) {
                    end.remote_disconnected.store(true, Ordering::Release);
                }
                cancel.cancel();
                break;
            }

            // Freeze the drive loop so its speculative lead can't run past
            // the rollback horizon while the link is down. Both peers
            // converge on the rebuild: whoever trips first goes silent,
            // which stall-trips the other within RECONNECT_QUEUE_LENGTH
            // frames.
            drive_paused.set(true);
            wake.notify_one();
            log::info!("pvp link dropped — pausing to reconnect");

            // Rebuild + hot-swap (the link's job), ticking the UI at
            // ~30 fps so the give-up bar drains smoothly while the paused
            // drive loop produces no frames.
            let restored = {
                let ui_tick = async {
                    let mut iv = crate::platform::Ticker::immediate(RECONNECT_UI_TICK);
                    loop {
                        iv.tick().await;
                        wake.notify_one();
                    }
                };
                let timeout = link.reconnect_timeout().unwrap_or_default();
                let watchdog_done = tokio_util::sync::CancellationToken::new();
                {
                    let end = end.clone();
                    let cancel = cancel.clone();
                    let drive_paused = drive_paused.clone();
                    let wake = wake.clone();
                    let watchdog_done = watchdog_done.clone();
                    // This task is deliberately independent of the reconnect
                    // future: when the displayed budget runs out, end the
                    // session even if WebRTC cleanup is still stuck.
                    crate::platform::spawn(async move {
                        tokio::select! {
                            biased;
                            _ = watchdog_done.cancelled() => {}
                            _ = cancel.cancelled() => {}
                            _ = crate::platform::sleep(timeout) => {
                                end.remote_disconnected.store(true, Ordering::Release);
                                drive_paused.set(false);
                                cancel.cancel();
                                wake.notify_one();
                            }
                        }
                    });
                }

                let restored = tokio::select! {
                    restored = link.reconnect() => restored,
                    _ = ui_tick => unreachable!(),
                };
                watchdog_done.cancel();
                restored
            };

            if !restored {
                // Timed out or cancelled — give up and end the match.
                end.remote_disconnected.store(true, Ordering::Release);
                cancel.cancel();
                drive_paused.set(false);
                break;
            }

            // Fresh receiver over the swapped channel, same `in_match` —
            // the rennet in-stream (seq/ack) carries across the swap, so
            // the peer's resent window fills our gap contiguously. The
            // drive loop resumes; its stall guard holds it below the
            // horizon until the resends drain the queue.
            receiver = make_receiver().expect("reconnect parks a fresh receiver");
            // The swap rebuilds the control channel too, so an earlier
            // `Primed` may have died with the old transport. Say it again:
            // a peer still waiting on the ready gate has nothing else to
            // wait for, and one it has already latched is a no-op. A
            // rejected send means the fresh channel is already gone —
            // same as the first announcement, there is nothing to retry
            // into, so end rather than leave the peer at a gate that
            // never opens.
            if local_primed.load(Ordering::Acquire) {
                if let Err(e) = link.send_primed().await {
                    log::warn!("pvp: primed re-announce failed after reconnect, ending match: {e}");
                    end.remote_disconnected.store(true, Ordering::Release);
                    cancel.cancel();
                    drive_paused.set(false);
                    break;
                }
            }
            // Hold the stall watch off until the resumed drive loop drains the
            // still-full queue back below the threshold (or the grace lapses),
            // so the stale-high `queue_len` can't instantly re-trip the stall
            // and bounce us straight back into another reconnect.
            drain_until = Some(web_time::Instant::now() + RECONNECT_DRAIN_GRACE);
            drive_paused.set(false);
            wake.notify_one();
            log::info!("pvp transparently reconnected the link");
        }

        // Teardown: retire latency so `latency()` reads `None` and the
        // telemetry panel retires, and wake the session to re-check
        // `is_ended` (the drive loop may already be gone, so no frame is
        // coming).
        link.retire_latency();
        drive_paused.set(false);
        wake.notify_one();
        done.cancel();
    });
}

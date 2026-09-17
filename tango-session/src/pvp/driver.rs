//! Boot and advance a match, publish frames, and finalize confirmed inputs.

use super::{EndState, Metrics, TpsCounter, MAX_FRAME_DELAY, MIN_FRAME_DELAY, PEER_END_GRACE};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tango_match::telemetry;

/// What the driver needs to boot the [`tango_match::Match`].
pub(super) struct BootPieces {
    pub(super) roms: [Vec<u8>; 2],
    pub(super) saves: [Vec<u8>; 2],
    /// Both sides' netplay support, in seat order. Starting the match
    /// goes through these rather than an engine, so a DS game boots the
    /// same way a GBA one does.
    pub(super) pvp: [&'static (dyn tango_match::Backend + Send + Sync); 2],
    /// The peer's cartridge — a match can span two variants and two
    /// regions, and the local game's crate resolves that seat's engine
    /// support from it.
    pub(super) peer_rom: tango_match::PeerRom,
    pub(super) match_type: (u8, u8),
    pub(super) rng_seed: [u8; 16],
    pub(super) rtc: std::time::SystemTime,
    pub(super) local_player: usize,
    pub(super) present_delay: u32,
    pub(super) disable_bgm: bool,
}

pub(super) struct DriveContext {
    pub(super) local_input: Arc<crate::InputCell>,
    pub(super) frame_delay: Arc<AtomicU32>,
    pub(super) metrics: Arc<Metrics>,
    pub(super) drive_paused: Arc<crate::PauseGate>,
    pub(super) cancel: tokio_util::sync::CancellationToken,
    pub(super) completed: Arc<AtomicBool>,
    /// See the session's copy.
    pub(super) displayed_screens: Arc<std::sync::atomic::AtomicU8>,
    pub(super) end: EndState,
    pub(super) event_rx: std::sync::mpsc::Receiver<crate::net::data::Input>,
    pub(super) sender: crate::net::PvpSender,
    pub(super) in_match: crate::net::InMatchTx,
    pub(super) replay_writer: Option<tango_replay::Writer>,
    pub(super) stats: Arc<Mutex<tango_match::analysis::StatsBuilder>>,
    /// Recording identity and the host's optional statistics destination.
    pub(super) stats_key: Option<std::path::PathBuf>,
    pub(super) stats_sink: Option<Arc<dyn crate::stats::StatsSink>>,
    pub(super) tps_counter: Arc<Mutex<TpsCounter>>,
    pub(super) screen: Arc<crate::Framebuffer>,
    pub(super) wake: Arc<tokio::sync::Notify>,
    /// The ready gate. `local_primed` is set (and `announce_primed`
    /// notified) the moment our pair reaches its link battle;
    /// `peer_primed` is latched by the link's control watch when the
    /// peer says the same. [`PvpDriver::tick`] advances nothing until
    /// both are set.
    pub(super) local_primed: Arc<AtomicBool>,
    pub(super) peer_primed: Arc<AtomicBool>,
    pub(super) announce_primed: Arc<tokio::sync::Notify>,
    pub(super) local_player: usize,
    /// The close signal as the priming walk can read it — a plain flag
    /// because backends know nothing of this crate's cancellation
    /// token, raised beside it by
    /// [`request_close`](crate::Session::request_close).
    pub(super) boot_cancel: Arc<AtomicBool>,
}

impl DriveContext {
    /// Boot the match, then hand back the driver that runs it and a
    /// readout handle to its pair.
    fn boot(
        self,
        pieces: BootPieces,
        expected_fps: f32,
        audio: tango_match::AudioIn,
    ) -> Result<PvpDriver, tango_match::Error> {
        // The game's registration starts the match on whatever engine it
        // runs; this session never learns which.
        let local = pieces.pvp[pieces.local_player];
        let match_ = local.start(tango_match::StartConfig {
            roms: [&pieces.roms[0], &pieces.roms[1]],
            saves: [Some(&pieces.saves[0]), Some(&pieces.saves[1])],
            rng_seed: pieces.rng_seed,
            rtc: pieces.rtc,
            match_type: pieces.match_type,
            peer_rom: pieces.peer_rom,
            local_player: pieces.local_player,
            present_delay: pieces.present_delay.clamp(MIN_FRAME_DELAY, MAX_FRAME_DELAY),
            disable_bgm: pieces.disable_bgm,
            // The pair pushes the local seat's sound into the ring the
            // host's stream is already bound to, on its way out of every
            // tick.
            audio: Some(audio),
            // The session is on screen (and leavable) for the whole
            // walk, so a close has to reach into it — otherwise the
            // host's drive-thread join waits the walk out.
            cancel: Some(&self.boot_cancel),
        })?;

        // Our half of the ready gate: the pair is at its link battle.
        // Release the announcer so the peer learns it — priming ran at
        // whatever speed this machine manages, and until both sides are
        // here neither may advance a tick.
        self.local_primed.store(true, Ordering::Release);
        self.announce_primed.notify_one();

        Ok(PvpDriver {
            ctx: self,
            expected_fps,
            match_,
            throttler: tango_match::Throttler::new(),
            fired_end_of_match: false,
            confirmed_through: 0,
        })
    }

    /// Fold a batch of confirmed telemetry into the stats builder (the
    /// shared [`tango_match::analysis::fold_confirmed`], so live stats
    /// and offline re-analysis stay byte-equivalent).
    fn fold_confirmed_telemetry(
        &mut self,
        samples: Vec<(u32, telemetry::BattleObs)>,
        events: Vec<(u32, telemetry::Event)>,
    ) {
        let mut stats = self.stats.lock().unwrap();
        tango_match::analysis::fold_confirmed(&mut stats, self.local_player, samples, events);
    }
}

/// The match as the thing a host drives: the first tick boots and
/// primes the pair, every tick after that runs the match.
///
/// The boot rides the drive loop rather than [`super::PvpSession::new`]
/// because priming the pair is seconds of blocking emulation and the
/// session is on screen for all of it — saying so ([`is_booting`](super::PvpSession::is_booting),
/// [`prime_error`](super::PvpSession::prime_error)) instead of making the
/// user wait at the lobby with nothing to read. Replay playback comes
/// up the same way, and for the same reason.
pub struct PvpBoot {
    /// What the first tick needs to bring the pair up, taken there.
    /// `None` once the boot has run, whichever way it went.
    pub(super) pending: Option<(BootPieces, DriveContext)>,
    /// The live match, once the boot has produced one.
    pub(super) driver: Option<PvpDriver>,
    /// The producing end of the ring the host's stream is already bound
    /// to, handed to the pair when the boot builds it. `None` once it
    /// has been. Until then the ring reads empty and the stream primes.
    pub(super) audio: Option<tango_match::AudioIn>,
    /// Where a failed boot leaves its reason, for the session to
    /// publish ([`super::PvpSession::prime_error`]).
    pub(super) prime_error: Arc<Mutex<Option<crate::Error>>>,
    pub(super) expected_fps: f32,
    pub(super) metrics: Arc<Metrics>,
    /// Repaint wake, so the failure reaches a host whose session is
    /// otherwise sitting on a frame that will never come.
    pub(super) wake: Arc<tokio::sync::Notify>,
    /// The engine the boot will run on, for its readiness gate.
    pub(super) backend: &'static (dyn tango_match::Backend + Send + Sync),
}

impl crate::Drive for PvpBoot {
    fn tick(&mut self) -> bool {
        // What `prepare` started may still be coming up — a browser
        // engine's worker threads finish starting only between ticks,
        // while the host's loop yields. Booting before then would spin
        // on threads that can never arrive, so the boot waits out the
        // gate; the priming notice is already up either way.
        if self.pending.is_some() && !self.backend.ready(2) {
            return true;
        }
        if let Some((pieces, drive)) = self.pending.take() {
            let audio = self.audio.take().expect("the boot runs once");
            let booted = drive.boot(pieces, self.expected_fps, audio);
            // Either outcome changes what the session shows, and
            // neither produces a frame — so wake the host itself.
            match booted {
                Ok(driver) => {
                    self.driver = Some(driver);
                    self.wake.notify_one();
                }
                // Cancelled is the session being torn down mid-walk,
                // not a failure to report: there is nobody left to
                // read it.
                Err(tango_match::Error::Cancelled) => return false,
                Err(e) => {
                    log::error!("pvp: boot failed: {e}");
                    *self.prime_error.lock().unwrap() = Some(e.into());
                    self.wake.notify_one();
                    return false;
                }
            }
        }
        match self.driver.as_mut() {
            Some(driver) => driver.tick(),
            // A boot that failed leaves the session up with its reason
            // on screen; there is simply nothing left to drive.
            None => false,
        }
    }

    fn finish(self) {
        // Only a match that ran has a replay tail to write.
        if let Some(driver) = self.driver {
            driver.finish();
        }
    }

    /// The throttler's target once the match runs; before that, the
    /// rate the pacer should idle the boot at.
    fn fps_target(&self) -> f32 {
        f32::from_bits(self.metrics.fps_target.load(Ordering::Relaxed))
    }
}

/// A received wire input in the seam's vocabulary. The wire's touch is
/// a byte per axis — every touch screen a backend has fits one — so
/// widening here is lossless.
fn host_input_of_wire(input: &crate::net::data::Input) -> tango_match::HostInput {
    tango_match::HostInput {
        keys: input.joyflags as u32,
        touch: input.touch.map(|(x, y)| (x as u16, y as u16)),
    }
}

/// A committed input as the wire ships it. The engine sanitized it
/// through the backend's conversions, so the narrowing casts cannot
/// truncate.
fn wire_input_of(input: tango_match::HostInput, tick_advantage: i16) -> crate::net::data::Input {
    crate::net::data::Input {
        joyflags: input.keys as u16,
        touch: input.touch.map(|(x, y)| (x.min(0xff) as u8, y.min(0xff) as u8)),
        tick_advantage,
    }
}

/// A confirmed input as the replay records it — same narrowing story as
/// [`wire_input_of`].
fn replay_input_of(input: tango_match::HostInput) -> tango_replay::stream::Input {
    tango_replay::stream::Input {
        keys: input.keys as u16,
        touch: input.touch.map(|(x, y)| (x.min(0xff) as u8, y.min(0xff) as u8)),
    }
}

/// A live match, one tick at a time.
///
/// The state below used to be locals of a `loop` on a thread of its
/// own; naming it is what lets a host drive the match instead — a
/// desktop thread paced by the wall clock today, an event loop once the
/// browser's signaling exists.
pub struct PvpDriver {
    ctx: DriveContext,
    expected_fps: f32,
    match_: tango_match::Match,
    throttler: tango_match::Throttler,
    fired_end_of_match: bool,
    /// Number of authoritative input rows consumed so far: the tick boundary
    /// shared by the replay stream and confirmed telemetry.
    confirmed_through: u32,
}

impl PvpDriver {
    /// Advance the match one tick. `false` once it's over — cancelled,
    /// a dead link, or a failed advance — after which the host calls
    /// [`PvpDriver::finish`].
    pub fn tick(&mut self) -> bool {
        if self.ctx.cancel.is_cancelled() {
            return false;
        }
        if self.ctx.drive_paused.paused() {
            // The reconnect supervisor releases the gate on every
            // exit path. Nothing to advance meanwhile, so the tick
            // goes back to the host — which is also what lets a
            // browser pause without a thread to park.
            return true;
        }

        // The ready gate. Reaching `tick` means our own pair is primed;
        // hold here until the peer says the same, so both sides start
        // within a round trip of each other.
        //
        // Advancing alone is not merely early — `advance` buffers a
        // local input per tick that the absent peer cannot match, and at
        // `RECONNECT_QUEUE_LENGTH` (~3 s) the supervisor's stall watch
        // reads that as a dead link and tears the transport down for a
        // reconnect the peer never needed. Idling here keeps the
        // published `queue_len` at zero, so the watch stays quiet however
        // long the slower machine takes; the in-match heartbeat runs off
        // its own task and keeps the link alive meanwhile.
        //
        // The wait is bounded without a timer: a peer that never primes
        // fails its own boot (`MAX_PRIME_TICKS`) and tears down, which
        // reaches us as the control channel's `Goodbye` or EOF.
        if !self.ctx.peer_primed.load(Ordering::Acquire) {
            return true;
        }

        // Live present-delay adjustment from the footer slider.
        let pd = self.ctx.frame_delay.load(Ordering::Relaxed);
        if pd != self.match_.present_delay() {
            self.match_.set_present_delay(pd);
        }

        // Drain the network before advancing: every confirmed tick we
        // ingest now is a rollback we don't take deeper.
        for input in self.ctx.event_rx.try_iter() {
            self.match_
                .add_remote_input(host_input_of_wire(&input), input.tick_advantage);
        }

        // Stall guard: the peer is too far behind (or gone) — advancing
        // further would run the input stream past the rennet horizon.
        // The in-match heartbeat keeps the redundancy window + acks
        // flowing while we wait; the supervisor watches queue_len and
        // decides whether this is a reconnect.
        //
        // But hold ONLY when advancing can't make progress. `advance` is
        // the sole thing that drains the local queue (it matches buffered
        // remote inputs against local ones and confirms the pairs); merely
        // ingesting remote inputs above just buffers them. So if the peer
        // is still feeding us matchable inputs — exactly the case during a
        // post-reconnect resend burst — we must keep advancing to settle
        // them, even at a full queue: each such advance nets the queue
        // *down* (drains ≥1, adds 1 local). Skipping advance whenever the
        // queue is full instead would leave those resends forever
        // unconsumed — the queue never drains, the stall never clears, and
        // the link "reconnects but never resumes". Only a genuinely dead
        // link (nothing matchable) parks here.
        let queue_len = self.match_.local_queue_length() as u32;
        self.ctx.metrics.queue_len.store(queue_len, Ordering::Relaxed);
        if queue_len as usize >= crate::net::data::RECONNECT_QUEUE_LENGTH && self.match_.matchable() == 0 {
            // Block on the event queue rather than poll it: the only
            // thing that clears a stall is the peer's next input, and
            // it arrives exactly here. Ingest it and loop — the drain
            // + stall re-check above decide whether we're unstuck.
            // The only thing that clears a stall is the peer's
            // next input. Take whatever has landed and give the
            // tick back; the drain + stall re-check above decide
            // next time whether we're unstuck. A dead channel
            // yields too — the supervisor owns what happens next,
            // and spinning on it would burn the host's loop.
            if let Ok(input) = self.ctx.event_rx.try_recv() {
                self.match_
                    .add_remote_input(host_input_of_wire(&input), input.tick_advantage);
            }
            return true;
        }

        // Sample the skew before `advance` enqueues this tick's local
        // input, so our half matches the advantage we ship the peer.
        let skew = self.match_.skew();
        self.ctx.metrics.skew.store(skew, Ordering::Relaxed);

        let mut local = self.ctx.local_input.load();
        local.keys &= tango_match::keys::MASK;
        let advanced = match self.match_.advance(local) {
            Ok(r) => r,
            Err(e) => {
                log::error!("pvp: sio advance failed: {e}");
                self.ctx.cancel.cancel();
                return false;
            }
        };
        self.ctx
            .metrics
            .depth
            .store(self.match_.last_rollback_depth(), Ordering::Relaxed);

        // A successful advance returns every row it settled. Nothing else
        // settles the engine, so every early return below is already consumed
        // and teardown has no tail path.
        let tango_match::Advance {
            outgoing,
            tick_advantage,
            confirmed_inputs,
            ..
        } = advanced;
        self.confirmed_through += confirmed_inputs.len() as u32;
        let (samples, events) = match self.match_.telemetry() {
            Some(telemetry) => telemetry.lock().unwrap().take_through(self.confirmed_through),
            None => (Vec::new(), Vec::new()),
        };
        let match_ended = events
            .iter()
            .any(|(_, event)| matches!(event, telemetry::Event::MatchEnded));
        if let Some(w) = self.ctx.replay_writer.as_mut() {
            for inputs in &confirmed_inputs {
                if let Err(e) = w.write_input(inputs.map(replay_input_of)) {
                    log::warn!("pvp: replay write failed (recording stops): {e}");
                    self.ctx.replay_writer = None;
                    break;
                }
            }
        }
        if !samples.is_empty() || !events.is_empty() {
            self.ctx.fold_confirmed_telemetry(samples, events);
        }

        // Ship this tick's local input. Push-before-send semantics live
        // in the pump; a transport error is non-terminal (the heartbeat
        // retransmits once the reconnect swaps a live channel back in).
        if self.ctx.sender.send(&wire_input_of(outgoing, tick_advantage)).is_err() {
            log::warn!("pvp: send pump terminated; ending match");
            self.ctx.end.remote_disconnected.store(true, Ordering::Release);
            self.ctx.cancel.cancel();
            return false;
        }

        // The players abandoned the match in-game before any battle
        // (the store's abort latch — see it for why this needs no
        // confirmation wait): end uncleanly, right now. The peer's
        // simulation raises the same latch on the same pair tick, so
        // both sessions end together — neither side's teardown reads
        // as a disconnect to the other (the supervisor checks this
        // flag on every trip).
        if self
            .match_
            .telemetry()
            .is_some_and(|handle| handle.lock().unwrap().aborted())
        {
            log::info!("pvp: match aborted in-game before any battle; ending uncleanly");
            self.ctx.end.aborted.store(true, Ordering::Release);
            self.ctx.cancel.cancel();
            return false;
        }

        // Completion: the games' own match-end path ran (confirmed —
        // both peers see it at the same pair tick). No runout needed:
        // the anchor fires after the result screens have played.
        if match_ended {
            self.ctx.completed.store(true, Ordering::Release);
        }
        if self.ctx.completed.load(Ordering::Acquire) && !self.fired_end_of_match {
            self.fired_end_of_match = true;
            let first_completion = {
                let mut completed_at = self.ctx.end.local_ended_at.lock().unwrap();
                if completed_at.is_some() {
                    false
                } else {
                    *completed_at = Some(web_time::Instant::now());
                    true
                }
            };
            if first_completion {
                // In-band EndOfMatch: rides the same ordered seq stream
                // as inputs, so the peer sees it exactly once and only
                // after every preceding input.
                let in_match = self.ctx.in_match.clone();
                crate::platform::spawn(async move {
                    if let Err(e) = in_match.send_end_of_match().await {
                        log::warn!("pvp: send EndOfMatch failed: {e}");
                    }
                });
                // Wall-clock fallback wake so `is_ended` is rechecked
                // even if the peer never sends EndOfMatch.
                let wake = self.ctx.wake.clone();
                crate::platform::spawn(async move {
                    crate::platform::sleep(PEER_END_GRACE).await;
                    wake.notify_one();
                });
            }
        }

        // Present the local screen to the UI. (Audio needs no push —
        // the output stream pulls it straight off the pair.)
        if let Some(buf) = self.match_.frame() {
            self.ctx.screen.write(&buf);
        }
        self.ctx.tps_counter.lock().unwrap().mark();
        self.ctx.wake.notify_one();

        // Whatever the host is arranging to show, handed over each tick:
        // a console composes nothing for a screen nobody is looking at,
        // and the setting behind this can move mid-match.
        self.match_
            .set_displayed_screens(self.ctx.displayed_screens.load(Ordering::Relaxed));

        // Clock sync: only the leading peer shaves tick rate, and only
        // once the presented frame actually speculates past the present
        // delay.
        let slowdown = self.throttler.step(skew, self.match_.speculation_balance());
        let target = self.expected_fps - slowdown;
        self.ctx.metrics.fps_target.store(target.to_bits(), Ordering::Relaxed);

        true
    }

    /// Flush the replay tail and cache the match's stats. Called once,
    /// after [`PvpDriver::tick`] reports the match over — reached through
    /// [`crate::Drive::finish`], which is why a host must wind a driver
    /// down rather than drop it.
    pub fn finish(mut self) {
        // Finalize (write the EOR sentinel) only if the match completed
        // — same policy as the trap engine, so an aborted match leaves
        // a truncated-but-parseable recording.
        if let Some(w) = self.ctx.replay_writer.take() {
            if self.ctx.completed.load(Ordering::Acquire) {
                if let Err(e) = w.finish() {
                    log::error!("finish replay failed: {e}");
                }
            }
            // Cache the match's stats, completed or not: the fold keeps
            // a round the match never decided (only its verdict is
            // absent), and writing the sidecar here is what spares the
            // Replays tab a full re-simulation of an aborted match — the
            // telemetry it would recover was already collected live.
            if let (Some(sink), Some(key)) = (self.ctx.stats_sink.as_ref(), self.ctx.stats_key.as_ref()) {
                let snapshot = self.ctx.stats.lock().unwrap().snapshot();
                if let Err(e) = sink.record(key, &snapshot) {
                    log::warn!("failed to write replay stats cache entry: {e}");
                }
            }
        }
    }
}

impl crate::Drive for PvpDriver {
    fn tick(&mut self) -> bool {
        PvpDriver::tick(self)
    }

    fn finish(self) {
        PvpDriver::finish(self)
    }

    /// The throttler's target: the base rate minus whatever it shaved
    /// to bring the peers' clocks together.
    fn fps_target(&self) -> f32 {
        f32::from_bits(self.ctx.metrics.fps_target.load(Ordering::Relaxed))
    }
}

//! Data plane: the live in-match protocol.
//!
//! tango's concrete [`protocol`] [`Element`](protocol::Element) (wired into the
//! generic [`rennet`] frame codec + redundancy-window / cumulative-ack
//! reliability streams), plus the [`InMatchTx`] / [`PvpSender`] / [`PvpReceiver`]
//! adapters that run them over the unreliable in-match channel and present the
//! engine's ordered `tango_pvp::net::Event` stream. Loss-tolerant by design — it
//! never assumes the reliable/ordered guarantee the control plane relies on.

pub mod protocol;

use super::{LatencyCounter, Receiver, Sender};

/// The in-match streams' element type: tango's [`protocol::Element`].
type OutStream = rennet::OutStream<protocol::Element>;
type InStream = rennet::InStream<protocol::Element>;

/// Send-pump queue depth. Deeper than the engine's unacked-local-input cap
/// so that under a genuinely stalled wire the engine's overflow bail fires
/// before the pump's channel ever blocks the frame — backpressure semantics
/// match the old inline send. The slack on top covers the non-Input events
/// interleaved into the same channel (one `EndOfRound` per round).
const SEND_PUMP_DEPTH: usize = tango_pvp::battle::MAX_QUEUE_LENGTH + 8;

/// Upper bound on the outstanding-send timestamps kept for RTT measurement.
/// The deque is trimmed by the peer's acks every frame, so it stays tiny in
/// steady state; this only bounds the book-keeping if the peer goes silent
/// (acks stop), at which point a sample beyond the rollback horizon would be
/// meaningless anyway. Sized to the horizon for that reason.
const MAX_RTT_SAMPLES: usize = tango_pvp::battle::MAX_QUEUE_LENGTH;

/// Shared per-match stream state: the outbound seq/redundancy window
/// ([`rennet::OutStream`]) and the inbound reorder/ack machinery
/// ([`rennet::InStream`]). Locked briefly — no awaits held — from the send
/// pump, the receive loop, and the session's in-band EndOfMatch fire.
struct InMatchState {
    out: OutStream,
    inn: InStream,
    /// First-send wall time of each unconfirmed seq, ascending by seq. Pushed
    /// when a brand-new seq ships; popped as the peer's cumulative ack confirms
    /// it, and the freshest just-confirmed entry dates the round-trip. This is
    /// how RTT is measured now that there's no separate ping/pong probe — the
    /// ack the peer already piggybacks on every frame is the echo.
    sent_times: std::collections::VecDeque<(u32, std::time::Instant)>,
    /// Smoothed round-trip, fed by each ack-derived sample. Drives the
    /// out-stream's adaptive redundancy floor (see [`recv`](InMatchTx::recv)).
    /// `None` until the first sample.
    rtt_ewma: Option<std::time::Duration>,
}

/// EWMA weight for a fresh RTT sample (the rest carries the prior estimate).
/// Per-seq samples are jittery; smoothing keeps the redundancy floor from
/// flapping on a single late ack. A flap would only add/drop one window element
/// (~2 bytes) anyway, so this is about steadiness, not correctness.
const RTT_EWMA_ALPHA: f64 = 0.125;

/// Frames of round-trip that add one element to the proactive redundancy floor.
/// The floor is a continuous `1 + rtt_frames / FRAMES_PER_REDUNDANCY` (rounded,
/// then capped at [`rennet::MAX_REDUNDANCY`]), where `rtt_frames` is the smoothed
/// RTT in [`InMatchTx::heartbeat`] (≈ one frame) units. A redundant copy recovers
/// a dropped datagram in ~one frame, where an ack-driven resend costs ~one whole
/// RTT — so the deeper the RTT, the more a copy is worth. At a sub-frame (LAN)
/// round-trip the floor stays 1; it reaches the cap near a 4-frame round-trip.
const FRAMES_PER_REDUNDANCY: f64 = 2.0;

/// What [`InMatchTx::recv`] extracts from one incoming frame: the elements it
/// made contiguous (seq order), the freshest frame-advantage for the throttler,
/// and an RTT sample if the frame's ack confirmed a timestamped seq of ours.
struct Delivery {
    elements: Vec<protocol::Element>,
    tick_advantage: i16,
    rtt: Option<std::time::Duration>,
}

/// Send handle for the unreliable in-match channel (tango-rtc stream-1 data
/// channel). Shared by the per-frame input pump, the receive loop's ack
/// replies, and the session's in-band `EndOfMatch`. Carries [`protocol`] frames
/// over [`Sender::send_raw`].
#[derive(Clone)]
pub struct InMatchTx {
    state: std::sync::Arc<std::sync::Mutex<InMatchState>>,
    sink: std::sync::Arc<tokio::sync::Mutex<Sender>>,
    /// Count of data frames (inputs/markers) sent. The heartbeat watches this
    /// to tell "the emulator is sending" from "the emulator has stalled".
    data_sends: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// Retransmit-heartbeat cadence. The heartbeat task is spawned in [`new`]
    /// and runs for the life of the channel.
    heartbeat: std::time::Duration,
}

impl InMatchTx {
    /// Build the in-match send handle and start its retransmit heartbeat. The
    /// heartbeat is transparent — callers just `send_*`; the channel keeps the
    /// unacked window flowing on its own when the emulator goes quiet (see
    /// [`run_heartbeat`](Self::run_heartbeat)). `heartbeat` is the resend
    /// cadence (the caller picks it — typically one emulator frame). `cancel`
    /// stops the heartbeat at match teardown — and *only* then: a transport
    /// error doesn't end it, so the heartbeat keeps running (and the unacked
    /// window keeps living in the out-stream) across a mid-match reconnect,
    /// resending the moment the coordinator swaps a live `sink` back in.
    ///
    /// Must be called within a Tokio runtime (it spawns the heartbeat task).
    pub fn new(
        sink: std::sync::Arc<tokio::sync::Mutex<Sender>>,
        heartbeat: std::time::Duration,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Self {
        let this = Self {
            state: std::sync::Arc::new(std::sync::Mutex::new(InMatchState {
                out: OutStream::new(protocol::HORIZON),
                inn: InStream::new(protocol::HORIZON),
                sent_times: std::collections::VecDeque::new(),
                rtt_ewma: None,
            })),
            sink,
            data_sends: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            heartbeat,
        };
        tokio::spawn(this.clone().run_heartbeat(cancel));
        this
    }

    /// Push an element, snapshot the current redundancy window + cumulative ack into
    /// one frame, and ship it. The state lock is dropped before the await.
    async fn send_frame_with(&self, push: impl FnOnce(&mut OutStream)) -> std::io::Result<()> {
        self.data_sends.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let frame = {
            let mut st = self.state.lock().unwrap();
            push(&mut st.out);
            // Stamp the newest seq's first-send time for ack-derived RTT. Only a
            // brand-new (higher) seq lands here; resends and the heartbeat don't
            // touch this, so the time stays anchored to first transmission.
            if let Some(seq) = st.out.newest_seq() {
                if st.sent_times.back().map_or(true, |&(prev, _)| seq > prev) {
                    st.sent_times.push_back((seq, std::time::Instant::now()));
                    while st.sent_times.len() > MAX_RTT_SAMPLES {
                        st.sent_times.pop_front();
                    }
                }
            }
            let w = st.out.window().expect("window is non-empty after a push");
            let ack = st.inn.ack();
            protocol::data_frame(w.base, w.tick_advantage, w.entries, ack)
        };
        self.sink.lock().await.send_raw(&frame.encode()).await
    }

    pub async fn send_input(&self, joyflags: u16, tick_advantage: i16) -> std::io::Result<()> {
        self.send_frame_with(move |out| {
            out.push_advantaged(
                protocol::Element::Input(joyflags & tango_pvp::input::JOYFLAGS_MASK),
                tick_advantage,
            );
        })
        .await
    }

    pub async fn send_end_of_round(&self) -> std::io::Result<()> {
        self.send_frame_with(move |out| {
            out.push(protocol::Element::EndOfRound);
        })
        .await
    }

    /// In-band match-end: rides the same ordered seq stream as inputs, so the
    /// peer sees it exactly once and only after every preceding input.
    pub async fn send_end_of_match(&self) -> std::io::Result<()> {
        self.send_frame_with(move |out| {
            out.push(protocol::Element::EndOfMatch);
        })
        .await
    }

    /// Re-send the current redundancy window (or a bare ack if no inputs yet)
    /// without advancing the stream — the heartbeat's retransmit. Lets recovery
    /// and the peer's acks keep flowing while the emulator is throttled/stalled.
    /// (Also keeps RTT samples coming: the peer keeps acking our resent window,
    /// and each returning ack dates a round-trip.)
    async fn resend_window(&self) -> std::io::Result<()> {
        let frame = {
            let st = self.state.lock().unwrap();
            let ack = st.inn.ack();
            match st.out.window() {
                Some(w) => protocol::data_frame(w.base, w.tick_advantage, w.entries, ack),
                None => protocol::Frame::ack_only(st.out.next_seq(), ack),
            }
        };
        self.sink.lock().await.send_raw(&frame.encode()).await
    }

    /// Retransmit heartbeat: keep the unacked window (and our ack) flowing at
    /// roughly the frame rate even when the emulator slows or stalls.
    ///
    /// Sends are otherwise emulator-driven, so throttling the local sim to brake
    /// a runaway lead would also throttle *recovery* — defeating the brake and
    /// risking a bail on otherwise-recoverable loss. This decouples the two: it
    /// resends only on intervals where the emulator sent nothing, so it adds no
    /// traffic during normal play but fills the gaps during a stall. Runs until
    /// `cancel` fires (match teardown) — a transport error is non-terminal so it
    /// survives a mid-match reconnect.
    async fn run_heartbeat(self, cancel: tokio_util::sync::CancellationToken) {
        let mut interval = tokio::time::interval(self.heartbeat);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_seen = self.data_sends.load(std::sync::atomic::Ordering::Relaxed);
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                _ = interval.tick() => {}
            }
            let now = self.data_sends.load(std::sync::atomic::Ordering::Relaxed);
            if now != last_seen {
                // The emulator sent a frame this interval; nothing to fill in.
                last_seen = now;
                continue;
            }
            if let Err(e) = self.resend_window().await {
                // Not terminal: mid-reconnect the `sink` is a dropped channel,
                // and the coordinator swaps a live one back under us. Keep
                // ticking — the unacked window persists in the out-stream across
                // the swap, so the next resend bridges the whole gap. Only
                // `cancel` (teardown) stops this loop.
                log::debug!("pvp heartbeat resend failed (continuing): {e}");
            }
        }
    }

    /// Apply an incoming frame: feed its cumulative ack to the out-stream and its
    /// entries to the in-stream. Returns the newly-contiguous elements (seq
    /// order), the freshest frame-advantage, and an RTT sample when the ack
    /// confirmed one of our timestamped seqs. `Err` => a gap blew past the
    /// rollback horizon and the match must tear down.
    fn recv(&self, frame: &protocol::Frame) -> Result<Delivery, rennet::HorizonExceeded> {
        let mut st = self.state.lock().unwrap();
        // The ack is the out-stream's concern (it acks what we sent); the
        // in-stream reassembly below only consumes the data entries. Every frame
        // carries one in its header, so applying it is unconditional.
        st.out.apply_ack(frame.ack());
        // The ack confirms every seq below the peer's frontier. Retire the
        // confirmed first-send timestamps; the newest one just retired dates
        // the round-trip (now − when we first sent that seq).
        let frontier = st.out.peer_ack_base();
        let mut confirmed_at = None;
        while st.sent_times.front().is_some_and(|&(seq, _)| seq < frontier) {
            confirmed_at = st.sent_times.pop_front().map(|(_, t)| t);
        }
        let rtt = confirmed_at.map(|t| std::time::Instant::now().saturating_duration_since(t));
        // Re-aim the proactive redundancy floor at the smoothed round-trip:
        // the deeper the RTT, the more an ack-driven resend would cost, so
        // the more a redundant copy in the very next datagram is worth.
        if let Some(sample) = rtt {
            let ewma = match st.rtt_ewma {
                Some(prev) => prev.mul_f64(1.0 - RTT_EWMA_ALPHA) + sample.mul_f64(RTT_EWMA_ALPHA),
                None => sample,
            };
            st.rtt_ewma = Some(ewma);
            let frames = ewma.as_secs_f64() / self.heartbeat.as_secs_f64();
            // `set_min_redundancy` clamps to [1, MAX_REDUNDANCY]; the f64->u32
            // cast saturates, so a degenerate (huge/inf/NaN) `frames` is safe.
            let redundancy = (1.0 + frames / FRAMES_PER_REDUNDANCY).round() as u32;
            st.out.set_min_redundancy(redundancy);
        }
        let delivered = st.inn.accept(frame)?;
        Ok(Delivery {
            elements: delivered.entries,
            tick_advantage: delivered.tick_advantage,
            rtt,
        })
    }
}

/// `tango_pvp::net::Sender` adapter — pushes each per-frame `Event` through a
/// bounded pump ([`SEND_PUMP_DEPTH`]) that ships it as a [`protocol`] frame over
/// the unreliable in-match channel. The pump keeps the emulator thread off the
/// shared sink mutex (also taken by the heartbeat's window resends) and off the
/// await, and preserves Input/EndOfRound ordering into the out-stream's seq
/// space.
pub struct PvpSender {
    tx: tokio::sync::mpsc::Sender<tango_pvp::net::Event>,
}

impl PvpSender {
    pub fn new(im: InMatchTx) -> Self {
        // The retransmit heartbeat is the in-match channel's own concern
        // (started by `InMatchTx::new`), so the pump just forwards events.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<tango_pvp::net::Event>(SEND_PUMP_DEPTH);
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let result = match event {
                    tango_pvp::net::Event::Input(input) => im.send_input(input.joyflags, input.tick_advantage).await,
                    tango_pvp::net::Event::EndOfRound => im.send_end_of_round().await,
                };
                if let Err(e) = result {
                    // Non-terminal: the element was pushed into the out-stream's
                    // window *before* the send await (see `send_frame_with`), so a
                    // failed send loses nothing — the heartbeat/next frame
                    // retransmits it once the reconnect coordinator swaps a live
                    // channel back in. Keep pumping; the task ends when `rx`
                    // closes (the `PvpSender`/`Match` dropped at teardown).
                    log::debug!("pvp send pump (continuing): {e}");
                }
            }
        });
        Self { tx }
    }
}

impl tango_pvp::net::Sender for PvpSender {
    fn send(&mut self, event: &tango_pvp::net::Event) -> std::io::Result<()> {
        // blocking_send, not try_send: the pump channel feeds the reliable
        // out-stream window, so dropping here would lose an input *before* it
        // becomes retransmittable — a permanent hole in the ordered input
        // stream, i.e. desync. Blocking applies the same backpressure the old
        // `send().await` did (the emulator thread waited on channel space via
        // block_on then too), and is safe only because the emulator thread no
        // longer has a tokio runtime entered.
        self.tx
            .blocking_send(event.clone())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pvp send pump terminated"))
    }
}

/// `tango_pvp::net::Receiver` adapter — reads [`protocol`] frames off the
/// unreliable in-match channel, feeds them through the shared [`InMatchTx`]
/// reassembly, and yields the resulting `Event`s in strict seq order. The ack
/// piggybacked on each frame drives both loss recovery and the latency readout:
/// when it confirms one of our timestamped seqs, the round-trip is marked as a
/// latency sample (there's no separate ping/pong probe). An in-band
/// `EndOfMatch` marker raises `remote_ended`. One frame can deliver several
/// elements, so surplus events buffer in `pending` and drain before the next
/// read.
pub struct PvpReceiver {
    receiver: Receiver,
    im: InMatchTx,
    /// `None` once the remote drops — the session swaps the counter out so the
    /// UI can tell "no live link" from "0 ms ping on LAN". While the link is up
    /// it's `Some` and latency samples land here.
    latency_counter: std::sync::Arc<tokio::sync::Mutex<Option<LatencyCounter>>>,
    /// Flipped `true` the first time an in-band `EndOfMatch` marker is
    /// delivered. `PvpSession::is_ended` reads this to know the remote reached
    /// its match_end_ret hook and the connection is safe to tear down.
    remote_ended: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Session subscription wake, pinged after `remote_ended` flips so
    /// `is_ended` is re-checked without waiting on the next vblank.
    end_of_match_notify: std::sync::Arc<tokio::sync::Notify>,
    /// Elements made contiguous by the last frame but not yet yielded.
    pending: std::collections::VecDeque<tango_pvp::net::Event>,
}

impl PvpReceiver {
    pub fn new(
        receiver: Receiver,
        im: InMatchTx,
        latency_counter: std::sync::Arc<tokio::sync::Mutex<Option<LatencyCounter>>>,
        remote_ended: std::sync::Arc<std::sync::atomic::AtomicBool>,
        end_of_match_notify: std::sync::Arc<tokio::sync::Notify>,
    ) -> Self {
        Self {
            receiver,
            im,
            latency_counter,
            remote_ended,
            end_of_match_notify,
            pending: std::collections::VecDeque::new(),
        }
    }
}

#[async_trait::async_trait]
impl tango_pvp::net::Receiver for PvpReceiver {
    async fn receive(&mut self) -> std::io::Result<tango_pvp::net::Event> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(event);
            }
            let msg = self.receiver.recv_raw().await?;
            let frame = protocol::Frame::decode(&msg)?;
            let delivery = self
                .im
                .recv(&frame)
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "remote overflowed our input buffer"))?;
            // A returning ack that confirmed one of our seqs dates a round-trip.
            if let Some(rtt) = delivery.rtt {
                if let Some(c) = self.latency_counter.lock().await.as_mut() {
                    c.mark(rtt);
                }
            }
            for element in delivery.elements {
                match element {
                    protocol::Element::Input(joyflags) => {
                        self.pending
                            .push_back(tango_pvp::net::Event::Input(tango_pvp::net::Input {
                                joyflags,
                                tick_advantage: delivery.tick_advantage,
                            }));
                    }
                    protocol::Element::EndOfRound => {
                        self.pending.push_back(tango_pvp::net::Event::EndOfRound);
                    }
                    protocol::Element::EndOfMatch => {
                        self.remote_ended.store(true, std::sync::atomic::Ordering::Release);
                        self.end_of_match_notify.notify_one();
                    }
                }
            }
        }
    }
}

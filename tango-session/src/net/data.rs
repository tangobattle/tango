//! Data plane: the live in-match protocol.
//!
//! tango's concrete [`protocol`] [`Element`](protocol::Element) (wired into the
//! generic [`rennet`] frame codec + redundancy-window / cumulative-ack
//! reliability streams), plus the [`InMatchTx`] / [`PvpSender`] / [`PvpReceiver`]
//! adapters that run them over the unreliable in-match channel and present the
//! match's ordered [`Input`] stream. Loss-tolerant by design — it never assumes
//! the reliable/ordered guarantee the control plane relies on.

use tango_net_protocol::data as protocol;

use super::{LatencyCounter, PacketSink, PacketStream};

/// Send half of the data plane's transport: a thin, untyped byte pipe over the
/// unreliable in-match channel. Where the control plane's
/// [`Sender`](super::Sender) frames typed `Packet`s, this one ships whatever
/// bytes it's handed — a `protocol::Frame` already owns its wire format — so its
/// whole surface is [`send`](Self::send). Pairs with [`Receiver`].
pub struct Sender {
    sink: Box<dyn PacketSink>,
}

impl Sender {
    pub fn new(sink: Box<dyn PacketSink>) -> Self {
        Self { sink }
    }

    /// Ship one pre-serialized `protocol::Frame` (or any opaque datagram) to the
    /// peer — one `send` is one datagram on the wire.
    pub async fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.sink.send(bytes).await
    }
}

/// Receive half of the data plane's transport — the counterpart to [`Sender`].
/// Yields whole datagrams as raw bytes; the caller ([`PvpReceiver`]) decodes each
/// into a `protocol::Frame`. A clean channel close surfaces as
/// `io::ErrorKind::UnexpectedEof`.
pub struct Receiver {
    stream: Box<dyn PacketStream>,
}

impl Receiver {
    pub fn new(stream: Box<dyn PacketStream>) -> Self {
        Self { stream }
    }

    /// Read one datagram off the unreliable channel.
    pub async fn recv(&mut self) -> std::io::Result<Vec<u8>> {
        self.stream.recv().await
    }
}

/// The in-match streams, keyed on tango's [`protocol::InMatch`] descriptor.
type OutStream = rennet::OutStream<protocol::InMatch>;
type InStream = rennet::InStream<protocol::InMatch>;

/// In-match input-buffer budget — two coupled depths expressed as one.
///
/// The depth the session waits for before declaring a dead link and the
/// rollback horizon the match bails at used to be tuned by hand (the former as
/// a silence *duration*) and could drift apart. [`RECONNECT_QUEUE_LENGTH`] is
/// now the single knob; [`MAX_QUEUE_LENGTH`] (the horizon) is *derived* from it,
/// so the horizon can't end up smaller than the depth it has to out-cover.
///
/// Why watch the queue and not elapsed silence: a dead link keeps the sim
/// committing ~one local input per displayed frame (the throttler caps its
/// slowdown, so it never fully stalls) with nothing from the peer to match them
/// against, so the local input queue climbs steadily. The session polls that
/// depth directly and pauses to reconnect once it reaches
/// [`RECONNECT_QUEUE_LENGTH`]. Measuring the very resource that overflows — not
/// a time proxy for it — means the trip can't drift from the bail no matter how
/// fast the throttled sim actually grows the queue: the watchdog always fires a
/// fixed margin below the horizon.
///
/// That margin is [`STALL_HEADROOM`]: it covers the watchdog's poll interval and
/// the frame or two the pause takes to land, plus a safety factor — sized so the
/// overflow bail can never beat the watchdog + pause to the punch.
///
/// The session ([`crate::pvp`]) reads [`RECONNECT_QUEUE_LENGTH`] back
/// to drive its watchdog. Lower it to trip reconnect sooner (the horizon shrinks
/// with it); raise it to ride out longer blips (the horizon grows). Nothing else
/// to retune.
///
/// 180 frames ≈ 3 s of play (at 60 fps, just above the GBA frame rate).
/// The value lives with the wire codec in tango-net-protocol (the
/// horizon is protocol-visible — every implementation must agree).
pub use tango_net_protocol::data::{MAX_QUEUE_LENGTH, RECONNECT_QUEUE_LENGTH};

// The pure codec crate carries its own copy of the joyflags mask so it
// doesn't drag in the emulator stack; this crate sees both, so a drift
// becomes a build failure here.
const _: () = assert!(
    tango_net_protocol::data::JOYFLAGS_MASK == tango_match::input::JOYFLAGS_MASK,
    "tango-net-protocol's JOYFLAGS_MASK drifted from tango-match's"
);

/// Send-pump queue depth. Deeper than the unacked-local-input cap so that
/// under a genuinely stalled wire the overflow bail fires before the pump's
/// channel ever blocks the frame — backpressure semantics match the old
/// inline send. The slack on top is margin, nothing sized against it.
const SEND_PUMP_DEPTH: usize = MAX_QUEUE_LENGTH + 8;

/// Upper bound on the outstanding-send timestamps kept for RTT measurement.
/// The deque is trimmed by the peer's acks every frame, so it stays tiny in
/// steady state; this only bounds the book-keeping if the peer goes silent
/// (acks stop), at which point a sample beyond the rollback horizon would be
/// meaningless anyway. Sized to the horizon for that reason.
const MAX_RTT_SAMPLES: usize = MAX_QUEUE_LENGTH;

/// One tick's input as it crosses the wire, oriented to its sender: the
/// committed local joyflags for that tick, plus the sender's local tick
/// advantage at send time — how far its local input leads the remote input
/// it has received (the input queue's signed lead). The receiver subtracts
/// the advantage from its own to get the raw skew that drives the time-sync
/// throttler ([`tango_match::Throttler`]). Tick is positional — seq order on
/// the wire, never embedded.
#[derive(Clone, Debug)]
pub struct Input {
    pub joyflags: u16,
    pub tick_advantage: i16,
}

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
}

/// What [`InMatchTx::recv`] extracts from one incoming frame: the elements it
/// made contiguous (seq order), the freshest per-frame [`protocol::Meta`] (whose
/// `tick_advantage` feeds the throttler), and an RTT sample if the frame's ack
/// confirmed a timestamped seq of ours.
struct Delivery {
    elements: Vec<protocol::Element>,
    meta: protocol::Meta,
    rtt: Option<std::time::Duration>,
}

/// Send handle for the unreliable in-match channel (stream-1 data channel).
/// Shared by the per-frame input pump, the receive loop's ack
/// replies, and the session's in-band `EndOfMatch`. Carries [`protocol`] frames
/// over [`Sender::send`].
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
    pub fn new(sink: Sender, heartbeat: std::time::Duration, cancel: tokio_util::sync::CancellationToken) -> Self {
        let this = Self {
            state: std::sync::Arc::new(std::sync::Mutex::new(InMatchState {
                out: OutStream::new(protocol::HORIZON),
                inn: InStream::new(protocol::HORIZON),
                sent_times: std::collections::VecDeque::new(),
            })),
            sink: std::sync::Arc::new(tokio::sync::Mutex::new(sink)),
            data_sends: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            heartbeat,
        };
        tokio::spawn(this.clone().run_heartbeat(cancel));
        this
    }

    /// Retarget the sink at a rebuilt transport — the reconnect hot-swap. The
    /// out/in streams (and the unacked window) are untouched: the heartbeat's
    /// next resend through the new sink bridges the whole outage.
    pub async fn swap_sink(&self, new: Sender) {
        *self.sink.lock().await = new;
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
                if st.sent_times.back().is_none_or(|&(prev, _)| seq > prev) {
                    st.sent_times.push_back((seq, std::time::Instant::now()));
                    while st.sent_times.len() > MAX_RTT_SAMPLES {
                        st.sent_times.pop_front();
                    }
                }
            }
            let w = st.out.window();
            let ack = st.inn.ack();
            protocol::data_frame(w.base, ack, w.meta, w.entries)
        };
        self.sink.lock().await.send(&frame.to_vec()).await
    }

    pub async fn send_input(&self, joyflags: u16, tick_advantage: i16) -> std::io::Result<()> {
        self.send_frame_with(move |out| {
            out.push_with_meta(
                protocol::Element::Input(joyflags & tango_match::input::JOYFLAGS_MASK),
                protocol::Meta { tick_advantage },
            );
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

    /// Re-send the current redundancy window without advancing the stream — the
    /// heartbeat's retransmit. Before the first input the window is empty, so this
    /// is just a bare ack. Lets recovery and the peer's acks keep flowing while
    /// the emulator is throttled/stalled. (Also keeps RTT samples coming: the
    /// peer keeps acking our resent window, and each returning ack dates a
    /// round-trip.)
    async fn resend_window(&self) -> std::io::Result<()> {
        let frame = {
            let st = self.state.lock().unwrap();
            let ack = st.inn.ack();
            let w = st.out.window();
            protocol::data_frame(w.base, ack, w.meta, w.entries)
        };
        self.sink.lock().await.send(&frame.to_vec()).await
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
        let delivered = st.inn.accept(frame)?;
        Ok(Delivery {
            elements: delivered.entries,
            meta: delivered.meta,
            rtt,
        })
    }
}

/// Send adapter — pushes each per-frame [`Input`] through a bounded pump
/// ([`SEND_PUMP_DEPTH`]) that ships it as a [`protocol`] frame over the
/// unreliable in-match channel. The pump keeps the emulator thread off the
/// shared sink mutex (also taken by the heartbeat's window resends) and off the
/// await, and preserves input ordering into the out-stream's seq space.
pub struct PvpSender {
    tx: tokio::sync::mpsc::Sender<Input>,
}

impl PvpSender {
    pub fn new(im: InMatchTx) -> Self {
        // The retransmit heartbeat is the in-match channel's own concern
        // (started by `InMatchTx::new`), so the pump just forwards inputs.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Input>(SEND_PUMP_DEPTH);
        tokio::spawn(async move {
            while let Some(input) = rx.recv().await {
                if let Err(e) = im.send_input(input.joyflags, input.tick_advantage).await {
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

    pub fn send(&mut self, input: &Input) -> std::io::Result<()> {
        // blocking_send, not try_send: the pump channel feeds the reliable
        // out-stream window, so dropping here would lose an input *before* it
        // becomes retransmittable — a permanent hole in the ordered input
        // stream, i.e. desync. Blocking applies the same backpressure the old
        // `send().await` did (the emulator thread waited on channel space via
        // block_on then too), and is safe only because the emulator thread no
        // longer has a tokio runtime entered.
        self.tx
            .blocking_send(input.clone())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pvp send pump terminated"))
    }
}

/// Receive adapter — reads [`protocol`] frames off the unreliable in-match
/// channel, feeds them through the shared [`InMatchTx`]
/// reassembly, and yields the resulting [`Input`]s in strict seq order. The ack
/// piggybacked on each frame drives both loss recovery and the latency readout:
/// when it confirms one of our timestamped seqs, the round-trip is marked as a
/// latency sample (there's no separate ping/pong probe). An in-band
/// `EndOfMatch` marker raises `remote_ended`. One frame can deliver several
/// elements, so surplus inputs buffer in `pending` and drain before the next
/// read.
pub struct PvpReceiver {
    receiver: Receiver,
    im: InMatchTx,
    /// `None` once the remote drops — the session swaps the counter out so the
    /// UI can tell "no live link" from "0 ms ping on LAN". While the link is up
    /// it's `Some` and latency samples land here.
    latency_counter: std::sync::Arc<std::sync::Mutex<Option<LatencyCounter>>>,
    /// Flipped `true` the first time an in-band `EndOfMatch` marker is
    /// delivered. `PvpSession::is_ended` reads this to know the remote reached
    /// its match_end_ret hook and the connection is safe to tear down.
    remote_ended: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Session subscription wake, pinged after `remote_ended` flips so
    /// `is_ended` is re-checked without waiting on the next vblank.
    end_of_match_notify: std::sync::Arc<tokio::sync::Notify>,
    /// Inputs made contiguous by the last frame but not yet yielded.
    pending: std::collections::VecDeque<Input>,
}

impl PvpReceiver {
    pub fn new(
        receiver: Receiver,
        im: InMatchTx,
        latency_counter: std::sync::Arc<std::sync::Mutex<Option<LatencyCounter>>>,
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

    pub async fn receive(&mut self) -> std::io::Result<Input> {
        loop {
            if let Some(input) = self.pending.pop_front() {
                return Ok(input);
            }
            let msg = self.receiver.recv().await?;
            let frame = protocol::Frame::decode(&mut &msg[..])?;
            let delivery = self
                .im
                .recv(&frame)
                .map_err(|_| std::io::Error::other("remote overflowed our input buffer"))?;
            // A returning ack that confirmed one of our seqs dates a round-trip.
            if let Some(rtt) = delivery.rtt {
                if let Some(c) = self.latency_counter.lock().unwrap().as_mut() {
                    c.mark(rtt);
                }
            }
            for element in delivery.elements {
                match element {
                    protocol::Element::Input(joyflags) => {
                        self.pending.push_back(Input {
                            joyflags,
                            tick_advantage: delivery.meta.tick_advantage,
                        });
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

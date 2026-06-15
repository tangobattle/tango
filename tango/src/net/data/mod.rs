//! Data plane: the live in-match protocol.
//!
//! The byte-minimized [`protocol`] frame codec, the [`stream`] reliability state
//! machines (redundancy window + cumulative-ack reassembly), and the [`InMatchTx`]
//! / [`PvpSender`] / [`PvpReceiver`] adapters that run them over the
//! unreliable in-match channel and present the engine's ordered
//! `tango_pvp::net::Event` stream. Loss-tolerant by design — it never assumes
//! the reliable/ordered guarantee the control plane relies on.

pub mod protocol;
pub mod stream;

use super::transport::{Receiver, Sender};
use super::{LatencyCounter, PING_INTERVAL};

/// Send-pump queue depth. Deeper than the engine's unacked-local-input cap
/// so that under a genuinely stalled wire the engine's overflow bail fires
/// before the pump's channel ever blocks the frame — backpressure semantics
/// match the old inline send. The slack on top covers the non-Input events
/// interleaved into the same channel (one `EndOfRound` per round).
const SEND_PUMP_DEPTH: usize = tango_pvp::battle::MAX_QUEUE_LENGTH + 8;

/// Sender's short wall-clock stamp (ms, wrapping into a u16) for a ping. The
/// pong echoes it; the round-trip delta is the latency sample.
fn ping_timestamp() -> u16 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u16
}

/// Shared per-match stream state: the outbound seq/redundancy window
/// ([`stream::OutStream`]) and the inbound reorder/ack machinery
/// ([`stream::InStream`]). Locked briefly — no awaits held — from the send
/// pump, the receive loop, and the session's in-band EndOfMatch fire.
struct InMatchState {
    out: stream::OutStream,
    inn: stream::InStream,
}

/// Send handle for the unreliable in-match channel (WebRTC stream-1 data
/// channel, or the direct path's QUIC datagrams). Shared by the per-frame input
/// pump, the receive loop's ping/pong + ack replies, and the session's in-band
/// `EndOfMatch`. Carries [`protocol`] frames + ping/pong over [`Sender::send_raw`].
#[derive(Clone)]
pub struct InMatchTx {
    state: std::sync::Arc<std::sync::Mutex<InMatchState>>,
    sink: std::sync::Arc<tokio::sync::Mutex<Sender>>,
}

impl InMatchTx {
    pub fn new(sink: std::sync::Arc<tokio::sync::Mutex<Sender>>) -> Self {
        Self {
            state: std::sync::Arc::new(std::sync::Mutex::new(InMatchState {
                out: stream::OutStream::new(),
                inn: stream::InStream::new(),
            })),
            sink,
        }
    }

    /// Push an element, snapshot the current redundancy window + cumulative ack into
    /// one frame, and ship it. The state lock is dropped before the await.
    async fn send_frame_with(&self, push: impl FnOnce(&mut stream::OutStream)) -> std::io::Result<()> {
        let frame = {
            let mut st = self.state.lock().unwrap();
            push(&mut st.out);
            let (base, fa, entries) = st.out.window().expect("window is non-empty after a push");
            let ack = st.inn.ack();
            protocol::Packet::Frame(protocol::Frame::data(base, fa, entries, Some(ack)))
        };
        self.sink.lock().await.send_raw(&frame.encode()).await
    }

    pub async fn send_input(&self, joyflags: u16, frame_advantage: i16) -> std::io::Result<()> {
        self.send_frame_with(move |out| {
            out.push_input(joyflags, frame_advantage);
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

    async fn send_ping(&self, ts: u16) -> std::io::Result<()> {
        self.sink
            .lock()
            .await
            .send_raw(&protocol::Packet::Ping(ts).encode())
            .await
    }

    async fn send_pong(&self, ts: u16) -> std::io::Result<()> {
        self.sink
            .lock()
            .await
            .send_raw(&protocol::Packet::Pong(ts).encode())
            .await
    }

    /// Apply an incoming frame: feed its cumulative ack to the out-stream and its
    /// entries to the in-stream. Returns the newly-contiguous elements (seq
    /// order) plus the freshest frame-advantage. `Err` => a gap blew past the
    /// rollback horizon and the match must tear down.
    fn recv(&self, frame: &protocol::Frame) -> Result<(Vec<protocol::Element>, Option<i16>), stream::HorizonExceeded> {
        let mut st = self.state.lock().unwrap();
        // The ack is the out-stream's concern (it acks what we sent);
        // the in-stream reassembly below only consumes the data entries.
        let ack = match frame {
            protocol::Frame::Ack(ack) => Some(*ack),
            protocol::Frame::Data { ack, .. } => *ack,
        };
        if let Some(ack) = ack {
            st.out.apply_ack(ack);
        }
        let delivered = st.inn.accept(frame)?;
        let fa = st.inn.latest_advantage();
        Ok((delivered, fa))
    }
}

/// `tango_pvp::net::Sender` adapter — pushes each per-frame `Event` through a
/// bounded pump ([`SEND_PUMP_DEPTH`]) that ships it as a [`protocol`] frame over
/// the unreliable in-match channel. The pump keeps the emulator thread off the
/// shared sink mutex (contended by the receive loop's ping/pong + ack replies)
/// and off the await, and preserves Input/EndOfRound ordering into the
/// out-stream's seq space.
pub struct PvpSender {
    tx: tokio::sync::mpsc::Sender<tango_pvp::net::Event>,
}

impl PvpSender {
    pub fn new(im: InMatchTx) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<tango_pvp::net::Event>(SEND_PUMP_DEPTH);
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let result = match event {
                    tango_pvp::net::Event::Input(input) => im.send_input(input.joyflags, input.frame_advantage).await,
                    tango_pvp::net::Event::EndOfRound => im.send_end_of_round().await,
                };
                if let Err(e) = result {
                    // Dropping rx closes the channel; the next trap-side send
                    // sees BrokenPipe and cancels the match, same as an inline
                    // send failure would have.
                    log::error!("pvp send pump: {e}");
                    break;
                }
            }
        });
        Self { tx }
    }
}

#[async_trait::async_trait]
impl tango_pvp::net::Sender for PvpSender {
    async fn send(&mut self, event: &tango_pvp::net::Event) -> std::io::Result<()> {
        self.tx
            .send(event.clone())
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pvp send pump terminated"))
    }
}

/// `tango_pvp::net::Receiver` adapter — reads [`protocol`] frames off the
/// unreliable in-match channel, feeds them through the shared [`InMatchTx`]
/// reassembly, and yields the resulting `Event`s in strict seq order. Ping is
/// answered with Pong; Pong marks a latency sample; an in-band `EndOfMatch`
/// marker raises `remote_ended`. One frame can deliver several elements, so
/// surplus events buffer in `pending` and drain before the next read. The ping
/// timer keeps the round-trip clock ticking on the live match's actual
/// (unreliable) path.
pub struct PvpReceiver {
    receiver: Receiver,
    im: InMatchTx,
    /// `None` once the remote drops — the session swaps the counter out so the
    /// UI can tell "no live link" from "0 ms ping on LAN". While the link is up
    /// it's `Some` and ping samples land here.
    latency_counter: std::sync::Arc<tokio::sync::Mutex<Option<LatencyCounter>>>,
    ping_timer: tokio::time::Interval,
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
            ping_timer: tokio::time::interval(PING_INTERVAL),
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
            tokio::select! {
                _ = self.ping_timer.tick() => {
                    self.im.send_ping(ping_timestamp()).await?;
                }
                msg = self.receiver.recv_raw() => {
                    match protocol::Packet::decode(&msg?)? {
                        protocol::Packet::Ping(ts) => {
                            self.im.send_pong(ts).await?;
                        }
                        protocol::Packet::Pong(ts) => {
                            let dt = ping_timestamp().wrapping_sub(ts);
                            if let Some(c) = self.latency_counter.lock().await.as_mut() {
                                c.mark(std::time::Duration::from_millis(dt as u64));
                            }
                        }
                        protocol::Packet::Frame(frame) => {
                            let (delivered, fa) = self.im.recv(&frame).map_err(|_| {
                                std::io::Error::new(
                                    std::io::ErrorKind::Other,
                                    "remote overflowed our input buffer",
                                )
                            })?;
                            let frame_advantage = fa.unwrap_or(0);
                            for element in delivered {
                                match element {
                                    protocol::Element::Input(joyflags) => {
                                        self.pending.push_back(tango_pvp::net::Event::Input(
                                            tango_pvp::net::Input { joyflags, frame_advantage },
                                        ));
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
            }
        }
    }
}

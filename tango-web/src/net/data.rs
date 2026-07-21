//! The data plane, browser flavor: tango's rennet streams over the
//! unreliable in-match channel, shaped like gbaroll's single-thread
//! netplay plumbing — synchronous sends from the tick (no send pump),
//! a receive pump feeding an event channel, and a heartbeat task that
//! resends the redundancy window on intervals where the tick sent
//! nothing (stall/pause/hidden-tab), keeping acks and loss recovery
//! flowing.
//!
//! Wire format and reliability semantics come from
//! `tango_net_protocol::data` + `rennet` — identical bytes to the
//! desktop peer.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Mutex;

use futures::channel::mpsc;
use gloo_timers::future::TimeoutFuture;
use tango_net_protocol::data::{Element, Frame, InMatch, Meta, HORIZON};
use web_time::Instant;

use super::webrtc::{ChannelReceiver, ChannelSender};

type OutStream = rennet::OutStream<InMatch>;
type InStream = rennet::InStream<InMatch>;

/// Heartbeat cadence — ~one emulator frame, clamped to ~1 Hz by the
/// browser in hidden tabs (where the audio pump keeps real sends
/// flowing anyway).
const HEARTBEAT_MS: u32 = 16;

/// How many (seq, send time) samples to keep for ack-derived RTT.
const MAX_RTT_SAMPLES: usize = 256;

/// One tick's input as delivered to the driver, sender-oriented.
pub struct Input {
    pub joyflags: u16,
    pub tick_advantage: i16,
}

/// What the receive pump reports up to the driver.
pub enum NetEvent {
    Input(Input),
    /// The peer's in-band end-of-match marker.
    EndOfMatch,
    /// The channel closed or the stream fell past the horizon. Carries
    /// the transport generation that observed it — stale generations
    /// (an old pump dying after a swap) are ignored.
    Gone { generation: u32 },
}

struct Streams {
    out: OutStream,
    inn: InStream,
    sent_times: VecDeque<(u32, Instant)>,
}

/// The shared send half: the tick pushes + ships, the heartbeat resends.
/// The channel half is swappable — a transparent reconnect replaces the
/// transport while the rennet streams (and their unacked windows)
/// survive, refilling the peer's gap on the first resend.
#[derive(Clone)]
pub struct InMatchTx {
    streams: Rc<Mutex<Streams>>,
    tx: Rc<RefCell<ChannelSender>>,
    sent: Rc<Cell<bool>>,
    rtt_ms: Rc<Cell<Option<f32>>>,
    /// Bumped per transport swap; Gone events from a stale pump are
    /// ignored by the driver.
    generation: Rc<Cell<u32>>,
    event_tx: mpsc::UnboundedSender<NetEvent>,
}

impl InMatchTx {
    /// Push one element with this frame's meta and ship the whole
    /// redundancy window. Transport errors are non-terminal (the
    /// heartbeat retries; a dead channel surfaces via the recv pump).
    pub fn ship(&self, element: Element, meta: Meta) {
        let bytes = {
            let mut s = self.streams.lock().unwrap();
            let seq = s.out.push_with_meta(element, meta);
            s.sent_times.push_back((seq, Instant::now()));
            if s.sent_times.len() > MAX_RTT_SAMPLES {
                s.sent_times.pop_front();
            }
            let w = s.out.window();
            Frame::new(w.base, s.inn.ack(), w.meta, w.entries).to_vec()
        };
        let _ = self.tx.borrow().send(&bytes);
        self.sent.set(true);
    }

    /// The freshest ack-derived round-trip estimate.
    pub fn rtt_ms(&self) -> Option<f32> {
        self.rtt_ms.get()
    }

    /// The current transport generation — Gone events carry the
    /// generation of the pump that observed the close, so the driver
    /// can drop stale ones after a swap.
    pub fn generation(&self) -> u32 {
        self.generation.get()
    }

    /// Swap in a fresh transport after a reconnect: the send half is
    /// replaced, a new receive pump feeds the same streams and event
    /// channel, and the redundancy window ships immediately to refill
    /// the peer's gap.
    pub fn swap_transport(&self, tx: ChannelSender, rx: ChannelReceiver) {
        *self.tx.borrow_mut() = tx;
        self.generation.set(self.generation.get() + 1);
        wasm_bindgen_futures::spawn_local(recv_pump(
            rx,
            self.streams.clone(),
            self.rtt_ms.clone(),
            self.event_tx.clone(),
            self.generation.get(),
        ));
        let bytes = {
            let s = self.streams.lock().unwrap();
            let w = s.out.window();
            Frame::new(w.base, s.inn.ack(), w.meta, w.entries).to_vec()
        };
        let _ = self.tx.borrow().send(&bytes);
    }
}

/// Wire the in-match channel: returns the tick's send half and the
/// driver's event stream, spawning the receive + heartbeat tasks. The
/// `stop` flag ends the heartbeat at teardown.
pub fn wire(
    tx: ChannelSender,
    rx: ChannelReceiver,
    stop: Rc<Cell<bool>>,
) -> (InMatchTx, mpsc::UnboundedReceiver<NetEvent>) {
    let streams = Rc::new(Mutex::new(Streams {
        out: OutStream::new(HORIZON),
        inn: InStream::new(HORIZON),
        sent_times: VecDeque::new(),
    }));
    let sent = Rc::new(Cell::new(false));
    let rtt_ms = Rc::new(Cell::new(None));
    let (event_tx, event_rx) = mpsc::unbounded();
    let tx = Rc::new(RefCell::new(tx));

    wasm_bindgen_futures::spawn_local(recv_pump(
        rx,
        streams.clone(),
        rtt_ms.clone(),
        event_tx.clone(),
        0,
    ));
    wasm_bindgen_futures::spawn_local(heartbeat(
        tx.clone(),
        streams.clone(),
        sent.clone(),
        stop,
    ));

    (
        InMatchTx {
            streams,
            tx,
            sent,
            rtt_ms,
            generation: Rc::new(Cell::new(0)),
            event_tx,
        },
        event_rx,
    )
}

async fn recv_pump(
    mut rx: ChannelReceiver,
    streams: Rc<Mutex<Streams>>,
    rtt_ms: Rc<Cell<Option<f32>>>,
    event_tx: mpsc::UnboundedSender<NetEvent>,
    generation: u32,
) {
    while let Some(dgram) = rx.receive().await {
        let frame = match Frame::decode(&mut &dgram[..]) {
            Ok(f) => f,
            Err(e) => {
                log::debug!("bad in-match datagram: {e}");
                continue;
            }
        };
        let delivered = {
            let mut s = streams.lock().unwrap();
            s.out.apply_ack(frame.ack());
            // Ack-derived RTT: when the peer's frontier passes a
            // timestamped seq, the freshest just-confirmed one dates
            // the round trip.
            let frontier = s.out.peer_ack_base();
            let mut newest = None;
            while s.sent_times.front().is_some_and(|(seq, _)| *seq < frontier) {
                newest = s.sent_times.pop_front();
            }
            if let Some((_, at)) = newest {
                rtt_ms.set(Some(at.elapsed().as_secs_f32() * 1000.0));
            }
            match s.inn.accept(&frame) {
                Ok(window) => window,
                Err(rennet::HorizonExceeded) => {
                    let _ = event_tx.unbounded_send(NetEvent::Gone { generation });
                    return;
                }
            }
        };
        for element in delivered.entries {
            let ev = match element {
                Element::Input(joyflags) => NetEvent::Input(Input {
                    joyflags,
                    tick_advantage: delivered.meta.tick_advantage,
                }),
                Element::EndOfMatch => NetEvent::EndOfMatch,
            };
            if event_tx.unbounded_send(ev).is_err() {
                return;
            }
        }
    }
    let _ = event_tx.unbounded_send(NetEvent::Gone { generation });
}

/// Resend the redundancy window on intervals where the tick sent
/// nothing, so acks and loss recovery keep flowing.
async fn heartbeat(
    tx: Rc<RefCell<ChannelSender>>,
    streams: Rc<Mutex<Streams>>,
    sent: Rc<Cell<bool>>,
    stop: Rc<Cell<bool>>,
) {
    loop {
        TimeoutFuture::new(HEARTBEAT_MS).await;
        if stop.get() {
            return;
        }
        if sent.replace(false) {
            continue;
        }
        let bytes = {
            let s = streams.lock().unwrap();
            let w = s.out.window();
            Frame::new(w.base, s.inn.ack(), w.meta, w.entries).to_vec()
        };
        // A send error here just means the transport is mid-swap; the
        // reconnect refills the peer from the same window.
        let _ = tx.borrow().send(&bytes);
    }
}

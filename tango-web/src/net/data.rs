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

use std::cell::Cell;
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
    /// The channel closed or the stream fell past the horizon.
    Gone,
}

struct Streams {
    out: OutStream,
    inn: InStream,
    sent_times: VecDeque<(u32, Instant)>,
}

/// The shared send half: the tick pushes + ships, the heartbeat resends.
#[derive(Clone)]
pub struct InMatchTx {
    streams: Rc<Mutex<Streams>>,
    tx: ChannelSender,
    sent: Rc<Cell<bool>>,
    rtt_ms: Rc<Cell<Option<f32>>>,
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
        let _ = self.tx.send(&bytes);
        self.sent.set(true);
    }

    /// The freshest ack-derived round-trip estimate.
    pub fn rtt_ms(&self) -> Option<f32> {
        self.rtt_ms.get()
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

    wasm_bindgen_futures::spawn_local(recv_pump(
        rx,
        streams.clone(),
        rtt_ms.clone(),
        event_tx,
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
        },
        event_rx,
    )
}

async fn recv_pump(
    mut rx: ChannelReceiver,
    streams: Rc<Mutex<Streams>>,
    rtt_ms: Rc<Cell<Option<f32>>>,
    event_tx: mpsc::UnboundedSender<NetEvent>,
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
                    let _ = event_tx.unbounded_send(NetEvent::Gone);
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
    let _ = event_tx.unbounded_send(NetEvent::Gone);
}

/// Resend the redundancy window on intervals where the tick sent
/// nothing, so acks and loss recovery keep flowing.
async fn heartbeat(
    tx: ChannelSender,
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
        if tx.send(&bytes).is_err() {
            return;
        }
    }
}

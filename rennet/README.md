# rennet

Reliable, ordered delivery over an **unreliable, unordered datagram channel**, in
Rust — the live-match netplay transport, split out as a transport-, engine-, and
packing-agnostic crate.

A rollback netcode session (see the sibling `getgud` crate) needs every peer
input delivered exactly once, in order. But the live match runs over a lossy
datagram channel — a WebRTC unordered/unreliable data channel — where datagrams
drop, reorder, and arrive twice. rennet is the layer in between: it turns that
channel into an in-order element stream and recovers losses **proactively** — a
lost element rides again in the *next* frame's redundancy window, so a single or
short loss costs about one frame rather than a round-trip.

The crate is **pure**: no async, no I/O, no transport, no clock. You pump `Frame`
bytes over whatever datagram channel you have and map the delivered elements to
your own event type. rennet owns the seq line, the redundancy window, the
cumulative ack, and the byte-minimized envelope — nothing above or below it.

## Two layers

### `frame` — the wire envelope

The on-wire `Frame<B>`: a per-tick seq (`base`), a piggybacked cumulative `ack`,
an optional time-sync `tick_advantage`, and an opaque `Body` of entries. One
datagram is exactly one `Frame` — there is no envelope tag, and no separate
ping/pong probe (round-trip latency falls out of the ack round-trip). It is
byte-minimized: LEB128 varints, and the ack travels as a *signed delta from
`base`* rather than an absolute frontier (both counters advance per tick, so they
differ only by the lead/redundancy span — one byte instead of three over a long
match).

rennet owns only that envelope; the `Body` owns its own bytes. *How* a body packs
its elements — continuation-delimited, length-prefixed, fixed-width — is entirely
the caller's business.

### `stream` — the reliability state machines

Two halves, both pure and generic over the element type `E`:

- **`OutStream`** — assigns a monotonic seq to each local element, keeps a
  redundancy window of recent unconfirmed elements, and trims it as the peer's
  cumulative acks confirm receipt. `OutStream::window` is what goes into an
  outbound `Frame`.
- **`InStream`** — reassembles the peer's stream from possibly-lossy, reordered,
  duplicated frames: a reorder buffer feeds elements out in strict seq order
  (`InStream::accept`), dedups redundant copies, generates the cumulative ack to
  send back (`InStream::ack`), and bails when a gap grows past the rollback
  horizon.

Recovery is proactive, not request/response: the ack only drives window
*trimming* (and would drive selective resend for bursts longer than the window).
Nothing here knows what an element *means* — the caller maps elements to its own
event type.

## Wire format

Because the body is **last**, the datagram boundary delimits it: the body never
self-delimits, carries no length prefix, and an ack-only frame is told from a data
frame simply by whether any bytes follow the header.

```text
base             uvarint   always
ack              svarint   always; encoded as (frontier − base)
tick_advantage  svarint   present iff a body follows
body             Body      present iff there are bytes left; runs to the
                           end of the datagram
```

## The `Body` trait

You supply one type — the packing of your element run — by implementing `Body`.
rennet never inspects the packing: it calls `encode` / `decode` and reads
`elements` for the reliability streams. Because the body is the datagram tail,
`decode` is handed exactly its own bytes and `encode` just appends.

```rust
pub trait Body: Sized {
    type Elem;
    fn encode(&self, out: &mut Vec<u8>);     // append to the datagram tail
    fn decode(bytes: &[u8]) -> std::io::Result<Self>;  // exactly the tail bytes
    fn elements(&self) -> &[Self::Elem];     // entries, in seq order
}
```

Markers (round/match boundaries, etc.) ride in-band on the same seq line as
ordinary inputs, so an element type is typically an enum of "input" plus whatever
boundaries your engine needs.

## Usage

Each peer holds one `OutStream` and one `InStream`, both built with the engine's
rollback horizon (the depth past which a gap can never be reconciled, so the
receiver bails rather than buffering forever):

```rust
let mut out = OutStream::<Element>::new(HORIZON);
let mut inn = InStream::<Element>::new(HORIZON);
```

**Sending** — push local elements, then build a frame from the current window
(or an ack-only frame before the first push), tagging it with the ack the
in-stream wants next:

```rust
out.push_advantaged(Element::Input(joyflags), local_tick_advantage); // or out.push(marker)

let ack = inn.ack();
let frame = match out.window() {
    Some(w) => Frame::data(w.base, w.tick_advantage, MyBody(w.entries), ack),
    None    => Frame::ack_only(out.next_seq(), ack),
};
send_datagram(&frame.encode());
```

**Receiving** — apply the peer's ack to trim your window, then ingest the frame;
`accept` returns the run that just became contiguous (possibly empty), in strict
seq order, tagged with the freshest time-sync advantage:

```rust
let frame = Frame::<MyBody>::decode(&datagram)?;
out.apply_ack(frame.ack());
let delivered = inn.accept(&frame)?;   // Err(HorizonExceeded) → tear the match down
for element in delivered.entries {
    feed_engine(element, delivered.tick_advantage);
}
```

The redundancy floor is **adaptive**: drive `out.set_min_redundancy(n)` from the
measured RTT. Redundancy buys recovery in ~one frame instead of the ack-driven
resend's full round-trip, so a longer RTT makes a deeper floor worth more and a
sub-frame RTT makes it worthless. RTT itself is derived from the ack round-trip —
`out.newest_seq()` is the seq to timestamp, and `out.peer_ack_base()` advancing
past it tells you the round-trip is known.

## Key terms

- **Seq / `base`** — every element gets a monotonic 0-based seq; `base` is the seq
  of a frame's first entry (or, on an ack-only frame, the sender's next unsent
  seq).
- **Redundancy window** — the recent unconfirmed elements re-sent on every data
  frame. Its floor is the proactive recovery budget; it grows automatically while
  the peer lags and shrinks as acks confirm receipt.
- **Cumulative ack** — the receiver's contiguous frontier: the lowest seq it
  hasn't received, i.e. "resend your window from here." A contiguous resend window
  is all the sender can act on, so a single frontier is the whole ack — no bitmap.
- **Tick advantage** — an opaque time-sync lead carried alongside the entry run
  (the newest local input's lead outbound, the freshest seen inbound). rennet only
  shuttles it; the clock-sync policy lives in the engine.
- **Rollback horizon** — a constructor parameter, not a constant: it's a property
  of the consuming engine's input buffer, not of the protocol. A gap wider than it
  yields `HorizonExceeded`, the signal to tear down (and, later, reconnect).

## What rennet does and doesn't do

It **does**: ordered in-band delivery, dedup, proactive loss recovery, cumulative
acks, adaptive redundancy, and a compact self-delimiting wire envelope.

It **doesn't**: own a transport (you pump bytes), an engine (it doesn't know what
an element means), a clock (it only shuttles `tick_advantage`), a body packing
(you implement `Body`), or a connection lifecycle (handshake, reconnect, and the
reliable out-of-match channel live above it).

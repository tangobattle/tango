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
your own event type. Everything is generic over one `Codec` impl — usually a
zero-sized marker — that fixes the `Element` each seq slot carries and the
per-frame `Meta` side-channel, so call sites read `Frame<MyCodec>` rather than
threading each type separately.

## Two layers

### `frame` — the wire envelope

The on-wire `Frame<P>`: a per-tick seq (`base`), a piggybacked cumulative `ack`,
a caller-defined `Meta` side-channel, and a run of `Element`s. One datagram is
exactly one `Frame` — there is no envelope tag, and no separate ping/pong probe
(round-trip latency falls out of the ack round-trip). It is byte-minimized: LEB128
varints, and the ack travels as a *signed delta from `base`* rather than an
absolute frontier (both counters advance per tick, so they differ only by the
lead/redundancy span — one byte instead of three over a long match).

rennet owns the envelope **and** the run framing: it concatenates the elements on
the way out and decodes them back until the datagram runs out. *How a single
element* packs — continuation-delimited, length-prefixed, fixed-width — and *what
the `Meta` means* are entirely the caller's business.

### `stream` — the reliability state machines

Two halves, both pure and generic over the `Codec` `P`:

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

Because the element run is **last**, the datagram boundary delimits it: rennet
decodes elements until the bytes run out — no length prefix, no count. There is no
data-vs-ack-only distinction; an "ack-only" frame is simply one whose run is empty.

```text
base   uvarint   always
ack    svarint   always; encoded as (frontier − base)
meta   Meta      always; self-delimiting (Meta::decode reads exactly its bytes)
run    Element*  always; each element self-delimits; runs to the datagram end
                 (may be empty)
```

With the zero-width `()` meta, a frame is just `base | ack | run`.

## The `Codec` trait

You supply one type — usually a zero-sized marker — implementing `Codec`. It names
the `Element` each seq slot carries and the per-frame `Meta`, and supplies the
codec for each. The element and meta are *plain data* (primitives, foreign types,
your own structs — no rennet trait impls on them); the wire packing lives on the
`Codec`, so one `impl Codec` is all you write:

```rust
pub trait Codec {
    type Element: Copy + Debug + PartialEq + Eq;
    type Meta: Copy + Default + Debug + PartialEq + Eq;   // use () for no side-channel
    const MAX_RUN: usize;   // cap on elements per datagram (the rollback horizon)

    fn encode_element<W: Write>(element: &Self::Element, w: &mut W) -> std::io::Result<()>;
    fn decode_element<R: Read>(r: &mut R) -> std::io::Result<Self::Element>;
    fn encode_meta<W: Write>(meta: &Self::Meta, w: &mut W) -> std::io::Result<()>;
    fn decode_meta<R: Read>(r: &mut R) -> std::io::Result<Self::Meta>;
}
```

rennet owns the run framing, so each element only has to self-delimit:
`encode_element` is called per element on the way out, `decode_element` until the
datagram's bytes run out on the way in. The meta precedes the run, so it
self-delimits too (a `()` meta writes nothing). Markers (round/match boundaries,
etc.) ride in-band on the same seq line as ordinary inputs, so an element type is
typically an enum of "input" plus whatever boundaries your engine needs.
`write_uvarint` / `write_svarint` (and their `read_` counterparts) are exported as
the byte-minimal toolkit for the codec methods.

## Usage

Each peer holds one `OutStream` and one `InStream`, both built with the engine's
rollback horizon (the depth past which a gap can never be reconciled, so the
receiver bails rather than buffering forever):

```rust
let mut out = OutStream::<MyCodec>::new(HORIZON);
let mut inn = InStream::<MyCodec>::new(HORIZON);
```

**Sending** — push local elements, then build a frame from the current window
(empty before the first push — that is an "ack-only" frame), tagging it with the
absolute ack frontier the in-stream wants next:

```rust
out.push_with_meta(Element::Input(joyflags), my_meta); // or out.push(marker), which leaves the meta unchanged

let w = out.window();
let frame = Frame::<MyCodec>::new(w.base, inn.ack(), w.meta, w.entries);
send_datagram(&frame.to_vec());
```

**Receiving** — apply the peer's ack to trim your window, then ingest the frame;
`accept` returns the run that just became contiguous (possibly empty), in strict
seq order, tagged with the freshest meta:

```rust
let frame = Frame::<MyCodec>::decode(&mut datagram)?;  // any io::Read of one datagram
out.apply_ack(frame.ack());
let delivered = inn.accept(&frame)?;   // Err(HorizonExceeded) → tear the match down
for element in delivered.entries {
    feed_engine(element, delivered.meta);
}
```

`Frame::to_vec()` is the byte-returning convenience over `Frame::encode(&mut w)`,
which writes into any `io::Write`; `decode` reads from any `io::Read` yielding
exactly one datagram.

Redundancy is proactive: every data frame re-sends the unconfirmed tail, so a
short loss recovers in ~one frame instead of the ack-driven resend's full
round-trip. The window is `max(REDUNDANCY, unconfirmed_span)` — a small fixed
floor plus however much the peer is currently lagging. RTT itself is derived from
the ack round-trip: `out.newest_seq()` is the seq to timestamp, and
`out.peer_ack_base()` advancing past it tells you the round-trip is known.

## Key terms

- **Seq / `base`** — every element gets a monotonic 0-based seq; `base` is the seq
  of a frame's first entry (or, on an empty-run frame, the sender's next unsent
  seq).
- **Redundancy window** — the recent unconfirmed elements re-sent on every data
  frame. A small fixed floor (`REDUNDANCY`) is the proactive recovery budget; the
  window grows automatically while the peer lags and shrinks back to the floor as
  acks confirm receipt.
- **Cumulative ack** — the receiver's contiguous frontier: the lowest seq it
  hasn't received, i.e. "resend your window from here." A contiguous resend window
  is all the sender can act on, so a single frontier is the whole ack — no bitmap.
- **Meta** — an opaque per-frame side-channel carried alongside the run (e.g. a
  time-sync `tick_advantage`: the newest local value outbound, the freshest seen
  inbound). rennet only shuttles it; its meaning lives in the caller (use `()` for
  none).
- **Rollback horizon** — a constructor parameter, not a constant: it's a property
  of the consuming engine's input buffer, not of the protocol. A gap wider than it
  yields `HorizonExceeded`, the signal to tear down (and, later, reconnect).

## What rennet does and doesn't do

It **does**: ordered in-band delivery, dedup, proactive loss recovery, cumulative
acks, and a compact self-delimiting wire envelope generic over your `Codec`.

It **doesn't**: own a transport (you pump bytes), an engine (it doesn't know what
an element means), a clock (it only shuttles `Meta`), an element packing (you
implement `Element`), or a connection lifecycle (handshake, reconnect, and the
reliable out-of-match channel live above it).

//! The netplay wire protocol, shared by every tango client
//! implementation (the desktop app and tango-web) so they speak
//! byte-identical framing by construction: the control-plane packet
//! codec ([`control`]), the data-plane element/meta codec over rennet
//! ([`data`]), the determinism-critical derivations both peers must
//! compute identically ([`derive`]), and the data-channel identity
//! ([`channel_spec`]).
//!
//! Pure codecs only — no transport, no async, no emulator. Builds for
//! native and wasm32 alike.

pub mod channel_spec;
pub mod control;
pub mod data;
pub mod derive;

// 0x47: in-match Input/EndOfRound/EndOfMatch moved off the reliable lobby
// channel onto a separate unreliable channel with the `data::wire` redundancy
// protocol. Incompatible with 0x46 peers, so the version gate rejects them.
// 0x48: the data frame's piggybacked ack is now a signed delta from `base`
// instead of an absolute frontier (smaller on the wire). Incompatible with 0x47.
// 0x4a: mid-match disconnect reworked — a bare channel close reconnects on a
// short window, and a deliberate quit announces itself with a `Closing` marker
// so the peer ends at once. Incompatible with the interim 0x49 `Reconnecting`
// marker, which sat at the same packet tag with the opposite meaning.
// 0x4b: `NegotiatedState` gained `ts` — the commit-time wall clock whose
// offerer-side value becomes the match clock every core pins its cart RTC to
// (deterministic exe45 PvP/replays). Old peers can't decode the reveal.
// 0x4c: the WebRTC stack moved from libdatachannel to tango-rtc (str0m).
// Data channels are now negotiated in-band (DCEP) instead of pre-agreed
// stream ids, so a 0x4b peer's channels would never open against ours; the
// version gate keeps the two stacks from ever pairing.
// (Still 0x4c: the `Closing` marker was removed — the transport's own DTLS
// close_notify already hands the peer a prompt EOF on a deliberate quit. No
// bump: an older 0x4c peer's marker decodes as stray traffic, which the
// mid-match reliable-channel watch ignores.)
// 0x4d: live PvP moved from the trap engine to the SIO-lockstep engine.
// The in-match frame layout is unchanged, but its semantics aren't: seq n
// now carries the joypad for pair tick n from session start (previously
// per-battle-tick, rounds delimited by EndOfRound), and the simulations
// are mutually incompatible — a 0x4c peer would desync instantly.
// 0x4e: the 5.0.51 release — libdatachannel transport over the old (trap
// engine) simulation. Not us.
// 0x4f: the WebRTC stack moved back to libdatachannel here too (ported from
// 5.0.51). Data channels return to pre-agreed stream ids, so a DCEP-era
// peer's channels would never open against ours; the sio-engine simulation
// rides on top, so neither 0x4d (str0m + sio) nor 0x4e (libdatachannel +
// trap sim) peers may pair with us.
// (Still 0x4f: a deliberate mid-match quit announces itself again — a
// control-channel `Goodbye` sent just before teardown — so the peer ends at
// once instead of burning the short clean-close reconnect window (which
// exists because our own reconnect's transport drop produces the same clean
// EOF a quit does — the 0x4c-era rationale for dropping the old `Closing`
// marker predates that ambiguity). No bump: an older peer can't decode the
// packet, which its mid-match watch ignores as stray traffic, falling back
// to that window.)
// 0x50: the lobby reveal announces its total byte length up front
// (ChunkStart) and the receiver counts arriving bytes against it, replacing
// the empty-sentinel Chunk that used to terminate the stream. The new Packet
// variant sits ahead of Chunk, shifting later discriminants, and a 0x4f peer
// would wait forever for a sentinel that never comes.
// 0x51: a match starts on a two-sided ready gate — each peer announces
// `Primed` once its pair reaches the link battle, and neither advances a
// tick until both have. Incompatible twice over: a 0x50 peer never sends
// it, so a 0x51 peer would sit at the gate for a packet that isn't
// coming; and the new variant sits ahead of Goodbye, shifting that
// discriminant, so the two would disagree about what a quit looks like.
// 0x52: the DS engine's emulated timeline changed (batched wifi timer,
// timestamp-gated concurrent consoles). The wire format is untouched,
// but a match is two peers simulating the SAME pair tick for tick — a
// 0x51 peer's simulation diverges from a 0x52 peer's within seconds and
// presents as in-game communication errors, so they must not pair.
// (Also 0x52 — the two landed together, unreleased: the in-match input
// element grew the DS's inputs. The joypad word widened to 12 bits (X
// and Y) and a stylus sample rides behind a flag bit in the element's
// first byte; a 0x51 peer would read a touched element's coordinate
// bytes as the next element and the stream would shear.)
// 0x53: the commit reveal carries an opaque session payload beside the
// save bytes (NegotiatedState grew `session_payload` — a 0x52 peer's
// reveal fails to decode against ours), minted by the sender's save
// view and read only by the game's pvp engine. BN5DS uses it to say
// which of the committed cartridge's two files is being played instead
// of rewriting the committed bytes to carry the choice. Priming steers
// the game's own file select by it on both peers, so even if the
// reveal decoded, the walks — and the simulations — would disagree.
// 0x54: the DS engine's emulated timeline changed again, for the same
// reason 0x52 did and with the same consequence. melonDS was keeping
// state its savestates did not carry, and its JIT baked that state into
// compiled blocks — so what a block cost depended on the path execution
// first took to reach it. Fixing that moved the cycle counts of ordinary
// execution, not just of a restore: a 0x53 peer and a 0x54 peer simulate
// the same pair tick for tick and drift apart within seconds, surfacing
// as the game's own communication errors. The wire format is untouched;
// they simply must not pair.
// 0x55: a peer's lobby settings announce how its build simulates the
// game it has picked — `GameInfo` grew `sim_version`, the same number
// its replays are already stamped with — and a pairing whose two
// values differ is refused. A 0x54 peer's Settings can't decode against
// ours, hence the bump; what the field buys is that the *next* change of
// this kind won't need one. 0x52 and 0x54 were both a single game's
// simulation moving under a wire format that never changed, and both
// turned away every other game's players to say so.
// 0x56: the commit reveal drops the session payload 0x53 added
// (`NegotiatedState` is back to nonce + clock + save bytes, so a 0x55
// peer's reveal fails to decode against ours). Nothing needs it any
// more: BN5DS's file choice was the only payload ever minted, and a
// cartridge now says which of its files is being played in its own
// bytes — the file the game itself calls most recently saved — so the
// choice travels inside `save_data` and priming reads it there on both
// peers.
pub const PROTOCOL_VERSION: u32 = 0x56;

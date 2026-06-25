//! Wire protocol for the netplay data channel. Slim port of
//! `tango/src/net/protocol.rs` — same `Packet` enum + bincode framing,
//! but the bincode option builders are `std::sync::LazyLock` instead
//! of the legacy `lazy_static!` macro.
//!
//! `VERSION` is shared with [`crate::netplay::PROTOCOL_VERSION`] — keep
//! the two in sync so the signaling-server reject path and the per-
//! peer hello path can't ever disagree.

use bincode::Options;
use std::sync::LazyLock;

pub const VERSION: u8 = crate::netplay::PROTOCOL_VERSION as u8;

static BINCODE_OPTIONS: LazyLock<
    bincode::config::WithOtherLimit<
        bincode::config::WithOtherIntEncoding<bincode::config::DefaultOptions, bincode::config::VarintEncoding>,
        bincode::config::Bounded,
    >,
> = LazyLock::new(|| {
    bincode::DefaultOptions::new()
        .with_varint_encoding()
        .with_limit(64 * 1024)
});

static STATE_BINCODE_OPTIONS: LazyLock<
    bincode::config::WithOtherIntEncoding<bincode::config::DefaultOptions, bincode::config::VarintEncoding>,
> = LazyLock::new(|| bincode::DefaultOptions::new().with_varint_encoding());

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum Packet {
    // Handshake.
    Hello(Hello),

    // Ping.
    Ping(Ping),
    Pong(Pong),

    // Lobby.
    Settings(Settings),
    Commit(Commit),
    Uncommit(Uncommit),
    Chunk(Chunk),
    StartMatch(StartMatch),

    // Mid-match control.
    /// The sender's in-match link fell silent and it's about to tear the
    /// connection down to re-rendezvous (transparent reconnect). Tells the peer
    /// the imminent channel close is a reconnect, not a quit, so it follows
    /// instead of ending the match. Appended last so existing tags don't shift;
    /// older peers ignore it (the reliable channel is drained raw mid-match).
    Reconnecting(Reconnecting),
    // The live match's per-frame Input / EndOfRound / EndOfMatch traffic no
    // longer rides this reliable channel — it's the data plane's job, carried
    // as `data::wire` frames/markers over a separate unreliable channel (see
    // [`crate::net::data`]). Only lobby/handshake packets (and the reconnect
    // marker above) remain here.
}

impl Packet {
    pub fn serialize(&self) -> bincode::Result<Vec<u8>> {
        BINCODE_OPTIONS.serialize(self)
    }

    pub fn deserialize(d: &[u8]) -> bincode::Result<Self> {
        BINCODE_OPTIONS.deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Hello {
    pub protocol_version: u8,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Commit {
    pub commitment: [u8; 16],
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Uncommit {}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Chunk {
    pub chunk: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Ping {
    pub ts: u16,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Pong {
    pub ts: u16,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PatchInfo {
    pub name: String,
    pub version: semver::Version,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct GameInfo {
    pub family_and_variant: (String, u8),
    pub patch: Option<PatchInfo>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub nickname: String,
    pub match_type: (u8, u8),
    pub game_info: Option<GameInfo>,
    pub blind_setup: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct StartMatch {}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Reconnecting {}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct NegotiatedState {
    pub nonce: [u8; 16],
    pub save_data: Vec<u8>,
}

impl NegotiatedState {
    pub fn serialize(&self) -> bincode::Result<Vec<u8>> {
        STATE_BINCODE_OPTIONS.serialize(self)
    }

    pub fn deserialize(d: &[u8]) -> bincode::Result<Self> {
        STATE_BINCODE_OPTIONS.deserialize(d)
    }
}

/// `Shake128("tango:lobby:" || buf)` truncated to 16 bytes — the commitment
/// hash over the compressed reveal. Shared by the lobby challenge flow and the
/// peer-to-peer reveal verification so both ends agree on the construction.
pub fn make_commitment(buf: &[u8]) -> [u8; 16] {
    use sha3::digest::{ExtendableOutput, Update, XofReader};
    let mut h = sha3::Shake128::default();
    h.update(b"tango:lobby:");
    h.update(buf);
    let mut out = [0u8; 16];
    h.finalize_xof().read(&mut out);
    out
}

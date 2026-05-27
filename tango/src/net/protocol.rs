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

    // In match.
    Input(tango_pvp::net::Input),
    /// Sent by each side from its local round-ending trap. The
    /// receiver bumps its `peer_round_idx`; subsequent Inputs are
    /// tagged with the new round id so `try_attach_remote_input`
    /// can drop stale tails from the just-finished round and hold
    /// inputs from the next round until the local side catches up.
    EndOfRound(EndOfRound),
    /// Sent once by each side when its local `match_end_ret`
    /// hook fires. The peer waits for this before tearing down
    /// the connection so the lagging side can finish writing its
    /// replay. See `PvpSession::is_ended` for the wait logic.
    EndOfMatch(EndOfMatch),
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
    pub ts: std::time::SystemTime,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Pong {
    pub ts: std::time::SystemTime,
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
    pub available_games: Vec<(String, u8)>,
    pub available_patches: Vec<(String, Vec<semver::Version>)>,
    pub reveal_setup: bool,
    /// This side's requested frame delay. Both peers exchange it before the
    /// match; `min` of the two becomes the shared input delay (rollback
    /// reduction), the local remainder becomes presentation delay. See
    /// `tango_pvp::battle` and `PvpSession::new`.
    pub frame_delay: u32,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct StartMatch {}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct EndOfRound {}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct EndOfMatch {}

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

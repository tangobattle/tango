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
    /// Opening packet, sent once by each side as soon as the connection is up.
    /// Carries the protocol version plus the match settings + reveal commitment
    /// — the lobby has already brokered agreement, so there's no in-connection
    /// settings/ready negotiation: it's just `Hello -> Chunk… -> StartMatch`.
    Hello(Hello),
    /// One slice of the zstd-compressed reveal (an empty `chunk` is the
    /// end-of-stream sentinel). Verified against the peer's `Hello.commitment`.
    Chunk(Chunk),
    /// Sent once each side has streamed + verified the reveal.
    StartMatch(StartMatch),
    // The live match's per-frame Input / EndOfRound / EndOfMatch traffic no
    // longer rides this reliable channel — it's the data plane's job, carried
    // as `data::wire` frames/markers over a separate unreliable channel (see
    // [`crate::net::data`]). Only the handshake packets remain here.
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
    /// Our match settings (nickname / game / match type / blind flag). The
    /// lobby already agreed these; this is what the PvP session reads off the
    /// wire so the two sides don't have to re-derive them.
    pub settings: Settings,
    /// Commitment hash over our compressed reveal — the peer checks the reveal
    /// chunks against it. (`Shake128` of the reveal; see [`make_commitment`].)
    pub commitment: [u8; 16],
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Chunk {
    pub chunk: Vec<u8>,
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

/// Our half of a match commitment: a fresh nonce, the compressed reveal bytes
/// (`zstd(bincode(NegotiatedState))`) the peer reassembles and verifies, and the
/// commitment hash to publish.
pub struct LocalReveal {
    pub nonce: [u8; 16],
    pub commitment: [u8; 16],
    pub compressed: Vec<u8>,
}

/// Build a commitment + reveal from the local save SRAM. The commitment is sent
/// over the lobby (challenge/accept); the compressed reveal is streamed
/// peer-to-peer once connected, where the peer checks it against the commitment.
pub fn build_commitment(save_sram: Vec<u8>) -> anyhow::Result<LocalReveal> {
    let mut nonce = [0u8; 16];
    rand::Rng::fill(&mut rand::thread_rng(), &mut nonce);
    let state = NegotiatedState {
        nonce,
        save_data: save_sram,
    };
    let bin = state.serialize()?;
    let compressed = zstd::stream::encode_all(std::io::Cursor::new(&bin), 3)?;
    let commitment = make_commitment(&compressed);
    Ok(LocalReveal {
        nonce,
        commitment,
        compressed,
    })
}

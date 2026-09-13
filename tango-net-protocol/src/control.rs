//! Wire protocol for the reliable control/lobby channel: the `Packet`
//! enum + bincode framing (moved verbatim from the tango bin crate's
//! `net/control/protocol.rs`), the commit/reveal `NegotiatedState` and
//! its commitment construction, and the reveal's chunking constant.
//!
//! `VERSION` is derived from [`crate::PROTOCOL_VERSION`] (the
//! signaling-server reject path sends the full u32; the per-peer Hello
//! sends this u8), so the two can't disagree — but the Hello field is a
//! byte, so the version must stay ≤ 0xff. The assert below turns the
//! eventual overflow into a build failure instead of a silent wrap that
//! would let peers 256 versions apart negotiate as "equal".

use bincode::Options;
use std::sync::LazyLock;

pub const VERSION: u8 = crate::PROTOCOL_VERSION as u8;
const _: () = assert!(
    crate::PROTOCOL_VERSION <= u8::MAX as u32,
    "PROTOCOL_VERSION no longer fits the Hello packet's u8; widen the wire field before bumping past 0xff"
);

/// The reveal stream's chunking: the zstd'd [`NegotiatedState`] travels
/// as [`Chunk`] packets of at most this many payload bytes (the last
/// one smaller), inside the control channel's 64 KiB packet limit.
pub const REVEAL_CHUNK_SIZE: usize = 32 * 1024;

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
    ChunkStart(ChunkStart),
    Chunk(Chunk),
    StartMatch(StartMatch),

    /// Sent once the sender's pair has finished priming to its link
    /// battle. Neither side advances a tick until it has both primed
    /// itself and seen this, so the two start within a round trip of
    /// each other however far apart their prime times land. See
    /// [`Primed`].
    Primed(Primed),

    // The live match's per-frame Input / EndOfRound / EndOfMatch traffic no
    // longer rides this reliable channel — it's the data plane's job, carried
    // as `data::wire` frames/markers over a separate unreliable channel (see
    // [`crate::data`]). Only lobby/handshake packets remain here, plus:
    /// Deliberate mid-match quit, sent just before teardown. The teardown's
    /// close_notify alone is ambiguous to the peer — its own reconnect
    /// produces the same clean EOF — so without this it burns the short
    /// reconnect path on us; the goodbye lets it end at once.
    /// A peer that predates this variant fails to decode it, which its
    /// mid-match watch ignores as stray traffic (hence no version bump) —
    /// it just falls back to that window.
    Goodbye(Goodbye),
}

impl Packet {
    pub fn serialize(&self) -> bincode::Result<Vec<u8>> {
        self.validate()?;
        BINCODE_OPTIONS.serialize(self)
    }

    pub fn deserialize(d: &[u8]) -> bincode::Result<Self> {
        let packet: Self = BINCODE_OPTIONS.deserialize(d)?;
        packet.validate()?;
        Ok(packet)
    }

    fn validate(&self) -> bincode::Result<()> {
        if let Self::Settings(settings) = self {
            if let Some(gamemode) = &settings.gamemode {
                gamemode
                    .validate()
                    .map_err(|error| Box::new(bincode::ErrorKind::Custom(error.to_string())))?;
            }
        }
        Ok(())
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

/// Opens a reveal stream: the total byte length of the zstd'd
/// NegotiatedState the Chunk packets that follow will carry. The
/// receiver counts arriving bytes against this — there is no
/// end-of-stream sentinel.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ChunkStart {
    pub len: u64,
}

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
    /// How the sender's build simulates this game
    /// (`tango_match::Backend::sim_version`, the same number its
    /// replays are stamped with). Two builds that disagree here would
    /// simulate the same inputs into different matches, so the lobby
    /// refuses the pairing before either side commits.
    ///
    /// This is what keeps a change to one game's netplay from costing
    /// every game a [`crate::PROTOCOL_VERSION`] bump: the protocol
    /// version gates the wire, this gates the simulation, and only the
    /// games that actually changed are turned away.
    pub sim_version: u32,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub nickname: String,
    pub match_type: u8,
    pub game_info: Option<GameInfo>,
    pub blind_setup: bool,
    /// Exact package simulation configuration. Native game metadata is only
    /// descriptive when this is present; it must never select a fallback.
    pub gamemode: Option<tango_match::gamemode::Configuration>,
}

impl Settings {
    /// Commit to simulation terms without nickname or setup-visibility churn.
    pub fn simulation_digest(&self) -> bincode::Result<[u8; 32]> {
        use sha3::{Digest, Sha3_256};
        Packet::Settings(self.clone()).validate()?;
        let bytes = BINCODE_OPTIONS.serialize(&(&self.game_info, self.match_type, &self.gamemode))?;
        let mut hash = Sha3_256::new();
        hash.update(b"tango-lobby-simulation-v1\0");
        hash.update(bytes);
        Ok(hash.finalize().into())
    }
}

#[cfg(test)]
mod gamemode_tests {
    use super::*;
    use tango_match::gamemode::{
        identity::{Identity, Package, Runtime},
        Configuration, OptionValue,
    };

    fn settings() -> Settings {
        Settings {
            gamemode: Some(Configuration {
                disable_bgm: false,
                identity: Identity {
                    engine: Runtime {
                        name: "test".into(),
                        revision: 1,
                    },
                    script: Runtime {
                        name: "luau".into(),
                        revision: 1,
                    },
                    package: Package {
                        name: "game".into(),
                        version: "1.0.0".into(),
                        digest: [7; 32],
                    },
                    export: "main".into(),
                    dependencies: vec![],
                    rom: [8; 32],
                    environment: [9; 32],
                },
                options: [
                    ("choice".into(), OptionValue::String("alternate".into())),
                    ("value".into(), OptionValue::Number(-0.0)),
                ]
                .into(),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn settings_roundtrip_pinned_gamemode_and_validate_before_use() {
        for settings in [Settings::default(), settings()] {
            let encoded = Packet::Settings(settings.clone()).serialize().unwrap();
            let Packet::Settings(decoded) = Packet::deserialize(&encoded).unwrap() else {
                panic!("settings packet")
            };
            assert_eq!(decoded, settings);
        }
        let mut invalid = settings();
        invalid
            .gamemode
            .as_mut()
            .unwrap()
            .options
            .insert("value".into(), OptionValue::Number(f64::NAN));
        assert!(Packet::Settings(invalid.clone()).serialize().is_err());
        let bytes = BINCODE_OPTIONS.serialize(&Packet::Settings(invalid)).unwrap();
        assert!(Packet::deserialize(&bytes).is_err());
    }

    #[test]
    fn simulation_digest_binds_options_and_pins_but_not_display_preferences() {
        let settings = settings();
        let digest = settings.simulation_digest().unwrap();
        let mut cosmetic = settings.clone();
        cosmetic.nickname = "new nickname".into();
        cosmetic.blind_setup = true;
        assert_eq!(cosmetic.simulation_digest().unwrap(), digest);
        for mutate in [
            |s: &mut Settings| {
                s.match_type = 1;
            },
            |s: &mut Settings| {
                s.gamemode = None;
            },
            |s: &mut Settings| {
                s.gamemode.as_mut().unwrap().identity.package.digest[0] ^= 1;
            },
            |s: &mut Settings| {
                s.gamemode
                    .as_mut()
                    .unwrap()
                    .options
                    .insert("value".into(), OptionValue::Number(0.0));
            },
        ] {
            let mut changed = settings.clone();
            mutate(&mut changed);
            assert_ne!(changed.simulation_digest().unwrap(), digest);
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct StartMatch {}

/// "My pair is primed and I am ready to tick."
///
/// Priming is a pure function of ROM/save/rtc, so both peers walk to the
/// same state — but they take as long as their own hardware takes, and
/// nothing else on the wire marks the boundary. Without this the peer
/// that finishes first starts advancing alone, piling its own inputs up
/// with nothing to match them against, and trips the input-queue stall
/// watchdog into a spurious mid-match reconnect while the other side is
/// still walking menus.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Primed {}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Goodbye {}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct NegotiatedState {
    pub nonce: [u8; 16],
    /// Sender's wall clock at commit time, milliseconds since the unix epoch.
    /// The offerer's value becomes the match clock: the fixed time every core
    /// on both sides pins its cart RTC to, and the `ts` recorded in the replay
    /// metadata — so RTC-reading games (exe45) run deterministically in PvP
    /// and on playback.
    pub ts: u64,
    pub save_data: Vec<u8>,
    /// Binds this reveal to the sender's exact advertised simulation settings.
    pub settings_digest: [u8; 32],
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

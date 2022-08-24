use bincode::Options;

pub const VERSION: u8 = 0x30;

lazy_static! {
    static ref BINCODE_OPTIONS: bincode::config::WithOtherLimit<
        bincode::config::WithOtherIntEncoding<
            bincode::config::DefaultOptions,
            bincode::config::VarintEncoding,
        >,
        bincode::config::Bounded,
    > = bincode::DefaultOptions::new()
        .with_varint_encoding()
        .with_limit(64 * 1024);
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum Packet {
    // Handshake.
    Hello(Hello),
    Hola(Hola),

    // Ping.
    Ping(Ping),
    Pong(Pong),

    // Lobby.
    SetSettings(SetSettings),
    Commit(Commit),
    Uncommit(Uncommit),
    Chunk(Chunk),
    StartMatch(StartMatch),

    // In match.
    Input(Input),
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
pub struct Hola {}

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
    pub ts: u64,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Pong {
    pub ts: u64,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PatchInfo {
    pub name: String,
    pub version: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct GameInfo {
    pub family: String,
    pub variant: u8,
    pub patch: Option<PatchInfo>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct SetSettings {
    pub nickname: String,
    pub match_type: u8,
    pub match_subtype: u8,
    pub game_info: GameInfo,
    pub avaiable_games: Vec<GameInfo>,
    pub reveal_setup: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Input {
    pub round_number: u8,
    pub local_tick: u32,
    pub tick_diff: i8,
    pub joyflags: u16,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct StartMatch {}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct NegotiatedState {
    pub nonce: [u8; 16],
    pub save_data: Vec<u8>,
}

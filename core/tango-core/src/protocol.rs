use bincode::Options;

pub const VERSION: u8 = 0x21;

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
    Hello(Hello),
    Smuggle(Smuggle),
    Hola(Hola),
    Ping(Ping),
    Pong(Pong),
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
pub struct Smuggle {
    pub data: Vec<u8>,
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
pub struct Input {
    pub round_number: u8,
    pub local_tick: u32,
    pub tick_diff: i8,
    pub joyflags: u16,
}

use bincode::Options;

pub const VERSION: u8 = 0x0f;

lazy_static! {
    static ref BINCODE_OPTIONS: bincode::config::WithOtherLimit<
        bincode::config::WithOtherIntEncoding<
            bincode::config::DefaultOptions,
            bincode::config::FixintEncoding,
        >,
        bincode::config::Bounded,
    > = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(128 * 1024);
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum Packet {
    Hello(Hello),
    Hola(Hola),
    Init(Init),
    StateChunk(StateChunk),
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
    pub rng_commitment: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Hola {
    pub rng_nonce: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Init {
    pub battle_number: u8,
    pub input_delay: u32,
    pub marshaled: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct StateChunk {
    pub chunk: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Input {
    pub battle_number: u8,
    pub local_tick: u32,
    pub remote_tick: u32,
    pub joyflags: u16,
    pub custom_screen_state: u8,
    pub turn: Vec<u8>,
}

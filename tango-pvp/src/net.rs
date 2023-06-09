#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Input {
    pub round_number: u8,
    pub local_tick: u32,
    pub tick_diff: i8,
    pub joyflags: u16,
}

#[async_trait::async_trait]
pub trait Sender {
    async fn send(&mut self, input: &Input) -> std::io::Result<()>;
}

#[async_trait::async_trait]
pub trait Receiver {
    async fn receive(&mut self) -> std::io::Result<Input>;
}

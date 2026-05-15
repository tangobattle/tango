#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Input {
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

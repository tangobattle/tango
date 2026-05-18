#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Input {
    pub joyflags: u16,
    /// Sender's `current_tick - last_remote_received_tick` at send time.
    /// The receiver subtracts this from its own advantage and halves to
    /// get a network-delay-cancelled skew estimate that drives the time-
    /// sync throttle (see `Round::update_fps_target`).
    pub frame_advantage: i16,
}

#[async_trait::async_trait]
pub trait Sender {
    async fn send(&mut self, input: &Input) -> std::io::Result<()>;
}

#[async_trait::async_trait]
pub trait Receiver {
    async fn receive(&mut self) -> std::io::Result<Input>;
}

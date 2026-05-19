#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Input {
    pub joyflags: u16,
    /// Sender's `current_tick - last_remote_received_tick` at send time.
    /// The receiver subtracts this from its own advantage to get the
    /// raw skew that drives the time-sync throttle (see
    /// `Round::update_fps_target`).
    pub frame_advantage: i16,
}

/// What the in-match receive loop yields. `Input` is the per-tick
/// joyflags packet; `EndOfRound` is the boundary marker the peer sends
/// from its round-ending trap. See `Match::run` for the dispatch.
#[derive(Clone, Debug)]
pub enum Event {
    Input(Input),
    EndOfRound,
}

#[async_trait::async_trait]
pub trait Sender {
    async fn send(&mut self, input: &Input) -> std::io::Result<()>;
    /// Tell the peer our local round ended. Must be sent after the last
    /// `send(input)` call for the just-finished round and before any
    /// `send(input)` call for the next round, so the peer's receive loop
    /// can use it as an in-order round-boundary marker.
    async fn send_end_of_round(&mut self) -> std::io::Result<()>;
}

#[async_trait::async_trait]
pub trait Receiver {
    async fn receive(&mut self) -> std::io::Result<Event>;
}

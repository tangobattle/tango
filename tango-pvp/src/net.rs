#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Input {
    pub joyflags: u16,
    /// Sender's local tick advantage at send time — how far its local input
    /// leads the remote input it has received (the input queue's signed lead).
    /// The receiver subtracts this from its own advantage to get the raw skew
    /// that drives the time-sync throttler (see `battle::throttler::Throttler`).
    pub tick_advantage: i16,
}

/// What the in-match receive loop yields. `Input` is the per-tick
/// joyflags packet; `EndOfRound` is the boundary marker the peer sends
/// from its round-ending trap. See `Match::run` for the dispatch.
#[derive(Clone, Debug)]
pub enum Event {
    Input(Input),
}

pub trait Sender {
    fn send(&mut self, event: &Event) -> std::io::Result<()>;
}

#[async_trait::async_trait]
pub trait Receiver {
    async fn receive(&mut self) -> std::io::Result<Event>;
}

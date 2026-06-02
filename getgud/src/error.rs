/// Errors the engine itself raises. Adapter-supplied seams (simulator,
/// cosimulator, …) report their own failures through
/// [`World::Error`](crate::World::Error); the bound `World::Error:
/// From<EngineError>` lets the engine fold these in without the library
/// committing to a concrete error type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    /// The local side queued more unconfirmed inputs than the queue allows —
    /// the peer has fallen too far behind to keep speculating.
    #[error("local input buffer overflow")]
    LocalInputOverflow,
}

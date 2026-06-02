use crate::world::World;

/// The sink for what to draw. The [`Session`](crate::Session) calls
/// [`present`](Presenter::present) once per [`advance`](crate::Session::advance).
pub trait Presenter<W: World> {
    /// Render `state`, the world for this frame. `state` is usually a
    /// speculative (predicted) view and is recomputed each frame; don't
    /// retain it.
    ///
    /// `skew` is the raw time-sync signal in frames —
    /// `local_frame_advantage - remote_frame_advantage`. A positive value means
    /// you're running ahead of the peer and should slow down to let it catch up.
    /// The engine doesn't act on it; feed it to your own throttle (and turn that
    /// into a frame-rate target). See the crate-level docs on time
    /// synchronization.
    fn present(&mut self, state: &W::State, skew: i32);
}

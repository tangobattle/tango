use crate::world::World;

/// Where the driver hands off each frame's chosen display state and the
/// time-sync throttle. The host builds a fresh `Presenter` (or borrows a
/// per-frame one) for each [`Session::advance`](crate::Session::advance) call.
pub trait Presenter<W: World> {
    /// Show the state captured at `tick`.
    fn present(&mut self, state: &W::State, tick: u32);
    /// Apply the time-sync throttle: `slowdown` is how many fps *below* the
    /// host's nominal rate the simulation should run this frame (0 = full
    /// speed; the engine never assumes what the nominal rate is). The host
    /// turns this into whatever its clock wants.
    fn set_slowdown(&mut self, slowdown: f32);
}

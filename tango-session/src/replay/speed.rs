//! Replay speed: the transport's preset plus the custom-screen override,
//! carried to the drive loop and audio stream on the shared [`Pacing`]
//! dial.

use std::sync::Mutex;

use crate::local::Pacing;

/// Replay speed has two layers: the base preset selected in the transport,
/// and the effective rate the [`Pacing`] dial carries to the drive loop and
/// audio stream. With the custom-screen option on, an open custom screen
/// raises that effective rate to at least 2× without replacing the user's
/// preset.
pub(super) struct SpeedControl {
    pacing: Pacing,
    /// Source settings are kept together so every effective-rate update is
    /// derived from one coherent state.
    state: Mutex<SpeedState>,
}

struct SpeedState {
    factor: f32,
    custom_screen_speedup: bool,
    custom_screen_active: bool,
}

impl SpeedControl {
    pub(super) fn new(pacing: Pacing) -> Self {
        Self {
            pacing,
            state: Mutex::new(SpeedState {
                factor: 1.0,
                custom_screen_speedup: false,
                custom_screen_active: false,
            }),
        }
    }

    /// The dial the drive loop and audio stream follow.
    pub(super) fn pacing(&self) -> &Pacing {
        &self.pacing
    }

    pub(super) fn factor(&self) -> f32 {
        self.state.lock().unwrap().factor
    }

    pub(super) fn set_factor(&self, factor: f32) {
        let mut state = self.state.lock().unwrap();
        state.factor = factor;
        self.refresh_locked(&state);
    }

    pub(super) fn custom_screen_speedup(&self) -> bool {
        self.state.lock().unwrap().custom_screen_speedup
    }

    pub(super) fn set_custom_screen_speedup(&self, enabled: bool) {
        let mut state = self.state.lock().unwrap();
        state.custom_screen_speedup = enabled;
        self.refresh_locked(&state);
    }

    pub(super) fn set_custom_screen_active(&self, active: bool) {
        let mut state = self.state.lock().unwrap();
        if state.custom_screen_active == active {
            return;
        }
        state.custom_screen_active = active;
        self.refresh_locked(&state);
    }

    fn refresh_locked(&self, state: &SpeedState) {
        self.pacing.set_speed(effective_factor(
            state.factor,
            state.custom_screen_speedup,
            state.custom_screen_active,
        ));
    }

    pub(super) fn fps_target(&self) -> f32 {
        self.pacing.fps_target()
    }
}

fn effective_factor(factor: f32, speedup_enabled: bool, custom_open: bool) -> f32 {
    if speedup_enabled && custom_open {
        factor.max(2.0)
    } else {
        factor
    }
}

#[cfg(test)]
mod tests {
    use super::{effective_factor, Pacing, SpeedControl};

    #[test]
    fn custom_screen_speedup_is_a_two_x_floor() {
        assert_eq!(effective_factor(1.0, true, true), 2.0);
        assert_eq!(effective_factor(0.5, true, true), 2.0);
        assert_eq!(effective_factor(4.0, true, true), 4.0);
        assert_eq!(effective_factor(1.0, false, true), 1.0);
        assert_eq!(effective_factor(1.0, true, false), 1.0);

        const NATIVE: f32 = 59.7275;
        let speed = SpeedControl::new(Pacing::at(NATIVE));
        speed.set_factor(0.5);
        speed.set_custom_screen_speedup(true);
        speed.set_custom_screen_active(true);
        assert_eq!(speed.factor(), 0.5);
        assert_eq!(speed.fps_target(), NATIVE * 2.0);
        speed.set_custom_screen_active(false);
        assert_eq!(speed.fps_target(), NATIVE * 0.5);
    }
}

//! The app's own motion helpers, over the shared timelines in
//! [`tango_ui::anim`] — re-exported here, so `crate::ui::anim::*`
//! covers both. Everything is presentation-only: state stays in plain
//! bools / enums, and these wrappers project it through time.
//!
//! Redraws while something is mid-flight come from an
//! `iced::window::frames()` subscription that `App::subscription`
//! gates on [`tango_ui::anim::any_active`] — when nothing is moving,
//! the app goes back to redrawing only on events.

pub use tango_ui::anim::*;

use iced::time::Instant;
use iced::Element;

/// A toggleable overlay: a plain `bool` source of truth bundled with
/// the [`Transition`] that animates its show/hide. Handlers flip the
/// bool freely with [`open`]/[`close`]/[`toggle`]/[`set`] (no clock
/// needed); a single [`sync`] call per update drives the animation
/// toward it. Folds the old hand-paired `show_x: bool` + `x_anim:
/// Transition` fields — and their easy-to-forget mirror block — into
/// one field.
///
/// [`open`]: Overlay::open
/// [`close`]: Overlay::close
/// [`toggle`]: Overlay::toggle
/// [`set`]: Overlay::set
/// [`sync`]: Overlay::sync
#[derive(Debug, Clone)]
pub struct Overlay {
    shown: bool,
    anim: Transition,
}

impl Overlay {
    pub fn new(shown: bool) -> Self {
        Self {
            shown,
            anim: Transition::new(shown),
        }
    }

    pub fn open(&mut self) {
        self.shown = true;
    }

    pub fn close(&mut self) {
        self.shown = false;
    }

    pub fn toggle(&mut self) {
        self.shown = !self.shown;
    }

    /// The source-of-truth target the next [`sync`](Overlay::sync)
    /// drives toward. Mid-flight the animation may still be catching
    /// up, but logic should branch on this.
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// Push the bool into the animation. Call once per update, after
    /// the handlers have settled the bool.
    pub fn sync(&mut self, now: Instant) {
        self.anim.set(self.shown, now);
    }

    /// Whether the overlay should be in the tree at all — shown or
    /// still animating out.
    pub fn visible(&self, now: Instant) -> bool {
        self.anim.visible(now)
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        self.anim.is_animating(now)
    }

    /// 0.0 = fully hidden, 1.0 = fully shown.
    pub fn progress(&self, now: Instant) -> f32 {
        self.anim.progress(now)
    }
}

/// Entrance/exit transform for popovers and modal panels: rises
/// `rise` px while scaling 0.96 → 1.0 around its center. Uses
/// `Float`, which transforms the drawn layer — layout is computed
/// at rest size, so nothing around the element reflows during the
/// motion. At `progress == 1.0` the transform is identity and the
/// wrapper draws inline (no overlay layer, no extra cost).
pub fn pop<'a, M: 'a>(content: impl Into<Element<'a, M>>, progress: f32, rise: f32) -> Element<'a, M> {
    let dy = (1.0 - progress) * rise;
    let scale = 0.96 + 0.04 * progress;
    iced::widget::float(content)
        .scale(scale)
        .translate(move |_bounds, _viewport| iced::Vector::new(0.0, dy))
        .into()
}

/// Modal backdrop style — black wash at `alpha`. Call sites scale
/// their resting alpha by a [`Transition::progress`] so the dim
/// fades in with the panel instead of slamming on.
pub fn backdrop_style(alpha: f32) -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    move |_theme: &iced::Theme| iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba(0.0, 0.0, 0.0, alpha))),
        ..Default::default()
    }
}

/// Slow sinusoidal pulse in [0, 1] for "something is alive"
/// indicators (the connecting / waiting-for-opponent status
/// lines). Stateless — phase comes off the process-wide epoch —
/// so callers just sample it per frame; the App's subscription
/// keeps frames coming while a pulsing line is on screen.
pub fn pulse() -> f32 {
    const PERIOD_SECS: f32 = 1.6;
    static EPOCH: std::sync::LazyLock<std::time::Instant> = std::sync::LazyLock::new(std::time::Instant::now);
    let t = EPOCH.elapsed().as_secs_f32();
    0.5 - 0.5 * (t * std::f32::consts::TAU / PERIOD_SECS).cos()
}

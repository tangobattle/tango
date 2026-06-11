//! UI motion helpers. Everything here is presentation-only: state
//! stays in plain bools / enums, and these wrappers project it
//! through time so overlays and screen swaps ease in instead of
//! popping. Redraws while something is mid-flight are driven by an
//! `iced::window::frames()` subscription that `App::subscription`
//! gates on [`Transition::is_animating`] / the screen fade — when
//! nothing is moving, the app goes back to redrawing only on
//! events.

use iced::animation::{Animation, Easing};
use iced::time::Instant;
use iced::Element;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;

/// One duration for every show/hide transition in the app, so all
/// the chrome moves at the same tempo. Short enough to never gate
/// the user (an overlay is interactive the frame it appears — the
/// motion is purely visual).
pub const TRANSITION: std::time::Duration = std::time::Duration::from_millis(160);

/// Process-wide animation clock base. Both the activity registry
/// and [`pulse`] measure off it.
static EPOCH: LazyLock<std::time::Instant> = LazyLock::new(std::time::Instant::now);

/// Millis-since-[`EPOCH`] until which at least one animation is
/// known to be running. Every animation starter bumps it via
/// [`kick`]; `App::subscription` polls [`any_active`] to decide
/// whether to keep the per-frame redraw subscription alive. A
/// central registry instead of threading `is_animating` through
/// every tab/state that owns an animation.
static ACTIVE_UNTIL_MS: AtomicU64 = AtomicU64::new(0);

/// Register that an animation of `duration` just started, keeping
/// per-frame redraws flowing until it (plus a small scheduling
/// margin) has finished.
pub fn kick(duration: std::time::Duration) {
    let until = (EPOCH.elapsed() + duration + std::time::Duration::from_millis(80)).as_millis() as u64;
    ACTIVE_UNTIL_MS.fetch_max(until, Ordering::Relaxed);
}

/// Whether any [`kick`]ed animation may still be in flight.
pub fn any_active() -> bool {
    (EPOCH.elapsed().as_millis() as u64) < ACTIVE_UNTIL_MS.load(Ordering::Relaxed)
}

/// A restartable 0 → 1 entrance animation. States keep one per
/// thing that enters (a pane, a band, a row of controls), restart
/// it with [`start`] when that thing (re)appears, and views shape
/// the entrance with [`progress`]. `Default`s to at-rest so
/// owning states can keep `#[derive(Default)]`.
///
/// [`start`]: Enter::start
/// [`progress`]: Enter::progress
#[derive(Debug, Clone)]
pub struct Enter {
    anim: Animation<f32>,
}

impl Default for Enter {
    fn default() -> Self {
        // At rest: progress 1.0, not animating.
        Self {
            anim: Animation::new(1.0),
        }
    }
}

impl Enter {
    /// Restart the entrance at `now`, keeping redraws flowing
    /// through the activity registry while it runs.
    pub fn start(&mut self, now: Instant) {
        self.start_delayed(now, std::time::Duration::ZERO);
    }

    /// [`start`], but holding at progress 0 for `delay` first.
    /// Used to chain an entrance after some exit finishes (e.g.
    /// the bottom strip rising in once the lobby band has slid
    /// away) — there's no completion callback to hook, so the
    /// follow-up is scheduled up front.
    ///
    /// [`start`]: Enter::start
    pub fn start_delayed(&mut self, now: Instant, delay: std::time::Duration) {
        kick(TRANSITION + delay);
        self.anim = Animation::new(0.0)
            .duration(TRANSITION)
            .delay(delay)
            .easing(Easing::EaseOutCubic)
            .go(1.0, now);
    }

    /// `Some(progress)` while mid-flight, `None` once at rest —
    /// so views can skip the transform wrapper entirely outside
    /// the entrance window.
    pub fn progress(&self, now: Instant) -> Option<f32> {
        self.anim
            .is_animating(now)
            .then(|| self.anim.interpolate_with(|v| v, now))
    }
}

/// A show/hide animation around a boolean. The owning state keeps
/// its plain `bool` as the source of truth and mirrors it in here
/// (see `session::State::sync_transitions`); views render while
/// [`visible`] and shape the entrance/exit with [`progress`].
///
/// [`visible`]: Transition::visible
/// [`progress`]: Transition::progress
#[derive(Debug, Clone)]
pub struct Transition {
    anim: Animation<bool>,
}

impl Transition {
    pub fn new(shown: bool) -> Self {
        Self {
            anim: Animation::new(shown).duration(TRANSITION).easing(Easing::EaseOutCubic),
        }
    }

    /// Drive toward `shown`. No-op when already there / heading
    /// there, so it's safe to call unconditionally once per update.
    pub fn set(&mut self, shown: bool, now: Instant) {
        if self.anim.value() != shown {
            self.anim.go_mut(shown, now);
            kick(TRANSITION);
        }
    }

    /// Whether the element should be in the tree at all — true
    /// while shown *or* still animating out.
    pub fn visible(&self, now: Instant) -> bool {
        self.anim.value() || self.anim.is_animating(now)
    }

    /// The state the transition is heading toward.
    pub fn shown(&self) -> bool {
        self.anim.value()
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        self.anim.is_animating(now)
    }

    /// 0.0 = fully hidden, 1.0 = fully shown.
    pub fn progress(&self, now: Instant) -> f32 {
        self.anim.interpolate(0.0, 1.0, now)
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

/// Entrance for freshly-swapped screens/panes: the content starts
/// displaced by `from` and glides into its resting position —
/// e.g. `Vector::new(24.0, 0.0)` enters from the right,
/// `Vector::new(0.0, 10.0)` rises up from below. Translate-only
/// (no scale) — a whole page zooming reads as a glitch, but a
/// short glide reads as "the new screen arrived". Like [`pop`],
/// layout and hit-testing use the rest position; only the drawing
/// moves, and at `progress == 1.0` the wrapper is a free
/// pass-through.
pub fn slide_in<'a, M: 'a>(content: impl Into<Element<'a, M>>, progress: f32, from: iced::Vector) -> Element<'a, M> {
    let offset = iced::Vector::new(from.x * (1.0 - progress), from.y * (1.0 - progress));
    iced::widget::float(content)
        .translate(move |_bounds, _viewport| offset)
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
    let t = EPOCH.elapsed().as_secs_f32();
    0.5 - 0.5 * (t * std::f32::consts::TAU / PERIOD_SECS).cos()
}

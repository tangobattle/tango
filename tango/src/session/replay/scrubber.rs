//! Canvas-based scrub bar with overlays the stock `iced::widget::slider`
//! can't render: a dimmer fill for the prefetched range and vertical
//! tick marks at round boundaries. Mouse press + drag inside the bar
//! emits the caller's preview message per position change (deduped, so
//! sub-pixel mouse moves don't spam); release emits the commit message
//! with the last previewed tick and ends the drag. Plain mouseover
//! (no button) emits the hover message per tick change so the caller
//! can float a thumbnail preview above the bar.

use iced::widget::canvas::{self, Canvas, Frame, Path, Stroke};
use iced::{mouse, Element, Length, Point, Rectangle, Renderer, Size, Theme};

/// Where the cursor is resting on the bar, published through
/// `on_hover`. `x` is absolute (window space) — the canvas is the only
/// widget that knows where the bar landed in layout, and the caller's
/// floating preview is positioned in the session view's full-window
/// overlay stack, which shares that origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoverInfo {
    /// The tick a click here would seek to (snapped + clamped exactly
    /// like a press, so the preview never promises an unreachable
    /// frame).
    pub tick: u32,
    /// Cursor x, clamped into the bar.
    pub x: f32,
}

pub struct Scrubber<M> {
    current: u32,
    total: u32,
    prefetched: u32,
    round_boundaries: Vec<u32>,
    on_seek: Box<dyn Fn(u32) -> M>,
    on_commit: Box<dyn Fn(u32) -> M>,
    on_hover: Box<dyn Fn(Option<HoverInfo>) -> M>,
    height: f32,
}

#[derive(Default)]
pub struct State {
    dragging: bool,
    /// Last tick published through `on_seek` during this drag, so
    /// repeated cursor moves over the same tick stay silent and the
    /// commit lands exactly on the frame the user last previewed.
    last_emitted: Option<u32>,
    /// Last tick published through `on_hover`, deduping mouseover the
    /// same way `last_emitted` dedupes drags. `Some` also means a
    /// trailing `on_hover(None)` is owed when the cursor leaves.
    hovered: Option<u32>,
}

impl<M> Scrubber<M> {
    pub fn new(
        current: u32,
        total: u32,
        prefetched: u32,
        on_seek: impl Fn(u32) -> M + 'static,
        on_commit: impl Fn(u32) -> M + 'static,
        on_hover: impl Fn(Option<HoverInfo>) -> M + 'static,
    ) -> Self {
        Self {
            current,
            total,
            prefetched,
            round_boundaries: Vec::new(),
            on_seek: Box::new(on_seek),
            on_commit: Box::new(on_commit),
            on_hover: Box::new(on_hover),
            // Tall enough for the largest (hover/drag) playhead handle
            // plus its border to protrude above + below the slim track
            // without clipping against the canvas edges.
            height: 26.0,
        }
    }

    pub fn round_boundaries(mut self, b: Vec<u32>) -> Self {
        self.round_boundaries = b;
        self
    }

    /// Translate an x within the bar (0..width) to an absolute tick,
    /// without the prefetch clamp — what the position *says*, not what
    /// it can deliver. Lets hover handling tell "over the unloaded
    /// region" apart from "over the watermark itself".
    fn raw_tick_at_x(&self, x: f32, width: f32) -> u32 {
        let pct = (x / width.max(1.0)).clamp(0.0, 1.0);
        (pct * self.total.max(1) as f32).round() as u32
    }

    /// Translate an x within the bar (0..width) to an absolute tick.
    /// Clamped to the prefetched range so a click past the loaded
    /// edge doesn't trigger a long stall while the rest decodes
    /// (the prefetcher is a background task; let it catch up
    /// before the user can scrub into uncached frames).
    fn tick_at_x(&self, x: f32, width: f32) -> u32 {
        self.raw_tick_at_x(x, width).min(self.prefetched)
    }

    pub fn view(self) -> Element<'static, M>
    where
        M: 'static,
    {
        let height = self.height;
        Canvas::new(self)
            .width(Length::Fill)
            .height(Length::Fixed(height))
            .into()
    }
}

impl<M> canvas::Program<M> for Scrubber<M> {
    type State = State;

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();
        let w = bounds.width;
        let h = bounds.height;
        let total = self.total.max(1) as f32;

        // Pull the rail + handle colors/sizes straight from the
        // `chunky_slider` style for the current interaction state, so the
        // scrub bar reads as the same widget family as every other slider.
        let hovered = state.dragging || cursor.is_over(bounds);
        let status = if state.dragging {
            iced::widget::slider::Status::Dragged
        } else if hovered {
            iced::widget::slider::Status::Hovered
        } else {
            iced::widget::slider::Status::Active
        };
        let style = crate::widgets::chunky_slider(theme, status);
        let color_of = |bg: &iced::Background, fallback: iced::Color| match bg {
            iced::Background::Color(c) => *c,
            _ => fallback,
        };
        let fill_color = color_of(&style.rail.backgrounds.0, palette.primary.base.color);
        let track_color = color_of(&style.rail.backgrounds.1, palette.background.weak.color);
        let handle_color = color_of(&style.handle.background, palette.primary.strong.color);
        let handle_r = match style.handle.shape {
            iced::widget::slider::HandleShape::Circle { radius } => radius,
            _ => 8.0,
        };

        // Slim rounded track centered vertically; its height matches the
        // real sliders' rail. The canvas bounds are taller so the round
        // playhead handle protrudes above + below without clipping.
        let track_h = style.rail.width;
        let track_y = ((h - track_h) / 2.0).round();
        let track_radius = track_h / 2.0;

        // Full-width unplayed/unprefetched track.
        let track = Path::rounded_rectangle(Point::new(0.0, track_y), Size::new(w, track_h), track_radius.into());
        frame.fill(&track, track_color);

        // Prefetched range — primary hue at weak strength so it reads
        // as a lower-contrast underlay beneath the played fill.
        let prefetched_w = (self.prefetched as f32 / total).clamp(0.0, 1.0) * w;
        if prefetched_w > 0.0 {
            let prefetched = Path::rounded_rectangle(
                Point::new(0.0, track_y),
                Size::new(prefetched_w, track_h),
                track_radius.into(),
            );
            frame.fill(&prefetched, palette.primary.weak.color);
        }

        // Played portion.
        let played_w = (self.current as f32 / total).clamp(0.0, 1.0) * w;
        if played_w > 0.0 {
            let played = Path::rounded_rectangle(
                Point::new(0.0, track_y),
                Size::new(played_w, track_h),
                track_radius.into(),
            );
            frame.fill(&played, fill_color);
        }

        // Round-boundary pips. Drawn as 2-px-wide full-height
        // notches so they pop on both the unplayed (light) track
        // and the played fill, with the color tuned for contrast
        // against whichever band they cross. Using `text` (= the
        // theme's body text color, near-white on Dark / near-black
        // on Light) gives reliable visibility everywhere — the
        // previous mid-bg-tone notches dissolved into the
        // unprefetched section.
        let notch_color = palette.background.strong.text;
        let notch_w = 2.0;
        let notch_h = track_h + 4.0;
        let notch_top = ((h - notch_h) / 2.0).round();
        for &b in &self.round_boundaries {
            // Skip 0 + total — they overlap the track ends.
            if b == 0 || b >= self.total {
                continue;
            }
            let x = (b as f32 / total).clamp(0.0, 1.0) * w;
            frame.fill_rectangle(
                Point::new((x - notch_w / 2.0).round(), notch_top),
                Size::new(notch_w, notch_h),
                notch_color,
            );
        }

        // Ghost notch under the cursor while merely hovering — ties
        // the floating thumbnail preview to the exact spot a click
        // would seek to. Suppressed over the unloaded region, in step
        // with the thumbnail (see the hover branch in `update`).
        if !state.dragging {
            if let Some(p) = cursor.position_in(bounds) {
                let tick = self.raw_tick_at_x(p.x, w);
                if tick <= self.prefetched {
                    let x = (tick as f32 / total).clamp(0.0, 1.0) * w;
                    frame.fill_rectangle(
                        Point::new((x - notch_w / 2.0).round(), notch_top),
                        Size::new(notch_w, notch_h),
                        palette.primary.strong.color,
                    );
                }
            }
        }

        // Playhead: a filled circle with the slider's 2 px border, so the
        // handle matches every other slider in the app. The radius (and
        // its hover/drag growth) comes from the slider style too.
        // Inset the handle center by its radius + half its border so the
        // full circle (border included) stays inside the canvas at both
        // ends instead of clipping against the bar edges.
        let handle_edge = handle_r + style.handle.border_width / 2.0;
        let handle_x = played_w.clamp(handle_edge, (w - handle_edge).max(handle_edge));
        let handle_y = h / 2.0;
        let handle = Path::circle(Point::new(handle_x, handle_y), handle_r);
        frame.fill(&handle, handle_color);
        if style.handle.border_width > 0.0 {
            frame.stroke(
                &handle,
                Stroke::default()
                    .with_color(style.handle.border_color)
                    .with_width(style.handle.border_width),
            );
        }

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<M>> {
        use iced::widget::Action;
        let inside = cursor.position_in(bounds);
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(p) = inside {
                    state.dragging = true;
                    // The session hides the hover preview when the drag's
                    // first on_seek lands (the full-screen blit takes
                    // over); dropping `hovered` here makes the first
                    // post-release mouseover republish it.
                    state.hovered = None;
                    let target = self.tick_at_x(p.x, bounds.width);
                    state.last_emitted = Some(target);
                    return Some(Action::publish((self.on_seek)(target)).and_capture());
                }
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.dragging {
                    state.dragging = false;
                    // Commit lands on the tick the user last saw
                    // previewed, even when the release happens outside
                    // the bar's bounds.
                    let target = state.last_emitted.take().unwrap_or(self.current);
                    return Some(Action::publish((self.on_commit)(target)).and_capture());
                }
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) if state.dragging => {
                // Track outside the bar's bounds too, so dragging past
                // either edge clamps to start/end instead of stopping
                // wherever the cursor crossed the edge. `position_in`
                // returns None outside, so re-derive against the raw
                // cursor position.
                if let Some(raw) = cursor.position() {
                    let relative_x = raw.x - bounds.x;
                    let target = self.tick_at_x(relative_x, bounds.width);
                    if state.last_emitted == Some(target) {
                        return Some(Action::capture());
                    }
                    state.last_emitted = Some(target);
                    return Some(Action::publish((self.on_seek)(target)).and_capture());
                }
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                // Plain mouseover. Published per tick change (not per
                // pixel) and NOT captured — hover is passive, and other
                // widgets are entitled to see cursor movement. The
                // unloaded region counts as "not hovering": unlike a
                // press (which usefully pins the seek at the
                // watermark), a preview there would show the watermark
                // frame under a cursor that's pointing somewhere else.
                let tick = inside
                    .map(|p| self.raw_tick_at_x(p.x, bounds.width))
                    .filter(|tick| *tick <= self.prefetched);
                if let (Some(p), Some(tick)) = (inside, tick) {
                    if state.hovered == Some(tick) {
                        return None;
                    }
                    state.hovered = Some(tick);
                    let info = HoverInfo {
                        tick,
                        x: bounds.x + p.x.clamp(0.0, bounds.width),
                    };
                    return Some(Action::publish((self.on_hover)(Some(info))));
                } else if state.hovered.take().is_some() {
                    return Some(Action::publish((self.on_hover)(None)));
                }
            }
            iced::Event::Mouse(mouse::Event::CursorLeft) => {
                // Cursor left the window entirely — no CursorMoved will
                // follow to clear the hover.
                if state.hovered.take().is_some() {
                    return Some(Action::publish((self.on_hover)(None)));
                }
            }
            _ => {}
        }
        None
    }

    fn mouse_interaction(&self, state: &State, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if state.dragging || cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

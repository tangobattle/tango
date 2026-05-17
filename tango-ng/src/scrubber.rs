//! Canvas-based scrub bar with overlays the stock `iced::widget::slider`
//! can't render: a dimmer fill for the prefetched range and vertical
//! tick marks at round boundaries. Mouse press + drag inside the bar
//! emits the caller's seek message; release ends the drag.

use iced::widget::canvas::{self, Canvas, Frame, Path};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

/// Linear-blend two colors. `t` runs 0..1; 0 returns `a`, 1 returns `b`.
fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::from_rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

pub struct Scrubber<F> {
    current: u32,
    total: u32,
    prefetched: u32,
    round_boundaries: Vec<u32>,
    on_seek: F,
    height: f32,
}

#[derive(Default)]
pub struct State {
    dragging: bool,
}

impl<F> Scrubber<F> {
    pub fn new(current: u32, total: u32, prefetched: u32, on_seek: F) -> Self {
        Self {
            current,
            total,
            prefetched,
            round_boundaries: Vec::new(),
            on_seek,
            // Tall enough for the playhead handle to protrude
            // above + below the slim track without clipping.
            height: 22.0,
        }
    }

    pub fn round_boundaries(mut self, b: Vec<u32>) -> Self {
        self.round_boundaries = b;
        self
    }

    /// Translate an x within the bar (0..width) to an absolute tick.
    fn tick_at_x(&self, x: f32, width: f32) -> u32 {
        let pct = (x / width.max(1.0)).clamp(0.0, 1.0);
        (pct * self.total.max(1) as f32).round() as u32
    }
}

impl<F, M> Scrubber<F>
where
    F: 'static + Fn(u32) -> M,
    M: 'static,
{
    pub fn view(self) -> Element<'static, M> {
        let height = self.height;
        Canvas::new(self)
            .width(Length::Fill)
            .height(Length::Fixed(height))
            .into()
    }
}

impl<F, M> canvas::Program<M> for Scrubber<F>
where
    F: Fn(u32) -> M,
{
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

        // Slim rounded track centered vertically. The canvas bounds
        // are tall (22 px) so the round playhead handle has room to
        // protrude above + below — the track itself is just 6 px.
        const TRACK_H: f32 = 6.0;
        let track_y = ((h - TRACK_H) / 2.0).round();
        let track_radius = TRACK_H / 2.0;

        // Full-width unplayed/unprefetched track.
        let track = Path::rounded_rectangle(Point::new(0.0, track_y), Size::new(w, TRACK_H), track_radius.into());
        frame.fill(&track, palette.background.weak.color);

        // Prefetched range — primary hue at weak strength so it reads
        // as a lower-contrast underlay beneath the played fill.
        let prefetched_w = (self.prefetched as f32 / total).clamp(0.0, 1.0) * w;
        if prefetched_w > 0.0 {
            let prefetched = Path::rounded_rectangle(
                Point::new(0.0, track_y),
                Size::new(prefetched_w, TRACK_H),
                track_radius.into(),
            );
            frame.fill(&prefetched, palette.primary.weak.color);
        }

        // Played portion.
        let played_w = (self.current as f32 / total).clamp(0.0, 1.0) * w;
        if played_w > 0.0 {
            let played = Path::rounded_rectangle(
                Point::new(0.0, track_y),
                Size::new(played_w, TRACK_H),
                track_radius.into(),
            );
            frame.fill(&played, palette.primary.base.color);
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
        let notch_h = TRACK_H + 4.0;
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

        // Playhead: filled circle with a thin border, sized larger
        // than the track height so it sits proud of the bar. Grows
        // slightly while dragging / hovering for tactile feedback.
        let hovered = state.dragging || cursor.is_over(bounds);
        let handle_r = if hovered { 7.0 } else { 6.0 };
        let handle_x = played_w.clamp(handle_r, w - handle_r);
        let handle_y = h / 2.0;
        let handle = Path::circle(Point::new(handle_x, handle_y), handle_r);
        // Outer ring (same color as the track bg) so the handle has
        // a halo against both played + unplayed regions.
        let halo = Path::circle(Point::new(handle_x, handle_y), handle_r + 1.5);
        frame.fill(&halo, palette.background.base.color);
        frame.fill(&handle, palette.primary.strong.color);

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
                    let target = self.tick_at_x(p.x, bounds.width);
                    return Some(Action::publish((self.on_seek)(target)).and_capture());
                }
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.dragging {
                    state.dragging = false;
                    return Some(Action::capture());
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
                    return Some(Action::publish((self.on_seek)(target)).and_capture());
                }
            }
            _ => {}
        }
        None
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.dragging || cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

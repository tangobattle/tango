//! Canvas-based scrub bar with overlays the stock `iced::widget::slider`
//! can't render: a dimmer fill for the prefetched range and vertical
//! tick marks at round boundaries. Mouse press + drag inside the bar
//! emits the caller's seek message; release ends the drag.

use iced::widget::canvas::{self, Canvas, Frame, Path, Stroke};
use iced::{mouse, Element, Length, Point, Rectangle, Renderer, Size, Theme};

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
            height: 18.0,
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
        _state: &State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();
        let w = bounds.width;
        let h = bounds.height;
        let total = self.total.max(1) as f32;

        // Full-width unplayed/unprefetched track.
        frame.fill_rectangle(
            Point::ORIGIN,
            Size::new(w, h),
            palette.background.weak.color,
        );

        // Prefetched range — same hue as the played fill but at the
        // weak slot for a lower-contrast underlay.
        let prefetched_w = (self.prefetched as f32 / total).clamp(0.0, 1.0) * w;
        frame.fill_rectangle(
            Point::ORIGIN,
            Size::new(prefetched_w, h),
            palette.primary.weak.color,
        );

        // Played portion.
        let played_w = (self.current as f32 / total).clamp(0.0, 1.0) * w;
        frame.fill_rectangle(
            Point::ORIGIN,
            Size::new(played_w, h),
            palette.primary.base.color,
        );

        // Round-boundary tick marks. Strong-background gives a visible
        // line on both Light and Dark themes without needing a separate
        // palette pick.
        let stroke = Stroke::default()
            .with_color(palette.background.strong.color)
            .with_width(1.0);
        for &b in &self.round_boundaries {
            let x = (b as f32 / total).clamp(0.0, 1.0) * w;
            let path = Path::line(Point::new(x, 0.0), Point::new(x, h));
            frame.stroke(&path, stroke.clone());
        }

        // Playhead: a 2px column at the current position so it stays
        // visible against either the played or unplayed fill.
        let thumb_x = played_w.clamp(1.0, w - 1.0);
        frame.fill_rectangle(
            Point::new(thumb_x - 1.0, 0.0),
            Size::new(2.0, h),
            palette.primary.strong.color,
        );

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

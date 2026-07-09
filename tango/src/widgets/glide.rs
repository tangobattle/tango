//! A selection plate that travels. Wraps a row/column of tab pills
//! and draws the active pill's gradient plate itself — behind the
//! children — so that when the selection moves, the plate glides
//! from the old pill's bounds to the new one's instead of
//! teleporting. The pills stop painting their own active plate (see
//! [`pill_tab_style`]'s active arm) and keep only their text/ink
//! treatment; this widget owns the chrome that moves.
//!
//! Passive: no events are captured, layout and hit-testing are the
//! wrapped content's own. Follows the app's motion rules — one
//! [`TRANSITION`] tempo, EaseOutCubic, and no entrance on first
//! mount (a fixture doesn't re-animate; only a selection *change*
//! travels).
//!
//! [`pill_tab_style`]: super::pill_tab_style
//! [`TRANSITION`]: crate::anim::TRANSITION

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{tree, Operation, Tree, Widget};
use iced::advanced::{overlay, renderer, Clipboard, Renderer as _, Shell};
use iced::animation::{Animation, Easing};
use iced::{mouse, Element, Event, Length, Rectangle, Size, Theme, Vector};

use crate::anim::TRANSITION;

pub struct Glide<'a, M> {
    content: Element<'a, M, Theme, iced::Renderer>,
    /// Child indices (within the wrapped row/column) that are tab
    /// pills, in nav order. Non-pill children (logos, spacers,
    /// decorations) are simply absent from this list.
    targets: Vec<usize>,
    /// Position of the active pill within `targets`.
    active: Option<usize>,
}

impl<'a, M> Glide<'a, M> {
    pub fn new(
        content: impl Into<Element<'a, M, Theme, iced::Renderer>>,
        targets: Vec<usize>,
        active: Option<usize>,
    ) -> Self {
        Self {
            content: content.into(),
            targets,
            active,
        }
    }

    /// The common case: every child is a pill, in order.
    pub fn over_all(
        content: impl Into<Element<'a, M, Theme, iced::Renderer>>,
        count: usize,
        active: Option<usize>,
    ) -> Self {
        Self::new(content, (0..count).collect(), active)
    }

    fn target_bounds(&self, layout: Layout<'_>, position: usize) -> Option<Rectangle> {
        let child = *self.targets.get(position)?;
        layout.children().nth(child).map(|l| l.bounds())
    }
}

struct State {
    /// Last seen active position — a change is what starts a glide.
    active: Option<usize>,
    /// Departure bounds of the in-flight plate.
    from: Option<Rectangle>,
    anim: Animation<f32>,
}

fn lerp_rect(a: Rectangle, b: Rectangle, t: f32) -> Rectangle {
    let l = |a: f32, b: f32| a + (b - a) * t;
    Rectangle {
        x: l(a.x, b.x),
        y: l(a.y, b.y),
        width: l(a.width, b.width),
        height: l(a.height, b.height),
    }
}

impl<'a, M> Widget<M, Theme, iced::Renderer> for Glide<'a, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        // Seeded at-rest on the current selection: mounting a
        // screen never plays a glide, only an in-place change does.
        tree::State::new(State {
            active: self.active,
            from: None,
            anim: Animation::new(1.0),
        })
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        // The content's node is our node (like `mouse_area`), so
        // `layout.children()` below indexes the pills directly.
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        if state.active != self.active {
            let now = iced::time::Instant::now();
            // Both endpoints real → glide. Appearing from / vanishing
            // to "no selection" just snaps; there's nothing to
            // travel between.
            if let (Some(prev), Some(next)) = (state.active, self.active) {
                if let (Some(from), Some(_)) = (self.target_bounds(layout, prev), self.target_bounds(layout, next)) {
                    // Re-targeted mid-flight: depart from wherever
                    // the plate is drawn right now, not from the
                    // old pill — no backwards jump.
                    let depart = match (state.from, state.anim.is_animating(now)) {
                        (Some(f), true) => lerp_rect(f, from, state.anim.interpolate_with(|v| v, now)),
                        _ => from,
                    };
                    state.from = Some(depart);
                    state.anim = Animation::new(0.0)
                        .duration(TRANSITION)
                        .easing(Easing::EaseOutCubic)
                        .go(1.0, now);
                    crate::anim::kick(TRANSITION);
                }
            }
            state.active = self.active;
        }
        // Local redraw fallback while mid-flight, mirroring
        // menu_button — the app-level animation subscription is the
        // primary driver, but a glide inside an overlay (the
        // in-session settings modal) shouldn't depend on it.
        if let Event::Window(iced::window::Event::RedrawRequested(_)) = event {
            if state.anim.is_animating(iced::time::Instant::now()) {
                shell.request_redraw();
            }
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer, operation: &mut dyn Operation) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if let Some(to) = self.active.and_then(|pos| self.target_bounds(layout, pos)) {
            let state = tree.state.downcast_ref::<State>();
            let now = iced::time::Instant::now();
            let in_flight = state.anim.is_animating(now);
            let plate = if in_flight {
                let t = state.anim.interpolate_with(|v| v, now);
                // While the plate is still traveling, back the
                // destination pill with a rising primary wash — its
                // label already wears the on-plate ink color, which
                // needs *something* behind it from frame one.
                let primary = theme.palette().primary;
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: to,
                        border: iced::Border {
                            radius: super::tech_radius(12.0),
                            width: 0.0,
                            color: iced::Color::TRANSPARENT,
                        },
                        ..renderer::Quad::default()
                    },
                    iced::Background::Color(iced::Color {
                        a: 0.35 + 0.65 * t,
                        ..primary
                    }),
                );
                state.from.map(|f| lerp_rect(f, to, t)).unwrap_or(to)
            } else {
                to
            };
            let (background, shadow) = super::pill_plate(theme, cursor.is_over(to));
            renderer.fill_quad(
                renderer::Quad {
                    bounds: plate,
                    border: iced::Border {
                        radius: super::tech_radius(12.0),
                        width: 0.0,
                        color: iced::Color::TRANSPARENT,
                    },
                    shadow,
                    snap: false,
                },
                background,
            );
        }
        self.content
            .as_widget()
            .draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, iced::Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

impl<'a, M: 'a> From<Glide<'a, M>> for Element<'a, M, Theme, iced::Renderer> {
    fn from(glide: Glide<'a, M>) -> Self {
        Self::new(glide)
    }
}

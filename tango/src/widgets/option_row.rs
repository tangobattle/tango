//! The hover plate behind a settings "options screen" row. Wraps
//! the row's content and lights a faint accent plate under the
//! cursor — the quiet cousin of [`list_item`]'s hover chrome, so
//! rows that hold controls respond to the pointer like the rows in
//! every list do, without being buttons (the controls inside keep
//! all the interaction).
//!
//! [`list_item`]: super::list_item

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{tree, Operation, Tree, Widget};
use iced::advanced::{overlay, renderer, Clipboard, Renderer as _, Shell};
use iced::{mouse, Element, Event, Length, Rectangle, Size, Theme, Vector};

pub struct OptionRow<'a, M> {
    content: Element<'a, M, Theme, iced::Renderer>,
}

impl<'a, M> OptionRow<'a, M> {
    pub fn new(content: impl Into<Element<'a, M, Theme, iced::Renderer>>) -> Self {
        Self {
            content: content.into(),
        }
    }
}

/// Hover state as of the last paint — a flip against the live
/// cursor is what asks for the redraw that repaints the plate.
#[derive(Default)]
struct State {
    hovered: bool,
}

impl<'a, M> Widget<M, Theme, iced::Renderer> for OptionRow<'a, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
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
        let hovered = cursor.is_over(layout.bounds());
        if let Event::Window(iced::window::Event::RedrawRequested(_)) = event {
            state.hovered = hovered;
        } else if state.hovered != hovered {
            if hovered {
                // The row responds to the pointer in sound as well
                // as light — the faintest cue in the sfx set.
                crate::audio::ui_sfx::play(crate::audio::ui_sfx::Sfx::Hover);
            }
            shell.request_redraw();
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
        if cursor.is_over(layout.bounds()) {
            let primary = theme.palette().primary;
            let bg = theme.palette().background;
            renderer.fill_quad(
                renderer::Quad {
                    bounds: layout.bounds(),
                    border: iced::Border {
                        radius: 4.0.into(),
                        width: 1.0,
                        color: iced::Color { a: 0.35, ..primary },
                    },
                    ..renderer::Quad::default()
                },
                iced::Background::Color(super::mix(bg, primary, 0.10)),
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

impl<'a, M: 'a> From<OptionRow<'a, M>> for Element<'a, M, Theme, iced::Renderer> {
    fn from(row: OptionRow<'a, M>) -> Self {
        Self::new(row)
    }
}

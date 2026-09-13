//! Request the next presentation frame without sending an action into Luau.
use iced::advanced::{
    layout, mouse, overlay, renderer,
    widget::{tree, Operation, Tree},
    Clipboard, Layout, Shell, Widget,
};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

use super::Message;

pub(super) fn wrap(content: Element<'_, Message>, active: bool) -> Element<'_, Message> {
    Element::new(Redraw { content, active })
}

struct Redraw<'a> {
    content: Element<'a, Message>,
    active: bool,
}

// Forward the child's tree identity, so starting/stopping clocks does not reset
// focus, text cursors, scroll positions, overlays or drag state.
impl Widget<Message, Theme, Renderer> for Redraw<'_> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }
    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(tree, layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget_mut()
            .update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
        if self.active && matches!(event, Event::Window(iced::window::Event::RedrawRequested(_))) {
            shell.publish(None);
            shell.request_redraw();
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::renderer::Headless;

    #[test]
    fn active_frames_request_redraw_without_a_script_action_and_stop_at_rest() {
        let renderer =
            iced::futures::executor::block_on(Renderer::new(iced::Font::DEFAULT, 13.0.into(), Some("tiny-skia")))
                .unwrap();
        for active in [true, false] {
            let mut widget = wrap(iced::widget::Space::new().width(100).height(40).into(), active);
            let mut tree = Tree::new(&widget);
            let layout = widget.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(100.0, 40.0)),
            );
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            widget.as_widget_mut().update(
                &mut tree,
                &Event::Window(iced::window::Event::RedrawRequested(iced::time::Instant::now())),
                Layout::new(&layout),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &Rectangle::with_size(Size::new(100.0, 40.0)),
            );
            assert_eq!(
                shell.redraw_request(),
                if active {
                    iced::window::RedrawRequest::NextFrame
                } else {
                    iced::window::RedrawRequest::Wait
                }
            );
            assert_eq!(messages, if active { vec![None] } else { vec![] });
        }
    }
}

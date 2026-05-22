//! Synchronous keyboard + gamepad capture widget. Wraps the session
//! view and delivers both kinds of input through the same iced
//! loop iteration the event arrives in.
//!
//! Why a widget instead of a subscription: `iced::event::listen_with`
//! and `iced::Subscription::run` both route messages through
//! `runtime.broadcast` → `mpsc::try_send` → the consumer task,
//! which doesn't surface the resulting `Message` back to
//! `program.update()` until the *next* `AboutToWait`. That extra
//! winit-loop iteration is what made the iced port of the
//! session feel laggier than the egui-era frontend. Widget
//! `update()` runs inside `interface.update()` on the same
//! iteration, so `shell.publish` lands the message in the very
//! next `program.update()` call.
//!
//! Keyboard events arrive as `Event::Keyboard` and are forwarded
//! verbatim. Gamepads aren't part of iced's event stream, so we
//! drain SDL3's event pump (via the thread-local helper in
//! [`crate::gamepad`]) on every `RedrawRequested` — which
//! `interface.update` synthesizes once per redraw (see the
//! `let redraw_event` block in `iced_winit::run_with_executor`).
//!
//! Redraws are driven by [`crate::session::subscription`]'s wake
//! on the active session's `tokio::sync::Notify` (signaled in the
//! per-frame `frame_callback`), so gamepad polling lines up with
//! the GBA's vblank instead of spinning on `shell.request_redraw()`.

use iced::advanced::layout;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::{mouse, window, Element, Event, Length, Rectangle, Size, Vector};

use crate::gamepad::GamepadEvent;

/// Tagged input handed to the [`InputCapture`] callback. Keyboard
/// events borrow the iced event so the caller can pattern-match
/// without cloning; gamepad events come pre-normalized from
/// [`crate::gamepad`] (SDL3-derived but with the call-site facing
/// surface narrowed).
pub enum Input<'a> {
    Keyboard(&'a iced::keyboard::Event),
    Gamepad(&'a GamepadEvent),
}

pub struct InputCapture<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    on_input: Box<dyn Fn(Input<'_>) -> Option<Message> + 'a>,
}

impl<'a, Message, Theme, Renderer> InputCapture<'a, Message, Theme, Renderer> {
    pub fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        on_input: impl Fn(Input<'_>) -> Option<Message> + 'a,
    ) -> Self {
        Self {
            content: content.into(),
            on_input: Box::new(on_input),
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for InputCapture<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        // Child first so focused widgets still get their keys —
        // we only fire on events nobody else consumed.
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

        if shell.is_event_captured() {
            return;
        }

        match event {
            Event::Keyboard(kb) => {
                if let Some(message) = (self.on_input)(Input::Keyboard(kb)) {
                    shell.publish(message);
                }
            }
            Event::Window(window::Event::RedrawRequested(_)) => {
                crate::gamepad::pump(|ev| {
                    if let Some(message) = (self.on_input)(Input::Gamepad(&ev)) {
                        shell.publish(message);
                    }
                });
                // No `shell.request_redraw()` — the session
                // subscription's vblank-notify wake is what
                // perpetuates the loop now. Pacing redraws here
                // would race the notify and waste CPU on
                // redundant uploads.
            }
            _ => {}
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
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
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
            .draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
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
            .overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

impl<'a, Message, Theme, Renderer> From<InputCapture<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(w: InputCapture<'a, Message, Theme, Renderer>) -> Self {
        Element::new(w)
    }
}

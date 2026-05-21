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
//! verbatim. Gamepads aren't part of iced's event stream, so the
//! widget owns a `Gilrs` in its `tree::State` and drains it on
//! every `RedrawRequested` (which `interface.update` synthesizes
//! once per redraw — see the `let redraw_event` block in
//! `iced_winit::run_with_executor`).
//!
//! The optional `on_tick` callback fires on the same
//! `RedrawRequested` and is meant for the per-frame refresh of
//! the GBA framebuffer Handle. Going through
//! `iced::window::frames()` for that would lose a full redraw
//! cycle: the Tick `Message` is broadcast *after* draw and
//! arrives at `program.update` one winit iteration later, so the
//! freshly-uploaded image only appears on the *next* present. A
//! widget-published Tick lands in the same `RedrawRequested`
//! call — iced re-enters `interface.update` and rebuilds the
//! view before drawing, so the new framebuffer is on screen
//! this very redraw.

use iced::advanced::layout;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{tree, Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::{mouse, window, Element, Event, Length, Rectangle, Size, Vector};

/// Tagged input handed to the [`InputCapture`] callback. Borrows the
/// underlying event so the caller can pattern-match without cloning;
/// produce an owned `Message` from the relevant fields.
pub enum Input<'a> {
    Keyboard(&'a iced::keyboard::Event),
    Gamepad(&'a gilrs::Event),
}

pub struct InputCapture<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    on_input: Box<dyn Fn(Input<'_>) -> Option<Message> + 'a>,
    on_tick: Box<dyn Fn() -> Option<Message> + 'a>,
}

impl<'a, Message, Theme, Renderer> InputCapture<'a, Message, Theme, Renderer> {
    pub fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        on_input: impl Fn(Input<'_>) -> Option<Message> + 'a,
        on_tick: impl Fn() -> Option<Message> + 'a,
    ) -> Self {
        Self {
            content: content.into(),
            on_input: Box::new(on_input),
            on_tick: Box::new(on_tick),
        }
    }
}

/// Per-widget state. `gilrs` is initialized lazily — `Gilrs::new()`
/// fails on hosts without an input subsystem and we don't want to
/// keep retrying. `init_failed` records that case so we log the
/// warning exactly once.
struct State {
    gilrs: Option<gilrs::Gilrs>,
    init_failed: bool,
}

impl State {
    fn new() -> Self {
        Self {
            gilrs: None,
            init_failed: false,
        }
    }

    fn ensure_gilrs(&mut self) -> Option<&mut gilrs::Gilrs> {
        if self.gilrs.is_none() && !self.init_failed {
            match gilrs::Gilrs::new() {
                Ok(g) => self.gilrs = Some(g),
                Err(e) => {
                    log::warn!("gilrs init failed: {e:?}");
                    self.init_failed = true;
                }
            }
        }
        self.gilrs.as_mut()
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for InputCapture<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new())
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
                let state: &mut State = tree.state.downcast_mut();
                if let Some(gilrs) = state.ensure_gilrs() {
                    while let Some(ev) = gilrs.next_event() {
                        if let Some(message) = (self.on_input)(Input::Gamepad(&ev)) {
                            shell.publish(message);
                        }
                    }
                }
                if let Some(message) = (self.on_tick)() {
                    shell.publish(message);
                }
                // Replaces what `iced::window::frames()` did for
                // us — keep the redraw loop self-perpetuating
                // without going through a subscription.
                shell.request_redraw();
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

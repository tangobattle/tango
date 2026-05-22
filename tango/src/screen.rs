//! Live-framebuffer widget. Pulls a fresh
//! `iced::widget::image::Handle` out of the active session on every
//! `RedrawRequested`, stores it in widget tree state, and renders it
//! at draw time — no `Message::Tick` round-trip through the iced
//! update loop.
//!
//! Why not a Tick message: iced 0.14's redraw loop in
//! `iced_winit::run_with_executor` will keep re-entering
//! `interface.update` as long as new messages keep landing in the
//! queue (the `message_count == messages.len()` check); after three
//! consecutive iterations it bails out and logs "More than 3
//! consecutive RedrawRequested events produced layout
//! invalidation". The old `InputCapture::on_tick` published a Tick
//! unconditionally on every RedrawRequested, which tripped that
//! check every single redraw. Moving the per-frame upload into the
//! widget's own `update` keeps the redraw cycle message-free in
//! the common case — `Screen` only publishes (the `on_ended`
//! message) when the session actually terminates.

use iced::advanced::image as advanced_image;
use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::widget::tree;
use iced::advanced::widget::Tree;
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::widget::image;
use iced::{mouse, window, Element, Event, Length, Rectangle, Rotation, Size};

/// Return value of [`Screen`]'s per-redraw callback.
pub enum Tick {
    /// Emulator hasn't produced a new frame — keep showing the
    /// cached one.
    Idle,
    /// Fresh frame available; widget should cache the handle and
    /// stamp `frame_id` so the next tick can skip the work.
    NewFrame { handle: image::Handle, frame_id: u64 },
    /// Session has ended on its own (peer disconnect / comm error
    /// for PvP, end-of-replay reached, etc.); publish the configured
    /// `on_ended` message so the parent tears the view down.
    Ended,
}

pub struct Screen<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    on_tick: Box<dyn Fn(u64) -> Tick + 'a>,
    on_ended: Option<Message>,
    width: Length,
    height: Length,
    content_fit: iced::ContentFit,
    filter_method: image::FilterMethod,
    _phantom: std::marker::PhantomData<(Theme, Renderer)>,
}

impl<'a, Message, Theme, Renderer> Screen<'a, Message, Theme, Renderer> {
    pub fn new(on_tick: impl Fn(u64) -> Tick + 'a) -> Self {
        Self {
            on_tick: Box::new(on_tick),
            on_ended: None,
            width: Length::Fill,
            height: Length::Fill,
            content_fit: iced::ContentFit::Contain,
            filter_method: image::FilterMethod::Nearest,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn content_fit(mut self, fit: iced::ContentFit) -> Self {
        self.content_fit = fit;
        self
    }

    pub fn on_ended(mut self, msg: Message) -> Self {
        self.on_ended = Some(msg);
        self
    }
}

struct State {
    handle: Option<image::Handle>,
    last_frame_id: u64,
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Screen<'_, Message, Theme, Renderer>
where
    Message: Clone,
    Renderer: advanced_image::Renderer<Handle = image::Handle>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            handle: None,
            last_frame_id: 0,
        })
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let size = limits.resolve(self.width, self.height, Size::ZERO);
        layout::Node::new(size)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            let state = tree.state.downcast_mut::<State>();
            match (self.on_tick)(state.last_frame_id) {
                Tick::Idle => {}
                Tick::NewFrame { handle, frame_id } => {
                    state.handle = Some(handle);
                    state.last_frame_id = frame_id;
                }
                Tick::Ended => {
                    if let Some(msg) = self.on_ended.as_ref() {
                        shell.publish(msg.clone());
                    }
                }
            }
            // `InputCapture` also calls this — harmless to double up;
            // the runtime collapses redundant redraw requests.
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let Some(handle) = state.handle.as_ref() else {
            return;
        };
        image::draw(
            renderer,
            layout,
            handle,
            None,
            0.0.into(),
            self.content_fit,
            self.filter_method,
            Rotation::default(),
            1.0,
            1.0,
        );
    }
}

impl<'a, Message, Theme, Renderer> From<Screen<'a, Message, Theme, Renderer>> for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: 'a,
    Renderer: advanced_image::Renderer<Handle = image::Handle> + 'a,
{
    fn from(s: Screen<'a, Message, Theme, Renderer>) -> Self {
        Element::new(s)
    }
}

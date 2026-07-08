//! An icon button that opens a dropdown of labeled actions — the ⋮
//! "more actions" affordance for rows that would otherwise carry a
//! string of rarely-used icon buttons. The trigger renders like an
//! [`icon_button`] (same [`neutral`] chrome), and the dropdown reuses
//! sweeten's pick_list menu overlay so it looks and behaves exactly
//! like the app's other dropdowns (click-away dismissal included) —
//! just with an action message per row instead of a selection.
//!
//! [`icon_button`]: super::icon_button
//! [`neutral`]: super::neutral

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{tree, Tree, Widget};
use iced::advanced::{overlay, renderer, Clipboard, Renderer as _, Shell};
use iced::widget::button;
use iced::{mouse, touch, Element, Event, Length, Point, Rectangle, Size, Theme, Vector};
use sweeten::widget::overlay::menu::{self, Menu};

/// One dropdown row: a pre-resolved display label (the menu renders
/// via `Display`, which can't reach the language) and the message
/// selecting it emits.
#[derive(Clone)]
pub struct MenuItem<M> {
    label: String,
    message: M,
}

impl<M> MenuItem<M> {
    pub fn new(label: String, message: M) -> Self {
        Self { label, message }
    }
}

impl<M> std::fmt::Display for MenuItem<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// Width of the dropdown pane. Independent of the trigger's width —
/// the whole point of an icon-sized trigger — and right-aligned with
/// it, since the trigger usually sits at a row's right edge.
const MENU_WIDTH: f32 = 180.0;

pub struct MenuButton<'a, M> {
    content: Element<'a, M, Theme, iced::Renderer>,
    items: Vec<MenuItem<M>>,
    enabled: bool,
    padding: iced::Padding,
    item_padding: iced::Padding,
    style: Box<dyn Fn(&Theme, button::Status) -> button::Style + 'a>,
    menu_class: <Theme as menu::Catalog>::Class<'a>,
    last_status: Option<button::Status>,
}

impl<'a, M> MenuButton<'a, M> {
    pub fn new(
        content: impl Into<Element<'a, M, Theme, iced::Renderer>>,
        items: Vec<MenuItem<M>>,
        enabled: bool,
        padding: impl Into<iced::Padding>,
        item_padding: impl Into<iced::Padding>,
        style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
    ) -> Self {
        Self {
            content: content.into(),
            items,
            enabled,
            padding: padding.into(),
            item_padding: item_padding.into(),
            style: Box::new(style),
            menu_class: <Theme as menu::Catalog>::default(),
            last_status: None,
        }
    }
}

#[derive(Default)]
struct State {
    menu: menu::State,
    is_open: bool,
    hovered_option: Option<usize>,
}

impl<M: Clone> Widget<M, Theme, iced::Renderer> for MenuButton<'_, M> {
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
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        layout::padded(limits, Length::Shrink, Length::Shrink, self.padding, |limits| {
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, limits)
        })
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if state.is_open {
                    // The overlay didn't take the press, so it landed
                    // outside the dropdown — close it. (A press on a
                    // row is consumed by the menu itself, which also
                    // closes via the on_select hook.)
                    state.is_open = false;
                    shell.capture_event();
                } else if self.enabled && cursor.is_over(layout.bounds()) {
                    state.is_open = true;
                    state.hovered_option = None;
                    shell.capture_event();
                }
            }
            _ => {}
        }

        // Same redraw bookkeeping as sweeten's pick_list: the menu UI
        // redraws on events only, so a hover/open status flip has to
        // ask for the frame that will repaint the new chrome.
        let status = if !self.enabled {
            button::Status::Disabled
        } else if state.is_open {
            button::Status::Pressed
        } else if cursor.is_over(layout.bounds()) {
            button::Status::Hovered
        } else {
            button::Status::Active
        };
        if let Event::Window(iced::window::Event::RedrawRequested(_)) = event {
            self.last_status = Some(status);
        } else if self.last_status.is_some_and(|last| last != status) {
            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if self.enabled && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let style = (self.style)(theme, self.last_status.unwrap_or(button::Status::Active));
        renderer.fill_quad(
            renderer::Quad {
                bounds: layout.bounds(),
                border: style.border,
                shadow: style.shadow,
                snap: style.snap,
            },
            style
                .background
                .unwrap_or(iced::Background::Color(iced::Color::TRANSPARENT)),
        );
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            &renderer::Style {
                text_color: style.text_color,
            },
            layout.children().next().unwrap(),
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'_>,
        _renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, iced::Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        if !state.is_open {
            return None;
        }
        let bounds = layout.bounds();
        let menu = Menu::new(
            &mut state.menu,
            &self.items,
            &mut state.hovered_option,
            |item: MenuItem<M>| {
                state.is_open = false;
                item.message
            },
            None,
            None,
            &self.menu_class,
        )
        .width(MENU_WIDTH)
        .padding(self.item_padding);
        // Right-align the dropdown with the trigger (the menu overlay
        // only drops leftward-from-position, and the trigger usually
        // sits at a row's right edge where a left-aligned pane would
        // run off screen).
        let position = layout.position() + translation;
        let position = Point::new((position.x - (MENU_WIDTH - bounds.width)).max(0.0), position.y);
        Some(menu.overlay(position, *viewport, bounds.height, Length::Shrink))
    }
}

impl<'a, M: Clone + 'a> From<MenuButton<'a, M>> for Element<'a, M, Theme, iced::Renderer> {
    fn from(menu_button: MenuButton<'a, M>) -> Self {
        Self::new(menu_button)
    }
}

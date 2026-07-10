//! An icon button that opens a dropdown of labeled actions — the ⋮
//! "more actions" affordance for rows that would otherwise carry a
//! string of rarely-used icon buttons. The trigger renders like an
//! [`icon_button`] (same [`neutral`] chrome). The dropdown is a
//! custom overlay rather than sweeten's pick_list menu: rows carry
//! an icon next to the label (one `Display` string in one font
//! can't mix a lucide glyph with text), and destructive actions get
//! the danger tint their toolbar buttons used to wear — but it
//! keeps the pick_list menu's Catalog style, row metrics, and
//! click-away semantics so it still reads as the same family of
//! dropdown.
//!
//! [`icon_button`]: super::icon_button
//! [`neutral`]: super::neutral

use iced::advanced::layout::{self, Layout};
use iced::advanced::text::{self, Renderer as _, Text};
use iced::advanced::widget::{tree, Tree, Widget};
use iced::advanced::{overlay, renderer, Clipboard, Renderer as _, Shell};
use iced::widget::button;
use iced::{alignment, mouse, touch, Element, Event, Length, Point, Rectangle, Size, Theme, Transformation, Vector};
use lucide_icons::Icon;
use sweeten::widget::overlay::menu;

/// One dropdown row: an icon, a pre-resolved display label (the
/// menu draws directly, with no reach into the language), and the
/// message selecting it emits. `danger` tints the row's resting
/// icon + label in the theme's danger color — for destructive
/// actions, matching the red their standalone buttons used to wear.
/// `checked` rows are selections: they wear a trailing primary check
/// while on, and nothing (the slot stays reserved by the fixed menu
/// width) while off.
#[derive(Clone)]
pub struct MenuItem<M> {
    /// `None` rows are label-only — selection menus whose choices are
    /// self-describing (the speed steps) skip the glyph column.
    icon: Option<Icon>,
    label: String,
    message: M,
    danger: bool,
    checked: Option<bool>,
}

impl<M> MenuItem<M> {
    pub fn new(icon: Icon, label: String, message: M) -> Self {
        Self {
            icon: Some(icon),
            label,
            message,
            danger: false,
            checked: None,
        }
    }

    /// [`new`](Self::new), tinted as a destructive action.
    pub fn danger(icon: Icon, label: String, message: M) -> Self {
        Self {
            icon: Some(icon),
            label,
            message,
            danger: true,
            checked: None,
        }
    }

    /// A label-only selection row: a trailing check marks it while
    /// `on`. Selecting it still just emits `message` (and closes the
    /// menu) — state lives with the caller.
    pub fn toggle(label: String, message: M, on: bool) -> Self {
        Self {
            icon: None,
            label,
            message,
            danger: false,
            checked: Some(on),
        }
    }
}

/// Default width of the dropdown pane. Independent of the trigger's
/// width — the whole point of an icon-sized trigger — and
/// right-aligned with it, since the trigger usually sits at a row's
/// right edge. Narrow selection menus override it via
/// [`MenuButton::menu_width`].
const MENU_WIDTH: f32 = 180.0;

/// Gap between a row's icon and its label.
const ICON_GAP: f32 = 8.0;

/// How far the pane glides on entrance — from tucked toward the
/// trigger to its rest position, so the motion reads as the menu
/// unfolding out of the button.
const ENTER_TRAVEL: f32 = 6.0;

pub struct MenuButton<'a, M> {
    content: Element<'a, M, Theme, iced::Renderer>,
    items: Vec<MenuItem<M>>,
    enabled: bool,
    padding: iced::Padding,
    item_padding: iced::Padding,
    style: Box<dyn Fn(&Theme, button::Status) -> button::Style + 'a>,
    menu_class: <Theme as menu::Catalog>::Class<'a>,
    last_status: Option<button::Status>,
    menu_width: f32,
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
            menu_width: MENU_WIDTH,
        }
    }

    /// Override the dropdown pane's width — for narrow selection menus
    /// whose rows are a short label + check.
    pub fn menu_width(mut self, width: f32) -> Self {
        self.menu_width = width;
        self
    }
}

#[derive(Default)]
struct State {
    is_open: bool,
    hovered_option: Option<usize>,
    /// When the dropdown last opened — the entrance animation's
    /// clock. `None` until first opened.
    opened_at: Option<iced::time::Instant>,
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
                    // row is consumed by the overlay itself, which
                    // also closes.)
                    state.is_open = false;
                    shell.capture_event();
                } else if self.enabled && !self.items.is_empty() && cursor.is_over(layout.bounds()) {
                    state.is_open = true;
                    state.hovered_option = None;
                    state.opened_at = Some(iced::time::Instant::now());
                    // Keep the app's per-frame redraw subscription
                    // alive for the entrance (the overlay also
                    // self-requests frames as a local fallback).
                    crate::anim::kick(crate::anim::TRANSITION);
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
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, iced::Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        if !state.is_open {
            return None;
        }
        let bounds = layout.bounds();
        // Right-align the dropdown with the trigger (the pane only
        // grows rightward from its position, and the trigger usually
        // sits at a row's right edge where a left-aligned pane would
        // run off screen).
        let position = layout.position() + translation;
        let position = Point::new((position.x - (self.menu_width - bounds.width)).max(0.0), position.y);
        Some(overlay::Element::new(Box::new(MenuOverlay {
            items: &self.items,
            hovered_option: &mut state.hovered_option,
            is_open: &mut state.is_open,
            opened_at: state.opened_at.unwrap_or_else(iced::time::Instant::now),
            position,
            target_height: bounds.height,
            item_padding: self.item_padding,
            width: self.menu_width,
            class: &self.menu_class,
            below: true,
        })))
    }
}

impl<'a, M: Clone + 'a> From<MenuButton<'a, M>> for Element<'a, M, Theme, iced::Renderer> {
    fn from(menu_button: MenuButton<'a, M>) -> Self {
        Self::new(menu_button)
    }
}

/// The dropdown pane. A trimmed-down cousin of sweeten's pick_list
/// menu overlay (same Catalog style, same row metrics, same
/// below-or-above placement and click semantics), redrawn by hand so
/// each row can carry a lucide icon next to its label and the whole
/// pane can play an entrance.
struct MenuOverlay<'a, 'b: 'a, M> {
    items: &'a [MenuItem<M>],
    hovered_option: &'a mut Option<usize>,
    is_open: &'a mut bool,
    opened_at: iced::time::Instant,
    /// Top-left of the trigger, already shifted for right-alignment.
    position: Point,
    /// The trigger's height — the pane drops this far below the
    /// position (or flips above when there's more room there).
    target_height: f32,
    item_padding: iced::Padding,
    width: f32,
    class: &'a <Theme as menu::Catalog>::Class<'b>,
    /// Whether layout placed the pane below the trigger (vs flipped
    /// above). Set by `layout`, read by `draw` so the entrance
    /// glides in from the trigger's side.
    below: bool,
}

impl<M> MenuOverlay<'_, '_, M> {
    fn row_height(&self, renderer: &iced::Renderer) -> f32 {
        let text_size = renderer.default_size();
        f32::from(text::LineHeight::default().to_absolute(text_size)) + self.item_padding.y()
    }

    /// Entrance progress, eased — 1.0 once at rest. Same curve and
    /// duration as the app-wide [`crate::anim`] entrances
    /// (EaseOutCubic over TRANSITION).
    fn entrance(&self, now: iced::time::Instant) -> f32 {
        let t = (now.duration_since(self.opened_at).as_secs_f32() / crate::anim::TRANSITION.as_secs_f32()).min(1.0);
        1.0 - (1.0 - t).powi(3)
    }
}

impl<M: Clone> overlay::Overlay<M, Theme, iced::Renderer> for MenuOverlay<'_, '_, M> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let height = self.row_height(renderer) * self.items.len() as f32;
        let space_below = bounds.height - (self.position.y + self.target_height);
        let space_above = self.position.y;
        self.below = space_below > space_above || space_below >= height;
        let node = layout::Node::new(Size::new(self.width, height));
        node.move_to(if self.below {
            self.position + Vector::new(0.0, self.target_height)
        } else {
            self.position - Vector::new(0.0, height)
        })
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
    ) {
        let bounds = layout.bounds();
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(position) = cursor.position_in(bounds) {
                    let hovered = ((position.y / self.row_height(renderer)) as usize).min(self.items.len() - 1);
                    if *self.hovered_option != Some(hovered) {
                        *self.hovered_option = Some(hovered);
                        shell.request_redraw();
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if let Some(position) = cursor.position_in(bounds) {
                    let index = ((position.y / self.row_height(renderer)) as usize).min(self.items.len() - 1);
                    shell.publish(self.items[index].message.clone());
                    *self.is_open = false;
                    shell.capture_event();
                }
            }
            Event::Window(iced::window::Event::RedrawRequested(_)) => {
                // Keep frames coming while the entrance plays, even
                // if the app-level animation subscription has gone
                // idle between this widget's updates.
                if self.entrance(iced::time::Instant::now()) < 1.0 {
                    shell.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
    ) {
        let style = menu::Catalog::style(theme, self.class);
        let bounds = layout.bounds();
        let text_size = renderer.default_size();
        let row_height = self.row_height(renderer);
        let danger = theme.palette().danger;
        let primary = theme.palette().primary;

        // Entrance: glide out of the trigger's edge while scaling
        // 0.96 → 1.0 about the pane's center — the overlay cousin of
        // [`crate::anim::pop`]. Layout and hit-testing stay at the
        // rest position; only the drawing moves.
        let progress = self.entrance(iced::time::Instant::now());
        let travel = if self.below { -ENTER_TRAVEL } else { ENTER_TRAVEL };
        let scale = 0.96 + 0.04 * progress;
        let center = bounds.center();
        let transformation = Transformation::translate(0.0, travel * (1.0 - progress))
            * Transformation::translate(center.x, center.y)
            * Transformation::scale(scale)
            * Transformation::translate(-center.x, -center.y);

        renderer.with_transformation(transformation, |renderer| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: style.border,
                    shadow: style.shadow,
                    snap: false,
                },
                style.background,
            );

            for (i, item) in self.items.iter().enumerate() {
                let row = Rectangle {
                    x: bounds.x,
                    y: bounds.y + row_height * i as f32,
                    width: bounds.width,
                    height: row_height,
                };
                let hovered = *self.hovered_option == Some(i);
                if hovered {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle {
                                x: row.x + style.border.width,
                                width: row.width - style.border.width * 2.0,
                                ..row
                            },
                            border: iced::border::rounded(style.border.radius),
                            ..renderer::Quad::default()
                        },
                        style.selected_background,
                    );
                }
                // Danger rows rest in the theme's danger color; the
                // hover highlight overrides it — red-on-selection
                // would fight the highlight for attention.
                let color = if hovered {
                    style.selected_text_color
                } else if item.danger {
                    danger
                } else {
                    style.text_color
                };
                // Advanced shaping for the label — it's localized
                // text and may be CJK; the icon is a single PUA
                // glyph in an explicit font, so Basic is fine there.
                let text_at = |content: String, font: iced::Font, shaping: text::Shaping| Text {
                    content,
                    bounds: Size::new(f32::INFINITY, row.height),
                    size: text_size,
                    line_height: text::LineHeight::default(),
                    font,
                    align_x: text::Alignment::Default,
                    align_y: alignment::Vertical::Center,
                    shaping,
                    wrapping: text::Wrapping::default(),
                };
                let mut label_x = row.x + self.item_padding.left;
                if let Some(icon) = item.icon {
                    renderer.fill_text(
                        text_at(
                            char::from(icon).to_string(),
                            iced::Font::with_name("lucide"),
                            text::Shaping::Basic,
                        ),
                        Point::new(label_x, row.center_y()),
                        color,
                        bounds,
                    );
                    label_x += f32::from(text_size) + ICON_GAP;
                }
                renderer.fill_text(
                    text_at(item.label.clone(), renderer.default_font(), text::Shaping::Advanced),
                    Point::new(label_x, row.center_y()),
                    color,
                    bounds,
                );
                // Selection rows: a trailing check while on. Primary at
                // rest so the on-state reads at a glance; the hover
                // color takes over with the rest of the row.
                if item.checked == Some(true) {
                    renderer.fill_text(
                        text_at(
                            char::from(Icon::Check).to_string(),
                            iced::Font::with_name("lucide"),
                            text::Shaping::Basic,
                        ),
                        Point::new(
                            row.x + row.width - self.item_padding.right - f32::from(text_size),
                            row.center_y(),
                        ),
                        if hovered { style.selected_text_color } else { primary },
                        bounds,
                    );
                }
            }
        });
    }
}

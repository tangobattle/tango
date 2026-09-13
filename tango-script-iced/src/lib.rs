//! Embedded, game-neutral rendering for package-owned UI descriptions.
//! The caller owns the document, transactions, storage, window and host effects.
mod canvas;
mod motion;
mod rasterize;
mod redraw;
mod resources;
mod style;

use iced::widget::{button, checkbox, column, container, image, scrollable, slider, stack, text, tooltip, Space};
use iced::{Element, Length};
use sweeten::widget::{Column, Row};
use tango_script::ui::{Action, Event, Kind, Node, Tooltip, TooltipPosition};
use tango_ui::widgets;

pub type Message = Option<Action>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Choice(tango_script::Choice);
impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.label)
    }
}

/// Owns presentation state for one complete document view. Reuse it across
/// updates; create a fresh renderer when opening another document.
/// None messages refresh presentation, including animation frames and transient
/// drag gestures. Only Some actions enter the package callback.
/// Returned elements own their data; a host can release its document/session
/// lock immediately after building a view. Resources remain shared and bounded.
pub struct Renderer {
    images: std::sync::Arc<resources::Images>,
    feedback_namespace: u64,
    timelines: std::sync::Mutex<motion::Timelines>,
}
impl Default for Renderer {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        Self {
            images: Default::default(),
            timelines: Default::default(),
            feedback_namespace: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }
}
impl Renderer {
    /// Restart presentation when a retained document is shown again. This does
    /// not touch the package's view state, edits, history or host capabilities.
    pub fn restart_motion(&self) {
        *self.timelines.lock().unwrap() = Default::default();
    }

    pub fn view(&self, node: &Node) -> Element<'static, Message> {
        let frame = self.timelines.lock().unwrap().sample(node, iced::time::Instant::now());
        redraw::wrap(render(self, &frame, node, true), frame.active())
    }

    /// Sample a fixed presentation time without scheduling further frames.
    /// Use monotonic times for screenshots and deterministic host tests.
    pub fn view_at(&self, node: &Node, now: iced::time::Instant) -> Element<'static, Message> {
        let frame = self.timelines.lock().unwrap().sample(node, now);
        render(self, &frame, node, true)
    }

    /// Acknowledge successful host work for this document's action button.
    pub fn acknowledge(&self, id: &str) {
        tango_ui::copy_feedback::flash(&self.feedback_key(id));
    }
    fn feedback_key(&self, id: &str) -> String {
        format!("script/{}/{id}", self.feedback_namespace)
    }

    fn widget_id(&self, id: &str) -> iced::widget::Id {
        iced::widget::Id::from(self.feedback_key(id))
    }

    /// Execute a validated scroll effect inside this renderer's widget namespace.
    pub fn scroll_to<T: Send + 'static>(&self, id: &str, x: f32, y: f32) -> iced::Task<T> {
        iced::widget::operation::snap_to(self.widget_id(id), scrollable::RelativeOffset { x, y })
    }

    pub fn render_image(
        &self,
        source: &tango_script::ui::canvas::ImageSource,
        theme: &iced::Theme,
        font: iced::Font,
    ) -> tango_script::Result<image::Handle> {
        Ok(match source {
            tango_script::ui::canvas::ImageSource::Raster(image) => self.images.get(image),
            tango_script::ui::canvas::ImageSource::Drawing(drawing) => self.images.rasterize(
                drawing.width,
                drawing.height,
                &drawing.commands,
                tango_script::ui::canvas::Rasterize {
                    corner_radius: drawing.corner_radius,
                    nearest: false,
                },
                theme,
                font,
            ),
        })
    }
}

fn render_children(
    renderer: &Renderer,
    frame: &motion::Frame,
    nodes: &[Node],
    enabled: bool,
) -> Vec<Element<'static, Message>> {
    nodes.iter().map(|n| render(renderer, frame, n, enabled)).collect()
}

fn render(renderer: &Renderer, frame: &motion::Frame, node: &Node, parent_enabled: bool) -> Element<'static, Message> {
    let enabled = parent_enabled && node.enabled;
    let feedback = match &node.kind {
        Kind::Button { id, feedback, .. } if enabled && tango_ui::copy_feedback::is_lit(&renderer.feedback_key(id)) => {
            feedback.as_ref()
        }
        _ => None,
    };
    let layout = &node.layout;
    let width = style::length(layout.width);
    let height = style::length(layout.height);
    let body: Element<'_, Message> = match &node.kind {
        Kind::Column { children } => Column::from_vec(render_children(renderer, frame, children, enabled))
            .spacing(layout.spacing)
            .align_x(style::alignment(layout.align_x))
            .width(width)
            .height(height)
            .into(),
        Kind::Row { children } => Row::from_vec(render_children(renderer, frame, children, enabled))
            .spacing(layout.spacing)
            .align_y(style::alignment(layout.align_y))
            .width(width)
            .height(height)
            .into(),
        Kind::Wrap { children } => Row::from_vec(render_children(renderer, frame, children, enabled))
            .spacing(layout.spacing)
            .width(width)
            .wrap()
            .into(),
        Kind::Stack { children } => stack(render_children(renderer, frame, children, enabled))
            .width(width)
            .height(height)
            .into(),
        Kind::Container { child } => render(renderer, frame, child, enabled),
        Kind::Scroll {
            child,
            horizontal,
            id,
            on_scroll,
            scrollbar,
        } => {
            let scrollbar = *scrollbar;
            let mut scroll = scrollable(render(renderer, frame, child, enabled)).style(move |theme, status| {
                use tango_script::ui::style::ScrollbarVisibility;
                let mut style = widgets::chunky_scrollable(theme, status);
                if scrollbar == ScrollbarVisibility::Hidden
                    || (scrollbar == ScrollbarVisibility::Hover && matches!(status, scrollable::Status::Active { .. }))
                {
                    for rail in [&mut style.horizontal_rail, &mut style.vertical_rail] {
                        rail.background = None;
                        rail.scroller.background = iced::Color::TRANSPARENT.into();
                    }
                }
                style
            });
            if let Some(id) = id {
                scroll = scroll.id(renderer.widget_id(id));
                if enabled && *on_scroll {
                    let id = id.clone();
                    scroll = scroll.on_scroll(move |viewport| {
                        let offset = viewport.relative_offset();
                        // Iced divides by the scroll extent, which is zero on
                        // a fitting axis. Scripts always receive finite offsets.
                        let normalized = |n: f32| if n.is_finite() { n.clamp(0.0, 1.0) } else { 0.0 };
                        Some(Action {
                            id: id.clone(),
                            event: Event::Scroll {
                                x: normalized(offset.x),
                                y: normalized(offset.y),
                            },
                        })
                    });
                }
            }
            let direction = if *horizontal {
                scrollable::Direction::Horizontal(scrollable::Scrollbar::default())
            } else {
                scrollable::Direction::Vertical(scrollable::Scrollbar::default())
            };
            scroll.direction(direction).width(width).height(height).into()
        }
        Kind::List { id, children, reorder } => {
            let mut list = sweeten::widget::Column::from_vec(render_children(renderer, frame, children, enabled))
                .spacing(layout.spacing)
                .width(width)
                .height(height)
                .style(widgets::reorder_drag_style);
            if enabled && *reorder {
                let id = id.clone().expect("validated list ID");
                list = list.on_drag(move |event| match event {
                    sweeten::widget::drag::DragEvent::Dropped { index, target_index } if index != target_index => {
                        Some(Action {
                            id: id.clone(),
                            event: Event::Reorder {
                                from: index + 1,
                                to: target_index + 1,
                            },
                        })
                    }
                    _ => None,
                });
            }
            list.into()
        }
        Kind::Text {
            text: value,
            size,
            font,
            color,
            strikethrough,
            underline,
            wrapping,
        } => {
            let color = *color;
            // Preserve Iced's automatic text direction at the default alignment.
            // Explicit Left also changes advanced shaping/wrapping in zero-width cells.
            let align_x = match layout.align_x {
                tango_script::ui::Align::Start => text::Alignment::Default,
                tango_script::ui::Align::Center => text::Alignment::Center,
                tango_script::ui::Align::End => text::Alignment::Right,
            };
            let wrapping = match wrapping {
                tango_script::ui::Wrapping::None => text::Wrapping::None,
                tango_script::ui::Wrapping::Word => text::Wrapping::Word,
                tango_script::ui::Wrapping::Glyph => text::Wrapping::Glyph,
                tango_script::ui::Wrapping::WordOrGlyph => text::Wrapping::WordOrGlyph,
            };
            if *strikethrough || *underline {
                let span = iced::widget::text::Span::new(value.clone())
                    .strikethrough(*strikethrough)
                    .underline(*underline);
                let mut label = iced::widget::rich_text([span])
                    .on_link_click(iced::never)
                    .wrapping(wrapping)
                    .width(width)
                    .height(height)
                    .align_x(align_x)
                    .align_y(style::alignment(layout.align_y))
                    .size(style::text_size(*size));
                if *font != tango_script::ui::Font::Normal {
                    label = label.font(style::font(*font));
                }
                label
                    .style(move |theme| text::Style {
                        color: color.map(|color| style::color(theme, color)),
                    })
                    .into()
            } else {
                let mut label = text(value.clone())
                    .size(style::text_size(*size))
                    .wrapping(wrapping)
                    .width(width)
                    .height(height)
                    .align_x(align_x)
                    .align_y(style::alignment(layout.align_y));
                if *font != tango_script::ui::Font::Normal {
                    label = label.font(style::font(*font));
                }
                label
                    .style(move |theme| text::Style {
                        color: color.map(|color| style::color(theme, color)),
                    })
                    .into()
            }
        }
        Kind::Image {
            image: raster,
            nearest,
            opacity,
        } => image(renderer.images.get(raster))
            .filter_method(filter(*nearest))
            .opacity(*opacity)
            .width(width)
            .height(height)
            .into(),
        Kind::Input {
            id,
            text: label,
            value,
            placeholder,
            secure,
            content_padding,
            size,
        } => {
            let id = id.clone();
            let mut input = sweeten::widget::TextInput::new(placeholder, value)
                .secure(*secure)
                .size(style::text_size(*size))
                .style(widgets::chunky_text_input)
                .on_input_maybe(enabled.then(|| move |value| Some(Action::change(id.clone(), value))));
            if let Some(padding) = content_padding {
                input = input.padding(style::padding(*padding));
            }
            if label.is_empty() {
                input.into()
            } else {
                column![text(label.clone()).size(tango_ui::style::TEXT_CAPTION), input]
                    .spacing(4)
                    .into()
            }
        }
        Kind::Select {
            id,
            text: label,
            value,
            choices,
            content_padding,
            size,
        } => {
            let choices: Vec<_> = choices.iter().cloned().map(Choice).collect();
            let selected = choices.iter().find(|c| &c.0.value == value).cloned();
            let id = id.clone();
            let mut choice = sweeten::widget::pick_list::PickList::new(choices, selected, move |c: Choice| {
                Some(Action::change(id.clone(), c.0.value))
            })
            .disabled(move |choices| vec![!enabled; choices.len()])
            .text_size(style::text_size(*size))
            .style(widgets::chunky_pick_list);
            if let Some(padding) = content_padding {
                choice = choice.padding(style::padding(*padding));
            }
            if label.is_empty() {
                choice.into()
            } else {
                column![text(label.clone()).size(tango_ui::style::TEXT_CAPTION), choice]
                    .spacing(4)
                    .into()
            }
        }
        Kind::Button {
            id,
            action_enabled,
            text: label,
            value,
            child,
            size,
            font,
            content_padding,
            hovered,
            pressed,
            disabled,
            ..
        } => {
            let child = feedback
                .map_or(child.as_deref(), |f| Some(f.child.as_ref()))
                .map_or_else(
                    || {
                        let mut label = text(label.clone()).size(style::text_size(*size));
                        if *font != tango_script::ui::Font::Normal {
                            label = label.font(style::font(*font));
                        }
                        label.into()
                    },
                    |child| render(renderer, frame, child, enabled && feedback.is_none()),
                );
            let tone = node.tone;
            let appearance = node.appearance.clone();
            let (hovered, pressed, disabled) = (hovered.clone(), pressed.clone(), disabled.clone());
            let mut button = button(child)
                .on_press_maybe((enabled && *action_enabled).then(|| Some(Action::activate(id, value))))
                .width(width)
                .height(height)
                .style(move |theme, status| {
                    let base = style::button_appearance(theme, style::button(theme, status, tone), &appearance);
                    let overrides = match status {
                        button::Status::Hovered => &hovered,
                        button::Status::Pressed => &pressed,
                        button::Status::Disabled => &disabled,
                        button::Status::Active => return base,
                    };
                    style::button_appearance(theme, base, overrides)
                });
            if let Some(padding) = content_padding {
                button = button.padding(style::padding(*padding));
            }
            button.into()
        }
        Kind::Checkbox {
            id,
            text: label,
            checked,
            size,
            text_size,
        } => {
            let id = id.clone();
            let mut control = checkbox(*checked)
                .label(label.clone())
                .on_toggle_maybe(enabled.then(|| {
                    move |checked| {
                        Some(Action {
                            id: id.clone(),
                            event: Event::Toggle { checked },
                        })
                    }
                }))
                .style(widgets::chunky_checkbox);
            if let Some(size) = size {
                control = control.size(*size);
            }
            if let Some(size) = text_size {
                control = control.text_size(style::text_size(*size));
            }
            control.into()
        }
        Kind::Slider {
            id,
            value,
            min,
            max,
            step,
        } => {
            let id = id.clone();
            slider(*min..=*max, *value, move |number| {
                enabled.then(|| Action {
                    id: id.clone(),
                    event: Event::Slide { number },
                })
            })
            .step(*step)
            .into()
        }
        Kind::Canvas {
            width: logical_width,
            height: logical_height,
            ..
        } => {
            let width = if width == Length::Shrink {
                Length::Fixed(*logical_width)
            } else {
                width
            };
            let height = if height == Length::Shrink {
                Length::Fixed(*logical_height)
            } else {
                height
            };
            iced::widget::canvas(canvas::Program {
                node: std::sync::Arc::new(node.clone()),
                enabled,
                resources: renderer.images.clone(),
            })
            .width(width)
            .height(height)
            .into()
        }
        Kind::Space => Space::new().width(width).height(height).into(),
    };
    // Buttons own their chrome; their outer layout wrapper must not paint it
    // again or override the foreground inherited by the button's child.
    let (tone, appearance) = if matches!(node.kind, Kind::Button { .. }) {
        (Default::default(), Default::default())
    } else {
        (node.tone, node.appearance.clone())
    };
    // A transparent container still culls children outside its bounds. Avoid
    // adding one to bare widgets: text can overflow its allocated width, and
    // that overflow should remain visible until a script requests clipping.
    let mut outer_layout = layout.clone();
    // These widgets already consume their dimensions and cross-axis alignment.
    // Only properties they cannot apply need an additional layout container.
    if matches!(
        node.kind,
        Kind::Column { .. }
            | Kind::Row { .. }
            | Kind::Stack { .. }
            | Kind::Scroll { .. }
            | Kind::List { .. }
            | Kind::Image { .. }
            | Kind::Text { .. }
            | Kind::Button { .. }
            | Kind::Canvas { .. }
            | Kind::Space
    ) {
        outer_layout.width = Default::default();
        outer_layout.height = Default::default();
    }
    match node.kind {
        Kind::Column { .. } => outer_layout.align_x = Default::default(),
        Kind::Row { .. } => outer_layout.align_y = Default::default(),
        Kind::Text { .. } => {
            outer_layout.align_x = Default::default();
            outer_layout.align_y = Default::default();
        }
        _ => {}
    }
    outer_layout.spacing = 0.0;
    let body = if matches!(node.kind, Kind::Container { .. })
        || outer_layout != Default::default()
        || tone != Default::default()
        || appearance != Default::default()
    {
        container(body)
            .padding(style::padding(layout.padding))
            .width(width)
            .height(height)
            .max_width(layout.max_width.unwrap_or(f32::INFINITY))
            .max_height(layout.max_height.unwrap_or(f32::INFINITY))
            .clip(layout.clip)
            .align_x(style::alignment(layout.align_x))
            .align_y(style::alignment(layout.align_y))
            .style(move |theme| style::container_appearance(theme, style::container(theme, tone), &appearance))
            .into()
    } else {
        body
    };
    let body: Element<'_, Message> = match node.cursor.filter(|_| enabled) {
        Some(cursor) => iced::widget::mouse_area(body).interaction(style::cursor(cursor)).into(),
        None => body.into(),
    };
    let body = match feedback.and_then(|f| f.tooltip.as_ref()).or(node.tooltip.as_ref()) {
        Some(Tooltip::Text(label)) => {
            tooltip(body, widgets::tooltip_bubble(label.clone()), tooltip::Position::Top).into()
        }
        Some(Tooltip::Rich { content, position, gap }) => {
            let position = match position {
                TooltipPosition::Top => tooltip::Position::Top,
                TooltipPosition::Bottom => tooltip::Position::Bottom,
                TooltipPosition::Left => tooltip::Position::Left,
                TooltipPosition::Right => tooltip::Position::Right,
                TooltipPosition::FollowCursor => tooltip::Position::FollowCursor,
            };
            tooltip(body, render(renderer, frame, content, false), position)
                .gap(*gap)
                .into()
        }
        None => body,
    };
    motion::apply(body, node.motion.as_ref(), frame)
}

fn filter(nearest: bool) -> image::FilterMethod {
    if nearest {
        image::FilterMethod::Nearest
    } else {
        image::FilterMethod::Linear
    }
}

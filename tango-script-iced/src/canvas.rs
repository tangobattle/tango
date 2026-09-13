use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{keyboard, mouse, Point, Radians, Rectangle, Renderer, Theme, Vector};
use tango_script::ui::{Action, Draw, Event, Kind, Modifiers, Node, PointerButton, PointerPhase};

use crate::{filter, style, Message};

pub(crate) struct Program {
    pub node: std::sync::Arc<Node>,
    pub resources: std::sync::Arc<crate::resources::Images>,
    pub enabled: bool,
}

#[derive(Default)]
pub(crate) struct State {
    focused: bool,
    hovered: bool,
    held: Option<mouse::Button>,
    position: Point,
    modifiers: keyboard::Modifiers,
    identity: Option<String>,
}

impl canvas::Program<Message> for Program {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<Message>> {
        let Kind::Canvas {
            id,
            width,
            height,
            pointer,
            pointer_capture,
            keyboard,
            scale_to_fit,
            rasterize,
            ..
        } = &self.node.kind
        else {
            return None;
        };
        let identity = self.node.key.as_ref().or(id.as_ref()).cloned();
        if identity != state.identity {
            *state = State {
                identity,
                ..State::default()
            };
        }
        if !self.enabled || bounds.width <= 0.0 || bounds.height <= 0.0 {
            return None;
        }
        let inside = cursor.is_over(bounds);
        let (scale, offset) = transform(*width, *height, bounds.size(), rasterize.is_some(), *scale_to_fit);
        if let Some(position) = cursor.position() {
            state.position = Point::new(
                (position.x - bounds.x - offset.x) / scale.x,
                (position.y - bounds.y - offset.y) / scale.y,
            );
        }
        let mut dx = 0.0;
        let mut dy = 0.0;
        let phase = match event {
            canvas::Event::Window(iced::window::Event::Unfocused) => {
                let was_hovered = state.hovered || state.held.is_some();
                state.focused = false;
                state.held = None;
                state.hovered = false;
                if !was_hovered {
                    return None;
                }
                PointerPhase::Leave
            }
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
                return None;
            }
            canvas::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
            | canvas::Event::Keyboard(keyboard::Event::KeyReleased { key, modifiers, .. })
                if *keyboard && state.focused =>
            {
                state.modifiers = *modifiers;
                let key = match key {
                    keyboard::Key::Character(s) => s.to_string(),
                    keyboard::Key::Named(key) => format!("{key:?}"),
                    keyboard::Key::Unidentified => return None,
                };
                let pressed = matches!(event, canvas::Event::Keyboard(keyboard::Event::KeyPressed { .. }));
                return Some(
                    iced::widget::Action::publish(Some(Action {
                        id: id.clone()?,
                        event: Event::Key {
                            key,
                            pressed,
                            modifiers: to_modifiers(*modifiers),
                        },
                    }))
                    .and_capture(),
                );
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                state.focused = inside;
                if !inside {
                    return None;
                }
                state.held = Some(*button);
                PointerPhase::Down
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(_)) if state.held.is_some() => PointerPhase::Up,
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) if inside || state.held.is_some() => {
                PointerPhase::Move
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) if state.hovered => {
                PointerPhase::Leave
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) if inside => {
                (dx, dy) = match delta {
                    mouse::ScrollDelta::Lines { x, y } => (x * 16.0, y * 16.0),
                    mouse::ScrollDelta::Pixels { x, y } => (*x, *y),
                };
                dx /= scale.x;
                dy /= scale.y;
                PointerPhase::Wheel
            }
            _ => return None,
        };
        state.hovered = inside && phase != PointerPhase::Leave;
        let button = state.held;
        if phase == PointerPhase::Up {
            state.held = None;
        }
        if !pointer {
            return None;
        }
        let button = match button {
            None => PointerButton::None,
            Some(mouse::Button::Left) => PointerButton::Left,
            Some(mouse::Button::Middle) => PointerButton::Middle,
            Some(mouse::Button::Right) => PointerButton::Right,
            Some(_) => PointerButton::Other,
        };
        let message = iced::widget::Action::publish(Some(Action {
            id: id.clone()?,
            event: Event::Pointer {
                phase,
                x: state.position.x,
                y: state.position.y,
                dx,
                dy,
                button,
                modifiers: to_modifiers(state.modifiers),
            },
        }));
        Some(if pointer_capture.matches(phase, button) {
            message.and_capture()
        } else {
            message
        })
    }

    fn draw(
        &self,
        _state: &State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let Kind::Canvas {
            width,
            height,
            commands,
            rasterize,
            scale_to_fit,
            ..
        } = &self.node.kind
        else {
            return Vec::new();
        };
        let mut frame = Frame::new(renderer, bounds.size());
        let default_font = iced::advanced::text::Renderer::default_font(renderer);
        if let Some(options) = rasterize {
            let size = iced::Size::new(*width, *height);
            let fitted = if *scale_to_fit {
                iced::ContentFit::Contain.fit(size, bounds.size())
            } else {
                size
            };
            frame.draw_image(
                Rectangle::new(
                    if *scale_to_fit {
                        Point::new(
                            (bounds.width - fitted.width) / 2.0,
                            (bounds.height - fitted.height) / 2.0,
                        )
                    } else {
                        Point::ORIGIN
                    },
                    fitted,
                ),
                iced::advanced::image::Image::new(self.resources.rasterize(
                    *width as u32,
                    *height as u32,
                    commands,
                    *options,
                    theme,
                    default_font,
                ))
                .filter_method(filter(options.nearest)),
            );
            return vec![frame.into_geometry()];
        }
        frame.with_clip(Rectangle::with_size(bounds.size()), |frame| {
            if *scale_to_fit {
                frame.scale_nonuniform(Vector::new(bounds.width / width, bounds.height / height));
            }
            for command in commands {
                draw(frame, theme, command, &self.resources, default_font);
            }
        });
        vec![frame.into_geometry()]
    }
}

fn transform(width: f32, height: f32, bounds: iced::Size, rasterized: bool, scale_to_fit: bool) -> (Vector, Vector) {
    if !scale_to_fit {
        return (Vector::new(1.0, 1.0), Vector::ZERO);
    }
    if rasterized {
        let fitted = iced::ContentFit::Contain.fit(iced::Size::new(width, height), bounds);
        (
            Vector::new(fitted.width / width, fitted.height / height),
            Vector::new(
                (bounds.width - fitted.width) / 2.0,
                (bounds.height - fitted.height) / 2.0,
            ),
        )
    } else {
        (Vector::new(bounds.width / width, bounds.height / height), Vector::ZERO)
    }
}

fn to_modifiers(m: keyboard::Modifiers) -> Modifiers {
    Modifiers {
        shift: m.shift(),
        control: m.control(),
        alt: m.alt(),
        command: m.command(),
    }
}

pub(crate) fn draw<R: iced::advanced::graphics::geometry::Renderer>(
    frame: &mut iced::advanced::graphics::geometry::Frame<R>,
    theme: &Theme,
    command: &Draw,
    resources: &crate::resources::Images,
    default_font: iced::Font,
) {
    let (path, paint) = match command {
        Draw::Rect {
            x,
            y,
            width,
            height,
            radius,
            paint,
        } => (
            if *radius == 0.0 {
                Path::rectangle(Point::new(*x, *y), iced::Size::new(*width, *height))
            } else {
                Path::rounded_rectangle(Point::new(*x, *y), iced::Size::new(*width, *height), (*radius).into())
            },
            paint,
        ),
        Draw::Ellipse { x, y, rx, ry, paint } => (
            Path::new(|p| {
                p.ellipse(canvas::path::arc::Elliptical {
                    center: Point::new(*x, *y),
                    radii: Vector::new(*rx, *ry),
                    rotation: Radians(0.0),
                    start_angle: Radians(0.0),
                    end_angle: Radians(std::f32::consts::TAU),
                });
            }),
            paint,
        ),
        Draw::Path { points, closed, paint } => (
            Path::new(|p| {
                if let Some([x, y]) = points.first() {
                    p.move_to(Point::new(*x, *y));
                }
                for [x, y] in points.iter().skip(1) {
                    p.line_to(Point::new(*x, *y));
                }
                if *closed && !points.is_empty() {
                    p.close();
                }
            }),
            paint,
        ),
        Draw::Text {
            x,
            y,
            text,
            size,
            font,
            color,
        } => {
            frame.fill_text(canvas::Text {
                content: text.clone(),
                position: Point::new(*x, *y),
                size: style::text_size(*size).into(),
                font: if *font == tango_script::ui::Font::Normal {
                    default_font
                } else {
                    style::font(*font)
                },
                color: style::color(theme, *color),
                shaping: iced::advanced::text::Shaping::Advanced,
                ..canvas::Text::default()
            });
            return;
        }
        Draw::Image {
            x,
            y,
            width,
            height,
            image,
            nearest,
        } => {
            frame.draw_image(
                Rectangle {
                    x: *x,
                    y: *y,
                    width: *width,
                    height: *height,
                },
                iced::advanced::image::Image::new(resources.get(image)).filter_method(filter(*nearest)),
            );
            return;
        }
    };
    let fill = style::color(theme, paint.fill);
    let stroke = style::color(theme, paint.stroke);
    if fill.a > 0.0 {
        if let Draw::Rect {
            x,
            y,
            width,
            height,
            radius: 0.0,
            ..
        } = command
        {
            // Preserve Iced's crisp fill-rectangle rasterization. Rounded
            // rectangles and arbitrary paths retain their antialiased edges.
            frame.fill_rectangle(Point::new(*x, *y), iced::Size::new(*width, *height), fill);
        } else {
            frame.fill(&path, fill);
        }
    }
    if stroke.a > 0.0 && paint.stroke_width > 0.0 {
        frame.stroke(
            &path,
            Stroke::default()
                .with_color(stroke)
                .with_width(paint.stroke_width)
                .with_line_cap(match paint.line_cap {
                    tango_script::ui::LineCap::Butt => canvas::LineCap::Butt,
                    tango_script::ui::LineCap::Square => canvas::LineCap::Square,
                    tango_script::ui::LineCap::Round => canvas::LineCap::Round,
                }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::canvas::Program as _;

    #[test]
    fn pointer_coordinates_follow_scaling_and_capture_release_outside() {
        let node = Node {
            motion: None,
            key: None,
            enabled: true,
            tooltip: None,
            cursor: None,
            layout: Default::default(),
            tone: Default::default(),
            appearance: Default::default(),
            kind: Kind::Canvas {
                id: Some("canvas".into()),
                width: 256.0,
                height: 256.0,
                pointer: true,
                pointer_capture: Default::default(),
                keyboard: true,
                rasterize: None,
                scale_to_fit: true,
                commands: vec![],
            },
        };
        let images = std::sync::Arc::new(crate::resources::Images::default());
        let program = Program {
            node: std::sync::Arc::new(node.clone()),
            resources: images.clone(),
            enabled: true,
        };
        let bounds = Rectangle {
            x: 10.0,
            y: 20.0,
            width: 512.0,
            height: 512.0,
        };
        let mut state = State::default();
        let output = program
            .update(
                &mut state,
                &canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                bounds,
                mouse::Cursor::Available(Point::new(138.0, 148.0)),
            )
            .unwrap();
        let (message, _, status) = output.into_inner();
        assert_eq!(status, iced::event::Status::Captured);
        let action = message.flatten().unwrap();
        assert!(matches!(
            action.event,
            Event::Pointer {
                phase: PointerPhase::Down,
                x: 64.0,
                y: 64.0,
                ..
            }
        ));
        assert!(state.focused);
        let output = program
            .update(
                &mut state,
                &canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                bounds,
                mouse::Cursor::Available(Point::new(600.0, 148.0)),
            )
            .unwrap();
        assert!(matches!(
            output.into_inner().0.flatten().unwrap().event,
            Event::Pointer {
                phase: PointerPhase::Up,
                x: 295.0,
                ..
            }
        ));
        assert!(state.held.is_none());
        let leave = program.update(
            &mut state,
            &canvas::Event::Window(iced::window::Event::Unfocused),
            bounds,
            mouse::Cursor::Unavailable,
        );
        assert!(!state.focused);
        // Losing the window also clears script-owned hover/ghost state.
        // Re-establish hover after the preceding out-of-bounds release.
        assert!(leave.is_none());
        program.update(
            &mut state,
            &canvas::Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(138.0, 148.0),
            }),
            bounds,
            mouse::Cursor::Available(Point::new(138.0, 148.0)),
        );
        let leave = program
            .update(
                &mut state,
                &canvas::Event::Window(iced::window::Event::Unfocused),
                bounds,
                mouse::Cursor::Unavailable,
            )
            .unwrap();
        assert!(matches!(
            leave.into_inner().0.flatten().unwrap().event,
            Event::Pointer {
                phase: PointerPhase::Leave,
                ..
            }
        ));
        assert!(!state.hovered);

        let mut node = node.clone();
        let Kind::Canvas { pointer_capture, .. } = &mut node.kind else {
            unreachable!()
        };
        *pointer_capture = tango_script::ui::PointerCapture::Matching(vec![tango_script::ui::canvas::PointerFilter {
            phase: PointerPhase::Down,
            button: Some(PointerButton::Left),
        }]);
        let program = Program {
            node: std::sync::Arc::new(node.clone()),
            resources: images.clone(),
            enabled: true,
        };
        // Observations still reach the script; only declared gestures stop
        // propagation to surrounding controls (such as scroll containers).
        for (event, captured) in [
            (mouse::Event::ButtonPressed(mouse::Button::Left), true),
            (mouse::Event::ButtonPressed(mouse::Button::Right), false),
            (
                mouse::Event::WheelScrolled {
                    delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
                },
                false,
            ),
            (
                mouse::Event::CursorMoved {
                    position: Point::new(138.0, 148.0),
                },
                false,
            ),
        ] {
            let output = program
                .update(
                    &mut State::default(),
                    &canvas::Event::Mouse(event),
                    bounds,
                    mouse::Cursor::Available(Point::new(138.0, 148.0)),
                )
                .unwrap();
            let (message, _, status) = output.into_inner();
            assert!(message.flatten().is_some());
            assert_eq!(status == iced::event::Status::Captured, captured);
        }
        // Fixed drawing units do not shrink with layout. Rasterized drawings
        // preserve aspect ratio, so pointer coordinates include letterboxing.
        for (fit, baked, expected_x, expected_y, expected_dy) in
            [(false, false, 128.0, 128.0, 16.0), (true, true, 64.0, 0.0, 8.0)]
        {
            let mut node = node.clone();
            let Kind::Canvas {
                height,
                scale_to_fit,
                rasterize,
                ..
            } = &mut node.kind
            else {
                unreachable!()
            };
            *height = 128.0;
            *scale_to_fit = fit;
            *rasterize = baked.then(Default::default);
            let program = Program {
                node: std::sync::Arc::new(node.clone()),
                resources: images.clone(),
                enabled: true,
            };
            let output = program
                .update(
                    &mut State::default(),
                    &canvas::Event::Mouse(mouse::Event::WheelScrolled {
                        delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
                    }),
                    bounds,
                    mouse::Cursor::Available(Point::new(138.0, 148.0)),
                )
                .unwrap();
            let Event::Pointer { x, y, dy, .. } = output.into_inner().0.flatten().unwrap().event else {
                panic!()
            };
            assert_eq!((x, y, dy), (expected_x, expected_y, expected_dy));
        }
    }
}

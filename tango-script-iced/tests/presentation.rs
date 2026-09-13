//! Pixel and interaction comparisons with native, game-neutral Tango widgets.
//! These cover primitives, not any game's complete embedded-editor parity gate.
use std::collections::BTreeMap;
use std::sync::Once;

use iced::advanced::renderer::Headless;
use iced::widget::{button, container, image as image_widget, row, text, tooltip, Space};
use iced::{mouse, Color, Element, Fill, Point, Theme};
use tango_script::{Action, Document, Package, PackageRef, Profile};
use tango_script_iced::Message;
use tango_ui::{style, widgets};

fn document(expression: &str) -> Document {
    document_with_files(expression, &[])
}

fn document_with_files(expression: &str, files: &[(&str, &str)]) -> Document {
    let source = format!(
        r#"--!strict
local FILL: Size = "fill"
local CENTER: Align = "center"
local CAPTION: TextSize = "caption"
local DANGER: Color = "danger"
local MUTED: Color = "muted"
local FOLLOW: TooltipPosition = "follow_cursor"
local function nodes(children: {{Node}}): {{Node}} return children end
local function tip(content: Node): Tooltip return {{position = FOLLOW, gap = 8, content = content}} end
return {{
    decode = function(bytes: buffer): buffer return bytes end,
    encode = function(bytes: buffer): buffer return bytes end,
    validate = function(_bytes: buffer): {{string}} return {{}} end,
    update = function(_bytes: buffer, _state: ViewState, _action: Action): {{Effect}}? return nil end,
    view = function(_bytes: buffer, _state: ViewState): Node local node: Node = {expression}; return node end,
    }}"#
    );
    let mut sources = BTreeMap::from([
        (
            "package.toml".into(),
            b"name = 'test.presentation'\nversion = '1.0.0'\napi = 1\n[[editor]]\nname = 'main'\npath = './init'\n"
                .to_vec(),
        ),
        ("init.luau".into(), source.into_bytes()),
    ]);
    sources.extend(
        files
            .iter()
            .map(|(path, source)| (path.to_string(), source.as_bytes().to_vec())),
    );
    let package = Package::load(sources).unwrap();
    let profile = Profile::resolve(
        &[package],
        &PackageRef {
            name: "test.presentation".into(),
            version: "1.0.0".parse().unwrap(),
        },
    )
    .unwrap();
    Document::open(profile, &[]).unwrap()
}

#[derive(Clone, Copy, Debug)]
enum Gesture {
    Idle,
    Hover,
    Press,
    Click,
}

#[test]
fn rendered_views_own_their_document_data_and_resources() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<tango_script_iced::Renderer>();
    let expression = r#"{kind = "button", id = "choose", content_padding = {6, 14, 6, 14},
        appearance = {border_width = 1, border_color = "primary" :: Color},
        child = {kind = "row" :: "row", spacing = 6, children = nodes({
            {kind = "text", text = "Owned view", underline = true},
            {kind = "canvas", canvas_width = 16, canvas_height = 16, commands = {
                {kind = "image", width = 16, height = 16, image = {width = 1, height = 1, rgba = buffer.fromstring("\255\0\0\255")}},
            }},
        })}}"#;
    let live_document = document(expression);
    let live_renderer = tango_script_iced::Renderer::default();
    for theme in [Theme::Dark, Theme::Light] {
        let owned = {
            let document = document(expression);
            let renderer = tango_script_iced::Renderer::default();
            // Both the source tree and renderer are gone before Iced lays out,
            // draws and clicks the element, as with a released session guard.
            renderer.view(document.view())
        };
        let actual = capture(owned, &theme, Gesture::Click);
        let expected = capture(live_renderer.view(live_document.view()), &theme, Gesture::Click);
        assert_eq!(actual, expected);
        assert_eq!(actual.1, [Some(Action::activate("choose", ""))]);
    }
}

#[test]
fn animated_entrances_match_native_pixels_and_transformed_hit_targets() {
    use std::time::Duration;
    for (x, y) in [(24, 0), (-24, 0), (0, 20)] {
        let doc = document(&format!(
            r#"{{kind = "button", id = "choose", text = "Moving content",
            content_padding = {{6, 14, 6, 14}},
            motion = {{key = "arrival", revision = "0", from = {{{x}, {y}}}, delay_ms = 40}}}}"#
        ));
        let script = tango_script_iced::Renderer::default();
        let now = iced::time::Instant::now();
        let mut native = tango_ui::anim::Enter::default();
        native.start_delayed(now, Duration::from_millis(40));
        for ms in [0, 40, 80, 160, 199, 240] {
            let at = now + Duration::from_millis(ms);
            for theme in [Theme::Dark, Theme::Light] {
                for gesture in [Gesture::Idle, Gesture::Click] {
                    let button = button(text("Moving content"))
                        .padding([6, 14])
                        .style(widgets::neutral)
                        .on_press(Some(Action::activate("choose", "")));
                    let expected = capture(
                        tango_ui::anim::slide_in_opt(
                            button,
                            native.progress(at),
                            iced::Vector::new(x as f32, y as f32),
                        ),
                        &theme,
                        gesture,
                    );
                    let actual = capture(script.view_at(doc.view(), at), &theme, gesture);
                    let different = actual
                        .0
                        .chunks_exact(4)
                        .zip(expected.0.chunks_exact(4))
                        .filter(|(a, b)| a != b)
                        .count();
                    assert_eq!(different, 0, "{x}/{y}/{ms}/{theme:?}/{gesture:?}");
                    assert_eq!(actual.1, expected.1);
                    assert_eq!(actual.2, expected.2);
                    if ms == 240 && matches!(gesture, Gesture::Click) {
                        assert_eq!(actual.1, [Some(Action::activate("choose", ""))]);
                    }
                }
            }
        }
    }
}

#[test]
fn gradient_backgrounds_match_native_theme_colors_and_transparency() {
    for angle in [0.0, std::f32::consts::FRAC_PI_2, 3.0 * std::f32::consts::FRAC_PI_2] {
        let doc = document(&format!(
            r#"{{kind = "space", width = 220, height = 44,
            appearance = {{background = {{angle = {angle}, stops = {{
                {{offset = 0, color = "plate" :: Color}},
                {{offset = 0.4, color = {{0.2, 0.6, 0.8, 0.5}} :: Color}},
                {{offset = 1, color = {{theme = "plate", alpha = 0}} :: Color}},
            }} }} }} }}"#
        ));
        compare(
            &format!("gradient-{angle}"),
            &doc,
            move || {
                container(Space::new().width(220).height(44))
                    .style(move |theme: &Theme| {
                        let plate = widgets::plate_color(theme);
                        container::Style::default().background(iced::Background::Gradient(iced::Gradient::Linear(
                            iced::gradient::Linear::new(angle)
                                .add_stop(0.0, plate)
                                .add_stop(
                                    0.4,
                                    Color {
                                        r: 0.2,
                                        g: 0.6,
                                        b: 0.8,
                                        a: 0.5,
                                    },
                                )
                                .add_stop(1.0, Color { a: 0.0, ..plate }),
                        )))
                    })
                    .into()
            },
            Gesture::Idle,
        );
    }
}

#[test]
fn scripted_tabs_match_native_buttons_badges_and_error_tooltips() {
    let files = [("tabs.luau", include_str!("../../packages/editor-common/tabs.luau"))];
    for (language, label) in [("en", "Storage"), ("jp", "保存データ")] {
        for active in [false, true] {
            for invalid in [false, true] {
                let errors = if invalid {
                    r#"{"A stored value exceeds its limit.", "Choose a value between zero and thirty."}"#
                } else {
                    "{}"
                };
                let doc = document_with_files(
                    &format!(
                        r#"require("./tabs").button({{id = "choose", label = {label:?}, icon = "\u{{e0cf}}", errors = {errors}}}, {active})"#
                    ),
                    &files,
                );
                for gesture in [Gesture::Idle, Gesture::Hover, Gesture::Press, Gesture::Click] {
                    compare(
                        &format!("tab-{language}-{active}-{invalid}-{gesture:?}"),
                        &doc,
                        move || {
                            let button: Element<'static, Message> = button(
                                row![lucide_icons::Icon::Files.widget().size(style::TEXT_BODY), text(label)]
                                    .spacing(8)
                                    .align_y(iced::Alignment::Center),
                            )
                            .padding([6, 14])
                            .style(widgets::pill_tab_style(active))
                            .on_press(Some(Action::activate("choose", "")))
                            .into();
                            if !invalid {
                                return button;
                            }
                            let button = widgets::pill_tab_badge(button, |theme| theme.palette().danger);
                            let errors = iced::widget::Column::with_children(
                                [
                                    "A stored value exceeds its limit.",
                                    "Choose a value between zero and thirty.",
                                ]
                                .into_iter()
                                .map(|message| {
                                    row![
                                        text("•").style(|theme: &Theme| text::Style {
                                            color: Some(theme.palette().danger)
                                        }),
                                        text(message).size(style::TEXT_CAPTION).width(360)
                                    ]
                                    .spacing(6)
                                    .align_y(iced::Alignment::Start)
                                    .into()
                                }),
                            )
                            .spacing(5);
                            tooltip(
                                button,
                                container(errors).padding(8).style(widgets::tooltip_chrome),
                                tooltip::Position::FollowCursor,
                            )
                            .gap(8)
                            .into()
                        },
                        gesture,
                    );
                }
            }
        }
    }
}

#[test]
fn list_drag_events_use_lua_array_positions_in_both_directions() {
    let doc = document(
        r#"{kind = "list", id = "items", reorder = true, width = 160,
        children = nodes({{kind = "space", width = 160, height = 40},
            {kind = "space", width = 160, height = 40}, {kind = "space", width = 160, height = 40}})}"#,
    );
    let script = tango_script_iced::Renderer::default();
    for (from, to) in [(1, 3), (3, 1)] {
        let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
            iced::Font::DEFAULT,
            style::TEXT_BODY.into(),
            Some("tiny-skia"),
        ))
        .unwrap();
        let mut ui = iced_runtime::UserInterface::build(
            script.view(doc.view()),
            iced::Size::new(160.0, 120.0),
            Default::default(),
            &mut renderer,
        );
        let start = Point::new(20.0, (from as f32 - 0.5) * 40.0);
        let end = Point::new(20.0, (to as f32 - 0.5) * 40.0);
        let mut messages = vec![];
        for (event, position) in [
            (mouse::Event::CursorMoved { position: start }, start),
            (mouse::Event::ButtonPressed(mouse::Button::Left), start),
            (mouse::Event::CursorMoved { position: end }, end),
            (mouse::Event::ButtonReleased(mouse::Button::Left), end),
        ] {
            ui.update(
                &[iced::Event::Mouse(event)],
                mouse::Cursor::Available(position),
                &mut renderer,
                &mut iced::advanced::clipboard::Null,
                &mut messages,
            );
        }
        let actions: Vec<_> = messages.into_iter().flatten().collect();
        assert_eq!(
            actions,
            [Action {
                id: "items".into(),
                event: tango_script::Event::Reorder { from, to }
            }]
        );
        assert!(doc.view().accepts(&actions[0]));
    }
}

#[test]
fn scroll_commands_are_scoped_to_the_renderer_and_emit_finite_scroll_events() {
    use iced::futures::StreamExt;
    let doc = document(
        r#"{kind = "scroll", id = "list", on_scroll = true, width = 180, height = 40,
        child = {kind = "column" :: "column", children = nodes({
            {kind = "space", width = 180, height = 40, appearance = {background = {1, 0, 0, 1}}},
            {kind = "space", width = 180, height = 40, appearance = {background = {0, 1, 0, 1}}},
            {kind = "space", width = 180, height = 40, appearance = {background = {0, 0, 1, 1}}},
        })}}"#,
    );
    let script = tango_script_iced::Renderer::default();
    let other = tango_script_iced::Renderer::default();
    let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
        iced::Font::DEFAULT,
        style::TEXT_BODY.into(),
        Some("tiny-skia"),
    ))
    .unwrap();
    let mut ui = iced_runtime::UserInterface::build(
        container(script.view(doc.view())).padding(16),
        iced::Size::new(220.0, 90.0),
        Default::default(),
        &mut renderer,
    );
    let mut apply = |task: iced::Task<Message>| {
        let mut stream = iced_runtime::task::into_stream(task).unwrap();
        let action = iced::futures::executor::block_on(stream.next()).unwrap();
        let iced_runtime::Action::Widget(mut operation) = action else {
            panic!("expected widget operation")
        };
        ui.operate(&renderer, operation.as_mut());
        ui.draw(
            &mut renderer,
            &Theme::Dark,
            &iced::advanced::renderer::Style {
                text_color: Color::WHITE,
            },
            mouse::Cursor::Unavailable,
        );
        renderer.screenshot(iced::Size::new(220, 90), 1.0, Color::BLACK)
    };
    let start = apply(script.scroll_to("list", 0.0, 0.0));
    let end = apply(script.scroll_to("list", 0.0, 1.0));
    assert_ne!(start, end, "the target must actually scroll");
    assert_eq!(
        end,
        apply(other.scroll_to("list", 0.0, 0.0)),
        "another document cannot address this scrollable"
    );
    assert_eq!(
        start,
        apply(script.scroll_to("list", 0.0, 0.0)),
        "scroll reset must restore the original pixels"
    );
    drop(apply);
    let cursor = mouse::Cursor::Available(Point::new(25.0, 25.0));
    let mut messages = vec![];
    ui.update(
        &[iced::Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -24.0 },
        })],
        cursor,
        &mut renderer,
        &mut iced::advanced::clipboard::Null,
        &mut messages,
    );
    let action = messages.iter().flatten().next().expect("scroll event");
    let tango_script::Event::Scroll { x, y } = action.event else {
        panic!("expected scroll event")
    };
    assert_eq!(x, 0.0, "a fitting axis must not emit NaN");
    assert!(y > 0.0 && y <= 1.0);
    assert!(doc.view().accepts(action));
}

#[test]
fn assembled_tab_strips_match_native_layout_and_pass_clicks_through_edge_fades() {
    use iced::widget::{scrollable, stack};
    let files = [("tabs.luau", include_str!("../../packages/editor-common/tabs.luau"))];
    for width in [250, 388] {
        for offset in [0.0, 0.5, 1.0] {
            for extras in [false, true] {
                let tail = if extras {
                    r#"nodes({{kind = "checkbox", id = "group", text = "Group", checked = true, size = 13, text_size = 12}})"#
                } else {
                    "{}"
                };
                let doc = document_with_files(
                    &format!(
                        r#"{{kind = "container", width = {width}, child = require("./tabs").view({{
                    {{id = "choose", label = "First section", icon = "\u{{e0cf}}"}},
                    {{id = "second", label = "Second section", icon = "\u{{e0aa}}"}},
                }}, "choose", {{kind = "row", spacing = 6, align_y = CENTER, children = {tail}}}, "tabs", {offset})}}"#
                    ),
                    &files,
                );
                for gesture in [Gesture::Idle, Gesture::Hover, Gesture::Click] {
                    compare(
                        &format!("tab-strip-{width}-{offset}-{extras}-{gesture:?}"),
                        &doc,
                        move || {
                            let tabs = sweeten::widget::row![
                                widgets::pill_tab(
                                    lucide_icons::Icon::Files,
                                    Some("First section".into()),
                                    Some(Action::activate("choose", "")),
                                    true,
                                    false
                                ),
                                widgets::pill_tab(
                                    lucide_icons::Icon::CreditCard,
                                    Some("Second section".into()),
                                    Some(Action::activate("second", "")),
                                    false,
                                    false
                                ),
                            ]
                            .spacing(2)
                            .align_y(iced::Alignment::Center);
                            let scroll = scrollable(tabs)
                                .width(Fill)
                                .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::default()))
                                .on_scroll(|viewport| {
                                    let x = viewport.relative_offset().x;
                                    Some(Action {
                                        id: "tabs".into(),
                                        event: tango_script::Event::Scroll {
                                            x: if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 },
                                            y: 0.0,
                                        },
                                    })
                                })
                                .style(|theme, status| {
                                    let mut style = widgets::chunky_scrollable(theme, status);
                                    if matches!(status, scrollable::Status::Active { .. }) {
                                        for rail in [&mut style.horizontal_rail, &mut style.vertical_rail] {
                                            rail.background = None;
                                            rail.scroller.background = Color::TRANSPARENT.into();
                                        }
                                    }
                                    style
                                });
                            let fade = |angle: f32, visible: bool| -> Element<'static, Message> {
                                if !visible {
                                    return Space::new().into();
                                }
                                container(Space::new())
                                    .width(24)
                                    .height(Fill)
                                    .style(move |theme: &Theme| {
                                        let plate = widgets::plate_color(theme);
                                        container::Style::default().background(iced::Background::Gradient(
                                            iced::Gradient::Linear(
                                                iced::gradient::Linear::new(angle)
                                                    .add_stop(0.0, plate)
                                                    .add_stop(1.0, Color { a: 0.0, ..plate }),
                                            ),
                                        ))
                                    })
                                    .into()
                            };
                            let fades = sweeten::widget::row![
                                fade(std::f32::consts::FRAC_PI_2, offset > 0.01),
                                Space::new().width(Fill),
                                fade(3.0 * std::f32::consts::FRAC_PI_2, offset < 0.99)
                            ]
                            .height(Fill);
                            let mut tail = sweeten::widget::Row::new().spacing(6).align_y(iced::Alignment::Center);
                            if extras {
                                tail = tail.push(
                                    iced::widget::checkbox(true)
                                        .label("Group")
                                        .size(13)
                                        .text_size(12)
                                        .style(widgets::chunky_checkbox)
                                        .on_toggle(|checked| {
                                            Some(Action {
                                                id: "group".into(),
                                                event: tango_script::Event::Toggle { checked },
                                            })
                                        }),
                                );
                            }
                            let row = sweeten::widget::row![
                                container(stack![scroll, fades]).width(Fill),
                                container(tail)
                                    .height(style::TEXT_BODY * 1.3 + 12.0)
                                    .align_y(iced::Alignment::Center)
                            ]
                            .spacing(8)
                            .align_y(iced::Alignment::Start);
                            container(container(row.padding([4, 8])).width(Fill).style(widgets::pane))
                                .width(width as f32)
                                .into()
                        },
                        gesture,
                    );
                }
            }
        }
    }
}

#[test]
fn rasterized_canvases_match_images_when_scaled_and_masked() {
    for nearest in [false, true] {
        for radius in [0, 2] {
            let mut pixels = Vec::new();
            for y in 0..4 {
                for x in 0..8 {
                    let mut pixel = if x < 4 { [255, 0, 0, 128] } else { [0, 255, 0, 255] };
                    if radius == 2 && (x == 0 || x == 7) && (y == 0 || y == 3) {
                        pixel[3] = 0;
                    }
                    pixels.extend_from_slice(&pixel);
                }
            }
            let handle = image_widget::Handle::from_rgba(8, 4, pixels);
            for (width, height) in [(31, 37), (80, 12)] {
                let doc = document(&format!(
                    r#"{{kind = "canvas", canvas_width = 8, canvas_height = 4, width = {width}, height = {height},
                    rasterize = {{nearest = {nearest}, corner_radius = {radius}}}, commands = {{
                        {{kind = "rect" :: "rect", width = 4, height = 4, fill = {{1, 0, 0, 0.5}} :: Color}} :: Draw,
                        {{kind = "rect" :: "rect", x = 4, width = 4, height = 4, fill = {{0, 1, 0, 1}} :: Color}} :: Draw,
                    }} }}"#
                ));
                compare(
                    &format!("raster-{nearest}-{radius}-{width}-{height}"),
                    &doc,
                    || {
                        image_widget(handle.clone())
                            .width(width as f32)
                            .height(height as f32)
                            .filter_method(if nearest {
                                image_widget::FilterMethod::Nearest
                            } else {
                                image_widget::FilterMethod::Linear
                            })
                            .into()
                    },
                    Gesture::Idle,
                );
            }
        }
    }
}

fn capture(
    element: Element<'_, Message>,
    theme: &Theme,
    gesture: Gesture,
) -> (Vec<u8>, Vec<Message>, mouse::Interaction) {
    static FONTS: Once = Once::new();
    FONTS.call_once(|| {
        iced::advanced::graphics::text::font_system()
            .write()
            .unwrap()
            .load_font(
                include_bytes!("../../tango/fonts/NotoSans-Regular.ttf")
                    .as_slice()
                    .into(),
            );
        iced::advanced::graphics::text::font_system()
            .write()
            .unwrap()
            .load_font(lucide_icons::LUCIDE_FONT_BYTES.into());
        iced::advanced::graphics::text::font_system()
            .write()
            .unwrap()
            .load_font(
                include_bytes!("../../tango/fonts/NotoSansJP-Regular.otf")
                    .as_slice()
                    .into(),
            );
    });
    let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
        iced::Font::with_name("Noto Sans"),
        style::TEXT_BODY.into(),
        Some("tiny-skia"),
    ))
    .expect("software renderer");
    let mut ui = iced_runtime::UserInterface::build(
        container(element).padding(16).width(Fill),
        iced::Size::new(420.0, 260.0),
        iced_runtime::user_interface::Cache::default(),
        &mut renderer,
    );
    let position = Point::new(24.0, 24.0);
    let cursor = if matches!(gesture, Gesture::Idle) {
        mouse::Cursor::Unavailable
    } else {
        mouse::Cursor::Available(position)
    };
    let mut events = vec![iced::Event::Mouse(mouse::Event::CursorMoved { position })];
    if matches!(gesture, Gesture::Press | Gesture::Click) {
        events.push(iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)));
    }
    if matches!(gesture, Gesture::Click) {
        events.push(iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)));
    }
    // Native buttons resolve their status on redraw; tooltips also update their
    // overlays at that point. No timer/sleep or window server is involved.
    events.push(iced::Event::Window(iced::window::Event::RedrawRequested(
        iced::time::Instant::now(),
    )));
    let mut messages = vec![];
    let (state, _) = ui.update(
        &events,
        cursor,
        &mut renderer,
        &mut iced::advanced::clipboard::Null,
        &mut messages,
    );
    let interaction = match state {
        iced_runtime::user_interface::State::Updated { mouse_interaction, .. } => mouse_interaction,
        _ => panic!("unchanged widget tree"),
    };
    ui.draw(
        &mut renderer,
        theme,
        &iced::advanced::renderer::Style {
            text_color: theme.palette().text,
        },
        cursor,
    );
    let pixels = renderer.screenshot(iced::Size::new(420, 260), 1.0, theme.palette().background);
    (pixels, messages, interaction)
}

#[test]
fn translated_canvas_clips_geometry_once_including_cached_geometry() {
    use iced::widget::canvas::{self, Frame, Geometry, Path};
    struct Cached(canvas::Cache);
    impl canvas::Program<Message> for Cached {
        type State = ();
        fn draw(
            &self,
            _: &(),
            renderer: &iced::Renderer,
            _: &Theme,
            bounds: iced::Rectangle,
            _: mouse::Cursor,
        ) -> Vec<Geometry> {
            vec![self.0.draw(renderer, bounds.size(), |frame: &mut Frame| {
                frame.fill(
                    &Path::rectangle(Point::new(-10.0, -10.0), iced::Size::new(100.0, 60.0)),
                    Color::from_rgb(1.0, 0.0, 0.0),
                );
            })]
        }
    }
    let doc = document(
        r#"{kind = "container", padding = {24, 0, 0, 40}, child = {
        kind = "canvas", width = 80, height = 40, canvas_width = 80, canvas_height = 40,
        commands = {{kind = "rect", x = -10, y = -10, width = 100, height = 60, fill = {1, 0, 0, 1}} :: Draw}
    } :: Node}"#,
    );
    let script = tango_script_iced::Renderer::default();
    for theme in [Theme::Dark, Theme::Light] {
        let cached: Element<'_, Message> =
            container(canvas::Canvas::new(Cached(canvas::Cache::new())).width(80).height(40))
                .padding(iced::Padding {
                    top: 24.0,
                    left: 40.0,
                    ..Default::default()
                })
                .into();
        for (name, element) in [("uncached", script.view(doc.view())), ("cached", cached)] {
            let pixels = capture(element, &theme, Gesture::Idle).0;
            for (index, pixel) in pixels.chunks_exact(4).enumerate() {
                let (x, y) = (index % 420, index / 420);
                let expected = (56..136).contains(&x) && (40..80).contains(&y);
                assert_eq!(pixel == [255, 0, 0, 255], expected, "{name}: pixel {x},{y}");
            }
        }
    }
}

#[test]
fn copy_buttons_match_native_before_after_and_after_feedback_expires() {
    let files = [(
        "clipboard.luau",
        include_str!("../../packages/editor-common/clipboard.luau"),
    )];
    let mut comparisons = 0;
    for (icon, glyph) in [
        (lucide_icons::Icon::ClipboardCopy, "\\u{e225}"),
        (lucide_icons::Icon::ImageDown, "\\u{e53c}"),
    ] {
        let doc = document_with_files(
            &format!(r#"require("./clipboard").button("copy", "{glyph}", "Copy", "Copied!")"#),
            &files,
        );
        let script = tango_script_iced::Renderer::default();
        for theme in [Theme::Dark, Theme::Light] {
            for gesture in [Gesture::Idle, Gesture::Hover, Gesture::Press, Gesture::Click] {
                for copied in [false, true] {
                    if copied {
                        tango_ui::copy_feedback::flash("reference-copy");
                    } else {
                        tango_ui::copy_feedback::flash("reset-copy");
                    }
                    let native = widgets::copy_icon_button(
                        "reference-copy",
                        icon,
                        style::TEXT_BODY,
                        "Copy".into(),
                        "Copied!".into(),
                        Some(Some(Action::activate("copy", ""))),
                        [4.0, 10.0],
                    );
                    let expected = capture(native, &theme, gesture);
                    if copied {
                        script.acknowledge("copy");
                    } else {
                        tango_ui::copy_feedback::flash("reset-copy");
                    }
                    let actual = capture(script.view(doc.view()), &theme, gesture);
                    assert_eq!(actual.1, expected.1, "copy/{copied}/{gesture:?} messages");
                    assert_eq!(actual.2, expected.2, "copy/{copied}/{gesture:?} cursor");
                    let different = actual
                        .0
                        .chunks_exact(4)
                        .zip(expected.0.chunks_exact(4))
                        .filter(|(a, b)| a != b)
                        .count();
                    assert_eq!(different, 0, "copy/{copied}/{gesture:?}/{theme:?} pixels");
                    if matches!(gesture, Gesture::Click) {
                        assert_eq!(actual.1, vec![Some(Action::activate("copy", ""))]);
                    }
                    comparisons += 1;
                }
            }
        }
    }
    assert_eq!(comparisons, 32);
    let doc = document_with_files(
        r#"require("./clipboard").button("copy", "\u{e225}", "Copy", "Copied!")"#,
        &files,
    );
    let script = tango_script_iced::Renderer::default();
    let other = tango_script_iced::Renderer::default();
    let idle = capture(script.view(doc.view()), &Theme::Dark, Gesture::Hover);
    script.acknowledge("copy");
    let copied = capture(script.view(doc.view()), &Theme::Dark, Gesture::Hover);
    assert_ne!(copied.0, idle.0);
    assert_eq!(
        capture(other.view(doc.view()), &Theme::Dark, Gesture::Hover),
        idle,
        "feedback belongs to its renderer"
    );
    // This exercises the same wall-clock expiry used by the host's frames subscription.
    std::thread::sleep(std::time::Duration::from_millis(1600));
    assert_eq!(capture(script.view(doc.view()), &Theme::Dark, Gesture::Hover), idle);

    let passive = document(
        r#"{kind = "button", id = "copy", feedback = {
        child = {kind = "button" :: "button", id = "hidden", width = 60, height = 30, text = "Done"} :: Node}}
    "#,
    );
    assert!(!passive.view().accepts(&Action::activate("hidden", "")));
    script.acknowledge("copy");
    assert_eq!(
        capture(script.view(passive.view()), &Theme::Dark, Gesture::Click).1,
        vec![Some(Action::activate("copy", ""))]
    );
}

#[test]
fn host_action_buttons_match_native_enabled_and_disabled_play_controls() {
    let files = [(
        "session.luau",
        include_str!("../../packages/editor-common/session.luau"),
    )];
    for label in ["Play", "プレイ"] {
        for enabled in [false, true] {
            let doc = document_with_files(
                &format!(r#"require("./session").host_action("play", "\u{{e13c}}", "{label}", {enabled})"#),
                &files,
            );
            let script = tango_script_iced::Renderer::default();
            for theme in [Theme::Dark, Theme::Light] {
                for gesture in [Gesture::Idle, Gesture::Hover, Gesture::Press, Gesture::Click] {
                    let native = button(
                        sweeten::widget::row![lucide_icons::Icon::Play.widget(), text(label)]
                            .spacing(6)
                            .align_y(iced::Alignment::Center),
                    )
                    .padding([4, 10])
                    .style(if enabled {
                        widgets::primary_button
                    } else {
                        widgets::neutral
                    })
                    .on_press_maybe(enabled.then(|| Some(Action::activate("editor.host:play", ""))));
                    let expected = capture(native.into(), &theme, gesture);
                    let actual = capture(script.view(doc.view()), &theme, gesture);
                    let different = actual
                        .0
                        .chunks_exact(4)
                        .zip(expected.0.chunks_exact(4))
                        .filter(|(a, b)| a != b)
                        .count();
                    assert_eq!(different, 0, "{label}/{enabled}/{theme:?}/{gesture:?}");
                    assert_eq!(actual.1, expected.1);
                    assert_eq!(actual.2, expected.2);
                    if matches!(gesture, Gesture::Click) {
                        assert_eq!(
                            actual.1,
                            if enabled {
                                vec![Some(Action::activate("editor.host:play", ""))]
                            } else {
                                vec![]
                            }
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn scripted_session_controls_match_native_edit_save_and_cancel() {
    let files = [(
        "session.luau",
        include_str!("../../packages/editor-common/session.luau"),
    )];
    for [edit, cancel, save] in [["Edit", "Cancel", "Save"], ["編集", "キャンセル", "保存"]] {
        for (editable, editing, saving, can_save) in [
            (true, false, false, false),
            (false, false, false, false),
            (true, true, false, true),
            (true, true, false, false),
            (true, true, true, false),
        ] {
            let doc = document_with_files(
                &format!(
                    r#"require("./session").controls({{
                editable = {editable}, editing = {editing}, saving = {saving}, can_save = {can_save}
            }}, {{edit = "{edit}", cancel = "{cancel}", save = "{save}"}})"#
                ),
                &files,
            );
            let renderer = tango_script_iced::Renderer::default();
            for theme in [Theme::Dark, Theme::Light] {
                for gesture in [Gesture::Idle, Gesture::Hover, Gesture::Press, Gesture::Click] {
                    let mut controls = sweeten::widget::Row::new().spacing(6).align_y(iced::Alignment::Center);
                    if editing {
                        controls = controls.push(widgets::labeled_icon_button_maybe(
                            lucide_icons::Icon::X,
                            cancel.into(),
                            (!saving).then(|| Some(Action::activate("editor.session:cancel", ""))),
                            [4.0, 10.0],
                            widgets::neutral,
                        ));
                        controls = controls.push(widgets::labeled_icon_button_maybe(
                            lucide_icons::Icon::Check,
                            save.into(),
                            can_save.then(|| Some(Action::activate("editor.session:save", ""))),
                            [4.0, 10.0],
                            widgets::primary_button,
                        ));
                    } else if editable {
                        controls = controls.push(widgets::labeled_icon_button(
                            lucide_icons::Icon::Pencil,
                            edit.into(),
                            Some(Action::activate("editor.session:begin", "")),
                            [4.0, 10.0],
                            widgets::neutral,
                        ));
                    }
                    let expected = capture(controls.into(), &theme, gesture);
                    let actual = capture(renderer.view(doc.view()), &theme, gesture);
                    let different = actual
                        .0
                        .chunks_exact(4)
                        .zip(expected.0.chunks_exact(4))
                        .filter(|(a, b)| a != b)
                        .count();
                    assert_eq!(
                        different, 0,
                        "{edit}/{editable}/{editing}/{saving}/{can_save}/{theme:?}/{gesture:?}: pixels"
                    );
                    assert_eq!(actual.1, expected.1);
                    assert_eq!(actual.2, expected.2);
                    if matches!(gesture, Gesture::Click) {
                        assert_eq!(
                            actual.1,
                            if saving || !editable {
                                vec![]
                            } else {
                                vec![Some(Action::activate(
                                    if editing {
                                        "editor.session:cancel"
                                    } else {
                                        "editor.session:begin"
                                    },
                                    "",
                                ))]
                            }
                        );
                    }
                }
            }
        }
    }
}

fn compare(name: &str, document: &Document, native: impl Fn() -> Element<'static, Message>, gesture: Gesture) {
    let script = tango_script_iced::Renderer::default();
    for (theme_name, theme) in [("dark", Theme::Dark), ("light", Theme::Light)] {
        let (expected, native_messages, native_cursor) = capture(native(), &theme, gesture);
        let (actual, script_messages, script_cursor) = capture(script.view(document.view()), &theme, gesture);
        assert_eq!(script_cursor, native_cursor, "{name}/{theme_name}/{gesture:?} cursor");
        if matches!(gesture, Gesture::Click) {
            let expected_messages = if document.view().enabled {
                vec![Some(Action::activate("choose", ""))]
            } else {
                vec![]
            };
            let presses: Vec<_> = native_messages
                .iter()
                .filter(|message| {
                    !matches!(
                        message,
                        Some(Action {
                            event: tango_script::Event::Scroll { .. },
                            ..
                        })
                    )
                })
                .cloned()
                .collect();
            assert_eq!(presses, expected_messages, "reference must exercise the click");
        }
        if let Ok(directory) = std::env::var("TANGO_UI_CAPTURE_DIR") {
            std::fs::create_dir_all(&directory).unwrap();
            for (label, pixels) in [("native", &expected), ("script", &actual)] {
                image::save_buffer(
                    format!("{directory}/{name}-{theme_name}-{gesture:?}-{label}.png"),
                    pixels,
                    420,
                    260,
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
        }
        assert_eq!(
            script_messages, native_messages,
            "{name}/{theme_name}/{gesture:?} messages"
        );
        assert!(
            expected.chunks_exact(4).any(|pixel| pixel != &expected[..4]),
            "blank reference"
        );
        let different = actual
            .chunks_exact(4)
            .zip(expected.chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(different, 0, "{name}/{theme_name}/{gesture:?}: differing pixels");
    }
}

#[test]
fn checkbox_metrics_and_toggles_match_native() {
    for checked in [true, false] {
        let doc = document(&format!(
            r#"{{kind = "checkbox", id = "group", text = "Group entries", checked = {checked}, size = 13, text_size = 12}}"#,
        ));
        let script = tango_script_iced::Renderer::default();
        for theme in [Theme::Dark, Theme::Light] {
            for gesture in [Gesture::Idle, Gesture::Hover, Gesture::Click] {
                let native = iced::widget::checkbox(checked)
                    .label("Group entries")
                    .size(13)
                    .text_size(12)
                    .style(widgets::chunky_checkbox)
                    .on_toggle(|checked| {
                        Some(Action {
                            id: "group".into(),
                            event: tango_script::Event::Toggle { checked },
                        })
                    });
                let expected = capture(native.into(), &theme, gesture);
                let actual = capture(script.view(doc.view()), &theme, gesture);
                assert!(
                    actual.0 == expected.0,
                    "checkbox pixels: {checked}/{theme:?}/{gesture:?}"
                );
                assert_eq!(actual.1, expected.1);
                assert_eq!(actual.2, expected.2);
                if matches!(gesture, Gesture::Click) {
                    assert_eq!(
                        actual.1,
                        vec![Some(Action {
                            id: "group".into(),
                            event: tango_script::Event::Toggle { checked: !checked }
                        })]
                    );
                }
            }
        }
    }
}

#[test]
fn narrow_rows_preserve_overflowing_centered_labels() {
    let cell = r#"{kind = "button", id = "choose", content_padding = 4, child = {
        kind = "column" :: "column", width = 88, spacing = 6, align_x = CENTER, children = nodes({
            {kind = "space", width = 72, height = 72},
            {kind = "text", text = "Long caption", size = CAPTION},
        })}}"#;
    let children = (0..7)
        .map(|id| cell.replace("choose", &format!("choose-{id}")))
        .collect::<Vec<_>>()
        .join(",");
    let doc = document(&format!(
        r#"{{kind = "row", spacing = 14, children = nodes({{{children}}})}}"#,
    ));
    compare(
        "narrow-labels",
        &doc,
        || {
            sweeten::widget::Row::with_children((0..7).map(|_| {
                button(
                    sweeten::widget::column![
                        Space::new().width(72).height(72),
                        text("Long caption").size(style::TEXT_CAPTION),
                    ]
                    .width(88)
                    .spacing(6)
                    .align_x(iced::Alignment::Center),
                )
                .padding(4)
                .style(widgets::neutral)
                .on_press(Some(Action::activate("choose", "")))
                .into()
            }))
            .spacing(14)
            .into()
        },
        Gesture::Idle,
    );
}

#[test]
fn overflowing_text_respects_explicit_clipping() {
    for clip in [false, true] {
        let doc = document(&format!(
            r#"{{kind = "container", width = 200, clip = {clip}, child = {{kind = "row" :: "row", children = nodes({{
                {{kind = "space", width = 190}},
                {{kind = "text", text = "Overflow", wrapping = "none" :: Wrapping}},
                {{kind = "text", text = "More", wrapping = "none" :: Wrapping}},
            }})}}}}"#,
        ));
        compare(
            "overflowing-text",
            &doc,
            move || {
                container(sweeten::widget::row![
                    Space::new().width(190),
                    text("Overflow").size(style::TEXT_BODY).wrapping(text::Wrapping::None),
                    text("More").size(style::TEXT_BODY).wrapping(text::Wrapping::None),
                ])
                .width(200)
                .clip(clip)
                .into()
            },
            Gesture::Idle,
        );
    }
}

#[test]
fn narrow_fill_text_matches_native_at_zero_and_positive_widths() {
    for label in ["Overflow", "キャノン"] {
        for decorated in [false, true] {
            let mut nonempty = 0;
            for width in [80, 96, 97, 100, 120] {
                let doc = document(&format!(
                    r#"{{kind = "row", width = {width}, spacing = 8, children = nodes({{
                {{kind = "space", width = 40, height = 40}},
                {{kind = "text", text = "{label}", width = FILL, underline = {decorated}}},
                {{kind = "space", width = 40, height = 40}},
            }})}}"#
                ));
                let script = tango_script_iced::Renderer::default();
                for theme in [Theme::Dark, Theme::Light] {
                    let label_widget: Element<'_, Message> = if decorated {
                        iced::widget::rich_text([iced::widget::text::Span::new(label).underline(true)])
                            .on_link_click(iced::never)
                            .size(style::TEXT_BODY)
                            .width(Fill)
                            .into()
                    } else {
                        text(label).size(style::TEXT_BODY).width(Fill).into()
                    };
                    let native = sweeten::widget::row![
                        Space::new().width(40).height(40),
                        label_widget,
                        Space::new().width(40).height(40)
                    ]
                    .spacing(8)
                    .width(width);
                    let expected = capture(native.into(), &theme, Gesture::Idle);
                    let actual = capture(script.view(doc.view()), &theme, Gesture::Idle);
                    nonempty += usize::from(expected.0.chunks_exact(4).any(|pixel| pixel != &expected.0[..4]));
                    let different = actual
                        .0
                        .chunks_exact(4)
                        .zip(expected.0.chunks_exact(4))
                        .filter(|(a, b)| a != b)
                        .count();
                    assert_eq!(different, 0, "{label}/{decorated}/{width}/{theme:?}");
                    assert_eq!(actual.1, expected.1);
                    assert_eq!(actual.2, expected.2);
                }
            }
            assert!(nonempty > 0, "exercise visible text as well as empty layout cells");
        }
    }
}

#[test]
fn wrapping_modes_match_native_plain_and_decorated_text() {
    let label = "Words with Supercalifragilisticexpialidocious endings";
    for (name, wrapping) in [
        ("none", text::Wrapping::None),
        ("word", text::Wrapping::Word),
        ("glyph", text::Wrapping::Glyph),
        ("word_or_glyph", text::Wrapping::WordOrGlyph),
    ] {
        for decorated in [false, true] {
            let doc = document(&format!(
                r#"{{kind = "container", width = 120, child = {{kind = "text" :: "text", text = "{label}",
                    wrapping = "{name}" :: Wrapping, underline = {decorated}}}}}"#,
            ));
            compare(
                "wrapping",
                &doc,
                move || {
                    let content: Element<'_, Message> = if decorated {
                        iced::widget::rich_text([iced::widget::text::Span::new(label).underline(true)])
                            .on_link_click(iced::never)
                            .size(style::TEXT_BODY)
                            .wrapping(wrapping)
                            .into()
                    } else {
                        text(label).size(style::TEXT_BODY).wrapping(wrapping).into()
                    };
                    container(content).width(120).into()
                },
                Gesture::Idle,
            );
        }
    }
}

#[test]
fn tinted_plates_and_shadows_match_native() {
    for as_button in [false, true] {
        let kind = if as_button {
            r#"kind = "button", id = "choose", content_padding = 0"#
        } else {
            r#"kind = "container""#
        };
        let doc = document(&format!(
            r#"{{{kind}, width = 72, height = 72, align_x = CENTER, align_y = CENTER,
                child = {{kind = "space" :: "space"}},
                appearance = {{background = {{theme = "background", tint = {{0.9, 0.2, 0.1, 0.7}}, mix = 0.4}} :: Color,
                    radius = 36, border_width = 2, border_color = {{0.9, 0.2, 0.1, 0.9}} :: Color,
                    shadow = {{color = {{0.9, 0.2, 0.1, 0.5}} :: Color, offset = {{3, -2}}, blur = 16}}}}}}"#,
        ));
        compare(
            "tinted-shadow",
            &doc,
            move || {
                let appearance = |theme: &Theme| {
                    let base = theme.palette().background;
                    let background = Color {
                        a: base.a * 0.6 + 0.7 * 0.4,
                        ..widgets::mix(base, Color::from_rgba(0.9, 0.2, 0.1, 0.7), 0.4)
                    };
                    container::Style {
                        background: Some(background.into()),
                        border: iced::Border {
                            color: Color::from_rgba(0.9, 0.2, 0.1, 0.9),
                            width: 2.0,
                            radius: 36.0.into(),
                        },
                        shadow: iced::Shadow {
                            color: Color::from_rgba(0.9, 0.2, 0.1, 0.5),
                            offset: iced::Vector::new(3.0, -2.0),
                            blur_radius: 16.0,
                        },
                        ..Default::default()
                    }
                };
                if as_button {
                    button(Space::new())
                        .padding(0)
                        .width(72)
                        .height(72)
                        .on_press(Some(Action::activate("choose", "")))
                        .style(move |theme, status| {
                            let plate = appearance(theme);
                            button::Style {
                                background: plate.background,
                                border: plate.border,
                                shadow: plate.shadow,
                                ..widgets::neutral(theme, status)
                            }
                        })
                        .into()
                } else {
                    container(Space::new())
                        .width(72)
                        .height(72)
                        .center_x(72)
                        .center_y(72)
                        .style(appearance)
                        .into()
                }
            },
            Gesture::Idle,
        );
    }
}

#[test]
fn text_decorations_match_native_rich_text() {
    for (strikethrough, underline) in [(true, false), (false, true), (true, true)] {
        let doc = document(&format!(
            r#"{{kind = "text", text = "Inactive entry", color = MUTED,
                strikethrough = {strikethrough}, underline = {underline}}}"#,
        ));
        compare(
            "text-decoration",
            &doc,
            move || {
                iced::widget::rich_text([iced::widget::text::Span::new("Inactive entry")
                    .strikethrough(strikethrough)
                    .underline(underline)])
                .on_link_click(iced::never)
                .size(style::TEXT_BODY)
                .style(widgets::muted_text_style)
                .into()
            },
            Gesture::Idle,
        );
    }
}

#[test]
fn nested_buttons_handle_inner_clicks_with_and_without_the_outer_action() {
    for outer_enabled in [false, true] {
        for subtree_enabled in [false, true] {
            let doc = document(&format!(
                r#"{{kind = "button", id = "outer", enabled = {subtree_enabled},
                action_enabled = {outer_enabled}, content_padding = 0,
                child = {{kind = "button" :: "button", id = "choose", text = "Rotate", content_padding = 8}}}}"#
            ));
            for gesture in [Gesture::Idle, Gesture::Hover, Gesture::Press, Gesture::Click] {
                compare(
                    "nested-buttons",
                    &doc,
                    move || {
                        let inner = button(text("Rotate").size(style::TEXT_BODY))
                            .padding(8)
                            .style(widgets::neutral)
                            .on_press_maybe(subtree_enabled.then(|| Some(Action::activate("choose", ""))));
                        button(inner)
                            .padding(0)
                            .style(widgets::neutral)
                            .on_press_maybe(
                                (subtree_enabled && outer_enabled).then(|| Some(Action::activate("outer", ""))),
                            )
                            .into()
                    },
                    gesture,
                );
            }
        }
    }
}

#[test]
fn image_opacity_matches_native() {
    for opacity in [0.35_f32, 1.0] {
        let doc = document(&format!(
            r#"{{kind = "image", width = 48, height = 48, opacity = {opacity},
            image = {{width = 1, height = 1, rgba = buffer.fromstring("rgba")}}}}"#
        ));
        compare(
            "image-opacity",
            &doc,
            move || {
                image_widget(image_widget::Handle::from_rgba(1, 1, b"rgba".to_vec()))
                    .width(48)
                    .height(48)
                    .filter_method(image_widget::FilterMethod::Nearest)
                    .opacity(opacity)
                    .into()
            },
            Gesture::Idle,
        );
    }
}

#[test]
fn cursor_hints_preserve_child_clicks_and_disabled_state() {
    for enabled in [true, false] {
        let doc = document(&format!(
            r#"{{kind = "container", enabled = {enabled}, cursor = "grab" :: Cursor,
                child = {{kind = "button" :: "button", id = "choose", text = "Choose"}}}}"#,
        ));
        for gesture in [Gesture::Hover, Gesture::Click] {
            compare(
                "cursor",
                &doc,
                move || {
                    let button = button(text("Choose").size(style::TEXT_BODY))
                        .style(widgets::neutral)
                        .on_press_maybe(enabled.then(|| Some(Action::activate("choose", ""))));
                    if enabled {
                        iced::widget::mouse_area(button)
                            .interaction(mouse::Interaction::Grab)
                            .into()
                    } else {
                        button.into()
                    }
                },
                gesture,
            );
        }
    }
}

#[test]
fn inherited_foregrounds_stripes_and_row_chrome_match_native() {
    for index in [0, 1] {
        let script_index = index + 1;
        let doc = document(&format!(
            r#"{{kind = "row", width = FILL, align_y = CENTER,
            tone = {{row = {script_index}}} :: Tone, appearance = {{text_color = DANGER}}, children = nodes({{
                {{kind = "space", width = 6, height = FILL, appearance = {{background = {{0.3, 0.5, 0.7, 1}}}}}},
                {{kind = "row", width = FILL, padding = {{3, 12, 3, 12}}, spacing = 8, align_y = CENTER, children = nodes({{
                    {{kind = "image", width = 28, height = 28, image = {{width = 1, height = 1, rgba = buffer.fromstring("rgba")}}}},
                    {{kind = "text", text = "An entry", width = FILL}},
                    {{kind = "text", text = "42", size = CAPTION}},
                }})}},
            }})}}"#
        ));
        compare(
            "row",
            &doc,
            move || {
                let icon = image_widget(image_widget::Handle::from_rgba(1, 1, b"rgba".to_vec()))
                    .width(28)
                    .height(28)
                    .filter_method(image_widget::FilterMethod::Nearest);
                let inner = row![
                    icon,
                    text("An entry").size(style::TEXT_BODY).width(Fill),
                    text("42").size(style::TEXT_CAPTION)
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .padding([3, 12]);
                container(
                    row![
                        container(Space::new())
                            .width(6)
                            .height(Fill)
                            .style(|_| container::Style::default().background(Color::from_rgb(0.3, 0.5, 0.7))),
                        container(inner).width(Fill),
                    ]
                    .height(iced::Length::Shrink)
                    .align_y(iced::Alignment::Center),
                )
                .width(Fill)
                .style(move |theme: &Theme| {
                    let mut s = widgets::zebra_row(index)(theme);
                    s.text_color = Some(theme.extended_palette().danger.base.color);
                    s
                })
                .into()
            },
            Gesture::Idle,
        );
    }
}

#[test]
fn button_tints_and_selectable_rows_match_native_in_each_state() {
    for tint in [false, true] {
        for enabled in [false, true] {
            for selected in [false, true] {
                if tint && selected {
                    continue;
                }
                let tone = if tint {
                    "{tint = {0.3, 0.5, 0.7, 1}}".into()
                } else {
                    format!("{{row = 2, selected = {selected}}}")
                };
                let doc = document(&format!(
                    r#"{{kind = "button", id = "choose", text = "Choose", size = CAPTION,
                    enabled = {enabled}, tone = {tone} :: Tone, content_padding = {{4, 8, 4, 8}}}}"#
                ));
                for gesture in [Gesture::Idle, Gesture::Hover, Gesture::Press, Gesture::Click] {
                    compare(
                        "button",
                        &doc,
                        move || {
                            button(text("Choose").size(style::TEXT_CAPTION))
                                .padding([4, 8])
                                .on_press_maybe(enabled.then(|| Some(Action::activate("choose", ""))))
                                .style(move |theme, status| {
                                    if tint {
                                        widgets::tinted_button(theme, status, Color::from_rgb(0.3, 0.5, 0.7))
                                    } else {
                                        widgets::list_item(selected, 1)(theme, status)
                                    }
                                })
                                .into()
                        },
                        gesture,
                    );
                }
            }
        }
    }
}

#[test]
fn rich_follow_cursor_tooltips_match_native() {
    let doc = document(
        r#"{kind = "button", id = "choose", text = "Choose", content_padding = {4, 8, 4, 8},
        tooltip = tip({
            kind = "column", spacing = 4, padding = 8,
            appearance = {background = {0.3, 0.5, 0.7, 1}, text_color = {1, 1, 1, 1},
                border_color = {1, 1, 1, 0.2}, border_width = 1, radius = 4},
            children = nodes({
                {kind = "image", width = 64, height = 64, image = {width = 1, height = 1, rgba = buffer.fromstring("rgba")}},
                {kind = "text", text = "Preview"},
                {kind = "text", text = "Details", size = CAPTION},
            }),
        }),
    }"#,
    );
    for gesture in [Gesture::Idle, Gesture::Hover] {
        compare(
            "tooltip",
            &doc,
            || {
                let content = button(text("Choose").size(style::TEXT_BODY))
                    .padding([4, 8])
                    .on_press(Some(Action::activate("choose", "")))
                    .style(widgets::neutral);
                let tip = container(
                    iced::widget::column![
                        image_widget(image_widget::Handle::from_rgba(1, 1, b"rgba".to_vec()))
                            .width(64)
                            .height(64)
                            .filter_method(image_widget::FilterMethod::Nearest),
                        text("Preview").size(style::TEXT_BODY),
                        text("Details").size(style::TEXT_CAPTION),
                    ]
                    .spacing(4),
                )
                .padding(8)
                .style(|_| container::Style {
                    background: Some(Color::from_rgb(0.3, 0.5, 0.7).into()),
                    text_color: Some(Color::WHITE),
                    border: iced::Border {
                        color: Color { a: 0.2, ..Color::WHITE },
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                });
                tooltip(content, tip, tooltip::Position::FollowCursor).gap(8).into()
            },
            gesture,
        );
    }
}

#[test]
fn input_and_choice_metrics_match_native() {
    let doc = document(
        r#"{kind = "row", width = FILL, spacing = 10, align_y = CENTER, children = nodes({
        {kind = "input", id = "filter", value = "Example", placeholder = "Search", width = FILL, content_padding = {5, 10, 5, 10}},
        {kind = "text", text = "Sort", size = CAPTION, color = MUTED},
        {kind = "select", id = "sort", value = "Name", choices = {{value = "Name", label = "Name"}, {value = "Size", label = "Size"}}, content_padding = {5, 10, 5, 10}},
    })}"#,
    );
    compare(
        "controls",
        &doc,
        || {
            row![
                sweeten::widget::text_input("Search", "Example")
                    .on_input(|value| Some(Action::change("filter", value)))
                    .padding([5, 10])
                    .size(style::TEXT_BODY)
                    .width(Fill)
                    .style(widgets::chunky_text_input),
                text("Sort").size(style::TEXT_CAPTION).style(widgets::muted_text_style),
                sweeten::widget::pick_list(["Name", "Size"], Some("Name"), |value| Some(Action::change(
                    "sort", value
                )))
                .padding([5, 10])
                .text_size(style::TEXT_BODY)
                .style(widgets::chunky_pick_list),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center)
            .width(Fill)
            .into()
        },
        Gesture::Idle,
    );
}

use super::*;
use crate::library::{package, Scanners};
use iced::advanced::renderer::Headless;
use iced::advanced::{layout, widget::Tree, Layout};

#[test]
fn package_mutations_remain_serialized_through_dialog_and_worker_completion() {
    let mut state = State::default();
    let reference = PackageRef {
        name: "example".into(),
        version: "1.0.0".parse().unwrap(),
    };
    assert!(matches!(state.update(Message::Install), Some(Effect::ChooseFiles)));
    assert!(state.update(Message::Uninstall(reference.clone())).is_none());
    assert!(matches!(
        state.update(Message::FilesPicked(Some(vec!["example.tangopkg".into()]))),
        Some(Effect::Install(_))
    ));
    assert!(state.update(Message::Install).is_none());
    assert!(state.update(Message::Refresh).is_none());
    assert!(matches!(
        state.update(Message::Finished(Err("missing dependency".into()))),
        Some(Effect::Rescan)
    ));
    assert!(state.error.as_ref().unwrap().contains("missing dependency"));
    assert!(matches!(state.update(Message::Install), Some(Effect::ChooseFiles)));
    state.update(Message::FilesPicked(None));
    assert!(matches!(
        state.update(Message::Uninstall(reference)),
        Some(Effect::Uninstall(_))
    ));
}

/// Render the actual manager with the shipped packages and an unresolved
/// library. Optional captures support visual review without a window server.
#[test]
fn package_inventory_renders_bundled_and_unresolved_entries() {
    for bytes in [
        include_bytes!("../../../fonts/NotoSans-Regular.ttf").as_slice(),
        include_bytes!("../../../fonts/NotoSansJP-Regular.otf").as_slice(),
        lucide_icons::LUCIDE_FONT_BYTES,
    ] {
        iced::advanced::graphics::text::font_system()
            .write()
            .unwrap()
            .load_font(bytes.into());
    }
    let mut bundled = crate::package::editor::bundled::packages().unwrap().to_vec();
    bundled.push(
        tango_script::Package::load(
            [
                (
                    "package.toml".into(),
                    b"api=1\nname='unresolved'\nversion='1.0.0'\n[dependencies]\nmissing='1.0.0'".to_vec(),
                ),
                ("init.luau".into(), b"--!strict\nreturn require('@missing')".to_vec()),
            ]
            .into(),
        )
        .unwrap(),
    );
    let catalog = futures::executor::block_on(package::Catalog::scan(
        crate::library::storage(),
        std::path::Path::new("unused"),
        &Default::default(),
        &bundled,
    ));
    let scanners = Scanners::new();
    scanners.packages.rescan(|| Some(catalog));
    for (name, version) in [("bn6", "0.1.0"), ("bn-common", "0.1.0"), ("unresolved", "1.0.0")] {
        for locale in ["en-US", "ja-JP"] {
            let lang = locale.parse().unwrap();
            for (theme_name, theme) in [("dark", iced::Theme::Dark), ("light", iced::Theme::Light)] {
                let mut state = State::default();
                state.update(Message::Selected(PackageRef {
                    name: name.into(),
                    version: version.parse().unwrap(),
                }));
                let mut element = state.view(&lang, &scanners, false);
                let mut renderer = futures::executor::block_on(iced::Renderer::new(
                    iced::Font::with_name("Noto Sans"),
                    crate::ui::style::TEXT_BODY.into(),
                    Some("tiny-skia"),
                ))
                .unwrap();
                let size = iced::Size::new(1024.0, 640.0);
                let mut tree = Tree::new(element.as_widget());
                let node =
                    element
                        .as_widget_mut()
                        .layout(&mut tree, &renderer, &layout::Limits::new(iced::Size::ZERO, size));
                let mut messages = Vec::new();
                let redraw = iced::Event::Window(iced::window::Event::RedrawRequested(iced::time::Instant::now()));
                element.as_widget_mut().update(
                    &mut tree,
                    &redraw,
                    Layout::new(&node),
                    iced::mouse::Cursor::Unavailable,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut iced::advanced::Shell::new(&mut messages),
                    &iced::Rectangle::with_size(size),
                );
                element.as_widget().draw(
                    &tree,
                    &mut renderer,
                    &theme,
                    &iced::advanced::renderer::Style {
                        text_color: theme.palette().text,
                    },
                    Layout::new(&node),
                    iced::mouse::Cursor::Unavailable,
                    &iced::Rectangle::with_size(size),
                );
                let pixels = renderer.screenshot(iced::Size::new(1024, 640), 1.0, theme.palette().background);
                assert_eq!(pixels.len(), 1024 * 640 * 4);
                // Click the install control in the rendered toolbar. This
                // catches accidentally disabled controls as well as layout
                // regressions that move them beyond the available window.
                for event in [
                    iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
                    iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)),
                ] {
                    element.as_widget_mut().update(
                        &mut tree,
                        &event,
                        Layout::new(&node),
                        iced::mouse::Cursor::Available(iced::Point::new(700.0, 34.0)),
                        &renderer,
                        &mut iced::advanced::clipboard::Null,
                        &mut iced::advanced::Shell::new(&mut messages),
                        &iced::Rectangle::with_size(size),
                    );
                }
                assert!(matches!(messages.as_slice(), [Message::Install]), "{messages:?}");
                if let Some(path) = std::env::var_os("TANGO_UI_CAPTURE_DIR") {
                    let path = PathBuf::from(path);
                    std::fs::create_dir_all(&path).unwrap();
                    image::save_buffer(
                        path.join(format!("packages-{name}-{locale}-{theme_name}.png")),
                        &pixels,
                        1024,
                        640,
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                }
            }
        }
    }
}

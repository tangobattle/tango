use iced::advanced::renderer::Headless;
use std::{collections::BTreeMap, path::Path};
use tango_match::telemetry::stream::{Batch, Context, Event, Frame, Record, Timeline, Value};
use tango_script::{Package, PackageRef, Profile, TelemetryViewContext};

fn package(root: &Path) -> Package {
    fn read(root: &Path, at: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                read(root, &path, files)
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_str().unwrap().replace('\\', "/"),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    read(root, root, &mut files);
    Package::load(files).unwrap()
}
#[test]
fn bundled_telemetry_renders_changes_metrics_and_translates_without_loading_an_editor() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../packages");
    let packages: Vec<_> = ["gba", "editor-common", "bn-common", "bn6"]
        .iter()
        .map(|name| package(&root.join(name)))
        .collect();
    let profile = Profile::resolve(
        &packages,
        &PackageRef {
            name: "bn6".into(),
            version: "0.1.0".parse().unwrap(),
        },
    )
    .unwrap();
    let mut history = Timeline::new([None, None]);
    history.apply(Batch {
        rewind_to: None,
        records: (0..200)
            .flat_map(|tick| {
                (1..=2).map(move |player| Record {
                    context: Context { tick, round: 1, player },
                    frame: Ok(Frame {
                        values: if (90..110).contains(&tick) {
                            BTreeMap::new()
                        } else {
                            [
                                ("p1_hp".into(), Value::Number((1000 - tick / 20 * 80) as f64)),
                                ("p2_hp".into(), Value::Number((1200 - tick / 30 * 110) as f64)),
                                ("p1_x".into(), Value::Number((tick / 9 % 3 + 1) as f64)),
                                ("p2_x".into(), Value::Number((tick / 11 % 3 + 4) as f64)),
                                ("p1_y".into(), Value::Number((tick / 10 % 3 + 1) as f64)),
                                ("p2_y".into(), Value::Number((tick / 12 % 3 + 1) as f64)),
                            ]
                            .into()
                        },
                        events: if tick % 30 == 0 {
                            vec![Event {
                                name: "chip_used".into(),
                                fields: BTreeMap::new(),
                            }]
                        } else {
                            vec![]
                        },
                    }),
                })
            })
            .collect(),
    });
    let context = TelemetryViewContext {
        player: 1,
        from_tick: 0,
        to_tick: 199,
        ticks_per_second: 60.0,
        incomplete: false,
    };
    let state = BTreeMap::new();
    let initial = profile.view_telemetry(&history, context, &state).unwrap().unwrap();
    let action = tango_script::Action {
        id: "telemetry-metric".into(),
        event: tango_script::Event::Change { value: "x".into() },
    };
    let state = profile.update_telemetry_view(&state, &action).unwrap();
    assert_eq!(state["metric"], "x");
    let moved = profile.view_telemetry(&history, context, &state).unwrap().unwrap();
    assert_ne!(initial, moved);
    let hovered = profile
        .update_telemetry_view(
            &state,
            &tango_script::Action {
                id: "telemetry-chart".into(),
                event: tango_script::Event::Pointer {
                    phase: tango_script::ui::PointerPhase::Move,
                    x: 0.0,
                    y: 10.0,
                    button: tango_script::ui::PointerButton::None,
                    dx: 0.0,
                    dy: 0.0,
                    modifiers: Default::default(),
                },
            },
        )
        .unwrap();
    assert_ne!(
        moved,
        profile.view_telemetry(&history, context, &hovered).unwrap().unwrap()
    );

    let japanese = profile
        .with_locale("ja-JP")
        .unwrap()
        .view_telemetry(&history, context, &state)
        .unwrap()
        .unwrap();
    assert_ne!(moved, japanese);

    for bytes in [
        include_bytes!("../../tango/fonts/NotoSans-Regular.ttf").as_slice(),
        include_bytes!("../../tango/fonts/NotoSansJP-Regular.otf").as_slice(),
    ] {
        iced::advanced::graphics::text::font_system()
            .write()
            .unwrap()
            .load_font(bytes.into());
    }
    for (name, node, theme) in [
        ("hp-dark", initial, iced::Theme::Dark),
        ("position-light", moved, iced::Theme::Light),
        ("position-ja", japanese, iced::Theme::Dark),
    ] {
        let script = tango_script_iced::Renderer::default();
        let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
            iced::Font::with_name("Noto Sans"),
            14.0.into(),
            Some("tiny-skia"),
        ))
        .unwrap();
        let mut ui = iced_runtime::UserInterface::build(
            iced::widget::container(script.view(&node)).padding(16),
            iced::Size::new(400.0, 200.0),
            iced_runtime::user_interface::Cache::default(),
            &mut renderer,
        );
        let cursor = iced::mouse::Cursor::Unavailable;
        ui.update(
            &[iced::Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now(),
            ))],
            cursor,
            &mut renderer,
            &mut iced::advanced::clipboard::Null,
            &mut Vec::new(),
        );
        ui.draw(
            &mut renderer,
            &theme,
            &iced::advanced::renderer::Style {
                text_color: theme.palette().text,
            },
            cursor,
        );
        let pixels = renderer.screenshot(iced::Size::new(400, 200), 1.0, theme.palette().background);
        assert!(pixels.chunks_exact(4).collect::<std::collections::BTreeSet<_>>().len() > 100);
        if let Some(dir) = std::env::var_os("TANGO_UI_CAPTURE_DIR") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            image::save_buffer(
                dir.join(format!("telemetry-{name}.png")),
                &pixels,
                400,
                200,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
}

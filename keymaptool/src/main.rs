use std::io::Write;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_default_env()
        .filter(Some("keymaptool"), log::LevelFilter::Info)
        .init();

    let event_loop = Some(winit::event_loop::EventLoop::new());

    let size = winit::dpi::LogicalSize::new(0u32, 0u32);
    let _window = winit::window::WindowBuilder::new()
        .with_title("keymaptool")
        .with_inner_size(size)
        .with_min_inner_size(size)
        .with_always_on_top(true)
        .with_decorations(false)
        .with_transparent(true)
        .build(event_loop.as_ref().expect("event loop"))?;

    event_loop
        .expect("event loop")
        .run(move |event, _, control_flow| {
            match event {
                winit::event::Event::WindowEvent {
                    event: ref window_event,
                    ..
                } => {
                    match window_event {
                        winit::event::WindowEvent::CloseRequested
                        | winit::event::WindowEvent::Focused(false) => {
                            *control_flow = winit::event_loop::ControlFlow::Exit;
                        }
                        winit::event::WindowEvent::KeyboardInput { input, .. } => {
                            let keycode = if let Some(keycode) = input.virtual_keycode {
                                keycode
                            } else {
                                return;
                            };
                            match input.state {
                                winit::event::ElementState::Pressed => {
                                    std::io::stdout()
                                        .write_all(
                                            serde_plain::to_string(&keycode).unwrap().as_bytes(),
                                        )
                                        .unwrap();
                                    std::io::stdout().write_all(b"\n").unwrap();
                                    *control_flow = winit::event_loop::ControlFlow::Exit;
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    };
                }
                _ => {}
            };
        });
}

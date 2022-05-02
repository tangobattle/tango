#![windows_subsystem = "windows"]
use std::io::Write;

use clap::StructOpt;

pub fn init_wgpu(
    window: &winit::window::Window,
) -> (
    wgpu::Device,
    wgpu::Queue,
    wgpu::Surface,
    wgpu::SurfaceConfiguration,
) {
    let backends = wgpu::util::backend_bits_from_env().unwrap_or(wgpu::Backends::PRIMARY);
    let instance = wgpu::Instance::new(backends);
    let (size, surface) = unsafe {
        let size = window.inner_size();
        let surface = instance.create_surface(&window);
        (size, surface)
    };

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: Some(&surface),
        ..Default::default()
    }))
    .expect("No adapters found!");

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("Device"),
            features: wgpu::Features::empty(),
            limits: wgpu::Limits::default(),
        },
        None,
    ))
    .unwrap();

    let format = surface.get_preferred_format(&adapter).unwrap();
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: size.width,
        height: size.height,
        present_mode: wgpu::PresentMode::Mailbox,
    };
    surface.configure(&device, &config);
    (device, queue, surface, config)
}

enum UserEvent {
    Gilrs(gilrs::Event),
}

#[derive(clap::Parser)]
struct Cli {
    #[clap(long)]
    lang: String,
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_default_env()
        .filter(Some("keymaptool"), log::LevelFilter::Info)
        .init();

    let args = Cli::parse();

    let event_loop = Some(winit::event_loop::EventLoop::with_user_event());

    let size = winit::dpi::LogicalSize::new(400u32, 100u32);
    let window = winit::window::WindowBuilder::new()
        .with_title("keymaptool")
        .with_inner_size(size)
        .with_min_inner_size(size)
        .with_always_on_top(true)
        .with_decorations(false)
        .build(event_loop.as_ref().expect("event loop"))?;

    let (device, queue, surface, config) = init_wgpu(&window);

    let mut brush = wgpu_text::BrushBuilder::using_font(match args.lang.as_str() {
        "ja" => ab_glyph::FontRef::try_from_slice(include_bytes!("fonts/NotoSansJP-Regular.otf"))?,
        "zh-Hans" => {
            ab_glyph::FontRef::try_from_slice(include_bytes!("fonts/NotoSansSC-Regular.otf"))?
        }
        _ => ab_glyph::FontRef::try_from_slice(include_bytes!("fonts/NotoSans-Regular.ttf"))?,
    })
    .build(&device, &config);

    let mut keys_pressed = [false; 255];

    let mut text = "".to_owned();
    match std::io::stdin().read_line(&mut text) {
        Ok(n) => {
            if n == 0 {
                return Ok(());
            }
        }
        Err(e) => {
            panic!("{}", e);
        }
    }

    let el_proxy = event_loop.as_ref().expect("event loop").create_proxy();
    let mut gilrs = gilrs::Gilrs::new().unwrap();
    std::thread::spawn(move || {
        while let Some(event) = gilrs.next_event() {
            if let Err(_) = el_proxy.send_event(UserEvent::Gilrs(event)) {
                break;
            }
        }
    });

    event_loop
        .expect("event loop")
        .run(move |event, _, control_flow| {
            match event {
                winit::event::Event::RedrawRequested(_) => {
                    let frame = match surface.get_current_texture() {
                        Ok(frame) => frame,
                        Err(_) => {
                            surface.configure(&device, &config);
                            surface
                                .get_current_texture()
                                .expect("Failed to acquire next surface texture!")
                        }
                    };
                    let view = frame
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());

                    let mut encoder =
                        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("Command Encoder"),
                        });

                    {
                        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("Render Pass"),
                            color_attachments: &[wgpu::RenderPassColorAttachment {
                                view: &view,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color {
                                        r: 1.0,
                                        g: 1.0,
                                        b: 1.0,
                                        a: 1.0,
                                    }),
                                    store: true,
                                },
                            }],
                            depth_stencil_attachment: None,
                        });
                    }

                    let window_size = window.inner_size();
                    let section = wgpu_text::section::Section::default()
                        .add_text(wgpu_text::section::Text::new(&text).with_scale(48.0))
                        .with_layout(
                            wgpu_text::section::Layout::default()
                                .h_align(wgpu_text::section::HorizontalAlign::Center)
                                .v_align(wgpu_text::section::VerticalAlign::Center),
                        )
                        .with_screen_position((
                            window_size.width as f32 / 2.0,
                            window_size.height as f32 / 2.0,
                        ));
                    brush.queue(&section);
                    let text_buffer = brush.draw(&device, &view, &queue);
                    queue.submit([encoder.finish(), text_buffer]);
                    frame.present();
                }
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
                                    if keys_pressed[keycode as usize] {
                                        return;
                                    }
                                    keys_pressed[keycode as usize] = true;

                                    std::io::stdout()
                                        .write_all(
                                            serde_plain::to_string(&keycode).unwrap().as_bytes(),
                                        )
                                        .unwrap();
                                    std::io::stdout().write_all(b"\n").unwrap();
                                    text.clear();
                                    match std::io::stdin().read_line(&mut text) {
                                        Ok(n) => {
                                            if n == 0 {
                                                *control_flow =
                                                    winit::event_loop::ControlFlow::Exit;
                                                return;
                                            }
                                            window.request_redraw();
                                        }
                                        Err(e) => {
                                            panic!("{}", e);
                                        }
                                    }
                                }
                                winit::event::ElementState::Released => {
                                    keys_pressed[keycode as usize] = false;
                                }
                            }
                        }
                        _ => {}
                    };
                }
                winit::event::Event::UserEvent(UserEvent::Gilrs(gilrs_ev)) => {
                    log::info!("{:?}", gilrs_ev);
                }
                _ => {}
            };
        });
}

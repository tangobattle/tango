#![windows_subsystem = "windows"]
use std::io::Write;

use clap::StructOpt;

#[derive(clap::ArgEnum, Clone, PartialEq)]
enum Target {
    Keyboard,
    Controller,
}

#[derive(clap::Parser)]
struct Cli {
    #[clap(long)]
    lang: String,

    #[clap(arg_enum, long)]
    target: Target,
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_default_env()
        .filter(Some("keymaptool"), log::LevelFilter::Info)
        .init();

    let args = Cli::parse();

    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();
    let game_controller = sdl.game_controller().unwrap();

    let ttf = sdl2::ttf::init().unwrap();
    let font = ttf
        .load_font_from_rwops(
            sdl2::rwops::RWops::from_bytes(match args.lang.as_str() {
                "ja" => include_bytes!("fonts/NotoSansJP-Regular.otf"),
                "zh-Hans" => include_bytes!("fonts/NotoSansSC-Regular.otf"),
                _ => include_bytes!("fonts/NotoSans-Regular.ttf"),
            })
            .unwrap(),
            32,
        )
        .unwrap();

    let window = video
        .window("keymaptool", 400, 100)
        .set_window_flags(sdl2::sys::SDL_WindowFlags::SDL_WINDOW_ALWAYS_ON_TOP as u32)
        .position_centered()
        .allow_highdpi()
        .borderless()
        .build()
        .unwrap();

    let mut event_loop = sdl.event_pump().unwrap();

    let mut canvas = window.into_canvas().present_vsync().build().unwrap();
    let texture_creator = canvas.texture_creator();

    let mut controllers: std::collections::HashMap<u32, sdl2::controller::GameController> =
        std::collections::HashMap::new();

    let mut next = move || {
        let mut text = "".to_owned();
        match std::io::stdin().read_line(&mut text) {
            Ok(n) => {
                if n == 0 {
                    return false;
                }

                let surface = font
                    .render(&text.trim_end())
                    .blended_wrapped(
                        sdl2::pixels::Color::RGBA(0, 0, 0, 255),
                        canvas.window().drawable_size().0,
                    )
                    .unwrap();
                let texture = texture_creator
                    .create_texture_from_surface(&surface)
                    .unwrap();
                let sdl2::render::TextureQuery { width, height, .. } = texture.query();

                canvas.set_draw_color(sdl2::pixels::Color::RGBA(0xff, 0xff, 0xff, 0xff));
                canvas.clear();
                canvas
                    .copy(
                        &texture,
                        None,
                        Some(sdl2::rect::Rect::new(
                            (canvas.window().drawable_size().0 as i32 - width as i32) / 2,
                            (canvas.window().drawable_size().1 as i32 - height as i32) / 2,
                            width,
                            height,
                        )),
                    )
                    .unwrap();
                canvas.present();
            }
            Err(e) => {
                panic!("{}", e);
            }
        }
        true
    };

    if !next() {
        return Ok(());
    }

    let mut input_state = sdl2_input_helper::State::new();

    'toplevel: loop {
        for event in event_loop.poll_iter() {
            match event {
                sdl2::event::Event::Quit { .. } => break 'toplevel,
                sdl2::event::Event::KeyDown {
                    scancode: Some(scancode),
                    ..
                } if args.target == Target::Keyboard => {
                    if input_state.is_key_pressed(scancode) {
                        continue;
                    }

                    std::io::stdout()
                        .write_all(scancode.name().as_bytes())
                        .unwrap();
                    std::io::stdout().write_all(b"\n").unwrap();

                    if !next() {
                        break 'toplevel;
                    }
                }
                sdl2::event::Event::ControllerDeviceAdded { which, .. } => {
                    let controller = game_controller.open(which).unwrap();
                    log::info!("controller added: {}", controller.name());
                    controllers.insert(which, controller);
                }
                sdl2::event::Event::ControllerDeviceRemoved { which, .. } => {
                    controllers.remove(&which);
                }
                sdl2::event::Event::ControllerButtonDown { button, .. }
                    if args.target == Target::Controller =>
                {
                    if input_state
                        .iter_controllers()
                        .any(|(_, c)| c.is_button_pressed(button))
                    {
                        continue;
                    }

                    std::io::stdout()
                        .write_all(button.string().as_bytes())
                        .unwrap();
                    std::io::stdout().write_all(b"\n").unwrap();

                    if !next() {
                        break 'toplevel;
                    }
                }
                sdl2::event::Event::ControllerAxisMotion { axis, value, .. }
                    if args.target == Target::Controller =>
                {
                    const THRESHOLD: i16 = 0x4000;
                    if input_state
                        .iter_controllers()
                        .any(|(_, c)| c.axis(axis) >= THRESHOLD)
                    {
                        continue;
                    }

                    let name = match axis {
                        sdl2::controller::Axis::TriggerLeft => "lefttrigger",
                        sdl2::controller::Axis::TriggerRight => "righttrigger",
                        _ => {
                            continue;
                        }
                    };

                    if value >= THRESHOLD {
                        std::io::stdout()
                            .write_all(name.to_string().as_bytes())
                            .unwrap();
                        std::io::stdout().write_all(b"\n").unwrap();

                        if !next() {
                            break 'toplevel;
                        }
                    }
                }
                sdl2::event::Event::Window {
                    win_event: sdl2::event::WindowEvent::FocusLost,
                    ..
                } => {
                    break 'toplevel;
                }
                _ => {}
            }
            input_state.handle_event(event);
        }
    }

    Ok(())
}

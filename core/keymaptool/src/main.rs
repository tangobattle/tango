#![windows_subsystem = "windows"]
use std::io::Write;

use clap::StructOpt;

// NOTE: This should match the same struct as in tango-core. Why it's here is just a goofy quirk.
#[derive(Clone, serde::Serialize)]
pub enum PhysicalInput {
    Key(String),
    Button(String),
    Axis(String, i16),
}

#[derive(clap::Parser)]
struct Cli {
    #[clap(long)]
    lang: String,

    #[clap()]
    text: std::ffi::OsString,
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_default_env()
        .filter(Some("keymaptool"), log::LevelFilter::Info)
        .init();

    let args = Cli::parse();

    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();
    let game_controller = sdl.game_controller().unwrap();

    let mut controllers: std::collections::HashMap<u32, sdl2::controller::GameController> =
        std::collections::HashMap::new();
    // Preemptively enumerate controllers.
    for which in 0..game_controller.num_joysticks().unwrap() {
        if !game_controller.is_game_controller(which) {
            continue;
        }
        let controller = game_controller.open(which).unwrap();
        log::info!("controller added: {}", controller.name());
        controllers.insert(which, controller);
    }

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

    let ttf = sdl2::ttf::init().unwrap();
    let font = ttf
        .load_font_from_rwops(
            sdl2::rwops::RWops::from_bytes(match args.lang.as_str() {
                "ja" => include_bytes!("fonts/NotoSansJP-Regular.otf"),
                "zh-Hans" => include_bytes!("fonts/NotoSansSC-Regular.otf"),
                _ => include_bytes!("fonts/NotoSans-Regular.ttf"),
            })
            .unwrap(),
            20 * (canvas.window().drawable_size().0 / canvas.window().size().0) as u16,
        )
        .unwrap();

    let surface = font
        .render(args.text.to_string_lossy().trim_end())
        .blended_wrapped(
            sdl2::pixels::Color::RGBA(0, 0, 0, 255),
            canvas.window().drawable_size().0 - 4,
        )
        .unwrap();
    let texture = texture_creator
        .create_texture_from_surface(&surface)
        .unwrap();
    let sdl2::render::TextureQuery { width, height, .. } = texture.query();

    canvas.set_draw_color(sdl2::pixels::Color::RGBA(0x00, 0x00, 0x00, 0xff));
    canvas.clear();

    canvas.set_draw_color(sdl2::pixels::Color::RGBA(0xff, 0xff, 0xff, 0xff));
    canvas
        .fill_rect(sdl2::rect::Rect::new(
            2,
            2,
            canvas.window().drawable_size().0 - 4,
            canvas.window().drawable_size().1 - 4,
        ))
        .unwrap();

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

    'toplevel: loop {
        for event in event_loop.poll_iter() {
            match event {
                sdl2::event::Event::Quit { .. } => break 'toplevel,
                sdl2::event::Event::KeyDown {
                    scancode: Some(scancode),
                    ..
                } => {
                    std::io::stdout()
                        .write_all(
                            serde_json::to_string(&PhysicalInput::Key(
                                scancode.name().to_string(),
                            ))?
                            .as_bytes(),
                        )
                        .unwrap();
                    std::io::stdout().write_all(b"\n").unwrap();

                    break 'toplevel;
                }
                sdl2::event::Event::ControllerDeviceAdded { which, .. } => {
                    if !game_controller.is_game_controller(which) {
                        continue;
                    }
                    let controller = game_controller.open(which).unwrap();
                    log::info!("controller added: {}", controller.name());
                    controllers.insert(which, controller);
                }
                sdl2::event::Event::ControllerDeviceRemoved { which, .. } => {
                    controllers.remove(&which);
                }
                sdl2::event::Event::ControllerButtonDown { button, .. } => {
                    std::io::stdout()
                        .write_all(
                            serde_json::to_string(&PhysicalInput::Button(button.string()))?
                                .as_bytes(),
                        )
                        .unwrap();
                    std::io::stdout().write_all(b"\n").unwrap();

                    break 'toplevel;
                }
                sdl2::event::Event::ControllerAxisMotion { axis, value, .. } => {
                    const THRESHOLD: i16 = 0x4000;
                    let sign = if value >= THRESHOLD {
                        1
                    } else if value <= -THRESHOLD {
                        -1
                    } else {
                        continue;
                    };

                    std::io::stdout()
                        .write_all(
                            serde_json::to_string(&PhysicalInput::Axis(axis.string(), sign))?
                                .as_bytes(),
                        )
                        .unwrap();
                    std::io::stdout().write_all(b"\n").unwrap();

                    break 'toplevel;
                }
                sdl2::event::Event::Window {
                    win_event: sdl2::event::WindowEvent::FocusLost,
                    ..
                } => {
                    break 'toplevel;
                }
                _ => {}
            }
        }
    }

    Ok(())
}

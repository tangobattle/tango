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

    let window = video
        .window("keymaptool", 400, 100)
        .position_centered()
        .borderless()
        .build()
        .unwrap();

    let mut event_loop = sdl.event_pump().unwrap();

    let next = move || {
        let mut text = "".to_owned();
        match std::io::stdin().read_line(&mut text) {
            Ok(n) => {
                if n == 0 {
                    return false;
                }
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

    let mut keys_pressed = [false; sdl2::keyboard::Scancode::Num as usize];
    let mut buttons_pressed = [false; 255];

    let mut canvas = window.into_canvas().present_vsync().build().unwrap();

    let mut controllers: std::collections::HashMap<u32, sdl2::controller::GameController> =
        std::collections::HashMap::new();

    'toplevel: loop {
        for event in event_loop.poll_iter() {
            match event {
                sdl2::event::Event::Quit { .. } => break 'toplevel,
                sdl2::event::Event::KeyDown {
                    scancode: Some(scancode),
                    ..
                } if args.target == Target::Keyboard => {
                    if keys_pressed[scancode as usize] {
                        continue;
                    }
                    keys_pressed[scancode as usize] = true;

                    std::io::stdout()
                        .write_all(scancode.name().as_bytes())
                        .unwrap();
                    std::io::stdout().write_all(b"\n").unwrap();

                    canvas.clear();
                    if !next() {
                        break 'toplevel;
                    }
                    canvas.present();
                }
                sdl2::event::Event::KeyUp {
                    scancode: Some(scancode),
                    ..
                } => {
                    keys_pressed[scancode as usize] = false;
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
                    if buttons_pressed[button as usize] {
                        continue;
                    }
                    buttons_pressed[button as usize] = true;

                    std::io::stdout()
                        .write_all(button.string().as_bytes())
                        .unwrap();
                    std::io::stdout().write_all(b"\n").unwrap();

                    canvas.clear();
                    if !next() {
                        break 'toplevel;
                    }
                    canvas.present();
                }
                sdl2::event::Event::ControllerButtonUp { button, .. }
                    if args.target == Target::Controller =>
                {
                    buttons_pressed[button as usize] = false;
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

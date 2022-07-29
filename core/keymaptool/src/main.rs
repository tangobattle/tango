#![windows_subsystem = "windows"]
use std::io::Write;

use ab_glyph::{Font, ScaleFont};
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

pub fn layout_paragraph<F, SF>(
    font: SF,
    position: ab_glyph::Point,
    max_width: f32,
    text: &str,
    target: &mut Vec<ab_glyph::Glyph>,
) where
    F: Font,
    SF: ScaleFont<F>,
{
    let v_advance = font.height() + font.line_gap();
    let mut caret = position + ab_glyph::point(0.0, font.ascent());
    let mut last_glyph: Option<ab_glyph::Glyph> = None;
    for c in text.chars() {
        if c.is_control() {
            if c == '\n' {
                caret = ab_glyph::point(position.x, caret.y + v_advance);
                last_glyph = None;
            }
            continue;
        }
        let mut glyph = font.scaled_glyph(c);
        if let Some(previous) = last_glyph.take() {
            caret.x += font.kern(previous.id, glyph.id);
        }
        glyph.position = caret;

        last_glyph = Some(glyph.clone());
        caret.x += font.h_advance(glyph.id);

        if !c.is_whitespace() && caret.x > position.x + max_width {
            caret = ab_glyph::point(position.x, caret.y + v_advance);
            glyph.position = caret;
            last_glyph = None;
        }

        target.push(glyph);
    }
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

    let font = ab_glyph::FontRef::try_from_slice(match args.lang.as_str() {
        "ja" => &include_bytes!("fonts/NotoSansJP-Regular.otf")[..],
        "zh-Hans" => &include_bytes!("fonts/NotoSansSC-Regular.otf")[..],
        _ => &include_bytes!("fonts/NotoSans-Regular.ttf")[..],
    })
    .unwrap();

    let scale = ab_glyph::PxScale::from(64.0);
    let scaled_font = font.as_scaled(scale);

    let mut glyphs = Vec::new();
    layout_paragraph(
        scaled_font,
        ab_glyph::point(0.0, 0.0),
        9999.0,
        &args.text.to_string_lossy(),
        &mut glyphs,
    );

    let height = scaled_font.height().ceil() as i32;
    let width = {
        let min_x = glyphs.first().unwrap().position.x;
        let last_glyph = glyphs.last().unwrap();
        let max_x = last_glyph.position.x + scaled_font.h_advance(last_glyph.id);
        (max_x - min_x).ceil() as i32
    };

    let mut texture = texture_creator
        .create_texture_streaming(
            sdl2::pixels::PixelFormatEnum::ABGR8888,
            width as u32,
            height as u32,
        )
        .unwrap();

    let mut vbuf = vec![0xffu8; (width * height * 4) as usize];
    for glyph in glyphs {
        if let Some(outlined) = scaled_font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|x, y, v| {
                let x = x as i32 + bounds.min.x as i32;
                let y = y as i32 + bounds.min.y as i32;
                if x >= width || y >= height || x < 0 || y < 0 {
                    return;
                }
                let gray = ((1.0 - v) * 0xff as f32) as u8;
                vbuf[((y * width + x) * 4) as usize + 0] = gray;
                vbuf[((y * width + x) * 4) as usize + 1] = gray;
                vbuf[((y * width + x) * 4) as usize + 2] = gray;
                vbuf[((y * width + x) * 4) as usize + 3] = 0xff;
            });
        }
    }
    texture
        .update(None, &vbuf[..], (width * 4) as usize)
        .unwrap();

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
                width as u32,
                height as u32,
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

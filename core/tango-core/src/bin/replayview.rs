#![windows_subsystem = "windows"]

pub const EXPECTED_FPS: f32 = 60.0;

use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

struct InputStateTypes;
impl input_helper::StateTypes for InputStateTypes {
    type Key = sdl2::keyboard::Scancode;
    type Button = sdl2::controller::Button;
}

#[derive(clap::Parser)]
struct Cli {
    #[clap(long)]
    remote: bool,

    #[clap(parse(from_os_str))]
    rom_path: std::path::PathBuf,

    #[clap(parse(from_os_str))]
    path: std::path::PathBuf,
}

fn main() -> Result<(), anyhow::Error> {
    env_logger::Builder::from_default_env()
        .filter(Some("tango_core"), log::LevelFilter::Info)
        .filter(Some("replayview"), log::LevelFilter::Info)
        .init();
    mgba::log::init();

    let args = Cli::parse();

    let mut f = std::fs::File::open(args.path.clone())?;

    let replay = tango_core::replay::Replay::decode(&mut f)?;

    log::info!(
        "replay is for {} (crc32 = {:08x})",
        replay.local_state.as_ref().unwrap().rom_title(),
        replay.local_state.as_ref().unwrap().rom_crc32()
    );

    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();

    let mut core = mgba::core::Core::new_gba("tango_core")?;

    let rom = std::fs::read(&args.rom_path)?;
    let vf = mgba::vfile::VFile::open_memory(&rom);
    core.as_mut().load_rom(vf)?;

    core.enable_video_buffer();

    let vbuf = std::sync::Arc::new(parking_lot::Mutex::new(vec![
        0u8;
        (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
            as usize
    ]));

    let window = video
        .window(
            "taango replayview",
            mgba::gba::SCREEN_WIDTH * 3,
            mgba::gba::SCREEN_HEIGHT * 3,
        )
        .opengl()
        .resizable()
        .build()
        .unwrap();

    let hooks = tango_core::hooks::get(core.as_mut()).unwrap();
    hooks.patch(core.as_mut());
    let local_player_index = if !args.remote {
        replay.local_player_index
    } else {
        1 - replay.local_player_index
    };

    let mut input_pairs = replay.input_pairs.clone();
    if args.remote {
        for pair in input_pairs.iter_mut() {
            std::mem::swap(&mut pair.local, &mut pair.remote);
        }
    }

    let replayer_state = tango_core::replayer::State::new(
        local_player_index,
        input_pairs,
        0,
        Box::new(|| {
            std::process::exit(0);
        }),
    );
    let mut traps = hooks.common_traps();
    traps.extend(hooks.replayer_traps(replayer_state.clone()));
    core.set_traps(traps);

    let thread = mgba::thread::Thread::new(core);
    thread.start().expect("start thread");
    let thread_handle = thread.handle();
    thread_handle.pause();
    thread_handle.lock_audio().sync_mut().set_fps_target(60.0);
    {
        let vbuf = vbuf.clone();
        thread.set_frame_callback(move |_core, video_buffer| {
            let mut vbuf = vbuf.lock();
            vbuf.copy_from_slice(video_buffer);
            for i in (0..vbuf.len()).step_by(4) {
                vbuf[i + 3] = 0xff;
            }
        });
    }

    let audio_device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| anyhow::format_err!("could not open audio device"))?;
    log::info!(
        "supported audio output configs: {:?}",
        audio_device.supported_output_configs()?.collect::<Vec<_>>()
    );
    let audio_supported_config = tango_core::audio::get_supported_config(&audio_device)?;
    log::info!("selected audio config: {:?}", audio_supported_config);

    let stream = tango_core::audio::open_stream(
        &audio_device,
        &audio_supported_config,
        tango_core::audio::MGBAStream::new(thread.handle(), audio_supported_config.sample_rate()),
    )?;
    stream.play()?;

    thread.handle().run_on_core(move |mut core| {
        core.load_state(replay.local_state.as_ref().unwrap())
            .expect("load state");
    });
    thread.handle().unpause();

    let mut event_loop = sdl.event_pump().unwrap();
    {
        let mut canvas = window
            .into_canvas()
            .present_vsync()
            .present_vsync()
            .build()
            .unwrap();
        canvas
            .set_logical_size(mgba::gba::SCREEN_WIDTH, mgba::gba::SCREEN_HEIGHT)
            .unwrap();
        canvas.set_integer_scale(true).unwrap();

        let texture_creator = canvas.texture_creator();
        let mut texture = texture_creator
            .create_texture_streaming(
                sdl2::pixels::PixelFormatEnum::ABGR8888,
                mgba::gba::SCREEN_WIDTH,
                mgba::gba::SCREEN_HEIGHT,
            )
            .unwrap();

        let mut input_state = input_helper::State::<InputStateTypes>::new();

        let mut take_screenshot_pressed = false;
        'toplevel: loop {
            let mut taking_screenshot = false;
            for event in event_loop.poll_iter() {
                match event {
                    sdl2::event::Event::Quit { .. } => break 'toplevel,
                    sdl2::event::Event::KeyDown {
                        scancode: Some(scancode),
                        repeat: false,
                        ..
                    } => {
                        input_state.handle_key_down(scancode);
                    }
                    sdl2::event::Event::KeyUp {
                        scancode: Some(scancode),
                        repeat: false,
                        ..
                    } => {
                        input_state.handle_key_up(scancode);
                    }
                    _ => {}
                }

                let last_take_screenshot_pressed = take_screenshot_pressed;
                take_screenshot_pressed = input_state.is_key_held(sdl2::keyboard::Scancode::S);
                taking_screenshot = take_screenshot_pressed && !last_take_screenshot_pressed;

                let audio_guard = thread_handle.lock_audio();
                audio_guard.sync_mut().set_fps_target(
                    if input_state.is_key_held(sdl2::keyboard::Scancode::Tab) {
                        EXPECTED_FPS * 3.0
                    } else {
                        EXPECTED_FPS
                    },
                );
            }

            let vbuf = {
                let mut replayer_state = replayer_state.lock_inner();
                if let Some(err) = replayer_state.take_error() {
                    Err(err)?;
                }

                if (!replay.is_complete && replayer_state.input_pairs_left() == 0)
                    || replayer_state.is_round_ended()
                {
                    break 'toplevel;
                }

                let current_tick = replayer_state.current_tick();

                let vbuf = vbuf.lock().clone();

                if taking_screenshot {
                    let ss_f = std::fs::File::create(format!(
                        "{}-tick{}.png",
                        args.path.clone().with_extension("").to_str().unwrap(),
                        current_tick
                    ))?;
                    let mut encoder =
                        png::Encoder::new(ss_f, mgba::gba::SCREEN_WIDTH, mgba::gba::SCREEN_HEIGHT);
                    encoder.set_color(png::ColorType::Rgba);
                    encoder.set_depth(png::BitDepth::Eight);
                    let mut writer = encoder.write_header().unwrap();
                    writer.write_image_data(&*vbuf)?;
                }

                vbuf
            };

            texture
                .update(None, &vbuf, mgba::gba::SCREEN_WIDTH as usize * 4)
                .unwrap();
            canvas.clear();
            canvas.copy(&texture, None, None).unwrap();
            canvas.present();
        }
    }

    Ok(())
}

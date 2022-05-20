#![windows_subsystem = "windows"]

use clap::Parser;
use cpal::traits::{HostTrait, StreamTrait};

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

    let mut f = std::fs::File::open(args.path)?;

    let replay = tango_core::replay::Replay::decode(&mut f)?;

    log::info!(
        "replay is for {} (crc32 = {:08x})",
        replay.local_state.as_ref().unwrap().rom_title(),
        replay.local_state.as_ref().unwrap().rom_crc32()
    );

    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();

    let mut core = mgba::core::Core::new_gba("tango_core")?;

    let vf = mgba::vfile::VFile::open(&args.rom_path, mgba::vfile::flags::O_RDONLY)?;
    core.as_mut().load_rom(vf)?;

    core.enable_video_buffer();

    let vbuf = std::sync::Arc::new(parking_lot::Mutex::new(vec![
        0u8;
        (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
            as usize
    ]));

    let audio_device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| anyhow::format_err!("could not open audio device"))?;

    let supported_config = tango_core::audio::get_supported_config(&audio_device)?;
    log::info!("selected audio config: {:?}", supported_config);

    let window = video
        .window(
            "tango replayview",
            mgba::gba::SCREEN_WIDTH * 3,
            mgba::gba::SCREEN_HEIGHT * 3,
        )
        .resizable()
        .build()
        .unwrap();

    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let hooks = tango_core::hooks::HOOKS
        .get(&core.as_ref().game_title())
        .unwrap();
    hooks.prepare_for_fastforward(core.as_mut());

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

    {
        let done = done.clone();
        core.set_traps(
            hooks.fastforwarder_traps(tango_core::fastforwarder::State::new(
                local_player_index,
                input_pairs,
                0,
                0,
                Box::new(move || {
                    done.store(true, std::sync::atomic::Ordering::Relaxed);
                }),
            )),
        );
    }

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

    let stream = tango_core::audio::open_stream(
        &audio_device,
        &supported_config,
        tango_core::audio::mgba_stream::MGBAStream::new(
            thread.handle(),
            supported_config.sample_rate(),
        ),
    )?;
    stream.play()?;

    thread.handle().run_on_core(move |mut core| {
        core.load_state(replay.local_state.as_ref().unwrap())
            .expect("load state");
    });
    thread.handle().unpause();

    let mut event_loop = sdl.event_pump().unwrap();
    {
        let mut canvas = window.into_canvas().present_vsync().build().unwrap();
        canvas
            .set_logical_size(mgba::gba::SCREEN_WIDTH, mgba::gba::SCREEN_HEIGHT)
            .unwrap();
        canvas.set_integer_scale(true).unwrap();

        let texture_creator = canvas.texture_creator();

        let mut texture = sdl2::surface::Surface::new(
            mgba::gba::SCREEN_WIDTH,
            mgba::gba::SCREEN_HEIGHT,
            sdl2::pixels::PixelFormatEnum::RGBA32,
        )
        .unwrap()
        .as_texture(&texture_creator)
        .unwrap();

        let vbuf = vbuf.clone();
        'toplevel: loop {
            for event in event_loop.poll_iter() {
                match event {
                    sdl2::event::Event::Quit { .. } => break 'toplevel,
                    _ => {}
                }
            }
            canvas.clear();
            texture
                .update(
                    sdl2::rect::Rect::new(0, 0, mgba::gba::SCREEN_WIDTH, mgba::gba::SCREEN_HEIGHT),
                    &*vbuf.lock(),
                    mgba::gba::SCREEN_WIDTH as usize * 4,
                )
                .unwrap();
            canvas.copy(&texture, None, None).unwrap();
            canvas.present();
        }
    }

    Ok(())
}

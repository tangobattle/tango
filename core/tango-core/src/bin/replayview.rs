#![windows_subsystem = "windows"]

use clap::Parser;

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

    let rom = std::fs::read(&args.rom_path)?;
    let vf = mgba::vfile::VFile::open_memory(&rom);
    core.as_mut().load_rom(vf)?;

    core.enable_video_buffer();

    let vbuf = std::sync::Arc::new(parking_lot::Mutex::new(vec![
        0u8;
        (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
            as usize
    ]));

    let audio = sdl.audio().unwrap();

    let window = video
        .window(
            "tango replayview",
            mgba::gba::SCREEN_WIDTH * 3,
            mgba::gba::SCREEN_HEIGHT * 3,
        )
        .opengl()
        .resizable()
        .build()
        .unwrap();

    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
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

    let ff_state = tango_core::replayer::State::new(
        local_player_index,
        input_pairs,
        {
            let done = done.clone();
            Box::new(move || {
                if !replay.is_complete {
                    done.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            })
        },
        {
            let done = done.clone();
            Box::new(move || {
                done.store(true, std::sync::atomic::Ordering::Relaxed);
            })
        },
    );
    let mut traps = hooks.common_traps();
    traps.extend(hooks.replayer_traps(ff_state.clone()));
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

    let device = audio
        .open_playback(
            None,
            &sdl2::audio::AudioSpecDesired {
                freq: Some(48000),
                channels: Some(2),
                samples: Some(512),
            },
            |spec| {
                tango_core::audio::mgba_stretch_stream::MGBAStretchStream::new(
                    thread.handle(),
                    spec.freq,
                )
            },
        )
        .unwrap();
    device.resume();

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

        let vbuf = vbuf;
        'toplevel: loop {
            for event in event_loop.poll_iter() {
                match event {
                    sdl2::event::Event::Quit { .. } => break 'toplevel,
                    _ => {}
                }
            }
            if let Some(err) = ff_state.take_error() {
                Err(err)?;
            }

            if done.load(std::sync::atomic::Ordering::Relaxed) {
                break 'toplevel;
            }

            texture
                .update(None, &*vbuf.lock(), mgba::gba::SCREEN_WIDTH as usize * 4)
                .unwrap();
            canvas.clear();
            canvas.copy(&texture, None, None).unwrap();
            canvas.present();
        }
    }

    Ok(())
}

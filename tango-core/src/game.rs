use crate::{audio, battle, current_input, facade, hooks, ipc, negotiation, tps};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use std::sync::Arc;

pub const EXPECTED_FPS: u32 = 60;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Keymapping {
    pub up: winit::event::VirtualKeyCode,
    pub down: winit::event::VirtualKeyCode,
    pub left: winit::event::VirtualKeyCode,
    pub right: winit::event::VirtualKeyCode,
    pub a: winit::event::VirtualKeyCode,
    pub b: winit::event::VirtualKeyCode,
    pub l: winit::event::VirtualKeyCode,
    pub r: winit::event::VirtualKeyCode,
    pub select: winit::event::VirtualKeyCode,
    pub start: winit::event::VirtualKeyCode,
}

pub struct Game {
    _rt: tokio::runtime::Runtime,
    ipc_client: ipc::Client,
    fps_counter: Arc<Mutex<tps::Counter>>,
    event_loop: Option<winit::event_loop::EventLoop<()>>,
    _audio_device: cpal::Device,
    _primary_mux_handle: audio::mux_stream::MuxHandle,
    _window: winit::window::Window,
    pixels: pixels::Pixels,
    vbuf: Arc<Mutex<Vec<u8>>>,
    current_input: std::rc::Rc<std::cell::RefCell<current_input::CurrentInput>>,
    emu_tps_counter: Arc<Mutex<tps::Counter>>,
    _stream: cpal::Stream,
    joyflags: Arc<std::sync::atomic::AtomicU32>,
    keymapping: Keymapping,
    _thread: mgba::thread::Thread,
}

impl Game {
    pub fn new(
        mut ipc_client: ipc::Client,
        keymapping: Keymapping,
        rom_path: std::path::PathBuf,
        save_path: std::path::PathBuf,
        patch_path: Option<std::path::PathBuf>,
        match_settings: Option<battle::Settings>,
    ) -> Result<Game, anyhow::Error> {
        log::info!(
            "wgpu adapters: {:?}",
            wgpu::Instance::new(wgpu::Backends::all())
                .enumerate_adapters(wgpu::Backends::all())
                .collect::<Vec<_>>()
        );
        let audio_device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| anyhow::format_err!("could not open audio device"))?;
        log::info!(
            "supported audio output configs: {:?}",
            audio_device.supported_output_configs()?.collect::<Vec<_>>()
        );

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        let handle = rt.handle().clone();

        let event_loop = Some(winit::event_loop::EventLoop::new());

        let current_input =
            std::rc::Rc::new(std::cell::RefCell::new(current_input::CurrentInput::new()));

        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));

        let window = {
            let size = winit::dpi::LogicalSize::new(
                mgba::gba::SCREEN_WIDTH * 3,
                mgba::gba::SCREEN_HEIGHT * 3,
            );
            winit::window::WindowBuilder::new()
                .with_title("tango")
                .with_inner_size(size)
                .with_min_inner_size(size)
                .build(event_loop.as_ref().expect("event loop"))?
        };

        let fps_counter = Arc::new(Mutex::new(tps::Counter::new(30)));
        let emu_tps_counter = Arc::new(Mutex::new(tps::Counter::new(10)));

        let pixels = {
            let window_size = window.inner_size();
            let surface_texture =
                pixels::SurfaceTexture::new(window_size.width, window_size.height, &window);
            pixels::PixelsBuilder::new(
                mgba::gba::SCREEN_WIDTH,
                mgba::gba::SCREEN_HEIGHT,
                surface_texture,
            )
            .build()?
        };

        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        let rom_vf = mgba::vfile::VFile::open(&rom_path, mgba::vfile::flags::O_RDONLY)?;
        core.as_mut().load_rom(rom_vf)?;

        let save_vf = mgba::vfile::VFile::open(
            &save_path,
            mgba::vfile::flags::O_CREAT | mgba::vfile::flags::O_RDWR,
        )?;
        core.as_mut().load_save(save_vf)?;

        if let Some(patch_path) = patch_path {
            let patch_vf = mgba::vfile::VFile::open(&patch_path, mgba::vfile::flags::O_RDONLY)?;
            core.as_mut().load_patch(patch_vf)?;
        }

        let hooks = hooks::HOOKS.get(&core.as_ref().game_title()).unwrap();

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let audio_supported_config = audio::get_supported_config(&audio_device)?;
        log::info!("selected audio config: {:?}", audio_supported_config);

        let cancellation_token = tokio_util::sync::CancellationToken::new();

        let match_ = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        if let Some(_) = match_settings {
            core.set_traps(hooks.primary_traps(
                handle.clone(),
                facade::Facade::new(
                    match_.clone(),
                    joyflags.clone(),
                    ipc_client.clone(),
                    cancellation_token.clone(),
                ),
            ));
        }

        let thread = mgba::thread::Thread::new(core);

        let audio_mux = audio::mux_stream::MuxStream::new();
        let primary_mux_handle =
            audio_mux.open_stream(audio::timewarp_stream::TimewarpStream::new(
                thread.handle(),
                audio_supported_config.sample_rate(),
                audio_supported_config.channels(),
            ));

        if let Some(match_settings) = match_settings {
            let negotiation = handle.block_on(async {
                negotiation::negotiate(
                    &mut ipc_client,
                    &match_settings.session_id,
                    &match_settings.matchmaking_connect_addr,
                    &match_settings.ice_servers,
                )
                .await
            })?;

            let match_ = match_.clone();
            handle.block_on(async {
                *match_.lock().await = Some(std::sync::Arc::new(battle::Match::new(
                    audio_supported_config.clone(),
                    rom_path.clone(),
                    hooks,
                    audio_mux.clone(),
                    negotiation.dc,
                    negotiation.rng,
                    negotiation.side,
                    thread.handle(),
                    match_settings,
                )));
            });

            handle.spawn(async move {
                {
                    let match_ = match_.lock().await.clone().unwrap();
                    tokio::select! {
                        Err(e) = match_.run() => {
                            log::info!("match thread ending: {:?}", e);
                        }
                        _ = cancellation_token.cancelled() => {
                        }
                    }
                }
                *match_.lock().await = None;
            });
        }

        thread.start()?;
        thread
            .handle()
            .lock_audio()
            .core_mut()
            .gba_mut()
            .sync_mut()
            .as_mut()
            .unwrap()
            .set_fps_target(EXPECTED_FPS as f32);

        {
            let joyflags = joyflags.clone();
            let vbuf = vbuf.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            thread.set_frame_callback(move |mut core, video_buffer| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                for i in (0..vbuf.len()).step_by(4) {
                    vbuf[i + 3] = 0xff;
                }
                core.set_keys(joyflags.load(std::sync::atomic::Ordering::Relaxed));
                let mut emu_tps_counter = emu_tps_counter.lock();
                emu_tps_counter.mark();
            });
        }

        let stream = audio::open_stream(&audio_device, &audio_supported_config, audio_mux.clone())?;
        stream.play()?;

        Ok(Game {
            _rt: rt,
            ipc_client,
            _audio_device: audio_device,
            _primary_mux_handle: primary_mux_handle,
            keymapping,
            fps_counter,
            current_input,
            event_loop,
            _window: window,
            pixels,
            vbuf,
            emu_tps_counter,
            _stream: stream,
            joyflags,
            _thread: thread,
        })
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        self.ipc_client
            .send_notification(ipc::Notification::State(ipc::State::Running))?;

        let current_input = self.current_input.clone();

        self.event_loop
            .take()
            .expect("event loop")
            .run(move |event, _, control_flow| {
                *control_flow = winit::event_loop::ControlFlow::Poll;

                match event {
                    winit::event::Event::WindowEvent {
                        event: ref window_event,
                        ..
                    } => {
                        match window_event {
                            winit::event::WindowEvent::CloseRequested => {
                                *control_flow = winit::event_loop::ControlFlow::Exit;
                            }
                            winit::event::WindowEvent::Resized(size) => {
                                self.pixels.resize_surface(size.width, size.height);
                            }
                            _ => {}
                        };

                        let mut current_input = current_input.borrow_mut();
                        current_input.handle_event(window_event);
                    }
                    winit::event::Event::MainEventsCleared => {
                        let mut current_input = current_input.borrow_mut();

                        let mut keys = 0u32;
                        if current_input.key_held[self.keymapping.left as usize] {
                            keys |= mgba::input::keys::LEFT;
                        }
                        if current_input.key_held[self.keymapping.right as usize] {
                            keys |= mgba::input::keys::RIGHT;
                        }
                        if current_input.key_held[self.keymapping.up as usize] {
                            keys |= mgba::input::keys::UP;
                        }
                        if current_input.key_held[self.keymapping.down as usize] {
                            keys |= mgba::input::keys::DOWN;
                        }
                        if current_input.key_held[self.keymapping.a as usize] {
                            keys |= mgba::input::keys::A;
                        }
                        if current_input.key_held[self.keymapping.b as usize] {
                            keys |= mgba::input::keys::B;
                        }
                        if current_input.key_held[self.keymapping.l as usize] {
                            keys |= mgba::input::keys::L;
                        }
                        if current_input.key_held[self.keymapping.r as usize] {
                            keys |= mgba::input::keys::R;
                        }
                        if current_input.key_held[self.keymapping.start as usize] {
                            keys |= mgba::input::keys::START;
                        }
                        if current_input.key_held[self.keymapping.select as usize] {
                            keys |= mgba::input::keys::SELECT;
                        }

                        self.joyflags
                            .store(keys, std::sync::atomic::Ordering::Relaxed);

                        let vbuf = self.vbuf.lock().clone();
                        self.pixels.get_frame().copy_from_slice(&vbuf);

                        self.pixels
                            .render_with(|encoder, render_target, context| {
                                context.scaling_renderer.render(encoder, render_target);
                                Ok(())
                            })
                            .expect("render pixels");
                        self.fps_counter.lock().mark();

                        current_input.step();
                    }
                    _ => {}
                }
            });
    }
}

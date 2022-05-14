use crate::{audio, battle, facade, gui, hooks, ipc, tps};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use glium::Surface;
use parking_lot::Mutex;
use rand::SeedableRng;
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
    rt: tokio::runtime::Runtime,
    gui: gui::Gui,
    ipc_sender: ipc::Sender,
    fps_counter: Arc<Mutex<tps::Counter>>,
    event_loop: Option<glium::glutin::event_loop::EventLoop<UserEvent>>,
    _audio_device: cpal::Device,
    _primary_mux_handle: audio::mux_stream::MuxHandle,
    display: glium::Display,
    vbuf: Arc<Mutex<Vec<u8>>>,
    texture: glium::texture::SrgbTexture2d,
    _stream: cpal::Stream,
    joyflags: Arc<std::sync::atomic::AtomicU32>,
    keymapping: Keymapping,
    _thread: mgba::thread::Thread,
}

enum UserEvent {
    Gilrs(gilrs::Event),
}

impl Game {
    pub fn new(
        rt: tokio::runtime::Runtime,
        ipc_sender: ipc::Sender,
        window_title: String,
        keymapping: Keymapping,
        rom_path: std::path::PathBuf,
        save_path: std::path::PathBuf,
        match_init: Option<battle::MatchInit>,
    ) -> Result<Game, anyhow::Error> {
        let audio_device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| anyhow::format_err!("could not open audio device"))?;
        log::info!(
            "supported audio output configs: {:?}",
            audio_device.supported_output_configs()?.collect::<Vec<_>>()
        );

        let handle = rt.handle().clone();

        let event_loop = Some(glium::glutin::event_loop::EventLoop::with_user_event());

        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));

        let wb = {
            let size = winit::dpi::LogicalSize::new(
                mgba::gba::SCREEN_WIDTH * 3,
                mgba::gba::SCREEN_HEIGHT * 3,
            );
            glium::glutin::window::WindowBuilder::new()
                .with_title(window_title.clone())
                .with_inner_size(size)
                .with_min_inner_size(size)
        };

        let fps_counter = Arc::new(Mutex::new(tps::Counter::new(30)));
        let emu_tps_counter = Arc::new(Mutex::new(tps::Counter::new(10)));

        let cb = glium::glutin::ContextBuilder::new().with_srgb(false);

        let display = glium::Display::new(wb, cb, event_loop.as_ref().expect("event loop"))?;
        let gui = gui::Gui::new(&display);

        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        let rom_vf = mgba::vfile::VFile::open(&rom_path, mgba::vfile::flags::O_RDONLY)?;
        core.as_mut().load_rom(rom_vf)?;

        log::info!("loaded game: {}", core.as_ref().game_title());

        let save_vf = mgba::vfile::VFile::open(
            &save_path,
            mgba::vfile::flags::O_CREAT | mgba::vfile::flags::O_RDWR,
        )?;
        core.as_mut().load_save(save_vf)?;

        let hooks = hooks::HOOKS.get(&core.as_ref().game_title()).unwrap();

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let audio_supported_config = audio::get_supported_config(&audio_device)?;
        log::info!("selected audio config: {:?}", audio_supported_config);

        let cancellation_token = tokio_util::sync::CancellationToken::new();

        let match_ = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        if let Some(match_init) = match_init.as_ref() {
            core.set_traps(hooks.primary_traps(
                handle.clone(),
                joyflags.clone(),
                facade::Facade::new(match_.clone(), cancellation_token.clone()),
            ));
            if let Some(opponent_nickname) = match_init.settings.opponent_nickname.as_ref() {
                hooks.replace_opponent_name(core.as_mut(), opponent_nickname);
            }
        }

        let thread = mgba::thread::Thread::new(core);

        let audio_mux = audio::mux_stream::MuxStream::new();
        let primary_mux_handle = audio_mux.open_stream(audio::mgba_stream::MGBAStream::new(
            thread.handle(),
            audio_supported_config.sample_rate(),
        ));

        if let Some(match_init) = match_init {
            let _ = std::fs::create_dir_all(&match_init.settings.replays_path);

            let match_ = match_.clone();
            handle.block_on(async {
                let is_offerer = match_init.peer_conn.local_description().unwrap().sdp_type
                    == datachannel_wrapper::SdpType::Offer;
                let rng_seed = match_init
                    .settings
                    .rng_seed
                    .clone()
                    .try_into()
                    .expect("rng seed");
                *match_.lock().await = Some(
                    battle::Match::new(
                        audio_supported_config.clone(),
                        rom_path.clone(),
                        hooks,
                        audio_mux.clone(),
                        match_init.peer_conn,
                        match_init.dc,
                        rand_pcg::Mcg128Xsl64::from_seed(rng_seed),
                        is_offerer,
                        thread.handle(),
                        match_init.settings,
                    )
                    .expect("new match"),
                );
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
            .sync_mut()
            .set_fps_target(EXPECTED_FPS as f32);

        let texture = glium::texture::SrgbTexture2d::empty_with_format(
            &display,
            glium::texture::SrgbFormat::U8U8U8,
            glium::texture::MipmapsOption::AutoGeneratedMipmaps,
            mgba::gba::SCREEN_WIDTH,
            mgba::gba::SCREEN_HEIGHT,
        )?;

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

        let gui_state = gui.state();
        {
            let match_ = Arc::downgrade(&match_);
            let fps_counter = fps_counter.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            let handle = handle;
            gui_state.set_debug_stats_getter(Some(Box::new(move || {
                handle.block_on(async {
                    let emu_tps_counter = emu_tps_counter.lock();
                    let fps_counter = fps_counter.lock();
                    Some(gui::DebugStats {
                        fps: 1.0 / fps_counter.mean_duration().as_secs_f32(),
                        emu_tps: 1.0 / emu_tps_counter.mean_duration().as_secs_f32(),
                        match_: {
                            match match_.upgrade() {
                                Some(match_) => match &*match_.lock().await {
                                    Some(match_) => Some(gui::MatchDebugStats {
                                        round: {
                                            let round_state = match_.lock_round_state().await;
                                            match &round_state.round {
                                                Some(round) => Some(gui::RoundDebugStats {
                                                    local_player_index: round.local_player_index(),
                                                    local_qlen: round.local_queue_length(),
                                                    remote_qlen: round.remote_queue_length(),
                                                    local_delay: round.local_delay(),
                                                    remote_delay: round.remote_delay(),
                                                    tps_adjustment: round.tps_adjustment(),
                                                }),
                                                None => None,
                                            }
                                        },
                                    }),
                                    None => None,
                                },
                                None => None,
                            }
                        },
                    })
                })
            })));
        }

        Ok(Game {
            rt,
            gui,
            ipc_sender,
            _audio_device: audio_device,
            _primary_mux_handle: primary_mux_handle,
            keymapping,
            fps_counter,
            event_loop,
            display,
            vbuf,
            texture,
            _stream: stream,
            joyflags,
            _thread: thread,
        })
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        self.rt.block_on(async {
            self.ipc_sender
                .send(tango_protos::ipc::FromCoreMessage {
                    which: Some(tango_protos::ipc::from_core_message::Which::StateEv(
                        tango_protos::ipc::from_core_message::StateEvent {
                            state:
                                tango_protos::ipc::from_core_message::state_event::State::Running
                                    .into(),
                        },
                    )),
                })
                .await?;
            anyhow::Result::<()>::Ok(())
        })?;

        let mut gilrs = gilrs::Gilrs::new().unwrap();
        for (_id, gamepad) in gilrs.gamepads() {
            log::info!(
                "found gamepad: {} is {:?}",
                gamepad.name(),
                gamepad.power_info()
            );
        }

        let el_proxy = self.event_loop.as_ref().expect("event loop").create_proxy();
        std::thread::spawn(move || {
            while let Some(event) = gilrs.next_event() {
                if let Err(_) = el_proxy.send_event(UserEvent::Gilrs(event)) {
                    break;
                }
            }
        });

        let mut console_key_pressed = false;

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
                            winit::event::WindowEvent::KeyboardInput { input, .. } => {
                                let mut keymask = 0u32;
                                if input.virtual_keycode == Some(self.keymapping.left) {
                                    keymask |= mgba::input::keys::LEFT;
                                }
                                if input.virtual_keycode == Some(self.keymapping.right) {
                                    keymask |= mgba::input::keys::RIGHT;
                                }
                                if input.virtual_keycode == Some(self.keymapping.up) {
                                    keymask |= mgba::input::keys::UP;
                                }
                                if input.virtual_keycode == Some(self.keymapping.down) {
                                    keymask |= mgba::input::keys::DOWN;
                                }
                                if input.virtual_keycode == Some(self.keymapping.a) {
                                    keymask |= mgba::input::keys::A;
                                }
                                if input.virtual_keycode == Some(self.keymapping.b) {
                                    keymask |= mgba::input::keys::B;
                                }
                                if input.virtual_keycode == Some(self.keymapping.l) {
                                    keymask |= mgba::input::keys::L;
                                }
                                if input.virtual_keycode == Some(self.keymapping.r) {
                                    keymask |= mgba::input::keys::R;
                                }
                                if input.virtual_keycode == Some(self.keymapping.start) {
                                    keymask |= mgba::input::keys::START;
                                }
                                if input.virtual_keycode == Some(self.keymapping.select) {
                                    keymask |= mgba::input::keys::SELECT;
                                }

                                match input.state {
                                    winit::event::ElementState::Pressed => {
                                        self.joyflags.fetch_or(
                                            keymask,
                                            std::sync::atomic::Ordering::Relaxed,
                                        );
                                    }
                                    winit::event::ElementState::Released => {
                                        self.joyflags.fetch_and(
                                            !keymask,
                                            std::sync::atomic::Ordering::Relaxed,
                                        );
                                    }
                                }

                                if input.virtual_keycode
                                    == Some(winit::event::VirtualKeyCode::Grave)
                                {
                                    match input.state {
                                        winit::event::ElementState::Pressed => {
                                            if console_key_pressed {
                                                return;
                                            }
                                            console_key_pressed = true;
                                            self.gui.state().toggle_debug();
                                        }
                                        winit::event::ElementState::Released => {
                                            console_key_pressed = false;
                                        }
                                    }
                                }
                            }
                            winit::event::WindowEvent::CloseRequested => {
                                *control_flow = winit::event_loop::ControlFlow::Exit;
                            }
                            _ => {}
                        };

                        self.gui.handle_event(window_event);
                    }
                    winit::event::Event::MainEventsCleared => {
                        let vbuf = self.vbuf.lock().clone();
                        self.texture.write(
                            glium::Rect {
                                left: 0,
                                bottom: 0,
                                width: self.texture.get_width(),
                                height: self.texture.get_height().unwrap(),
                            },
                            glium::texture::RawImage2d::from_raw_rgba_reversed(
                                &vbuf,
                                (mgba::gba::SCREEN_WIDTH, mgba::gba::SCREEN_HEIGHT),
                            ),
                        );

                        let (fb_width, fb_height) = self.display.get_framebuffer_dimensions();

                        let scaling_factor = std::cmp::min(
                            fb_width / mgba::gba::SCREEN_WIDTH,
                            fb_height / mgba::gba::SCREEN_HEIGHT,
                        );

                        let width = mgba::gba::SCREEN_WIDTH * scaling_factor;
                        let height = mgba::gba::SCREEN_HEIGHT * scaling_factor;

                        let mut target = self.display.draw();
                        target.clear_color(0.0, 0.0, 0.0, 1.0);
                        let surface = glium::framebuffer::SimpleFrameBuffer::new(
                            &self.display,
                            &self.texture,
                        )
                        .unwrap();
                        surface.blit_whole_color_to(
                            &target,
                            &glium::BlitTarget {
                                left: (fb_width - width) / 2,
                                bottom: (fb_height - height) / 2,
                                width: width as i32,
                                height: height as i32,
                            },
                            glium::uniforms::MagnifySamplerFilter::Nearest,
                        );
                        self.gui.render(&self.display, &mut target);
                        target.finish().unwrap();
                        self.fps_counter.lock().mark();
                    }
                    winit::event::Event::UserEvent(UserEvent::Gilrs(_gilrs_ev)) => {}
                    _ => {}
                }
            });
    }
}

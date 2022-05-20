use crate::{audio, battle, facade, hooks, ipc, tps};
use parking_lot::Mutex;
use rand::SeedableRng;
use std::sync::Arc;

pub const EXPECTED_FPS: u32 = 60;

#[derive(Clone)]
pub struct Keymapping {
    pub up: Option<sdl2::keyboard::Scancode>,
    pub down: Option<sdl2::keyboard::Scancode>,
    pub left: Option<sdl2::keyboard::Scancode>,
    pub right: Option<sdl2::keyboard::Scancode>,
    pub a: Option<sdl2::keyboard::Scancode>,
    pub b: Option<sdl2::keyboard::Scancode>,
    pub l: Option<sdl2::keyboard::Scancode>,
    pub r: Option<sdl2::keyboard::Scancode>,
    pub select: Option<sdl2::keyboard::Scancode>,
    pub start: Option<sdl2::keyboard::Scancode>,
}

impl Keymapping {
    fn to_mgba_keys(&self, scancode: sdl2::keyboard::Scancode) -> u32 {
        if Some(scancode) == self.left {
            mgba::input::keys::LEFT
        } else if Some(scancode) == self.right {
            mgba::input::keys::RIGHT
        } else if Some(scancode) == self.up {
            mgba::input::keys::UP
        } else if Some(scancode) == self.down {
            mgba::input::keys::DOWN
        } else if Some(scancode) == self.a {
            mgba::input::keys::A
        } else if Some(scancode) == self.b {
            mgba::input::keys::B
        } else if Some(scancode) == self.l {
            mgba::input::keys::L
        } else if Some(scancode) == self.r {
            mgba::input::keys::R
        } else if Some(scancode) == self.start {
            mgba::input::keys::START
        } else if Some(scancode) == self.select {
            mgba::input::keys::SELECT
        } else {
            0
        }
    }
}

#[derive(Clone)]
pub struct ControllerMapping {
    pub up: Option<sdl2::controller::Button>,
    pub down: Option<sdl2::controller::Button>,
    pub left: Option<sdl2::controller::Button>,
    pub right: Option<sdl2::controller::Button>,
    pub a: Option<sdl2::controller::Button>,
    pub b: Option<sdl2::controller::Button>,
    pub l: Option<sdl2::controller::Button>,
    pub r: Option<sdl2::controller::Button>,
    pub select: Option<sdl2::controller::Button>,
    pub start: Option<sdl2::controller::Button>,
    pub enable_left_stick: bool,
}

impl ControllerMapping {
    fn to_mgba_keys(&self, button: sdl2::controller::Button) -> u32 {
        if Some(button) == self.left {
            mgba::input::keys::LEFT
        } else if Some(button) == self.right {
            mgba::input::keys::RIGHT
        } else if Some(button) == self.up {
            mgba::input::keys::UP
        } else if Some(button) == self.down {
            mgba::input::keys::DOWN
        } else if Some(button) == self.a {
            mgba::input::keys::A
        } else if Some(button) == self.b {
            mgba::input::keys::B
        } else if Some(button) == self.l {
            mgba::input::keys::L
        } else if Some(button) == self.r {
            mgba::input::keys::R
        } else if Some(button) == self.start {
            mgba::input::keys::START
        } else if Some(button) == self.select {
            mgba::input::keys::SELECT
        } else {
            0
        }
    }
}

pub struct Game {
    rt: tokio::runtime::Runtime,
    ipc_sender: ipc::Sender,
    fps_counter: Arc<Mutex<tps::Counter>>,
    emu_tps_counter: Arc<Mutex<tps::Counter>>,
    match_: std::sync::Weak<tokio::sync::Mutex<Option<Arc<battle::Match>>>>,
    event_loop: sdl2::EventPump,
    _primary_mux_handle: audio::mux_stream::MuxHandle,
    game_controller: sdl2::GameControllerSubsystem,
    canvas: sdl2::render::Canvas<sdl2::video::Window>,
    _audio_device: sdl2::audio::AudioDevice<crate::audio::mux_stream::MuxStream>,
    vbuf: Arc<Mutex<Vec<u8>>>,
    joyflags: Arc<std::sync::atomic::AtomicU32>,
    keymapping: Keymapping,
    controller_mapping: ControllerMapping,
    _thread: mgba::thread::Thread,
}

impl Game {
    pub fn new(
        rt: tokio::runtime::Runtime,
        ipc_sender: ipc::Sender,
        window_title: String,
        keymapping: Keymapping,
        controller_mapping: ControllerMapping,
        rom_path: std::path::PathBuf,
        save_path: std::path::PathBuf,
        match_init: Option<battle::MatchInit>,
    ) -> Result<Game, anyhow::Error> {
        let handle = rt.handle().clone();

        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));

        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let game_controller = sdl.game_controller().unwrap();
        let audio = sdl.audio().unwrap();

        let event_loop = sdl.event_pump().unwrap();

        let window = video
            .window(
                &window_title,
                mgba::gba::SCREEN_WIDTH * 3,
                mgba::gba::SCREEN_HEIGHT * 3,
            )
            .opengl()
            .resizable()
            .build()
            .unwrap();

        let fps_counter = Arc::new(Mutex::new(tps::Counter::new(30)));
        let emu_tps_counter = Arc::new(Mutex::new(tps::Counter::new(10)));

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

        let audio_device = audio
            .open_playback(
                None,
                &sdl2::audio::AudioSpecDesired {
                    freq: Some(48000),
                    channels: Some(2),
                    samples: Some(512),
                },
                |_| audio_mux.clone(),
            )
            .unwrap();
        audio_device.resume();

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
                        audio_device.spec().freq,
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

        let primary_mux_handle = audio_mux.open_stream(audio::mgba_stream::MGBAStream::new(
            thread.handle(),
            audio_device.spec().freq,
        ));

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

        let mut canvas = window.into_canvas().present_vsync().build().unwrap();
        canvas
            .set_logical_size(mgba::gba::SCREEN_WIDTH, mgba::gba::SCREEN_HEIGHT)
            .unwrap();
        canvas.set_integer_scale(true).unwrap();

        Ok(Game {
            rt,
            ipc_sender,
            _audio_device: audio_device,
            _primary_mux_handle: primary_mux_handle,
            keymapping,
            controller_mapping,
            fps_counter,
            emu_tps_counter,
            event_loop,
            game_controller,
            canvas,
            vbuf,
            joyflags,
            match_: Arc::downgrade(&match_),
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

        let mut debug_key_pressed = false;
        let mut show_debug = false;

        let ttf = sdl2::ttf::init().unwrap();
        let font = ttf
            .load_font_from_rwops(
                sdl2::rwops::RWops::from_bytes(include_bytes!("fonts/04B_03__.TTF")).unwrap(),
                8,
            )
            .unwrap();

        let texture_creator = self.canvas.texture_creator();
        let mut texture = sdl2::surface::Surface::new(
            mgba::gba::SCREEN_WIDTH,
            mgba::gba::SCREEN_HEIGHT,
            sdl2::pixels::PixelFormatEnum::ABGR8888,
        )
        .unwrap()
        .as_texture(&texture_creator)
        .unwrap();

        let mut controllers: std::collections::HashMap<u32, sdl2::controller::GameController> =
            std::collections::HashMap::new();

        let mut dpad_x_active = false;
        let mut dpad_y_active = false;

        'toplevel: loop {
            for event in self.event_loop.poll_iter() {
                match event {
                    sdl2::event::Event::Quit { .. } => {
                        break 'toplevel;
                    }
                    sdl2::event::Event::KeyDown {
                        scancode: Some(scancode),
                        ..
                    } => {
                        self.joyflags.fetch_or(
                            self.keymapping.to_mgba_keys(scancode),
                            std::sync::atomic::Ordering::Relaxed,
                        );

                        if scancode == sdl2::keyboard::Scancode::Grave {
                            if debug_key_pressed {
                                continue;
                            }
                            debug_key_pressed = true;
                            show_debug = !show_debug;
                        }
                    }
                    sdl2::event::Event::KeyUp {
                        scancode: Some(scancode),
                        ..
                    } => {
                        self.joyflags.fetch_and(
                            !self.keymapping.to_mgba_keys(scancode),
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        if scancode == sdl2::keyboard::Scancode::Grave {
                            debug_key_pressed = false;
                        }
                    }
                    sdl2::event::Event::ControllerAxisMotion { axis, value, .. }
                        if self.controller_mapping.enable_left_stick =>
                    {
                        const STICK_THRESHOLD: i16 = 16384;
                        match axis {
                            sdl2::controller::Axis::LeftX => {
                                if value <= -STICK_THRESHOLD {
                                    self.joyflags.fetch_or(
                                        mgba::input::keys::LEFT,
                                        std::sync::atomic::Ordering::Relaxed,
                                    );
                                } else if value >= STICK_THRESHOLD {
                                    self.joyflags.fetch_or(
                                        mgba::input::keys::RIGHT,
                                        std::sync::atomic::Ordering::Relaxed,
                                    );
                                } else if !dpad_x_active {
                                    self.joyflags.fetch_and(
                                        !(mgba::input::keys::LEFT | mgba::input::keys::RIGHT),
                                        std::sync::atomic::Ordering::Relaxed,
                                    );
                                }
                            }
                            sdl2::controller::Axis::LeftY => {
                                if value <= -STICK_THRESHOLD {
                                    self.joyflags.fetch_or(
                                        mgba::input::keys::UP,
                                        std::sync::atomic::Ordering::Relaxed,
                                    );
                                } else if value >= STICK_THRESHOLD {
                                    self.joyflags.fetch_or(
                                        mgba::input::keys::DOWN,
                                        std::sync::atomic::Ordering::Relaxed,
                                    );
                                } else if !dpad_y_active {
                                    self.joyflags.fetch_and(
                                        !(mgba::input::keys::UP | mgba::input::keys::DOWN),
                                        std::sync::atomic::Ordering::Relaxed,
                                    );
                                }
                            }
                            _ => {
                                continue;
                            }
                        };
                    }
                    sdl2::event::Event::ControllerDeviceAdded { which, .. } => {
                        let controller = self.game_controller.open(which).unwrap();
                        log::info!("controller added: {}", controller.name());
                        controllers.insert(which, controller);
                    }
                    sdl2::event::Event::ControllerDeviceRemoved { which, .. } => {
                        controllers.remove(&which);
                    }
                    sdl2::event::Event::ControllerButtonDown { button, .. } => {
                        let mask = self.controller_mapping.to_mgba_keys(button);
                        if mask & mgba::input::keys::LEFT != 0
                            || mask & mgba::input::keys::RIGHT != 0
                        {
                            dpad_x_active = true;
                        }
                        if mask & mgba::input::keys::UP != 0 || mask & mgba::input::keys::DOWN != 0
                        {
                            dpad_y_active = true;
                        }
                        self.joyflags.fetch_or(
                            self.controller_mapping.to_mgba_keys(button),
                            std::sync::atomic::Ordering::Relaxed,
                        );
                    }
                    sdl2::event::Event::ControllerButtonUp { button, .. } => {
                        let mask = self.controller_mapping.to_mgba_keys(button);
                        if mask & mgba::input::keys::LEFT != 0
                            || mask & mgba::input::keys::RIGHT != 0
                        {
                            dpad_x_active = false;
                        }
                        if mask & mgba::input::keys::UP != 0 || mask & mgba::input::keys::DOWN != 0
                        {
                            dpad_y_active = false;
                        }
                        self.joyflags
                            .fetch_and(!mask, std::sync::atomic::Ordering::Relaxed);
                    }
                    _ => {}
                }
            }

            self.canvas.clear();
            texture
                .update(
                    sdl2::rect::Rect::new(0, 0, mgba::gba::SCREEN_WIDTH, mgba::gba::SCREEN_HEIGHT),
                    &*self.vbuf.lock(),
                    mgba::gba::SCREEN_WIDTH as usize * 4,
                )
                .unwrap();
            self.canvas.copy(&texture, None, None).unwrap();

            if show_debug {
                let mut lines = vec![format!(
                    "fps: {:.0}",
                    1.0 / self.fps_counter.lock().mean_duration().as_secs_f32()
                )];

                let tps_adjustment = if let Some(match_) = self.match_.upgrade() {
                    self.rt.block_on(async {
                        if let Some(match_) = &*match_.lock().await {
                            lines.push("match active".to_string());
                            let round_state = match_.lock_round_state().await;
                            if let Some(round) = round_state.round.as_ref() {
                                lines.push(format!(
                                    "local player index: {}",
                                    round.local_player_index()
                                ));
                                lines.push(format!(
                                    "qlen: {} (-{}) vs {} (-{})",
                                    round.local_queue_length(),
                                    round.local_delay(),
                                    round.remote_queue_length(),
                                    round.remote_delay(),
                                ));
                                round.tps_adjustment()
                            } else {
                                0
                            }
                        } else {
                            0
                        }
                    })
                } else {
                    0
                };

                lines.push(format!(
                    "emu tps: {:.0} (-{})",
                    1.0 / self.emu_tps_counter.lock().mean_duration().as_secs_f32(),
                    tps_adjustment
                ));

                for (i, line) in lines.iter().enumerate() {
                    let surface = font
                        .render(line)
                        .shaded(
                            sdl2::pixels::Color::RGBA(255, 255, 255, 255),
                            sdl2::pixels::Color::RGBA(0, 0, 0, 255),
                        )
                        .unwrap();
                    let texture = texture_creator
                        .create_texture_from_surface(&surface)
                        .unwrap();
                    let sdl2::render::TextureQuery { width, height, .. } = texture.query();
                    self.canvas
                        .copy(
                            &texture,
                            None,
                            Some(sdl2::rect::Rect::new(1, 1 + i as i32 * 8, width, height)),
                        )
                        .unwrap();
                }
            }

            self.canvas.present();
            self.fps_counter.lock().mark();
        }

        Ok(())
    }
}

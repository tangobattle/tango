#![windows_subsystem = "windows"]

use clap::StructOpt;

#[derive(Clone, serde::Deserialize)]
pub struct Keymapping {
    pub up: String,
    pub down: String,
    pub left: String,
    pub right: String,
    pub a: String,
    pub b: String,
    pub l: String,
    pub r: String,
    pub select: String,
    pub start: String,
}

impl Into<tango_core::game::Keymapping> for Keymapping {
    fn into(self) -> tango_core::game::Keymapping {
        tango_core::game::Keymapping {
            up: sdl2::keyboard::Scancode::from_name(&self.up)
                .into_iter()
                .collect(),
            down: sdl2::keyboard::Scancode::from_name(&self.down)
                .into_iter()
                .collect(),
            left: sdl2::keyboard::Scancode::from_name(&self.left)
                .into_iter()
                .collect(),
            right: sdl2::keyboard::Scancode::from_name(&self.right)
                .into_iter()
                .collect(),
            a: sdl2::keyboard::Scancode::from_name(&self.a)
                .into_iter()
                .collect(),
            b: sdl2::keyboard::Scancode::from_name(&self.b)
                .into_iter()
                .collect(),
            l: sdl2::keyboard::Scancode::from_name(&self.l)
                .into_iter()
                .collect(),
            r: sdl2::keyboard::Scancode::from_name(&self.r)
                .into_iter()
                .collect(),
            select: sdl2::keyboard::Scancode::from_name(&self.select)
                .into_iter()
                .collect(),
            start: sdl2::keyboard::Scancode::from_name(&self.start)
                .into_iter()
                .collect(),
        }
    }
}

#[derive(Clone, serde::Deserialize)]
pub struct ControllerMapping {
    pub up: String,
    pub down: String,
    pub left: String,
    pub right: String,
    pub a: String,
    pub b: String,
    pub l: String,
    pub r: String,
    pub select: String,
    pub start: String,
    #[serde(rename = "enableLeftStick")]
    pub enable_left_stick: bool,
}

impl Into<tango_core::game::ControllerMapping> for ControllerMapping {
    fn into(self) -> tango_core::game::ControllerMapping {
        const STICK_THRESHOLD: i16 = 16384;

        tango_core::game::ControllerMapping {
            up: vec![
                sdl2::controller::Button::from_string(&self.up)
                    .into_iter()
                    .map(|button| tango_core::game::ControllerInput::Button(button))
                    .collect(),
                if self.enable_left_stick {
                    vec![tango_core::game::ControllerInput::Axis(
                        sdl2::controller::Axis::LeftY,
                        -STICK_THRESHOLD,
                    )]
                } else {
                    vec![]
                },
            ]
            .concat(),
            down: vec![
                sdl2::controller::Button::from_string(&self.down)
                    .into_iter()
                    .map(|button| tango_core::game::ControllerInput::Button(button))
                    .collect(),
                if self.enable_left_stick {
                    vec![tango_core::game::ControllerInput::Axis(
                        sdl2::controller::Axis::LeftY,
                        STICK_THRESHOLD,
                    )]
                } else {
                    vec![]
                },
            ]
            .concat(),
            left: vec![
                sdl2::controller::Button::from_string(&self.left)
                    .into_iter()
                    .map(|button| tango_core::game::ControllerInput::Button(button))
                    .collect(),
                if self.enable_left_stick {
                    vec![tango_core::game::ControllerInput::Axis(
                        sdl2::controller::Axis::LeftX,
                        -STICK_THRESHOLD,
                    )]
                } else {
                    vec![]
                },
            ]
            .concat(),
            right: vec![
                sdl2::controller::Button::from_string(&self.right)
                    .into_iter()
                    .map(|button| tango_core::game::ControllerInput::Button(button))
                    .collect(),
                if self.enable_left_stick {
                    vec![tango_core::game::ControllerInput::Axis(
                        sdl2::controller::Axis::LeftX,
                        STICK_THRESHOLD,
                    )]
                } else {
                    vec![]
                },
            ]
            .concat(),
            a: sdl2::controller::Button::from_string(&self.a)
                .into_iter()
                .map(|button| tango_core::game::ControllerInput::Button(button))
                .collect(),
            b: sdl2::controller::Button::from_string(&self.b)
                .into_iter()
                .map(|button| tango_core::game::ControllerInput::Button(button))
                .collect(),
            l: sdl2::controller::Button::from_string(&self.l)
                .into_iter()
                .map(|button| tango_core::game::ControllerInput::Button(button))
                .collect(),
            r: sdl2::controller::Button::from_string(&self.r)
                .into_iter()
                .map(|button| tango_core::game::ControllerInput::Button(button))
                .collect(),
            select: sdl2::controller::Button::from_string(&self.select)
                .into_iter()
                .map(|button| tango_core::game::ControllerInput::Button(button))
                .collect(),
            start: sdl2::controller::Button::from_string(&self.start)
                .into_iter()
                .map(|button| tango_core::game::ControllerInput::Button(button))
                .collect(),
        }
    }
}

#[derive(clap::Parser)]
struct Cli {
    #[clap(long)]
    keymapping: String,

    #[clap(long)]
    controller_mapping: String,

    #[clap(long)]
    signaling_connect_addr: String,

    #[clap(long)]
    ice_servers: Vec<String>,

    #[clap(long)]
    session_id: Option<String>,
}

fn main() -> Result<(), anyhow::Error> {
    env_logger::Builder::from_default_env()
        .filter(Some("tango_core"), log::LevelFilter::Info)
        .filter(Some("datachannel"), log::LevelFilter::Info)
        .init();

    log::info!("welcome to tango-core {}!", git_version::git_version!());

    let args = Cli::parse();

    let keymapping = serde_json::from_str::<Keymapping>(&args.keymapping)?;
    let controller_mapping = serde_json::from_str::<ControllerMapping>(&args.controller_mapping)?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let mut ipc_sender = tango_core::ipc::Sender::new_from_stdout();
    let mut ipc_receiver = tango_core::ipc::Receiver::new_from_stdin();

    let (window_title, rom_path, save_path, pvp_init) = if let Some(session_id) = &args.session_id {
        rt.block_on(async {
            let (dc, peer_conn) = tango_core::negotiation::negotiate(
                &mut ipc_sender,
                &session_id,
                &args.signaling_connect_addr,
                &args.ice_servers,
            )
            .await?;

            let (mut dc_rx, mut dc_tx) = dc.split();

            let mut ping_timer = tokio::time::interval(std::time::Duration::from_secs(1));

            loop {
                tokio::select! {
                    msg = ipc_receiver.receive() => {
                        match msg?.which {
                            Some(tango_protos::ipc::to_core_message::Which::SmuggleReq(tango_protos::ipc::to_core_message::SmuggleRequest { data })) => {
                                dc_tx.send(&tango_core::protocol::Packet::Smuggle(tango_core::protocol::Smuggle {
                                    data,
                                }).serialize()?).await?;
                            },
                            Some(tango_protos::ipc::to_core_message::Which::StartReq(start_req)) => {
                                return Ok((start_req.window_title, start_req.rom_path, start_req.save_path, Some((peer_conn, dc_rx.unsplit(dc_tx), start_req.settings.unwrap()))))
                            },
                            None => {
                                anyhow::bail!("ipc channel closed");
                            },
                        }
                    }

                    _ = ping_timer.tick() => {
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
                        dc_tx.send(&tango_core::protocol::Packet::Ping(tango_core::protocol::Ping {
                            ts: now.as_nanos() as u64,
                        }).serialize()?).await?;
                    }

                    msg = dc_rx.receive() => {
                        match msg {
                            Some(msg) => {
                                match tango_core::protocol::Packet::deserialize(&msg)? {
                                    tango_core::protocol::Packet::Smuggle(tango_core::protocol::Smuggle {
                                        data,
                                    }) => {
                                        ipc_sender.send(tango_protos::ipc::FromCoreMessage {
                                            which: Some(tango_protos::ipc::from_core_message::Which::SmuggleEv(tango_protos::ipc::from_core_message::SmuggleEvent {
                                                data,
                                            }))
                                        }).await?;
                                    },
                                    tango_core::protocol::Packet::Ping(tango_core::protocol::Ping {
                                        ts
                                    }) => {
                                        dc_tx.send(&tango_core::protocol::Packet::Pong(tango_core::protocol::Pong {
                                            ts
                                        }).serialize()?).await?;
                                    },
                                    tango_core::protocol::Packet::Pong(tango_core::protocol::Pong {
                                        ts
                                    }) => {
                                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
                                        let then = std::time::Duration::from_nanos(ts);
                                        ipc_sender.send(tango_protos::ipc::FromCoreMessage {
                                            which: Some(tango_protos::ipc::from_core_message::Which::ConnectionQualityEv(tango_protos::ipc::from_core_message::ConnectionQualityEvent {
                                                rtt: (now - then).as_nanos() as u64,
                                            }))
                                        }).await?;
                                    },
                                    p => {
                                        anyhow::bail!("unexpected packet: {:?}", p);
                                    }
                                }
                            },
                            None => {
                                anyhow::bail!("data channel closed");
                            },
                        }
                    }
                }
            }
        })?
    } else {
        rt.block_on(async {
            ipc_sender
                .send(tango_protos::ipc::FromCoreMessage {
                    which: Some(tango_protos::ipc::from_core_message::Which::StateEv(
                        tango_protos::ipc::from_core_message::StateEvent {
                            state:
                                tango_protos::ipc::from_core_message::state_event::State::Starting
                                    .into(),
                        },
                    )),
                })
                .await?;

            let msg = ipc_receiver.receive().await;
            match msg?.which {
                Some(tango_protos::ipc::to_core_message::Which::StartReq(start_req)) => {
                    return Ok((
                        start_req.window_title,
                        start_req.rom_path,
                        start_req.save_path,
                        None,
                    ))
                }
                Some(p) => {
                    anyhow::bail!("unexpected ipc request: {:?}", p);
                }
                None => {
                    anyhow::bail!("ipc channel closed");
                }
            }
        })?
    };

    mgba::log::init();

    let g = tango_core::game::Game::new(
        rt,
        ipc_sender,
        window_title,
        keymapping.into(),
        controller_mapping.into(),
        rom_path.into(),
        save_path.into(),
        match pvp_init {
            None => None,
            Some((peer_conn, dc, settings)) => Some(tango_core::battle::MatchInit {
                dc,
                peer_conn,
                settings: tango_core::battle::Settings {
                    replay_metadata: settings.replay_metadata,
                    replays_path: settings.replays_path.into(),
                    shadow_save_path: settings.shadow_save_path.into(),
                    shadow_rom_path: settings.shadow_rom_path.into(),
                    match_type: settings.match_type as u16,
                    input_delay: settings.input_delay,
                    shadow_input_delay: settings.shadow_input_delay,
                    rng_seed: settings.rng_seed,
                    opponent_nickname: settings.opponent_nickname,
                },
            }),
        },
    )?;
    g.run()?;
    Ok(())
}

#![windows_subsystem = "windows"]

use clap::StructOpt;

#[derive(clap::Parser)]
struct Cli {
    #[clap(long)]
    keymapping: String,

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

    let keymapping = serde_json::from_str(&args.keymapping)?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let mut ipc_sender = tango_core::ipc::Sender::new_from_stdout();
    let mut ipc_receiver = tango_core::ipc::Receiver::new_from_stdin();

    let (window_title, rom_path, save_path, pvp_init) = if let Some(session_id) = &args.session_id {
        rt.block_on(async {
            let (mut dc, peer_conn) = tango_core::negotiation::negotiate(
                &mut ipc_sender,
                &session_id,
                &args.signaling_connect_addr,
                &args.ice_servers,
            )
            .await?;

            let mut ping_timer = tokio::time::interval(std::time::Duration::from_secs(1));

            loop {
                tokio::select! {
                    msg = ipc_receiver.receive() => {
                        match msg?.which {
                            Some(tango_protos::ipc::to_core_message::Which::SmuggleReq(tango_protos::ipc::to_core_message::SmuggleRequest { data })) => {
                                dc.send(&tango_core::protocol::Packet::Smuggle(tango_core::protocol::Smuggle {
                                    data,
                                }).serialize()?).await?;
                            },
                            Some(tango_protos::ipc::to_core_message::Which::StartReq(start_req)) => {
                                return Ok((start_req.window_title, start_req.rom_path, start_req.save_path, Some((peer_conn, dc, start_req.settings.unwrap()))))
                            },
                            None => {
                                anyhow::bail!("ipc channel closed");
                            },
                        }
                    }

                    _ = ping_timer.tick() => {
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
                        dc.send(&tango_core::protocol::Packet::Ping(tango_core::protocol::Ping {
                            ts: now.as_nanos() as u64,
                        }).serialize()?).await?;
                    }

                    msg = dc.receive() => {
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
                                        dc.send(&tango_core::protocol::Packet::Pong(tango_core::protocol::Pong {
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
                            Non     e => {
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
        keymapping,
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

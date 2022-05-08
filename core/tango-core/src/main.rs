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
            let (dc, peer_conn) = tango_core::negotiation::negotiate(
                &mut ipc_sender,
                &session_id,
                &args.signaling_connect_addr,
                &args.ice_servers,
            )
            .await?;

            let (dc_rx, mut dc_tx) = dc.split();
            let dc_rx = std::sync::Arc::new(tokio::sync::Mutex::new(Some(dc_rx)));

            let (start_req, dc_tx) = {
                let dc_rx = dc_rx.clone();
                    tokio::select! {
                    r = async {
                        loop {
                            let msg = ipc_receiver.receive().await;
                            match msg?.which {
                                Some(tango_protos::ipc::to_core_message::Which::SmuggleReq(
                                    tango_protos::ipc::to_core_message::SmuggleRequest { data },
                                )) => {
                                    dc_tx.send(
                                        &tango_core::protocol::Packet::Smuggle(
                                            tango_core::protocol::Smuggle { data },
                                        )
                                        .serialize()?,
                                    )
                                    .await?;
                                }
                                Some(tango_protos::ipc::to_core_message::Which::StartReq(
                                    start_req,
                                )) => {
                                    return Ok(start_req);
                                }
                                None => {
                                    anyhow::bail!("ipc channel closed");
                                }
                            }
                        }
                    } => {
                        Ok((r?, dc_tx))
                    },
                    r = async {
                        loop {
                            let msg = dc_rx.lock().await.as_mut().unwrap().receive().await;
                            match msg {
                                Some(msg) => {
                                    match tango_core::protocol::Packet::deserialize(&msg)? {
                                        tango_core::protocol::Packet::Smuggle(tango_core::protocol::Smuggle {
                                            data,
                                        }) => {
                                            ipc_sender.send(tango_protos::ipc::FromCoreMessage {
                                                which: Some(tango_protos::ipc::from_core_message::Which::SmuggleInd(tango_protos::ipc::from_core_message::SmuggleIndication {
                                                    data,
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
                    } => {
                        r
                    }
                }?
            };

            log::info!("start request = {:?}", start_req);

            let mut dc_rx = dc_rx.lock().await;
            let dc_rx = dc_rx.take().unwrap();
            Ok::<_, anyhow::Error>((
                start_req.window_title,
                start_req.rom_path,
                start_req.save_path,
                Some((peer_conn, dc_tx.unsplit(dc_rx), start_req.settings.unwrap())),
            ))
        })?
    } else {
        rt.block_on(async {
            ipc_sender
                .send(tango_protos::ipc::FromCoreMessage {
                    which: Some(tango_protos::ipc::from_core_message::Which::StateInd(
                        tango_protos::ipc::from_core_message::StateIndication {
                            state:
                                tango_protos::ipc::from_core_message::state_indication::State::ReadyToStart
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
                    replay_metadata: settings.replay_metadata.into(),
                    replays_path: settings.replays_path.into(),
                    shadow_save_path: settings.shadow_save_path.into(),
                    shadow_rom_path: settings.shadow_rom_path.into(),
                    match_type: settings.match_type as u16,
                    input_delay: settings.input_delay,
                    rng_seed: settings.rng_seed,
                },
            }),
        },
    )?;
    g.run()?;
    Ok(())
}

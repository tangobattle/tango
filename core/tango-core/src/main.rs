#![windows_subsystem = "windows"]

use clap::StructOpt;
use tango_core::ipc::protos::ExitCode;

#[derive(clap::Parser)]
struct Cli {
    #[clap(long)]
    input_mapping: String,

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
        .filter(Some("mgba"), log::LevelFilter::Info)
        .init();

    log::info!("welcome to tango-core {}!", git_version::git_version!());

    let args = Cli::parse();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let mut ipc_sender = tango_core::ipc::Sender::new_from_stdout();
    let mut ipc_receiver = tango_core::ipc::Receiver::new_from_stdin();

    // let (start_req, pvp_init) = if let Some(session_id) = &args.session_id {
    //     rt.block_on(async {
    //         let (dc, peer_conn) = match tango_core::net::negotiate(
    //             &mut ipc_sender,
    //             session_id,
    //             &args.signaling_connect_addr,
    //             &args.ice_servers,
    //         )
    //         .await {
    //             Ok(v) => v,
    //             Err(err) => {
    //                 match err {
    //                     tango_core::net::Error::ExpectedHello => {
    //                         return Err(err.into());
    //                     }
    //                     tango_core::net::Error::ProtocolVersionTooOld => {
    //                         std::process::exit(ExitCode::ProtocolVersionTooOld as i32);
    //                     }
    //                     tango_core::net::Error::ProtocolVersionTooNew => {
    //                         std::process::exit(ExitCode::ProtocolVersionTooNew as i32);
    //                     }
    //                     tango_core::net::Error::Other(_) => {
    //                         return Err(err.into());
    //                     }
    //                 }
    //             }
    //         };

    //         let (mut dc_tx, mut dc_rx) = dc.split();

    //         let mut ping_timer = tokio::time::interval(std::time::Duration::from_secs(1));
    //         let mut hola_received = false;

    //         let start_req = loop {
    //             tokio::select! {
    //                 msg = ipc_receiver.receive() => {
    //                     match msg?.which {
    //                         Some(tango_core::ipc::protos::to_core_message::Which::SmuggleReq(tango_core::ipc::protos::to_core_message::SmuggleRequest { data })) => {
    //                             dc_tx.send(&tango_core::protocol::Packet::Smuggle(tango_core::protocol::Smuggle {
    //                                 data,
    //                             }).serialize()?).await?;
    //                         },
    //                         Some(tango_core::ipc::protos::to_core_message::Which::StartReq(start_req)) => {
    //                             dc_tx.send(&tango_core::protocol::Packet::Hola(tango_core::protocol::Hola {}).serialize()?).await?;
    //                             break start_req;
    //                         },
    //                         None => {
    //                             anyhow::bail!("ipc channel closed");
    //                         },
    //                     }
    //                 }

    //                 _ = ping_timer.tick() => {
    //                     let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
    //                     dc_tx.send(&tango_core::protocol::Packet::Ping(tango_core::protocol::Ping {
    //                         ts: now.as_nanos() as u64,
    //                     }).serialize()?).await?;
    //                 }

    //                 msg = dc_rx.receive() => {
    //                     match msg {
    //                         Some(msg) => {
    //                             match tango_core::protocol::Packet::deserialize(&msg)? {
    //                                 tango_core::protocol::Packet::Hola(_) => {
    //                                     hola_received = true;
    //                                 }
    //                                 tango_core::protocol::Packet::Smuggle(tango_core::protocol::Smuggle {
    //                                     data,
    //                                 }) => {
    //                                     ipc_sender.send(tango_core::ipc::protos::FromCoreMessage {
    //                                         which: Some(tango_core::ipc::protos::from_core_message::Which::SmuggleEv(tango_core::ipc::protos::from_core_message::SmuggleEvent {
    //                                             data,
    //                                         }))
    //                                     }).await?;
    //                                 },
    //                                 tango_core::protocol::Packet::Ping(tango_core::protocol::Ping {
    //                                     ts
    //                                 }) => {
    //                                     dc_tx.send(&tango_core::protocol::Packet::Pong(tango_core::protocol::Pong {
    //                                         ts
    //                                     }).serialize()?).await?;
    //                                 },
    //                                 tango_core::protocol::Packet::Pong(tango_core::protocol::Pong {
    //                                     ts
    //                                 }) => {
    //                                     let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
    //                                     let then = std::time::Duration::from_nanos(ts);
    //                                     ipc_sender.send(tango_core::ipc::protos::FromCoreMessage {
    //                                         which: Some(tango_core::ipc::protos::from_core_message::Which::ConnectionQualityEv(tango_core::ipc::protos::from_core_message::ConnectionQualityEvent {
    //                                             rtt: (now - then).as_nanos() as u64,
    //                                         }))
    //                                     }).await?;
    //                                 },
    //                                 p => {
    //                                     anyhow::bail!("unexpected packet: {:?}", p);
    //                                 }
    //                             }
    //                         },
    //                         None => {
    //                             std::process::exit(ExitCode::LostConnection as i32);
    //                         },
    //                     }
    //                 }
    //             }
    //         };

    //         if !hola_received {
    //             // If we haven't received an Hola, pull packets until we do.
    //             loop {
    //                 match dc_rx.receive().await {
    //                     Some(msg) => {
    //                         match tango_core::protocol::Packet::deserialize(&msg)? {
    //                             tango_core::protocol::Packet::Hola(_) => {
    //                                 break;
    //                             }
    //                             tango_core::protocol::Packet::Ping(_) => {
    //                                 // Ignore stray pings.
    //                             }
    //                             tango_core::protocol::Packet::Pong(_) => {
    //                                 // Ignore stray pongs.
    //                             }
    //                             p => {
    //                                 anyhow::bail!("unexpected packet: {:?}", p);
    //                             }
    //                         }
    //                     }
    //                     None => {
    //                         std::process::exit(ExitCode::LostConnection as i32);
    //                     },
    //                 }
    //             }
    //         }

    //         let settings = start_req.settings.clone().unwrap();
    //         Ok((
    //             start_req,
    //             Some((peer_conn, dc_rx.unsplit(dc_tx), settings))
    //         ))
    //     })?
    // } else {
    let start_req = rt.block_on(async {
        ipc_sender
            .send(tango_core::ipc::protos::FromCoreMessage {
                which: Some(tango_core::ipc::protos::from_core_message::Which::StateEv(
                    tango_core::ipc::protos::from_core_message::StateEvent {
                        state:
                            tango_core::ipc::protos::from_core_message::state_event::State::Starting
                                .into(),
                    },
                )),
            })
            .await?;

        let msg = ipc_receiver.receive().await;
        match msg?.which {
            Some(tango_core::ipc::protos::to_core_message::Which::StartReq(start_req)) => {
                Ok(start_req)
            }
            Some(p) => {
                anyhow::bail!("unexpected ipc request: {:?}", p);
            }
            None => {
                anyhow::bail!("ipc channel closed");
            }
        }
    })?;
    // };

    mgba::log::init();

    tango_core::game::run(
        rt,
        std::sync::Arc::new(parking_lot::Mutex::new(ipc_sender)),
        start_req.rom_path.into(),
        start_req.save_path.into(),
        None,
        // match pvp_init {
        //     None => None,
        //     Some((peer_conn, dc, settings)) => Some(tango_core::battle::MatchInit {
        //         dc,
        //         peer_conn,
        //         settings: tango_core::battle::Settings {
        //             replay_metadata: settings.replay_metadata,
        //             replays_path: settings.replays_path.into(),
        //             shadow_save_path: settings.shadow_save_path.into(),
        //             shadow_rom_path: settings.shadow_rom_path.into(),
        //             match_type: (settings.match_type as u8, settings.match_subtype as u8),
        //             input_delay: settings.input_delay,
        //             rng_seed: settings.rng_seed,
        //             opponent_nickname: settings.opponent_nickname,
        //             max_queue_length: settings.max_queue_length as usize,
        //         },
        //     }),
        // },
    )?;
    Ok(())
}

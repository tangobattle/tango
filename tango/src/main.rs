#![windows_subsystem = "windows"]

fn main() -> Result<(), anyhow::Error> {
    env_logger::Builder::from_default_env()
        .filter(Some("tango_core"), log::LevelFilter::Info)
        .filter(Some("datachannel"), log::LevelFilter::Info)
        .filter(Some("mgba"), log::LevelFilter::Info)
        .init();

    log::info!("welcome to tango-core {}!", git_version::git_version!());

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    mgba::log::init();

    tango_core::game::run(
        rt,
        // std::sync::Arc::new(parking_lot::Mutex::new(ipc_sender)),
        // start_req.rom_path.into(),
        // start_req.save_path.into(),
        // None,
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

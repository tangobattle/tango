use fluent_templates::Loader;
use rand::RngCore;
use sha3::digest::{ExtendableOutput, Update};
use subtle::ConstantTimeEq;

use crate::{audio, config, game, gui, net, patch, session, stats};

struct Lobby {
    attention_requested: bool,
    sender: Option<net::Sender>,
    selection: std::sync::Arc<parking_lot::Mutex<Option<gui::Selection>>>,
    nickname: String,
    match_type: (u8, u8),
    reveal_setup: bool,
    remote_rom: Option<Vec<u8>>,
    remote_settings: net::protocol::Settings,
    remote_commitment: Option<[u8; 16]>,
    latencies: stats::DeltaCounter,
    local_negotiated_state: Option<(net::protocol::NegotiatedState, Vec<u8>)>,
    roms_scanner: gui::ROMsScanner,
    patches_scanner: gui::PatchesScanner,
}

fn are_settings_compatible(
    local_settings: &net::protocol::Settings,
    remote_settings: &net::protocol::Settings,
    roms: &std::collections::HashMap<&'static (dyn game::Game + Send + Sync), Vec<u8>>,
    patches: &std::collections::BTreeMap<String, patch::Patch>,
) -> bool {
    let local_game_info = if let Some(gi) = local_settings.game_info.as_ref() {
        gi
    } else {
        return false;
    };

    let remote_game_info = if let Some(gi) = remote_settings.game_info.as_ref() {
        gi
    } else {
        return false;
    };

    if !remote_settings
        .available_games
        .iter()
        .any(|g| g == &local_game_info.family_and_variant)
    {
        return false;
    }

    if !local_settings
        .available_games
        .iter()
        .any(|g| g == &remote_game_info.family_and_variant)
    {
        return false;
    }

    if let Some(patch) = local_game_info.patch.as_ref() {
        if !remote_settings
            .available_patches
            .iter()
            .any(|(pn, pvs)| pn == &patch.name && pvs.contains(&patch.version))
        {
            return false;
        }
    }

    if let Some(patch) = remote_game_info.patch.as_ref() {
        if !local_settings
            .available_patches
            .iter()
            .any(|(pn, pvs)| pn == &patch.name && pvs.contains(&patch.version))
        {
            return false;
        }
    }

    #[derive(PartialEq)]
    struct SimplifiedSettings {
        netplay_compatibility: Option<String>,
        match_type: (u8, u8),
    }

    impl SimplifiedSettings {
        fn new(
            settings: &net::protocol::Settings,
            roms: &std::collections::HashMap<&'static (dyn game::Game + Send + Sync), Vec<u8>>,
            patches: &std::collections::BTreeMap<String, patch::Patch>,
        ) -> Self {
            Self {
                netplay_compatibility: settings.game_info.as_ref().and_then(|g| {
                    if game::find_by_family_and_variant(
                        g.family_and_variant.0.as_str(),
                        g.family_and_variant.1,
                    )
                    .map(|g| roms.contains_key(&g))
                    .unwrap_or(false)
                    {
                        if let Some(patch) = g.patch.as_ref() {
                            patches.get(&patch.name).and_then(|p| {
                                p.versions
                                    .get(&patch.version)
                                    .map(|vinfo| vinfo.netplay_compatibility.clone())
                            })
                        } else {
                            Some(g.family_and_variant.0.clone())
                        }
                    } else {
                        None
                    }
                }),
                match_type: settings.match_type,
            }
        }
    }

    let local_simplified_settings = SimplifiedSettings::new(&local_settings, roms, patches);
    let remote_simplified_settings = SimplifiedSettings::new(&remote_settings, roms, patches);

    local_simplified_settings.netplay_compatibility.is_some()
        && remote_simplified_settings.netplay_compatibility.is_some()
        && local_simplified_settings == remote_simplified_settings
}

fn make_commitment(buf: &[u8]) -> [u8; 16] {
    let mut shake128 = sha3::Shake128::default();
    shake128.update(b"tango:lobby:");
    shake128.update(buf);
    let mut commitment = [0u8; 16];
    shake128.finalize_xof_into(&mut commitment);
    commitment
}

impl Lobby {
    async fn uncommit(&mut self) -> Result<(), anyhow::Error> {
        let sender = if let Some(sender) = self.sender.as_mut() {
            sender
        } else {
            anyhow::bail!("no sender?")
        };

        sender.send_uncommit().await?;
        self.local_negotiated_state = None;
        Ok(())
    }

    async fn commit(&mut self, save_data: &[u8]) -> Result<(), anyhow::Error> {
        let mut nonce = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut nonce);
        let negotiated_state = net::protocol::NegotiatedState {
            nonce: nonce.clone(),
            save_data: save_data.to_vec(),
        };
        let buf = zstd::stream::encode_all(
            &net::protocol::NegotiatedState::serialize(&negotiated_state).unwrap()[..],
            0,
        )?;
        let commitment = make_commitment(&buf);

        log::info!("nonce = {:02x?}, commitment = {:02x?}", nonce, commitment);

        let sender = if let Some(sender) = self.sender.as_mut() {
            sender
        } else {
            anyhow::bail!("no sender?")
        };
        sender.send_commit(commitment).await?;
        self.local_negotiated_state = Some((negotiated_state, buf));
        Ok(())
    }

    fn make_local_settings(&self) -> net::protocol::Settings {
        let roms = self.roms_scanner.read();
        let patches = self.patches_scanner.read();

        net::protocol::Settings {
            nickname: self.nickname.clone(),
            match_type: self.match_type,
            game_info: self.selection.lock().as_ref().map(|selection| {
                let (family, variant) = selection.game.family_and_variant();
                net::protocol::GameInfo {
                    family_and_variant: (family.to_string(), variant),
                    patch: selection.patch.as_ref().map(|(name, version, _)| {
                        net::protocol::PatchInfo {
                            name: name.clone(),
                            version: version.clone(),
                        }
                    }),
                }
            }),
            available_games: roms
                .keys()
                .map(|g| {
                    let (family, variant) = g.family_and_variant();
                    (family.to_string(), variant)
                })
                .collect(),
            available_patches: patches
                .iter()
                .map(|(p, info)| (p.clone(), info.versions.keys().cloned().collect()))
                .collect(),
            reveal_setup: self.reveal_setup,
        }
    }

    async fn send_settings(
        &mut self,
        settings: net::protocol::Settings,
    ) -> Result<(), anyhow::Error> {
        let sender = if let Some(sender) = self.sender.as_mut() {
            sender
        } else {
            anyhow::bail!("no sender?")
        };
        sender.send_settings(settings).await?;
        Ok(())
    }

    async fn set_reveal_setup(&mut self, reveal_setup: bool) -> Result<(), anyhow::Error> {
        if reveal_setup == self.reveal_setup {
            return Ok(());
        }
        self.send_settings(net::protocol::Settings {
            reveal_setup,
            ..self.make_local_settings()
        })
        .await?;
        self.reveal_setup = reveal_setup;
        if !self.reveal_setup {
            self.remote_commitment = None;
        }
        Ok(())
    }

    async fn set_match_type(&mut self, match_type: (u8, u8)) -> Result<(), anyhow::Error> {
        if match_type == self.match_type {
            return Ok(());
        }
        self.send_settings(net::protocol::Settings {
            match_type,
            ..self.make_local_settings()
        })
        .await?;
        self.match_type = match_type;
        Ok(())
    }

    fn set_remote_settings(
        &mut self,
        settings: net::protocol::Settings,
        patches_path: &std::path::Path,
    ) {
        let roms = self.roms_scanner.read();
        let patches = self.patches_scanner.read();

        let old_reveal_setup = self.remote_settings.reveal_setup;
        self.remote_rom = settings.game_info.as_ref().and_then(|gi| {
            game::find_by_family_and_variant(&gi.family_and_variant.0, gi.family_and_variant.1)
                .and_then(|game| {
                    roms.get(&game).and_then(|rom| {
                        if let Some(pi) = gi.patch.as_ref() {
                            let (rom_code, revision) = game.rom_code_and_revision();

                            let bps = match std::fs::read(
                                patches_path
                                    .join(&pi.name)
                                    .join(format!("v{}", pi.version))
                                    .join(format!(
                                        "{}_{:02}.bps",
                                        std::str::from_utf8(rom_code).unwrap(),
                                        revision
                                    )),
                            ) {
                                Ok(bps) => bps,
                                Err(e) => {
                                    log::error!(
                                        "failed to load patch {} to {:?}: {:?}",
                                        pi.name,
                                        (rom_code, revision),
                                        e
                                    );
                                    return None;
                                }
                            };

                            let rom = match patch::bps::apply(&rom, &bps) {
                                Ok(r) => r.to_vec(),
                                Err(e) => {
                                    log::error!(
                                        "failed to apply patch {} to {:?}: {:?}",
                                        pi.name,
                                        (rom_code, revision),
                                        e
                                    );
                                    return None;
                                }
                            };

                            Some(rom)
                        } else {
                            Some(rom.clone())
                        }
                    })
                })
        });

        self.remote_settings = settings;
        if !are_settings_compatible(
            &self.make_local_settings(),
            &self.remote_settings,
            &roms,
            &patches,
        ) || (old_reveal_setup && !self.remote_settings.reveal_setup)
        {
            self.local_negotiated_state = None;
        }
    }

    async fn send_pong(&mut self, ts: std::time::SystemTime) -> Result<(), anyhow::Error> {
        let sender = if let Some(sender) = self.sender.as_mut() {
            sender
        } else {
            anyhow::bail!("no sender?")
        };
        sender.send_pong(ts).await?;
        Ok(())
    }

    async fn send_ping(&mut self) -> Result<(), anyhow::Error> {
        let sender = if let Some(sender) = self.sender.as_mut() {
            sender
        } else {
            anyhow::bail!("no sender?")
        };
        sender.send_ping(std::time::SystemTime::now()).await?;
        Ok(())
    }
}

async fn run_connection_task(
    config: std::sync::Arc<parking_lot::RwLock<config::Config>>,
    handle: tokio::runtime::Handle,
    egui_ctx: egui::Context,
    audio_binder: audio::LateBinder,
    emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    session: std::sync::Arc<parking_lot::Mutex<Option<session::Session>>>,
    selection: std::sync::Arc<parking_lot::Mutex<Option<gui::Selection>>>,
    roms_scanner: gui::ROMsScanner,
    patches_scanner: gui::PatchesScanner,
    matchmaking_addr: String,
    link_code: String,
    nickname: String,
    patches_path: std::path::PathBuf,
    replays_path: std::path::PathBuf,
    connection_task: std::sync::Arc<tokio::sync::Mutex<Option<ConnectionTask>>>,
    cancellation_token: tokio_util::sync::CancellationToken,
) {
    if let Err(e) = {
        let connection_task = connection_task.clone();

        tokio::select! {
            r = {
                let connection_task = connection_task.clone();
                let cancellation_token = cancellation_token.clone();
                (move || async move {
                    *connection_task.lock().await =
                        Some(ConnectionTask::InProgress {
                            state: ConnectionState::Signaling,
                            cancellation_token:
                                cancellation_token.clone(),
                        });
                    const OPEN_TIMEOUT: std::time::Duration =
                        std::time::Duration::from_secs(30);
                    let pending_conn = tokio::time::timeout(
                        OPEN_TIMEOUT,
                        net::signaling::open(
                            &matchmaking_addr,
                            &link_code,
                        ),
                    )
                    .await??;

                    *connection_task.lock().await =
                        Some(ConnectionTask::InProgress {
                            state: ConnectionState::Waiting,
                            cancellation_token:
                                cancellation_token.clone(),
                        });

                    let (dc, peer_conn) = pending_conn.connect().await?;
                    let (dc_tx, dc_rx) = dc.split();
                    let mut sender = net::Sender::new(dc_tx);
                    let mut receiver = net::Receiver::new(dc_rx);
                    net::negotiate(&mut sender, &mut receiver).await?;

                    let default_match_type = {
                        let config = config.read();
                        config.default_match_type
                    };

                    let lobby = std::sync::Arc::new(tokio::sync::Mutex::new(Lobby{
                        attention_requested: false,
                        sender: Some(sender),
                        selection: selection.clone(),
                        nickname,
                        match_type: (if selection.lock().as_ref().map(|selection| (default_match_type as usize) < selection.game.match_types().len()).unwrap_or(false) {
                            default_match_type
                        } else {
                            0
                        }, 0),
                        reveal_setup: false,
                        remote_rom: None,
                        remote_settings: net::protocol::Settings::default(),
                        remote_commitment: None,
                        latencies: stats::DeltaCounter::new(10),
                        local_negotiated_state: None,
                        roms_scanner: roms_scanner.clone(),
                        patches_scanner: patches_scanner.clone(),
                    }));
                    {
                        let mut lobby = lobby.lock().await;
                        let settings = lobby.make_local_settings();
                        lobby.send_settings(settings).await?;
                    }

                    *connection_task.lock().await =
                        Some(ConnectionTask::InProgress {
                            state: ConnectionState::InLobby(lobby.clone()),
                            cancellation_token:
                                cancellation_token.clone(),
                        });

                    let mut remote_chunks = vec![];
                    let mut ping_timer = tokio::time::interval(net::PING_INTERVAL);
                    'l: loop {
                        tokio::select! {
                            _ = ping_timer.tick() => {
                                lobby.lock().await.send_ping().await?;
                            }
                            p = receiver.receive() => {
                                match p? {
                                    net::protocol::Packet::Ping(ping) => {
                                        lobby.lock().await.send_pong(ping.ts).await?;
                                    },
                                    net::protocol::Packet::Pong(pong) => {
                                        let mut lobby = lobby.lock().await;
                                        if let Ok(d) = std::time::SystemTime::now().duration_since(pong.ts) {
                                            lobby.latencies.mark(d);
                                            egui_ctx.request_repaint();
                                        }
                                    },
                                    net::protocol::Packet::Settings(settings) => {
                                        let mut lobby = lobby.lock().await;
                                        lobby.set_remote_settings(settings, &patches_path);
                                        egui_ctx.request_repaint();
                                    },
                                    net::protocol::Packet::Commit(commit) => {
                                        let mut lobby = lobby.lock().await;
                                        lobby.remote_commitment = Some(commit.commitment);
                                        egui_ctx.request_repaint();

                                        if lobby.local_negotiated_state.is_some() {
                                            break 'l;
                                        }
                                    },
                                    net::protocol::Packet::Uncommit(_) => {
                                        lobby.lock().await.remote_commitment = None;
                                        egui_ctx.request_repaint();
                                    },
                                    net::protocol::Packet::Chunk(chunk) => {
                                        remote_chunks.push(chunk.chunk);
                                        break 'l;
                                    },
                                    p => {
                                        anyhow::bail!("unexpected packet: {:?}", p);
                                    }
                                }
                            }
                        }
                    }

                    log::info!("ending lobby");

                    let (mut sender, match_type, local_settings, mut remote_rom, remote_settings, remote_commitment, local_negotiated_state) = {
                        let mut lobby = lobby.lock().await;
                        let local_settings = lobby.make_local_settings();
                        let sender = if let Some(sender) = lobby.sender.take() {
                            sender
                        } else {
                            anyhow::bail!("no sender?");
                        };
                        (sender, lobby.match_type, local_settings, lobby.remote_rom.clone(), lobby.remote_settings.clone(), lobby.remote_commitment.clone(), lobby.local_negotiated_state.take())
                    };

                    let remote_rom = if let Some(remote_rom) = remote_rom.take() {
                        remote_rom
                    } else {
                        anyhow::bail!("missing shadow rom");
                    };

                    let (local_negotiated_state, raw_local_state) = if let Some((negotiated_state, raw_local_state)) = local_negotiated_state {
                        (negotiated_state, raw_local_state)
                    } else {
                        anyhow::bail!("attempted to start match in invalid state");
                    };

                    const CHUNK_SIZE: usize = 32 * 1024;
                    const CHUNKS_REQUIRED: usize = 5;
                    for (_, chunk) in std::iter::zip(
                        0..CHUNKS_REQUIRED,
                        raw_local_state.chunks(CHUNK_SIZE).chain(std::iter::repeat(&[][..]))
                     ) {
                        sender.send_chunk(chunk.to_vec()).await?;

                        if remote_chunks.len() < CHUNKS_REQUIRED {
                            loop {
                                match receiver.receive().await? {
                                    net::protocol::Packet::Ping(ping) => {
                                        sender.send_pong(ping.ts).await?;
                                    },
                                    net::protocol::Packet::Pong(_) => { },
                                    net::protocol::Packet::Chunk(chunk) => {
                                        remote_chunks.push(chunk.chunk);
                                        break;
                                    },
                                    p => {
                                        anyhow::bail!("unexpected packet: {:?}", p);
                                    }
                                }
                            }
                        }
                    }

                    let raw_remote_negotiated_state = remote_chunks.into_iter().flatten().collect::<Vec<_>>();

                    let received_remote_commitment = if let Some(commitment) = remote_commitment {
                        commitment
                    } else {
                        anyhow::bail!("no remote commitment?");
                    };

                    log::info!("remote commitment = {:02x?}", received_remote_commitment);

                    if !bool::from(make_commitment(&raw_remote_negotiated_state).ct_eq(&received_remote_commitment)) {
                        anyhow::bail!("commitment did not match");
                    }

                    let remote_negotiated_state = zstd::stream::decode_all(&raw_remote_negotiated_state[..]).map_err(|e| e.into()).and_then(|r| net::protocol::NegotiatedState::deserialize(&r))?;

                    let rng_seed = std::iter::zip(local_negotiated_state.nonce, remote_negotiated_state.nonce).map(|(x, y)| x ^ y).collect::<Vec<_>>().try_into().unwrap();
                    log::info!("session verified! rng seed = {:02x?}", rng_seed);

                    let (local_game, local_rom, patch) = if let Some(selection) = selection.lock().as_ref() {
                        (selection.game, selection.rom.clone(), selection.patch.clone())
                    } else {
                        anyhow::bail!("attempted to start match in invalid state");
                    };

                    sender.send_start_match().await?;
                    match receiver.receive().await? {
                        net::protocol::Packet::StartMatch(_) => {},
                        p => anyhow::bail!("unexpected packet when expecting start match: {:?}", p),
                    }

                    log::info!("starting session");
                    let is_offerer = peer_conn.local_description().unwrap().sdp_type == datachannel_wrapper::SdpType::Offer;
                    {
                        *session.lock() = Some(session::Session::new_pvp(
                            config.clone(),
                            handle,
                            audio_binder,
                            link_code,
                            patch
                                .and_then(|(patch_name, patch_version, _)| {
                                    patches_scanner // TODO: Avoid having to read the patches scanner here.
                                        .read()
                                        .get(&patch_name)
                                        .and_then(|p| p.versions.get(&patch_version))
                                        .as_ref()
                                        .map(|v| v.netplay_compatibility.clone())
                                })
                                .unwrap_or(local_game.family_and_variant().0.to_owned()),
                            local_settings,
                            local_game,
                            &local_rom,
                            &local_negotiated_state.save_data,
                            remote_settings,
                            &remote_rom,
                            &remote_negotiated_state.save_data,
                            emu_tps_counter.clone(),
                            sender,
                            receiver,
                            peer_conn,
                            is_offerer,
                            replays_path,
                            match_type,
                            rng_seed,
                        )?);
                    }
                    egui_ctx.request_repaint();
                    *connection_task.lock().await = None;

                    Ok(())
                })(
                )
            }
            => {
                r
            }
            _ = cancellation_token.cancelled() => {
                Ok(())
            }
        }
    } {
        log::info!("connection task failed: {:?}", e);
        *connection_task.lock().await = Some(ConnectionTask::Failed(e));
    } else {
        *connection_task.lock().await = None;
    }
}

enum ConnectionTask {
    InProgress {
        state: ConnectionState,
        cancellation_token: tokio_util::sync::CancellationToken,
    },
    Failed(anyhow::Error),
}

enum ConnectionState {
    Starting,
    Signaling,
    Waiting,
    InLobby(std::sync::Arc<tokio::sync::Mutex<Lobby>>),
}

pub struct State {
    link_code: String,
    connection_task: std::sync::Arc<tokio::sync::Mutex<Option<ConnectionTask>>>,
    show_save_select: Option<gui::save_select_window::State>,
}

impl State {
    pub fn new() -> Self {
        Self {
            link_code: String::new(),
            connection_task: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            show_save_select: None,
        }
    }
}

pub struct PlayPane {
    save_view: gui::save_view::SaveView,
}

impl PlayPane {
    pub fn new() -> Self {
        Self {
            save_view: gui::save_view::SaveView::new(),
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        selection: std::sync::Arc<parking_lot::Mutex<Option<gui::Selection>>>,
        font_families: &gui::FontFamilies,
        clipboard: &mut arboard::Clipboard,
        config: &config::Config,
    ) {
        let mut selection = selection.lock();
        if let Some(selection) = &mut *selection {
            if let Some(assets) = selection.assets.as_ref() {
                let game_language = selection.game.language();
                self.save_view.show(
                    ui,
                    clipboard,
                    font_families,
                    &config.language,
                    if let Some((_, _, metadata)) = selection.patch.as_ref() {
                        if let Some(language) = metadata.saveedit_overrides.language.as_ref() {
                            language
                        } else {
                            &game_language
                        }
                    } else {
                        &game_language
                    },
                    &selection.save.save,
                    assets,
                    &mut selection.save_view_state,
                );
            }
        }
    }
}

use fluent_templates::Loader;
use rand::RngCore;
use sha3::digest::{ExtendableOutput, Update};
use subtle::ConstantTimeEq;

use crate::{
    audio, config, discord, game, gui, i18n, net, patch, randomcode, rom, save, session, stats,
};

struct LobbySelection {
    pub game: &'static (dyn game::Game + Send + Sync),
    pub save: Box<dyn save::Save + Send + Sync>,
    pub rom: Vec<u8>,
    pub patch: Option<(String, semver::Version, patch::Version)>,
}

struct Lobby {
    attention_requested: bool,
    sender: Option<net::Sender>,
    selection: Option<LobbySelection>,
    nickname: String,
    match_type: (u8, u8),
    reveal_setup: bool,
    remote_rom: Option<Vec<u8>>,
    remote_settings: net::protocol::Settings,
    remote_commitment: Option<[u8; 16]>,
    latencies: stats::DeltaCounter,
    local_negotiated_state: Option<(net::protocol::NegotiatedState, Vec<u8>)>,
    roms_scanner: rom::Scanner,
    patches_scanner: patch::Scanner,
}

fn get_netplay_compatibility(
    game: &'static (dyn game::Game + Send + Sync),
    patch: Option<(String, semver::Version)>,
    patches: &std::collections::BTreeMap<String, patch::Patch>,
) -> Option<String> {
    if let Some(patch) = patch.as_ref() {
        patches.get(&patch.0).and_then(|p| {
            p.versions
                .get(&patch.1)
                .map(|vinfo| vinfo.netplay_compatibility.clone())
        })
    } else {
        Some(game.family_and_variant().0.to_string())
    }
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
                    if let Some(game) = game::find_by_family_and_variant(
                        g.family_and_variant.0.as_str(),
                        g.family_and_variant.1,
                    ) {
                        if roms.contains_key(&game) {
                            get_netplay_compatibility(
                                game,
                                g.patch
                                    .as_ref()
                                    .map(|pi| (pi.name.to_string(), pi.version.clone())),
                                patches,
                            )
                        } else {
                            None
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
            game_info: self.selection.as_ref().map(|selection| {
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

    async fn set_local_selection(
        &mut self,
        selection: &Option<gui::Selection>,
    ) -> Result<(), anyhow::Error> {
        if selection.as_ref().map(|selection| {
            (
                selection.game,
                selection
                    .patch
                    .as_ref()
                    .map(|(name, version, _)| (name.clone(), version.clone())),
            )
        }) == self.selection.as_ref().map(|selection| {
            (
                selection.game,
                selection
                    .patch
                    .as_ref()
                    .map(|(name, version, _)| (name.clone(), version.clone())),
            )
        }) {
            return Ok(());
        }

        let match_type = (
            if selection
                .as_ref()
                .map(|selection| (self.match_type.0 as usize) < selection.game.match_types().len())
                .unwrap_or(false)
            {
                self.match_type.0
            } else {
                0
            },
            0,
        );

        self.send_settings(net::protocol::Settings {
            game_info: selection.as_ref().map(|selection| {
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
            match_type,
            ..self.make_local_settings()
        })
        .await?;
        self.selection = if let Some(selection) = selection.as_ref() {
            Some(LobbySelection {
                game: selection.game,
                save: selection.save.save.clone(),
                rom: selection.rom.clone(),
                patch: selection.patch.clone(),
            })
        } else {
            None
        };
        self.match_type = match_type;
        if !self.can_ready() {
            self.remote_commitment = None;
        }
        Ok(())
    }

    fn can_ready(&self) -> bool {
        are_settings_compatible(
            &self.make_local_settings(),
            &self.remote_settings,
            &self.roms_scanner.read(),
            &self.patches_scanner.read(),
        )
    }

    fn set_remote_settings(
        &mut self,
        settings: net::protocol::Settings,
        patches_path: &std::path::Path,
    ) {
        let roms = self.roms_scanner.read();

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
        if !self.can_ready() || (old_reveal_setup && !self.remote_settings.reveal_setup) {
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
    roms_scanner: rom::Scanner,
    patches_scanner: patch::Scanner,
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
                        selection: None,
                        nickname,
                        match_type: (default_match_type, 0),
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

                    let (mut sender, match_type, local_settings, mut remote_rom, remote_settings, remote_commitment, local_negotiated_state, selection) = {
                        let mut lobby = lobby.lock().await;
                        let local_settings = lobby.make_local_settings();
                        let sender = if let Some(sender) = lobby.sender.take() {
                            sender
                        } else {
                            anyhow::bail!("no sender?");
                        };
                        (sender, lobby.match_type, local_settings, lobby.remote_rom.clone(), lobby.remote_settings.clone(), lobby.remote_commitment.clone(), lobby.local_negotiated_state.take(), lobby.selection.take())
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

                    let selection = if let Some(selection) = selection {
                        selection
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
                            selection.patch
                                .map(|(_, _, metadata)| metadata.netplay_compatibility.clone())
                                .unwrap_or(selection.game.family_and_variant().0.to_owned()),
                            local_settings,
                            selection.game,
                            &selection.rom,
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
    show_save_select: Option<gui::save_select_view::State>,
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

fn show_lobby_table(
    ui: &mut egui::Ui,
    handle: tokio::runtime::Handle,
    cancellation_token: &tokio_util::sync::CancellationToken,
    config: &mut config::Config,
    lobby: &mut Lobby,
    roms: &std::collections::HashMap<&'static (dyn game::Game + Send + Sync), Vec<u8>>,
    patches: &std::collections::BTreeMap<String, patch::Patch>,
) {
    egui_extras::TableBuilder::new(ui)
        .column(egui_extras::Size::remainder())
        .column(egui_extras::Size::exact(200.0))
        .column(egui_extras::Size::exact(200.0))
        .header(20.0, |mut header| {
            header.col(|_ui| {});
            header.col(|ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        if ui
                            .button(format!(
                                "🚶 {}",
                                i18n::LOCALES
                                    .lookup(&config.language, "play-leave")
                                    .unwrap()
                            ))
                            .clicked()
                        {
                            cancellation_token.cancel();
                        }

                        ui.horizontal_top(|ui| {
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                                ui.set_width(ui.available_width());
                                ui.strong(
                                    i18n::LOCALES.lookup(&config.language, "play-you").unwrap(),
                                );
                                if lobby.local_negotiated_state.is_some() || lobby.sender.is_none()
                                {
                                    ui.label(
                                        egui::RichText::new("✅")
                                            .color(egui::Color32::from_rgb(0x4c, 0xaf, 0x50)),
                                    );
                                }
                            });
                        });
                    });
                });
            });
            header.col(|ui| {
                ui.horizontal(|ui| {
                    ui.strong(lobby.remote_settings.nickname.clone());
                    if lobby.remote_commitment.is_some() {
                        ui.label(
                            egui::RichText::new("✅")
                                .color(egui::Color32::from_rgb(0x4c, 0xaf, 0x50)),
                        );
                    }
                });
            });
        })
        .body(|mut body| {
            body.row(20.0, |mut row| {
                row.col(|ui| {
                    ui.horizontal(|ui| {
                        ui.strong(
                            i18n::LOCALES
                                .lookup(&config.language, "play-details.game")
                                .unwrap(),
                        );

                        if let Some(selection) = lobby.selection.as_ref() {
                            if let Some(remote_gi) = lobby.remote_settings.game_info.as_ref() {
                                if let Some(remote_game) = game::find_by_family_and_variant(
                                    &remote_gi.family_and_variant.0,
                                    remote_gi.family_and_variant.1,
                                ) {
                                    if !roms.contains_key(&remote_game) {
                                        gui::warning::show(
                                            ui,
                                            i18n::LOCALES
                                                .lookup_with_args(
                                                    &config.language,
                                                    "lobby-issue.missing-rom",
                                                    &std::collections::HashMap::from([(
                                                        "game_name",
                                                        i18n::LOCALES
                                                            .lookup(
                                                                &config.language,
                                                                &format!(
                                                                    "game-{}.variant-{}",
                                                                    remote_game
                                                                        .family_and_variant()
                                                                        .0,
                                                                    remote_game
                                                                        .family_and_variant()
                                                                        .1
                                                                ),
                                                            )
                                                            .unwrap()
                                                            .into(),
                                                    )]),
                                                )
                                                .unwrap(),
                                        );
                                    } else if get_netplay_compatibility(
                                        selection.game,
                                        selection.patch.as_ref().map(|(name, version, _)| {
                                            (name.to_owned(), version.clone())
                                        }),
                                        patches,
                                    ) != get_netplay_compatibility(
                                        remote_game,
                                        remote_gi
                                            .patch
                                            .as_ref()
                                            .map(|pi| (pi.name.to_owned(), pi.version.clone())),
                                        patches,
                                    ) {
                                        gui::warning::show(
                                            ui,
                                            i18n::LOCALES
                                                .lookup(
                                                    &config.language,
                                                    "lobby-issue.incompatible",
                                                )
                                                .unwrap(),
                                        );
                                    }
                                } else {
                                    gui::warning::show(
                                        ui,
                                        i18n::LOCALES
                                            .lookup(
                                                &config.language,
                                                "lobby-issue.no-remote-game-selected",
                                            )
                                            .unwrap(),
                                    );
                                }
                            } else {
                                gui::warning::show(
                                    ui,
                                    i18n::LOCALES
                                        .lookup(
                                            &config.language,
                                            "lobby-issue.no-remote-game-selected",
                                        )
                                        .unwrap(),
                                );
                            }
                        } else {
                            gui::warning::show(
                                ui,
                                i18n::LOCALES
                                    .lookup(&config.language, "lobby-issue.no-local-game-selected")
                                    .unwrap(),
                            );
                        }
                    });
                });
                row.col(|ui| {
                    ui.label(if let Some(selection) = lobby.selection.as_ref() {
                        let (family, _) = selection.game.family_and_variant();
                        i18n::LOCALES
                            .lookup(
                                &config.language,
                                &format!("game-{}", family), // TODO: Show patch
                            )
                            .unwrap()
                    } else {
                        i18n::LOCALES
                            .lookup(&config.language, "play-no-game")
                            .unwrap()
                    });
                });
                row.col(|ui| {
                    ui.label(
                        if let Some(game) =
                            lobby
                                .remote_settings
                                .game_info
                                .as_ref()
                                .and_then(|game_info| {
                                    let (family, variant) = &game_info.family_and_variant;
                                    game::find_by_family_and_variant(&family, *variant)
                                })
                        {
                            let (family, _) = game.family_and_variant();
                            i18n::LOCALES
                                .lookup(
                                    &config.language,
                                    &format!("game-{}", family), // TODO: Show patch
                                )
                                .unwrap()
                        } else {
                            i18n::LOCALES
                                .lookup(&config.language, "play-no-game")
                                .unwrap()
                        },
                    );
                });
            });

            body.row(20.0, |mut row| {
                row.col(|ui| {
                    ui.horizontal(|ui| {
                        ui.strong(
                            i18n::LOCALES
                                .lookup(&config.language, "play-details.match-type")
                                .unwrap(),
                        );
                        if lobby.selection.is_some()
                            && lobby.remote_settings.game_info.is_some()
                            && lobby.match_type != lobby.remote_settings.match_type
                        {
                            gui::warning::show(
                                ui,
                                i18n::LOCALES
                                    .lookup(&config.language, "lobby-issue.match-type-mismatch")
                                    .unwrap(),
                            );
                        }
                    });
                });
                row.col(|ui| {
                    let game = lobby.selection.as_ref().map(|selection| selection.game);
                    ui.add_enabled_ui(game.is_some(), |ui| {
                        egui::ComboBox::new("start-match-type-combobox", "")
                            .width(150.0)
                            .selected_text(if let Some(game) = game.as_ref() {
                                i18n::LOCALES
                                    .lookup(
                                        &config.language,
                                        &format!(
                                            "game-{}.match-type-{}-{}",
                                            game.family_and_variant().0,
                                            lobby.match_type.0,
                                            lobby.match_type.1
                                        ),
                                    )
                                    .unwrap()
                            } else {
                                "".to_string()
                            })
                            .show_ui(ui, |ui| {
                                if let Some(game) = game {
                                    let mut match_type = lobby.match_type;
                                    for (typ, subtype_count) in
                                        game.match_types().iter().enumerate()
                                    {
                                        for subtype in 0..*subtype_count {
                                            ui.selectable_value(
                                                &mut match_type,
                                                (typ as u8, subtype as u8),
                                                i18n::LOCALES
                                                    .lookup(
                                                        &config.language,
                                                        &format!(
                                                            "game-{}.match-type-{}-{}",
                                                            game.family_and_variant().0,
                                                            typ,
                                                            subtype
                                                        ),
                                                    )
                                                    .unwrap(),
                                            );
                                        }
                                        config.default_match_type = match_type.0;
                                    }
                                    if match_type != lobby.match_type {
                                        handle.block_on(async {
                                            let _ = lobby.set_match_type(match_type).await;
                                        });
                                    }
                                }
                            });
                    });
                });
                row.col(|ui| {
                    ui.label(
                        if let Some(game_info) = lobby.remote_settings.game_info.as_ref() {
                            i18n::LOCALES
                                .lookup(
                                    &config.language,
                                    &format!(
                                        "game-{}.match-type-{}-{}",
                                        game_info.family_and_variant.0,
                                        lobby.remote_settings.match_type.0,
                                        lobby.remote_settings.match_type.1,
                                    ),
                                )
                                .unwrap()
                        } else {
                            "".to_string()
                        },
                    );
                });
            });

            body.row(20.0, |mut row| {
                row.col(|ui| {
                    ui.strong(
                        i18n::LOCALES
                            .lookup(&config.language, "play-details.reveal-setup")
                            .unwrap(),
                    );
                });
                row.col(|ui| {
                    let mut checked = lobby.reveal_setup;
                    ui.checkbox(&mut checked, "");
                    handle.block_on(async {
                        let _ = lobby.set_reveal_setup(checked).await;
                    });
                });
                row.col(|ui| {
                    ui.checkbox(&mut lobby.remote_settings.reveal_setup.clone(), "");
                });
            });
        });
}

pub fn show(
    ui: &mut egui::Ui,
    handle: tokio::runtime::Handle,
    font_families: &gui::FontFamilies,
    window: &winit::window::Window,
    clipboard: &mut arboard::Clipboard,
    config: &mut config::Config,
    config_arc: std::sync::Arc<parking_lot::RwLock<config::Config>>,
    roms_scanner: rom::Scanner,
    saves_scanner: save::Scanner,
    patches_scanner: patch::Scanner,
    audio_binder: audio::LateBinder,
    session: std::sync::Arc<parking_lot::Mutex<Option<session::Session>>>,
    selection: &mut Option<gui::Selection>,
    patch_selection: &mut Option<String>,
    emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    state: &mut State,
    discord_client: &mut discord::Client,
) {
    let mut connection_task = state.connection_task.blocking_lock();

    let roms = roms_scanner.read();
    let patches = patches_scanner.read();

    if state.show_save_select.is_none() {
        egui::TopBottomPanel::bottom("play-bottom-pane")
            .frame(egui::Frame::none())
            .show_inside(ui, |ui| {
                ui.vertical(|ui| {
                    {
                        if let Some(ConnectionTask::InProgress {
                            state: connection_state,
                            cancellation_token,
                        }) = connection_task.as_ref()
                        {
                            match connection_state {
                                ConnectionState::Starting => {
                                    ui.horizontal(|ui| {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Min),
                                            |ui| {
                                                if ui
                                                    .button(format!(
                                                        "❎ {}",
                                                        i18n::LOCALES
                                                            .lookup(&config.language, "play-cancel")
                                                            .unwrap()
                                                    ))
                                                    .clicked()
                                                {
                                                    cancellation_token.cancel();
                                                }

                                                ui.horizontal_top(|ui| {
                                                    ui.with_layout(
                                                        egui::Layout::left_to_right(
                                                            egui::Align::Min,
                                                        ),
                                                        |ui| {
                                                            ui.spinner();
                                                            ui.label(
                                                        i18n::LOCALES
                                                            .lookup(
                                                                &config.language,
                                                                "play-connection-task.starting",
                                                            )
                                                            .unwrap(),
                                                    );
                                                        },
                                                    );
                                                });
                                            },
                                        );
                                    });
                                }
                                ConnectionState::Signaling => {
                                    ui.horizontal(|ui| {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Min),
                                            |ui| {
                                                if ui
                                                    .button(format!(
                                                        "❎ {}",
                                                        i18n::LOCALES
                                                            .lookup(&config.language, "play-cancel")
                                                            .unwrap()
                                                    ))
                                                    .clicked()
                                                {
                                                    cancellation_token.cancel();
                                                }

                                                ui.horizontal_top(|ui| {
                                                    ui.with_layout(
                                                        egui::Layout::left_to_right(
                                                            egui::Align::Min,
                                                        ),
                                                        |ui| {
                                                            ui.set_width(ui.available_width());
                                                            ui.spinner();
                                                            ui.label(
                                                        i18n::LOCALES
                                                            .lookup(
                                                                &config.language,
                                                                "play-connection-task.signaling",
                                                            )
                                                            .unwrap(),
                                                    );
                                                        },
                                                    );
                                                });
                                            },
                                        );
                                    });
                                }
                                ConnectionState::Waiting => {
                                    ui.horizontal(|ui| {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Min),
                                            |ui| {
                                                if ui
                                                    .button(format!(
                                                        "❎ {}",
                                                        i18n::LOCALES
                                                            .lookup(&config.language, "play-cancel")
                                                            .unwrap()
                                                    ))
                                                    .clicked()
                                                {
                                                    cancellation_token.cancel();
                                                }

                                                ui.horizontal_top(|ui| {
                                                    ui.with_layout(
                                                        egui::Layout::left_to_right(
                                                            egui::Align::Min,
                                                        ),
                                                        |ui| {
                                                            ui.set_width(ui.available_width());
                                                            ui.spinner();
                                                            ui.label(
                                                        i18n::LOCALES
                                                            .lookup(
                                                                &config.language,
                                                                "play-connection-task.waiting",
                                                            )
                                                            .unwrap(),
                                                    );
                                                        },
                                                    );
                                                });
                                            },
                                        );
                                    });
                                }
                                ConnectionState::InLobby(lobby) => {
                                    let mut lobby = lobby.blocking_lock();
                                    if !lobby.attention_requested {
                                        window.request_user_attention(Some(
                                            winit::window::UserAttentionType::Critical,
                                        ));
                                        lobby.attention_requested = true;
                                    }

                                    ui.add_enabled_ui(
                                        lobby.local_negotiated_state.is_none()
                                            && lobby.sender.is_some(),
                                        |ui| {
                                            show_lobby_table(
                                                ui,
                                                handle.clone(),
                                                &cancellation_token,
                                                config,
                                                &mut lobby,
                                                &roms,
                                                &patches,
                                            );
                                        },
                                    );
                                }
                            }
                        }
                    }

                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (lobby, cancellation_token) =
                                if let Some(connection_task) = connection_task.as_ref() {
                                    match connection_task {
                                        ConnectionTask::InProgress {
                                            state: task_state,
                                            cancellation_token,
                                        } => (
                                            if let ConnectionState::InLobby(lobby) = task_state {
                                                Some(lobby.clone())
                                            } else {
                                                None
                                            },
                                            Some(cancellation_token.clone()),
                                        ),
                                        ConnectionTask::Failed(_) => (None, None),
                                    }
                                } else {
                                    (None, None)
                                };

                            let error_window_open = {
                                if let Some(ConnectionTask::Failed(err)) = connection_task.as_ref()
                                {
                                    let mut open = true;
                                    egui::Window::new("")
                                        .id(egui::Id::new("connection-failed-window"))
                                        .open(&mut open)
                                        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                                        .show(ui.ctx(), |ui| {
                                            // TODO: Localization
                                            ui.label(format!("{:?}", err));
                                        });
                                    open
                                } else {
                                    false
                                }
                            };

                            if !error_window_open {
                                if let Some(ConnectionTask::Failed(_)) = connection_task.as_ref() {
                                    *connection_task = None;
                                }
                            }

                            let mut submitted = false;
                            if cancellation_token.is_none() {
                                if ui
                                    .add_enabled(
                                        !error_window_open
                                            && (!state.link_code.is_empty() || selection.is_some()),
                                        egui::Button::new(egui::RichText::new(
                                            if state.link_code.is_empty() {
                                                format!(
                                                    "▶️ {}",
                                                    i18n::LOCALES
                                                        .lookup(&config.language, "play-play")
                                                        .unwrap()
                                                )
                                            } else {
                                                format!(
                                                    "🥊 {}",
                                                    i18n::LOCALES
                                                        .lookup(&config.language, "play-fight")
                                                        .unwrap()
                                                )
                                            },
                                        )),
                                    )
                                    .clicked()
                                {
                                    submitted = true;
                                }

                                if ui
                                    .add_enabled(
                                        !error_window_open,
                                        egui::Button::new(egui::RichText::new("🎲")),
                                    )
                                    .on_hover_text(
                                        i18n::LOCALES
                                            .lookup(&config.language, "play-random")
                                            .unwrap(),
                                    )
                                    .clicked()
                                {
                                    state.link_code = randomcode::generate(&config.language);
                                    let _ = clipboard.set_text(state.link_code.clone());
                                }
                            }

                            if let Some(lobby) = lobby {
                                let mut lobby = lobby.blocking_lock();
                                let mut ready = lobby.local_negotiated_state.is_some()
                                    || lobby.sender.is_none();
                                let was_ready = ready;
                                ui.add_enabled(
                                    selection.is_some()
                                        && are_settings_compatible(
                                            &lobby.make_local_settings(),
                                            &lobby.remote_settings,
                                            &roms,
                                            &patches,
                                        )
                                        && lobby.sender.is_some(),
                                    egui::Checkbox::new(
                                        &mut ready,
                                        i18n::LOCALES
                                            .lookup(&config.language, "play-ready")
                                            .unwrap(),
                                    ),
                                );
                                if error_window_open {
                                    ready = was_ready;
                                }
                                if lobby.sender.is_some() {
                                    handle.block_on(async {
                                        if !was_ready && ready {
                                            state.show_save_select = None;
                                            let save_data = lobby
                                                .selection
                                                .as_ref()
                                                .map(|selection| selection.save.to_vec());
                                            if let Some(save_data) = save_data {
                                                let _ = lobby.commit(&save_data).await;
                                            }
                                        } else if was_ready && !ready {
                                            let _ = lobby.uncommit().await;
                                        }
                                    });
                                }
                            }

                            let input_resp = ui.add_enabled(
                                cancellation_token.is_none() && !error_window_open,
                                egui::TextEdit::singleline(&mut state.link_code)
                                    .hint_text(
                                        i18n::LOCALES
                                            .lookup(&config.language, "play-link-code")
                                            .unwrap(),
                                    )
                                    .desired_width(f32::INFINITY),
                            );
                            state.link_code = state
                                .link_code
                                .to_lowercase()
                                .chars()
                                .filter(|c| {
                                    "abcdefghijklmnopqrstuvwxyz0123456789-"
                                        .chars()
                                        .any(|c2| c2 == *c)
                                })
                                .take(40)
                                .collect::<String>()
                                .trim_start_matches("-")
                                .to_string();

                            if let Some(last) = state.link_code.chars().last() {
                                if last == '-' {
                                    state.link_code = state
                                        .link_code
                                        .chars()
                                        .rev()
                                        .skip_while(|c| *c == '-')
                                        .collect::<Vec<_>>()
                                        .into_iter()
                                        .rev()
                                        .collect::<String>()
                                        + "-";
                                }
                            }

                            if input_resp.lost_focus()
                                && ui.ctx().input().key_pressed(egui::Key::Enter)
                            {
                                submitted = true;
                            }

                            if let Some(link_code) = discord_client.take_current_join_secret() {
                                state.link_code = link_code.to_string();
                                submitted = true;
                            }

                            if submitted {
                                let audio_binder = audio_binder.clone();
                                let egui_ctx = ui.ctx().clone();
                                let session = session.clone();
                                let emu_tps_counter = emu_tps_counter.clone();

                                if !state.link_code.is_empty() {
                                    let cancellation_token =
                                        tokio_util::sync::CancellationToken::new();
                                    *connection_task = Some(ConnectionTask::InProgress {
                                        state: ConnectionState::Starting,
                                        cancellation_token: cancellation_token.clone(),
                                    });

                                    handle.spawn({
                                        let matchmaking_endpoint =
                                            if !config.matchmaking_endpoint.is_empty() {
                                                config.matchmaking_endpoint.clone()
                                            } else {
                                                config::DEFAULT_MATCHMAKING_ENDPOINT.to_string()
                                            };
                                        let link_code = state.link_code.to_owned();
                                        let nickname = config
                                            .nickname
                                            .clone()
                                            .unwrap_or_else(|| "".to_string());
                                        let patches_path = config.patches_path();
                                        let replays_path = config.replays_path();
                                        let config_arc = config_arc.clone();
                                        let handle = handle.clone();
                                        let connection_task_arc = state.connection_task.clone();
                                        let roms_scanner = roms_scanner.clone();
                                        let patches_scanner = patches_scanner.clone();
                                        async move {
                                            run_connection_task(
                                                config_arc,
                                                handle,
                                                egui_ctx.clone(),
                                                audio_binder,
                                                emu_tps_counter,
                                                session,
                                                roms_scanner,
                                                patches_scanner,
                                                matchmaking_endpoint,
                                                link_code,
                                                nickname,
                                                patches_path,
                                                replays_path,
                                                connection_task_arc,
                                                cancellation_token,
                                            )
                                            .await;
                                            egui_ctx.request_repaint();
                                        }
                                    });
                                } else if let Some(selection) = selection.as_ref() {
                                    let save_path = selection.save.path.clone();
                                    let rom = selection.rom.clone();

                                    // We have to run this in a thread in order to lock main_view safely. Furthermore, we have to use a real thread because of parking_lot::Mutex.
                                    handle.spawn_blocking(move || {
                                        *session.lock() = Some(
                                            session::Session::new_singleplayer(
                                                audio_binder,
                                                &rom,
                                                &save_path,
                                                emu_tps_counter,
                                            )
                                            .unwrap(),
                                        ); // TODO: Don't unwrap maybe
                                        egui_ctx.request_repaint();
                                    });
                                }
                            }
                        });
                    });
                });
            });
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::none())
        .show_inside(ui, |ui| {
            let is_ready = connection_task
                .as_ref()
                .map(|task| match task {
                    ConnectionTask::InProgress { state, .. } => match state {
                        ConnectionState::InLobby(lobby) => {
                            lobby.blocking_lock().local_negotiated_state.is_some()
                        }
                        _ => false,
                    },
                    _ => false,
                })
                .unwrap_or(false);

            ui.add_enabled_ui(!is_ready, |ui| {
                if ui
                    .horizontal(|ui| {
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center)
                                .with_cross_justify(true),
                            |ui| {
                                ui.add({
                                    let text = egui::RichText::new(
                                        i18n::LOCALES
                                            .lookup(&config.language, "select-save.select-button")
                                            .unwrap(),
                                    );

                                    if state.show_save_select.is_some() {
                                        egui::Button::new(
                                            text.color(
                                                ui.ctx().style().visuals.selection.stroke.color,
                                            ),
                                        )
                                        .fill(ui.ctx().style().visuals.selection.bg_fill)
                                    } else {
                                        egui::Button::new(text)
                                    }
                                }) | ui
                                    .vertical_centered_justified(|ui| {
                                        let mut layouter =
                                            |ui: &egui::Ui, _: &str, _wrap_width: f32| {
                                                let mut layout_job =
                                                    egui::text::LayoutJob::default();
                                                if let Some(selection) = selection.as_ref() {
                                                    let (family, variant) =
                                                        selection.game.family_and_variant();
                                                    layout_job.append(
                                                        &format!(
                                                            "{}",
                                                            selection
                                                                .save
                                                                .path
                                                                .strip_prefix(&config.saves_path())
                                                                .unwrap_or(
                                                                    selection.save.path.as_path()
                                                                )
                                                                .display()
                                                        ),
                                                        0.0,
                                                        egui::TextFormat::simple(
                                                            ui.style()
                                                                .text_styles
                                                                .get(&egui::TextStyle::Body)
                                                                .unwrap()
                                                                .clone(),
                                                            ui.visuals().text_color(),
                                                        ),
                                                    );
                                                    layout_job.append(
                                                        &i18n::LOCALES
                                                            .lookup(
                                                                &config.language,
                                                                &format!(
                                                                    "game-{}.variant-{}",
                                                                    family, variant
                                                                ),
                                                            )
                                                            .unwrap(),
                                                        5.0,
                                                        egui::TextFormat::simple(
                                                            ui.style()
                                                                .text_styles
                                                                .get(&egui::TextStyle::Small)
                                                                .unwrap()
                                                                .clone(),
                                                            ui.visuals().text_color(),
                                                        ),
                                                    );
                                                } else {
                                                    layout_job.append(
                                                        &i18n::LOCALES
                                                            .lookup(
                                                                &config.language,
                                                                "select-save.no-save-selected",
                                                            )
                                                            .unwrap(),
                                                        0.0,
                                                        egui::TextFormat::simple(
                                                            ui.style()
                                                                .text_styles
                                                                .get(&egui::TextStyle::Body)
                                                                .unwrap()
                                                                .clone(),
                                                            ui.visuals().text_color(),
                                                        ),
                                                    );
                                                }
                                                ui.fonts().layout_job(layout_job)
                                            };
                                        ui.add(
                                            egui::TextEdit::singleline(&mut String::new())
                                                .layouter(&mut layouter),
                                        )
                                    })
                                    .inner
                            },
                        )
                        .inner
                    })
                    .inner
                    .clicked()
                {
                    state.show_save_select = if state.show_save_select.is_none() {
                        handle.spawn_blocking({
                            let roms_scanner = roms_scanner.clone();
                            let saves_scanner = saves_scanner.clone();
                            let roms_path = config.roms_path();
                            let saves_path = config.saves_path();
                            move || {
                                roms_scanner.rescan(move || Some(game::scan_roms(&roms_path)));
                                saves_scanner.rescan(move || Some(save::scan_saves(&saves_path)));
                            }
                        });
                        Some(gui::save_select_view::State::new(selection.as_ref().map(
                            |selection| (selection.game, Some(selection.save.path.to_path_buf())),
                        )))
                    } else {
                        None
                    };
                }
            });

            if state.show_save_select.is_some() {
                gui::save_select_view::show(
                    ui,
                    &mut state.show_save_select,
                    &mut *selection,
                    &config.language,
                    &config.saves_path(),
                    roms_scanner.clone(),
                    saves_scanner.clone(),
                );
            } else {
                ui.horizontal_top(|ui| {
                    let patches = patches_scanner.read();

                    let mut supported_patches = std::collections::BTreeMap::new();
                    {
                        let selection = if let Some(selection) = selection.as_mut() {
                            selection
                        } else {
                            return;
                        };

                        for (name, info) in patches.iter() {
                            let mut supported_versions = info
                                .versions
                                .iter()
                                .filter(|(_, v)| v.supported_games.contains(&selection.game))
                                .map(|(v, _)| v)
                                .collect::<Vec<_>>();
                            supported_versions.sort();
                            supported_versions.reverse();

                            if supported_versions.is_empty() {
                                continue;
                            }

                            supported_patches.insert(name, (info, supported_versions));
                        }
                    }

                    const PATCH_VERSION_COMBOBOX_WIDTH: f32 = 100.0;
                    ui.add_enabled_ui(!is_ready && selection.is_some(), |ui| {
                        egui::ComboBox::from_id_source("patch-select-combobox")
                            .selected_text(
                                selection
                                    .as_ref()
                                    .and_then(|s| s.patch.as_ref().map(|(name, _, _)| name.as_str()))
                                    .unwrap_or(
                                        &i18n::LOCALES
                                            .lookup(&config.language, "play-no-patch")
                                            .unwrap(),
                                    ),
                            )
                            .width(
                                ui.available_width()
                                    - ui.spacing().item_spacing.x
                                    - PATCH_VERSION_COMBOBOX_WIDTH,
                            )
                            .show_ui(ui, |ui| {
                                let selection = if let Some(selection) = selection.as_mut() {
                                    selection
                                } else {
                                    return;
                                };
                                if ui
                                    .selectable_label(
                                        selection.patch.is_none(),
                                        &i18n::LOCALES
                                            .lookup(&config.language, "play-no-patch")
                                            .unwrap(),
                                    )
                                    .clicked()
                                {
                                    *selection = gui::Selection::new(
                                        selection.game.clone(),
                                        selection.save.clone(),
                                        None,
                                        roms.get(&selection.game).unwrap().clone(),
                                    );
                                }

                                for (name, (_, supported_versions)) in supported_patches.iter() {
                                    if ui
                                        .selectable_label(
                                            selection.patch.as_ref().map(|(name, _, _)| name)
                                                == Some(*name),
                                            *name,
                                        )
                                        .clicked()
                                    {
                                        *patch_selection = Some(name.to_string());

                                        let rom = roms.get(&selection.game).unwrap().clone();
                                        let (rom_code, revision) =
                                            selection.game.rom_code_and_revision();
                                        let version = *supported_versions.first().unwrap();

                                        let version_metadata = if let Some(version_metadata) = patches
                                            .get(*name)
                                            .and_then(|p| p.versions.get(version))
                                            .cloned()
                                        {
                                            version_metadata
                                        } else {
                                            return;
                                        };

                                        let bps = match std::fs::read(
                                            config
                                                .patches_path()
                                                .join(name)
                                                .join(format!("v{}", version))
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
                                                    name,
                                                    (rom_code, revision),
                                                    e
                                                );
                                                return;
                                            }
                                        };

                                        let rom = match patch::bps::apply(&rom, &bps) {
                                            Ok(r) => r.to_vec(),
                                            Err(e) => {
                                                log::error!(
                                                    "failed to apply patch {} to {:?}: {:?}",
                                                    name,
                                                    (rom_code, revision),
                                                    e
                                                );
                                                return;
                                            }
                                        };

                                        *selection = gui::Selection::new(
                                            selection.game.clone(),
                                            selection.save.clone(),
                                            Some(((*name).clone(), version.clone(), version_metadata)),
                                            rom,
                                        );
                                    }
                                }
                            });
                        ui.add_enabled_ui(
                            !is_ready
                                && selection
                                    .as_ref()
                                    .and_then(|selection| selection.patch.as_ref())
                                    .and_then(|patch| supported_patches.get(&patch.0))
                                    .map(|(_, vs)| !vs.is_empty())
                                    .unwrap_or(false),
                            |ui| {
                                egui::ComboBox::from_id_source("patch-version-select-combobox")
                                    .width(
                                        PATCH_VERSION_COMBOBOX_WIDTH
                                            - ui.spacing().item_spacing.x * 2.0,
                                    )
                                    .selected_text(
                                        selection
                                            .as_ref()
                                            .and_then(|s| {
                                                s.patch
                                                    .as_ref()
                                                    .map(|(_, version, _)| version.to_string())
                                            })
                                            .unwrap_or("".to_string()),
                                    )
                                    .show_ui(ui, |ui| {
                                        let selection = if let Some(selection) = selection.as_mut() {
                                            selection
                                        } else {
                                            return;
                                        };

                                        let patch = if let Some(patch) = selection.patch.as_ref() {
                                            patch.clone()
                                        } else {
                                            return;
                                        };

                                        let supported_versions = if let Some(supported_versions) =
                                            supported_patches.get(&patch.0).map(|(_, vs)| vs)
                                        {
                                            supported_versions
                                        } else {
                                            return;
                                        };

                                        for version in supported_versions.iter() {
                                            if ui
                                                .selectable_label(
                                                    &patch.1 == *version,
                                                    version.to_string(),
                                                )
                                                .clicked()
                                            {
                                                let rom = roms.get(&selection.game).unwrap().clone();
                                                let (rom_code, revision) =
                                                    selection.game.rom_code_and_revision();

                                                let version_metadata = if let Some(version_metadata) =
                                                    patches
                                                        .get(&patch.0)
                                                        .and_then(|p| p.versions.get(version))
                                                        .cloned()
                                                {
                                                    version_metadata
                                                } else {
                                                    return;
                                                };

                                                let bps = match std::fs::read(
                                                    config
                                                        .patches_path()
                                                        .join(&patch.0)
                                                        .join(format!("v{}", version))
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
                                                            patch.0,
                                                            (rom_code, revision),
                                                            e
                                                        );
                                                        return;
                                                    }
                                                };

                                                let rom = match patch::bps::apply(&rom, &bps) {
                                                    Ok(r) => r.to_vec(),
                                                    Err(e) => {
                                                        log::error!(
                                                            "failed to apply patch {} to {:?}: {:?}",
                                                            patch.0,
                                                            (rom_code, revision),
                                                            e
                                                        );
                                                        return;
                                                    }
                                                };

                                                *selection = gui::Selection::new(
                                                    selection.game.clone(),
                                                    selection.save.clone(),
                                                    Some((
                                                        patch.0.clone(),
                                                        (*version).clone(),
                                                        version_metadata,
                                                    )),
                                                    rom,
                                                );
                                            }
                                        }
                                    });
                            },
                        );
                    });
                });

                if let Some(selection) = selection.as_mut() {
                    if let Some(assets) = selection.assets.as_ref() {
                        let game_language = selection.game.language();
                        gui::save_view::show(
                            ui,
                            config.streamer_mode,
                            clipboard,
                            font_families,
                            &config.language,
                            if let Some((_, _, metadata)) = selection.patch.as_ref() {
                                if let Some(language) =
                                    metadata.saveedit_overrides.language.as_ref()
                                {
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
        });

    if let Some(ConnectionTask::InProgress {
        state: ConnectionState::InLobby(lobby),
        ..
    }) = connection_task.as_ref()
    {
        handle.block_on(async {
            let mut lobby = lobby.lock().await;
            let _ = lobby.set_local_selection(&selection).await;
        });
    }
}

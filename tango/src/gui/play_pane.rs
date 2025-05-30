use crate::{audio, config, discord, game, gui, i18n, net, patch, randomcode, rom, session, stats, sync};
use fluent_templates::Loader;
use rand::RngCore;
use sha3::digest::{ExtendableOutput, Update};
use subtle::ConstantTimeEq;

pub enum Warning {
    Incompatible,
    UnrecognizedGame,
    NoLocalSelection,
    NoRemoteSelection,
    NoLocalROM(&'static (dyn game::Game + Send + Sync)),
    NoLocalPatch(String, semver::Version),
    NoRemoteROM(&'static (dyn game::Game + Send + Sync)),
    NoRemotePatch(String, semver::Version),
    NoRemotePatches(String),
}

impl Warning {
    pub fn description(&self, language: &unic_langid::LanguageIdentifier) -> String {
        match self {
            Warning::Incompatible => i18n::LOCALES.lookup(language, "lobby-issue-incompatible").unwrap(),
            Warning::UnrecognizedGame => i18n::LOCALES.lookup(language, "lobby-issue-unrecognized-game").unwrap(),
            Warning::NoLocalSelection => i18n::LOCALES
                .lookup(language, "lobby-issue-no-local-selection")
                .unwrap(),
            Warning::NoRemoteSelection => i18n::LOCALES
                .lookup(language, "lobby-issue-no-remote-selection")
                .unwrap(),
            Warning::NoLocalROM(game) => i18n::LOCALES
                .lookup_with_args(
                    language,
                    "lobby-issue-no-local-rom",
                    &std::collections::HashMap::from([(
                        "game_name",
                        i18n::LOCALES
                            .lookup(
                                language,
                                &format!(
                                    "game-{}.variant-{}",
                                    game.gamedb_entry().family_and_variant.0,
                                    game.gamedb_entry().family_and_variant.1
                                ),
                            )
                            .unwrap()
                            .into(),
                    )]),
                )
                .unwrap(),
            Warning::NoLocalPatch(name, version) => i18n::LOCALES
                .lookup_with_args(
                    language,
                    "lobby-issue-no-local-patch",
                    &std::collections::HashMap::from([
                        ("patch_name", name.as_str().into()),
                        ("patch_version", version.to_string().into()),
                    ]),
                )
                .unwrap(),
            Warning::NoRemoteROM(game) => i18n::LOCALES
                .lookup_with_args(
                    language,
                    "lobby-issue-no-remote-rom",
                    &std::collections::HashMap::from([(
                        "game_name",
                        i18n::LOCALES
                            .lookup(
                                language,
                                &format!(
                                    "game-{}.variant-{}",
                                    game.gamedb_entry().family_and_variant.0,
                                    game.gamedb_entry().family_and_variant.1
                                ),
                            )
                            .unwrap()
                            .into(),
                    )]),
                )
                .unwrap(),
            Warning::NoRemotePatch(name, version) => i18n::LOCALES
                .lookup_with_args(
                    language,
                    "lobby-issue-no-remote-patch",
                    &std::collections::HashMap::from([
                        ("patch_name", name.as_str().into()),
                        ("patch_version", version.to_string().into()),
                    ]),
                )
                .unwrap(),
            Warning::NoRemotePatches(name) => i18n::LOCALES
                .lookup_with_args(
                    language,
                    "lobby-issue-no-remote-patches",
                    &std::collections::HashMap::from([("patch_name", name.as_str().into())]),
                )
                .unwrap(),
        }
    }
}

fn make_warning(
    lobby: &Lobby,
    roms: &std::collections::HashMap<&'static (dyn game::Game + Send + Sync), Vec<u8>>,
    patches: &crate::patch::PatchMap,
) -> Option<Warning> {
    let local_selection = if let Some(local_selection) = lobby.local_selection.as_ref() {
        local_selection
    } else {
        return Some(Warning::NoLocalSelection);
    };

    let remote_gi = if let Some(remote_gi) = lobby.remote_settings.game_info.as_ref() {
        remote_gi
    } else {
        return Some(Warning::NoRemoteSelection);
    };

    let remote_game = if let Some(remote_game) =
        game::find_by_family_and_variant(&remote_gi.family_and_variant.0, remote_gi.family_and_variant.1)
    {
        remote_game
    } else {
        return Some(Warning::UnrecognizedGame);
    };

    if !roms.contains_key(&remote_game) {
        return Some(Warning::NoLocalROM(remote_game));
    }

    if !lobby
        .remote_settings
        .available_games
        .iter()
        .any(|(family, variant)| local_selection.game.gamedb_entry().family_and_variant == (family, *variant))
    {
        return Some(Warning::NoRemoteROM(local_selection.game));
    }

    if let Some(pi) = remote_gi.patch.as_ref() {
        if !patches.iter().any(|(patch_name, patch_metadata)| {
            *patch_name == pi.name && patch_metadata.versions.keys().any(|v| v == &pi.version)
        }) {
            return Some(Warning::NoLocalPatch(pi.name.clone(), pi.version.clone()));
        }
    }

    if let Some((patch_name, patch_version, _)) = local_selection.patch.as_ref() {
        if !lobby
            .remote_settings
            .available_patches
            .iter()
            .any(|(name, versions)| patch_name == name && versions.iter().any(|v| v == patch_version))
        {
            return Some(Warning::NoRemotePatch(patch_name.clone(), patch_version.clone()));
        }
    }

    let local_netplay_compatibility = get_netplay_compatibility(
        local_selection.game,
        local_selection
            .patch
            .as_ref()
            .map(|(name, version, _)| (name.as_str(), version)),
        patches,
    );

    let remote_netplay_compatibility = get_netplay_compatibility(
        remote_game,
        remote_gi.patch.as_ref().map(|pi| (pi.name.as_str(), &pi.version)),
        patches,
    );

    if local_netplay_compatibility != remote_netplay_compatibility {
        return Some(Warning::Incompatible);
    }

    None
}

#[derive(Clone)]
struct LocalSelection {
    pub game: &'static (dyn game::Game + Send + Sync),
    pub save: Box<dyn tango_dataview::save::Save + Send + Sync>,
    pub rom: Vec<u8>,
    pub patch: Option<(String, semver::Version, std::sync::Arc<patch::Version>)>,
}

#[derive(Clone)]
struct RemoteSelection {
    pub game: &'static (dyn game::Game + Send + Sync),
    pub rom: Vec<u8>,
    pub patch: Option<(String, semver::Version, std::sync::Arc<patch::Version>)>,
}

struct Lobby {
    attention_requested: bool,
    link_code: String,
    sender: Option<net::Sender>,
    local_selection: Option<LocalSelection>,
    remote_selection: Option<RemoteSelection>,
    nickname: String,
    match_type: (u8, u8),
    reveal_setup: bool,
    remote_settings: net::protocol::Settings,
    remote_commitment: Option<[u8; 16]>,
    latencies: crate::stats::LatencyCounter,
    local_negotiated_state: Option<(net::protocol::NegotiatedState, Vec<u8>)>,
    roms_scanner: rom::Scanner,
    patches_scanner: patch::Scanner,
}

pub fn get_netplay_compatibility(
    game: &'static (dyn game::Game + Send + Sync),
    patch: Option<(&str, &semver::Version)>,
    patches: &crate::patch::PatchMap,
) -> Option<String> {
    if let Some(patch) = patch.as_ref() {
        patches
            .get(patch.0)
            .and_then(|p| p.versions.get(patch.1).map(|vinfo| vinfo.netplay_compatibility.clone()))
    } else {
        Some(game.gamedb_entry().family_and_variant.0.to_string())
    }
}

pub fn get_netplay_compatibility_from_game_info(
    g: &net::protocol::GameInfo,
    patches: &crate::patch::PatchMap,
) -> Option<String> {
    game::find_by_family_and_variant(g.family_and_variant.0.as_str(), g.family_and_variant.1).and_then(|game| {
        get_netplay_compatibility(
            game,
            g.patch.as_ref().map(|pi| (pi.name.as_str(), &pi.version)),
            patches,
        )
    })
}

fn are_settings_compatible(
    local_settings: &net::protocol::Settings,
    remote_settings: &net::protocol::Settings,
    patches: &crate::patch::PatchMap,
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
        fn new(settings: &net::protocol::Settings, patches: &crate::patch::PatchMap) -> Self {
            Self {
                netplay_compatibility: settings
                    .game_info
                    .as_ref()
                    .and_then(|gi| get_netplay_compatibility_from_game_info(gi, patches)),
                match_type: settings.match_type,
            }
        }
    }

    let local_simplified_settings = SimplifiedSettings::new(local_settings, patches);
    let remote_simplified_settings = SimplifiedSettings::new(remote_settings, patches);

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
            nonce,
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
            game_info: self.local_selection.as_ref().map(|local_selection| {
                let (family, variant) = local_selection.game.gamedb_entry().family_and_variant;
                net::protocol::GameInfo {
                    family_and_variant: (family.to_string(), variant),
                    patch: local_selection
                        .patch
                        .as_ref()
                        .map(|(name, version, _)| net::protocol::PatchInfo {
                            name: name.clone(),
                            version: version.clone(),
                        }),
                }
            }),
            available_games: roms
                .keys()
                .map(|g| {
                    let (family, variant) = g.gamedb_entry().family_and_variant;
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

    async fn send_settings(&mut self, settings: net::protocol::Settings) -> Result<(), anyhow::Error> {
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

    async fn set_local_selection(&mut self, selection: &Option<gui::Selection>) -> Result<(), anyhow::Error> {
        if selection.as_ref().map(|selection| {
            (
                selection.game,
                selection
                    .patch
                    .as_ref()
                    .map(|(name, version, _)| (name.clone(), version.clone())),
                selection.save.save.as_raw_wram(),
            )
        }) == self.local_selection.as_ref().map(|selection| {
            (
                selection.game,
                selection
                    .patch
                    .as_ref()
                    .map(|(name, version, _)| (name.clone(), version.clone())),
                selection.save.as_raw_wram(),
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
                let (family, variant) = selection.game.gamedb_entry().family_and_variant;
                net::protocol::GameInfo {
                    family_and_variant: (family.to_string(), variant),
                    patch: selection
                        .patch
                        .as_ref()
                        .map(|(name, version, _)| net::protocol::PatchInfo {
                            name: name.clone(),
                            version: version.clone(),
                        }),
                }
            }),
            match_type,
            ..self.make_local_settings()
        })
        .await?;

        self.local_selection = selection.as_ref().map(|selection| LocalSelection {
            game: selection.game,
            save: selection.save.save.clone(),
            rom: selection.rom.clone(),
            patch: selection.patch.clone(),
        });

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
            &self.patches_scanner.read(),
        )
    }

    fn set_remote_settings(&mut self, settings: net::protocol::Settings, patches_path: &std::path::Path) {
        let roms = self.roms_scanner.read();

        let old_reveal_setup = self.remote_settings.reveal_setup;
        self.remote_selection = settings.game_info.as_ref().and_then(|gi| {
            game::find_by_family_and_variant(&gi.family_and_variant.0, gi.family_and_variant.1).and_then(|game| {
                roms.get(&game).and_then(|rom| {
                    if let Some(pi) = gi.patch.as_ref() {
                        let (rom_code, revision) = game.gamedb_entry().rom_code_and_revision;

                        let patch_version_metadata = if let Some(version_meta) = self
                            .patches_scanner
                            .read()
                            .get(&pi.name)
                            .and_then(|p| p.versions.get(&pi.version))
                            .cloned()
                        {
                            version_meta
                        } else {
                            log::error!("missing remote version metadata?");
                            return None;
                        };

                        let rom = match patch::apply_patch_from_disk(rom, game, patches_path, &pi.name, &pi.version) {
                            Ok(r) => r,
                            Err(e) => {
                                log::error!("failed to apply patch {}: {:?}: {:?}", pi.name, (rom_code, revision), e);
                                return None;
                            }
                        };

                        Some(RemoteSelection {
                            rom,
                            game,
                            patch: Some((pi.name.clone(), pi.version.clone(), patch_version_metadata)),
                        })
                    } else {
                        Some(RemoteSelection {
                            rom: rom.clone(),
                            game,
                            patch: None,
                        })
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
                async move {
                    *connection_task.lock().await =
                        Some(ConnectionTask::InProgress {
                            state: ConnectionState::Signaling,
                            cancellation_token:
                                cancellation_token.clone(),
                        });
                    const OPEN_TIMEOUT: std::time::Duration =
                        std::time::Duration::from_secs(30);
                    let use_relay = {
                        let config = config.read();
                        config.use_relay
                    };
                    let pending_conn = tokio::time::timeout(
                        OPEN_TIMEOUT,
                        tango_signaling::connect(
                            &matchmaking_addr,
                            &link_code,
                            use_relay,
                            crate::net::protocol::VERSION as u32,
                        ),
                    )
                    .await.map_err(|e| std::io::Error::new(std::io::ErrorKind::TimedOut, e))??;

                    *connection_task.lock().await =
                        Some(ConnectionTask::InProgress {
                            state: ConnectionState::Waiting,
                            cancellation_token:
                                cancellation_token.clone(),
                        });

                    let (dc, peer_conn) = pending_conn.await?;
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
                        local_selection: None,
                        remote_selection: None,
                        nickname,
                        link_code,
                        match_type: (default_match_type, 0),
                        reveal_setup: false,
                        remote_settings: net::protocol::Settings::default(),
                        remote_commitment: None,
                        latencies: crate::stats::LatencyCounter::new(5),
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
                                        return Err(ConnectionError::Other(anyhow::anyhow!("unexpected packet: {:?}", p)));
                                    }
                                }
                            }
                        }
                    }

                    log::info!("ending lobby");

                    let (mut sender, match_type, local_settings, remote_selection, remote_settings, remote_commitment, local_negotiated_state, local_selection, link_code) = {
                        let mut lobby = lobby.lock().await;
                        let local_settings = lobby.make_local_settings();
                        let sender = if let Some(sender) = lobby.sender.take() {
                            sender
                        } else {
                            return Err(ConnectionError::Other(anyhow::anyhow!("no sender?")));
                        };
                        (sender, lobby.match_type, local_settings, lobby.remote_selection.clone(), lobby.remote_settings.clone(), lobby.remote_commitment, lobby.local_negotiated_state.clone(), lobby.local_selection.clone(), lobby.link_code.clone())
                    };

                    let remote_selection = if let Some(remote_selection) = remote_selection {
                        remote_selection
                    } else {
                        return Err(ConnectionError::Other(anyhow::anyhow!("missing remote selection?")));
                    };

                    let remote_patch_overrides = remote_selection.patch.as_ref().map(|(_, _, version_meta)| version_meta.rom_overrides.clone()).unwrap_or_default();

                    let (local_negotiated_state, raw_local_state) = if let Some((negotiated_state, raw_local_state)) = local_negotiated_state {
                        (negotiated_state, raw_local_state)
                    } else {
                        return Err(ConnectionError::Other(anyhow::anyhow!("missing local state?")));
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
                                        return Err(ConnectionError::Other(anyhow::format_err!("unexpected packet: {:?}", p)));
                                    }
                                }
                            }
                        }
                    }

                    let raw_remote_negotiated_state = remote_chunks.into_iter().flatten().collect::<Vec<_>>();

                    let received_remote_commitment = if let Some(commitment) = remote_commitment {
                        commitment
                    } else {
                        return Err(ConnectionError::Other(anyhow::anyhow!("no remote commitment?")));
                    };

                    log::info!("remote commitment = {:02x?}", received_remote_commitment);

                    if !bool::from(make_commitment(&raw_remote_negotiated_state).ct_eq(&received_remote_commitment)) {
                        return Err(ConnectionError::Other(anyhow::anyhow!("commitment mismatch?")));
                    }

                    let raw_remote_negotiated_state = zstd::stream::decode_all(&raw_remote_negotiated_state[..])?;
                    let remote_negotiated_state = net::protocol::NegotiatedState::deserialize(&raw_remote_negotiated_state)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

                    let rng_seed = std::iter::zip(local_negotiated_state.nonce, remote_negotiated_state.nonce).map(|(x, y)| x ^ y).collect::<Vec<_>>().try_into().unwrap();
                    log::info!("session verified! rng seed = {:02x?}", rng_seed);

                    let local_selection = if let Some(local_selection) = local_selection {
                        local_selection
                    } else {
                        return Err(ConnectionError::Other(anyhow::anyhow!("attempted to start match in invalid state")));
                    };

                    sender.send_start_match().await?;
                    match receiver.receive().await? {
                        net::protocol::Packet::StartMatch(_) => {},
                        p => return Err(ConnectionError::Other(anyhow::anyhow!("unexpected packet when expecting start match: {:?}", p))),
                    }

                    log::info!("starting session");
                    let is_offerer = peer_conn.local_description().unwrap().sdp_type == datachannel_wrapper::SdpType::Offer;
                    {
                        *session.lock() = Some(session::Session::new_pvp(
                            config.clone(),
                            audio_binder,
                            link_code,
                            local_selection.patch.as_ref()
                                .map(|(_, _, metadata)| metadata.netplay_compatibility.clone())
                                .unwrap_or(local_selection.game.gamedb_entry().family_and_variant.0.to_owned()),
                            local_settings,
                            local_selection.game,
                            local_selection.patch.as_ref().map(|(name, version, _)| {
                                (name.clone(), version.clone())
                            }),
                            &local_selection.patch.as_ref().map(|(_, _, meta)| {
                                meta.rom_overrides.clone()
                            }).unwrap_or_default(),
                            &local_selection.rom,
                            local_selection.game.save_from_wram(&local_negotiated_state.save_data)?,
                            remote_settings,
                            remote_selection.game,
                            &remote_patch_overrides,
                            &remote_selection.rom,
                            remote_selection.game.save_from_wram(&remote_negotiated_state.save_data)?,
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
                }
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

#[derive(thiserror::Error, Debug)]
enum ConnectionError {
    #[error(transparent)]
    Negotiation(#[from] net::NegotiationError),

    #[error(transparent)]
    Signaling(#[from] tango_signaling::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

enum ConnectionTask {
    InProgress {
        state: ConnectionState,
        cancellation_token: tokio_util::sync::CancellationToken,
    },
    Failed(ConnectionError),
}

enum ConnectionState {
    Starting,
    Signaling,
    Waiting,
    InLobby(std::sync::Arc<tokio::sync::Mutex<Lobby>>),
}

pub struct State {
    link_code: String,
    show_link_code: bool,
    connection_task: std::sync::Arc<tokio::sync::Mutex<Option<ConnectionTask>>>,
    save_select_state: gui::save_select_view::State,
}

impl State {
    pub fn new(selection: Option<gui::save_select_view::Selection>) -> Self {
        Self {
            link_code: String::new(),
            show_link_code: false,
            connection_task: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            save_select_state: gui::save_select_view::State::new(selection),
        }
    }
}

fn show_lobby_table(
    ui: &mut egui::Ui,
    cancellation_token: &tokio_util::sync::CancellationToken,
    config: &mut config::Config,
    lobby: &mut Lobby,
    roms: &std::collections::HashMap<&'static (dyn game::Game + Send + Sync), Vec<u8>>,
    patches: &crate::patch::PatchMap,
) {
    let row_height = ui.text_style_height(&egui::TextStyle::Body);
    let spacing_x = ui.spacing().item_spacing.x;
    let spacing_y = ui.spacing().item_spacing.y;
    egui_extras::StripBuilder::new(ui)
        .size(egui_extras::Size::exact(row_height + spacing_y))
        .size(egui_extras::Size::exact(
            if lobby
                .local_selection
                .as_ref()
                .map(|s| s.patch.is_some())
                .unwrap_or(false)
                || lobby
                    .remote_settings
                    .game_info
                    .as_ref()
                    .map(|gi| gi.patch.is_some())
                    .unwrap_or(false)
            {
                row_height * 2.0 + spacing_y * 0.5
            } else {
                row_height
            },
        ))
        .size(egui_extras::Size::exact(row_height + spacing_y))
        .size(egui_extras::Size::exact(row_height + spacing_y))
        .size(egui_extras::Size::exact(row_height + spacing_y))
        .size(egui_extras::Size::exact(row_height + spacing_y))
        .vertical(|mut outer_strip| {
            const CELL_WIDTH: f32 = 200.0;
            outer_strip.strip(|sb| {
                sb.size(egui_extras::Size::remainder())
                    .size(egui_extras::Size::exact(CELL_WIDTH))
                    .size(egui_extras::Size::exact(CELL_WIDTH))
                    .horizontal(|mut strip| {
                        strip.cell(|_ui| {});
                        strip.cell(|ui| {
                            ui.horizontal(|ui| {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                    if ui
                                        .button(format!(
                                            "🚶 {}",
                                            i18n::LOCALES.lookup(&config.language, "play-leave").unwrap()
                                        ))
                                        .clicked()
                                    {
                                        cancellation_token.cancel();
                                    }

                                    ui.horizontal_top(|ui| {
                                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.strong(i18n::LOCALES.lookup(&config.language, "play-you").unwrap());
                                            if lobby.local_negotiated_state.is_some() || lobby.sender.is_none() {
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
                        strip.cell(|ui| {
                            ui.horizontal(|ui| {
                                ui.strong(lobby.remote_settings.nickname.clone());
                                ui.small(format!("{}ms", lobby.latencies.median().as_millis()));
                                if lobby.remote_commitment.is_some() {
                                    ui.label(
                                        egui::RichText::new("✅").color(egui::Color32::from_rgb(0x4c, 0xaf, 0x50)),
                                    );
                                }
                            });
                        });
                    });
            });

            outer_strip.strip(|sb| {
                sb.size(egui_extras::Size::remainder())
                    .size(egui_extras::Size::exact(CELL_WIDTH))
                    .size(egui_extras::Size::exact(CELL_WIDTH))
                    .horizontal(|mut strip| {
                        strip.cell(|ui| {
                            ui.horizontal(|ui| {
                                ui.strong(i18n::LOCALES.lookup(&config.language, "play-details-game").unwrap());

                                if let Some(warning) = make_warning(lobby, roms, patches) {
                                    gui::warning::show(ui, warning.description(&config.language));
                                }
                            });
                        });
                        strip.cell(|ui| {
                            ui.vertical(|ui| {
                                if let Some(local_selection) = lobby.local_selection.as_ref() {
                                    let (family, variant) = local_selection.game.gamedb_entry().family_and_variant;
                                    ui.label(if game::find_by_family_and_variant(family, variant).is_some() {
                                        i18n::LOCALES
                                            .lookup(&config.language, &format!("game-{}", family))
                                            .unwrap()
                                    } else {
                                        i18n::LOCALES
                                            .lookup(&config.language, "play-details-game.unknown")
                                            .unwrap()
                                    });
                                    if let Some((patch_name, version, _)) = local_selection.patch.as_ref() {
                                        ui.label(format!("{} v{}", patch_name, version));
                                    }
                                } else {
                                    ui.label(i18n::LOCALES.lookup(&config.language, "play-no-game").unwrap());
                                }
                            });
                        });
                        strip.cell(|ui| {
                            ui.vertical(|ui| {
                                if let Some(game_info) = lobby.remote_settings.game_info.as_ref() {
                                    let (family, variant) = &game_info.family_and_variant;
                                    if let Some(game) = game::find_by_family_and_variant(family, *variant) {
                                        let (family, _) = game.gamedb_entry().family_and_variant;
                                        ui.label(
                                            i18n::LOCALES
                                                .lookup(&config.language, &format!("game-{}", family))
                                                .unwrap(),
                                        );
                                        if let Some(pi) = game_info.patch.as_ref() {
                                            ui.label(format!("{} v{}", pi.name, pi.version));
                                        }
                                    } else {
                                        ui.label(i18n::LOCALES.lookup(&config.language, "play-no-game").unwrap());
                                    }
                                } else {
                                    ui.label(i18n::LOCALES.lookup(&config.language, "play-no-game").unwrap());
                                }
                            });
                        });
                    });
            });

            outer_strip.strip(|sb| {
                sb.size(egui_extras::Size::remainder())
                    .size(egui_extras::Size::exact(CELL_WIDTH))
                    .size(egui_extras::Size::exact(CELL_WIDTH))
                    .horizontal(|mut strip| {
                        strip.cell(|ui| {
                            ui.horizontal(|ui| {
                                ui.strong(
                                    i18n::LOCALES
                                        .lookup(&config.language, "play-details-match-type")
                                        .unwrap(),
                                );
                                if lobby.local_selection.is_some()
                                    && lobby.remote_settings.game_info.is_some()
                                    && lobby.match_type != lobby.remote_settings.match_type
                                {
                                    gui::warning::show(
                                        ui,
                                        i18n::LOCALES
                                            .lookup(&config.language, "lobby-issue-match-type-mismatch")
                                            .unwrap(),
                                    );
                                }
                            });
                        });
                        strip.cell(|ui| {
                            let game = lobby
                                .local_selection
                                .as_ref()
                                .map(|local_selection| local_selection.game);
                            ui.add_enabled_ui(game.is_some(), |ui| {
                                egui::ComboBox::new("start-match-type-combobox", "")
                                    .width(150.0)
                                    .selected_text(if let Some(game) = game.as_ref() {
                                        i18n::LOCALES
                                            .lookup(
                                                &config.language,
                                                &format!(
                                                    "game-{}.match-type-{}-{}",
                                                    game.gamedb_entry().family_and_variant.0,
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
                                            for (typ, subtype_count) in game.match_types().iter().enumerate() {
                                                for subtype in 0..*subtype_count {
                                                    ui.selectable_value(
                                                        &mut match_type,
                                                        (typ as u8, subtype as u8),
                                                        i18n::LOCALES
                                                            .lookup(
                                                                &config.language,
                                                                &format!(
                                                                    "game-{}.match-type-{}-{}",
                                                                    game.gamedb_entry().family_and_variant.0,
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
                                                let _ = sync::block_on(lobby.set_match_type(match_type));
                                            }
                                        }
                                    });
                            });
                        });
                        strip.cell(|ui| {
                            ui.label(if let Some(game_info) = lobby.remote_settings.game_info.as_ref() {
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
                            });
                        });
                    });
            });

            outer_strip.strip(|sb| {
                sb.size(egui_extras::Size::remainder())
                    .size(egui_extras::Size::exact(CELL_WIDTH))
                    .size(egui_extras::Size::exact(CELL_WIDTH))
                    .horizontal(|mut strip| {
                        strip.cell(|ui| {
                            ui.strong(
                                i18n::LOCALES
                                    .lookup(&config.language, "play-details-reveal-setup")
                                    .unwrap(),
                            );
                        });
                        strip.cell(|ui| {
                            let mut checked = lobby.reveal_setup;
                            ui.checkbox(&mut checked, "");
                            let _ = sync::block_on(lobby.set_reveal_setup(checked));
                        });
                        strip.cell(|ui| {
                            ui.checkbox(&mut lobby.remote_settings.reveal_setup.clone(), "");
                        });
                    });
            });

            outer_strip.strip(|sb| {
                sb.size(egui_extras::Size::remainder())
                    .size(egui_extras::Size::exact(CELL_WIDTH * 2.0 + spacing_x))
                    .horizontal(|mut strip| {
                        strip.cell(|ui| {
                            ui.strong(i18n::LOCALES.lookup(&config.language, "settings-input-delay").unwrap());
                        });
                        strip.cell(|ui| {
                            ui.horizontal(|ui| {
                                ui.add(egui::DragValue::new(&mut config.input_delay).speed(1).range(2..=10));
                                if ui
                                    .button(
                                        i18n::LOCALES
                                            .lookup(&config.language, "play-details-input-delay.suggest")
                                            .unwrap(),
                                    )
                                    .clicked()
                                {
                                    config.input_delay = std::cmp::min(
                                        10,
                                        std::cmp::max(
                                            2,
                                            ((lobby.latencies.median() * 60).as_nanos()
                                                / 2
                                                / std::time::Duration::from_secs(1).as_nanos())
                                                as i32
                                                + 1
                                                - 2,
                                        ),
                                    ) as u32;
                                }
                            });
                        });
                    });
            });

            outer_strip.strip(|sb| {
                sb.size(egui_extras::Size::remainder())
                    .size(egui_extras::Size::exact(CELL_WIDTH * 2.0 + spacing_x))
                    .horizontal(|mut strip| {
                        strip.cell(|ui| {
                            ui.strong(
                                i18n::LOCALES
                                    .lookup(&config.language, "settings-show-own-setup")
                                    .unwrap(),
                            );
                        });
                        strip.cell(|ui| {
                            ui.checkbox(&mut config.show_own_setup, "");
                        });
                    });
            });
        });
}

fn show_bottom_pane(
    ui: &mut egui::Ui,
    config: &mut config::Config,
    shared_root_state: &mut gui::SharedRootState,
    connection_task: &mut Option<ConnectionTask>,
    connection_task_arc: std::sync::Arc<tokio::sync::Mutex<Option<ConnectionTask>>>,
    link_code: &mut String,
    show_link_code: &mut bool,
    init_link_code: &mut Option<String>,
) {
    let selection = &mut shared_root_state.selection;

    let error_window_open = {
        if let Some(ConnectionTask::Failed(err)) = connection_task.as_ref() {
            let mut open = true;
            let mut open2 = true;
            egui::Window::new(format!(
                "🔌 {}",
                i18n::LOCALES.lookup(&config.language, "connection-error").unwrap()
            ))
            .id(egui::Id::new("connection-failed-window"))
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                ui.label(match err {
                    ConnectionError::Negotiation(net::NegotiationError::RemoteProtocolVersionTooOld) => i18n::LOCALES
                        .lookup(&config.language, "connection-error-remote-protocol-version-too-old")
                        .unwrap(),
                    ConnectionError::Signaling(tango_signaling::Error::ServerAbort(
                        tango_signaling::AbortReason::ProtocolVersionTooOld,
                    )) => i18n::LOCALES
                        .lookup(&config.language, "connection-error-protocol-version-too-old")
                        .unwrap(),
                    ConnectionError::Negotiation(net::NegotiationError::RemoteProtocolVersionTooNew) => i18n::LOCALES
                        .lookup(&config.language, "connection-error-remote-protocol-version-too-new")
                        .unwrap(),

                    ConnectionError::Io(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        i18n::LOCALES.lookup(&config.language, "connection-error-eof").unwrap()
                    }

                    e => i18n::LOCALES
                        .lookup_with_args(
                            &config.language,
                            "connection-error-other",
                            &std::collections::HashMap::from([("error", format!("{:?}", e).into())]),
                        )
                        .unwrap(),
                });
                if ui
                    .button(
                        i18n::LOCALES
                            .lookup(&config.language, "connection-error-confirm")
                            .unwrap(),
                    )
                    .clicked()
                {
                    open2 = false;
                }
            });
            open && open2
        } else {
            false
        }
    };

    if !error_window_open {
        if let Some(ConnectionTask::Failed(_)) = connection_task.as_ref() {
            *connection_task = None;
        }
    }

    let discord_client = &shared_root_state.discord_client;
    let roms = shared_root_state.scanners.roms.read();
    let patches = shared_root_state.scanners.patches.read();

    egui::TopBottomPanel::bottom("play-bottom-pane").show_inside(ui, |ui| {
        ui.vertical(|ui| {
            {
                if let Some(ConnectionTask::InProgress {
                    state: connection_state,
                    cancellation_token,
                }) = connection_task.as_ref()
                {
                    match connection_state {
                        ConnectionState::Starting | ConnectionState::Signaling | ConnectionState::Waiting => {
                            ui.horizontal(|ui| {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                    if ui
                                        .button(format!(
                                            "❎ {}",
                                            i18n::LOCALES.lookup(&config.language, "play-cancel").unwrap()
                                        ))
                                        .clicked()
                                    {
                                        cancellation_token.cancel();
                                    }

                                    ui.horizontal_top(|ui| {
                                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                                            ui.spinner();
                                            ui.label(match connection_state {
                                                ConnectionState::Starting => i18n::LOCALES
                                                    .lookup(&config.language, "play-connection-task-starting")
                                                    .unwrap(),
                                                ConnectionState::Signaling => i18n::LOCALES
                                                    .lookup(&config.language, "play-connection-task-signaling")
                                                    .unwrap(),
                                                ConnectionState::Waiting => i18n::LOCALES
                                                    .lookup(&config.language, "play-connection-task-waiting")
                                                    .unwrap(),
                                                _ => unreachable!(),
                                            });
                                        });
                                    });
                                });
                            });
                            discord_client.set_current_activity(Some(discord::make_looking_activity(
                                link_code,
                                &config.language,
                                selection.as_ref().map(|selection| {
                                    discord::make_game_info(
                                        selection.game,
                                        selection
                                            .patch
                                            .as_ref()
                                            .map(|(patch_name, patch_version, _)| (patch_name.as_str(), patch_version)),
                                        &config.language,
                                    )
                                }),
                            )));
                        }
                        ConnectionState::InLobby(lobby) => {
                            let mut lobby = lobby.blocking_lock();
                            if !lobby.attention_requested {
                                let window_request = crate::WindowRequest::Attention;
                                let _ = shared_root_state.event_loop_proxy.send_event(window_request);
                                lobby.attention_requested = true;
                            }

                            discord_client.set_current_activity(Some(discord::make_in_lobby_activity(
                                &lobby.link_code,
                                &config.language,
                                lobby.local_selection.as_ref().map(|selection| {
                                    discord::make_game_info(
                                        selection.game,
                                        selection
                                            .patch
                                            .as_ref()
                                            .map(|(patch_name, patch_version, _)| (patch_name.as_str(), patch_version)),
                                        &config.language,
                                    )
                                }),
                            )));

                            ui.add_enabled_ui(lobby.local_negotiated_state.is_none() && lobby.sender.is_some(), |ui| {
                                show_lobby_table(ui, cancellation_token, config, &mut lobby, &roms, &patches);
                            });
                        }
                    }
                } else {
                    discord_client.set_current_activity(Some(discord::make_base_activity(None)));
                }
            }

            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (lobby, cancellation_token) = if let Some(connection_task) = connection_task.as_ref() {
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

                    let mut submitted = false;
                    if cancellation_token.is_none() {
                        if ui
                            .add_enabled(
                                !error_window_open && (!link_code.is_empty() || selection.is_some()),
                                egui::Button::new(egui::RichText::new(if link_code.is_empty() {
                                    format!("▶️ {}", i18n::LOCALES.lookup(&config.language, "play-play").unwrap())
                                } else {
                                    format!("🥊 {}", i18n::LOCALES.lookup(&config.language, "play-fight").unwrap())
                                })),
                            )
                            .clicked()
                        {
                            submitted = true;
                        }

                        if ui
                            .add_enabled(!error_window_open, egui::Button::new(egui::RichText::new("🎲")))
                            .on_hover_text(i18n::LOCALES.lookup(&config.language, "play-random").unwrap())
                            .clicked()
                        {
                            *link_code = randomcode::generate(&config.language);
                            let _ = shared_root_state.clipboard.set_text(link_code.clone());
                        }

                        if config.streamer_mode
                            && ui
                                .selectable_label(*show_link_code, "👁️")
                                .on_hover_text(i18n::LOCALES.lookup(&config.language, "play-show-link-code").unwrap())
                                .clicked()
                        {
                            *show_link_code = !*show_link_code;
                        }
                    }

                    if let Some(lobby) = lobby {
                        let mut lobby = lobby.blocking_lock();
                        let mut ready = lobby.local_negotiated_state.is_some() || lobby.sender.is_none();
                        let was_ready = ready;
                        ui.add_enabled(
                            selection.is_some()
                                && are_settings_compatible(
                                    &lobby.make_local_settings(),
                                    &lobby.remote_settings,
                                    &patches,
                                )
                                && lobby.sender.is_some(),
                            egui::Checkbox::new(
                                &mut ready,
                                i18n::LOCALES.lookup(&config.language, "play-ready").unwrap(),
                            ),
                        );
                        if error_window_open {
                            ready = was_ready;
                        }
                        if lobby.sender.is_some() {
                            if !was_ready && ready {
                                let save_data = lobby
                                    .local_selection
                                    .as_ref()
                                    .map(|selection| selection.save.as_raw_wram().to_vec());
                                if let Some(save_data) = save_data {
                                    let _ = sync::block_on(lobby.commit(&save_data));
                                }
                            } else if was_ready && !ready {
                                let _ = sync::block_on(lobby.uncommit());
                            }
                        }
                    }

                    let input_resp = ui.add_enabled(
                        cancellation_token.is_none() && !error_window_open,
                        egui::TextEdit::singleline(link_code)
                            .password(config.streamer_mode && !*show_link_code)
                            .hint_text(i18n::LOCALES.lookup(&config.language, "play-link-code").unwrap())
                            .desired_width(f32::INFINITY),
                    );
                    *link_code = link_code
                        .to_lowercase()
                        .chars()
                        .filter(|c| "abcdefghijklmnopqrstuvwxyz0123456789-".chars().any(|c2| c2 == *c))
                        .take(40)
                        .collect::<String>()
                        .trim_start_matches('-')
                        .to_string();

                    if let Some(last) = link_code.chars().last() {
                        if last == '-' {
                            *link_code = link_code
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

                    if input_resp.lost_focus() && ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)) {
                        submitted = true;
                    }

                    if let Some(init_link_code) = init_link_code.take() {
                        *link_code = init_link_code.to_string();
                        submitted = true;
                    }

                    if let Some(join_secret) = discord_client.take_current_join_secret() {
                        *link_code = join_secret.to_string();
                        submitted = true;
                    }

                    if submitted {
                        let audio_binder = shared_root_state.audio_binder.clone();
                        let egui_ctx = ui.ctx().clone();
                        let session = shared_root_state.session.clone();
                        let emu_tps_counter = shared_root_state.emu_tps_counter.clone();

                        if !link_code.is_empty() {
                            let cancellation_token = tokio_util::sync::CancellationToken::new();
                            *connection_task = Some(ConnectionTask::InProgress {
                                state: ConnectionState::Starting,
                                cancellation_token: cancellation_token.clone(),
                            });

                            tokio::task::spawn({
                                let matchmaking_endpoint = if !config.matchmaking_endpoint.is_empty() {
                                    config.matchmaking_endpoint.clone()
                                } else {
                                    config::DEFAULT_MATCHMAKING_ENDPOINT.to_string()
                                };
                                let link_code = link_code.to_owned();
                                let nickname = config.nickname.clone().unwrap_or_default();
                                let patches_path = config.patches_path();
                                let replays_path = config.replays_path();
                                let config_arc = shared_root_state.config.clone();
                                let connection_task_arc = connection_task_arc.clone();
                                let roms_scanner = shared_root_state.scanners.roms.clone();
                                let patches_scanner = shared_root_state.scanners.patches.clone();
                                async move {
                                    run_connection_task(
                                        config_arc,
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
                            let game = selection.game;
                            let rom = selection.rom.clone();
                            let patch = selection
                                .patch
                                .as_ref()
                                .map(|(name, version, _)| (name.clone(), version.clone()));
                            let save_file = std::fs::OpenOptions::new()
                                .create(true)
                                .write(true)
                                .read(true)
                                .open(save_path)
                                .unwrap();

                            // We have to run this in a thread in order to lock main_view safely. Furthermore, we have to use a real thread because of parking_lot::Mutex.
                            tokio::task::spawn_blocking(move || {
                                *session.lock() = Some(
                                    session::Session::new_singleplayer(
                                        audio_binder,
                                        game,
                                        patch,
                                        &rom,
                                        save_file,
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

pub fn show(
    ui: &mut egui::Ui,
    config: &mut config::Config,
    shared_root_state: &mut gui::SharedRootState,
    patch_selection: &mut Option<String>,
    state: &mut State,
    init_link_code: &mut Option<String>,
) {
    let connection_task_arc = state.connection_task.clone();
    let mut connection_task = state.connection_task.blocking_lock();

    // must happen first to provide the imgui enough info to prevent the central panel from overflowing
    show_bottom_pane(
        ui,
        config,
        shared_root_state,
        &mut connection_task,
        connection_task_arc,
        &mut state.link_code,
        &mut state.show_link_code,
        init_link_code,
    );

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(ui.style().visuals.window_fill())
                .inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    bottom: 8,
                    top: 8,
                }),
        )
        .show_inside(ui, |ui| {
            let lobby = connection_task.as_ref().and_then(|task| match task {
                ConnectionTask::InProgress {
                    state: ConnectionState::InLobby(lobby),
                    ..
                } => Some(lobby.blocking_lock()),
                _ => None,
            });

            let is_ready = lobby
                .as_ref()
                .map(|lobby| lobby.local_negotiated_state.is_some())
                .unwrap_or(false);

            ui.add_enabled_ui(!is_ready, |ui| {
                gui::save_select_view::show(
                    ui,
                    config,
                    shared_root_state,
                    &mut state.save_select_state,
                    patch_selection,
                    if let Some(lobby) = lobby.as_ref() {
                        Some(&lobby.remote_settings)
                    } else {
                        None
                    },
                );
            });

            ui.separator();

            // we're only planning on viewing the data, should be safe to take the selection
            if let Some(mut selection) = shared_root_state.selection.take() {
                if let Some(assets) = selection.assets.as_ref() {
                    let game_language = selection
                        .patch
                        .as_ref()
                        .and_then(|(_, _, metadata)| metadata.rom_overrides.language.clone())
                        .unwrap_or_else(|| crate::game::region_to_language(selection.game.gamedb_entry().region));

                    gui::save_view::show(
                        ui,
                        config.streamer_mode,
                        config,
                        shared_root_state,
                        &game_language,
                        selection.save.save.as_ref(),
                        assets.as_ref(),
                        &mut selection.save_view_state,
                        false,
                    );
                }

                // put the selection back
                shared_root_state.selection = Some(selection);
            }
        });

    if let Some(ConnectionTask::InProgress {
        state: ConnectionState::InLobby(lobby),
        ..
    }) = connection_task.as_ref()
    {
        let mut lobby = lobby.blocking_lock();
        let selection = &shared_root_state.selection;
        let _ = sync::block_on(lobby.set_local_selection(selection));
    }
}

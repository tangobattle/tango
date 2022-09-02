use fluent_templates::Loader;
use rand::RngCore;
use sha3::digest::{ExtendableOutput, Update};
use subtle::ConstantTimeEq;

use crate::{
    audio, config, game, gui, i18n, input, net, patch, randomcode, rom, save, session, stats,
};

pub struct State {
    pub session: Option<session::Session>,
    pub selection: std::sync::Arc<parking_lot::Mutex<Option<Selection>>>,
    link_code: String,
    connection_task: std::sync::Arc<tokio::sync::Mutex<Option<ConnectionTask>>>,
    show_save_select: Option<gui::save_select_window::State>,
    show_patches: Option<gui::patches_window::State>,
    show_replays: Option<gui::replays_window::State>,
}

impl State {
    pub fn new() -> Self {
        Self {
            session: None,
            link_code: String::new(),
            selection: std::sync::Arc::new(parking_lot::Mutex::new(None)),
            connection_task: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            show_save_select: None,
            show_patches: None,
            show_replays: None,
        }
    }
}

enum ConnectionFailure {}

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

pub struct Selection {
    pub game: &'static (dyn game::Game + Send + Sync),
    pub assets: Option<Box<dyn rom::Assets + Send + Sync>>,
    pub save: save::ScannedSave,
    pub rom: Vec<u8>,
    pub patch: Option<(String, semver::Version)>,
    save_view_state: gui::save_view::State,
}

impl Selection {
    pub fn new(
        game: &'static (dyn game::Game + Send + Sync),
        save: save::ScannedSave,
        patch: Option<(String, semver::Version)>,
        rom: Vec<u8>,
    ) -> Self {
        let assets = game.load_rom_assets(&rom, save.save.as_raw_wram()).ok();
        Self {
            game,
            assets,
            save,
            patch,
            rom,
            save_view_state: gui::save_view::State::new(),
        }
    }

    pub fn reload_save(&mut self) -> anyhow::Result<()> {
        let raw = std::fs::read(&self.save.path)?;
        self.save.save = self.game.parse_save(&raw)?;
        Ok(())
    }
}

struct Lobby {
    attention_requested: bool,
    sender: Option<net::Sender>,
    selection: std::sync::Arc<parking_lot::Mutex<Option<Selection>>>,
    nickname: String,
    match_type: (u8, u8),
    reveal_setup: bool,
    remote_settings: net::protocol::Settings,
    remote_commitment: Option<[u8; 16]>,
    latencies: stats::DeltaCounter,
    local_negotiated_state: Option<(net::protocol::NegotiatedState, Vec<u8>)>,
    roms_scanner: gui::ROMsScanner,
    patches_scanner: gui::PatchesScanner,
}

#[derive(PartialEq)]
struct SimplifiedSettings {
    netplay_compatibility: Option<String>,
    match_type: (u8, u8),
}

impl SimplifiedSettings {
    fn new(
        settings: &net::protocol::Settings,
        patches: &std::collections::BTreeMap<String, patch::Patch>,
    ) -> Self {
        Self {
            netplay_compatibility: settings.game_info.as_ref().and_then(|g| {
                if let Some(patch) = g.patch.as_ref() {
                    patches.get(&patch.name).and_then(|p| {
                        p.versions
                            .get(&patch.version)
                            .map(|vinfo| vinfo.netplay_compatibility.clone())
                    })
                } else {
                    Some(g.family_and_variant.0.clone())
                }
            }),
            match_type: settings.match_type,
        }
    }
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
                    patch: selection.patch.as_ref().map(|(name, version)| {
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
        patches: &std::collections::BTreeMap<String, patch::Patch>,
    ) {
        let old_reveal_setup = self.remote_settings.reveal_setup;
        self.remote_settings = settings;
        if SimplifiedSettings::new(&self.make_local_settings(), &patches)
            != SimplifiedSettings::new(&self.remote_settings, &patches)
            || (old_reveal_setup && !self.remote_settings.reveal_setup)
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
    main_view: std::sync::Arc<parking_lot::Mutex<State>>,
    selection: std::sync::Arc<parking_lot::Mutex<Option<Selection>>>,
    roms_scanner: gui::ROMsScanner,
    patches_scanner: gui::PatchesScanner,
    matchmaking_addr: String,
    link_code: String,
    max_queue_length: usize,
    nickname: String,
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
                                        let patches = patches_scanner.read();
                                        lobby.set_remote_settings(settings, &patches);
                                        egui_ctx.request_repaint();
                                    },
                                    net::protocol::Packet::Commit(commit) => {
                                        let mut lobby = lobby.lock().await;
                                        lobby.remote_commitment = Some(commit.commitment);

                                        if lobby.local_negotiated_state.is_some() {
                                            break 'l;
                                        }
                                    },
                                    net::protocol::Packet::Uncommit(_) => {
                                        lobby.lock().await.remote_commitment = None;
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

                    let (mut sender, match_type, local_settings, remote_settings, remote_commitment, local_negotiated_state) = {
                        let mut lobby = lobby.lock().await;
                        let local_settings = lobby.make_local_settings();
                        let sender = if let Some(sender) = lobby.sender.take() {
                            sender
                        } else {
                            anyhow::bail!("no sender?");
                        };
                        (sender, lobby.match_type, local_settings, lobby.remote_settings.clone(), lobby.remote_commitment.clone(), lobby.local_negotiated_state.take())
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

                    let remote_game = if let Some(game) = remote_settings.game_info.as_ref().and_then(|gi| {
                        let (family, variant) = &gi.family_and_variant;
                        game::find_by_family_and_variant(family, *variant)
                    }) {
                        game
                    } else {
                        anyhow::bail!("attempted to start match in invalid state");
                    };

                    let remote_rom = {
                        let roms = roms_scanner.read();
                        if let Some(remote_rom) = roms.get(&remote_game).cloned() {
                            remote_rom
                        } else {
                            anyhow::bail!("missing shadow rom");
                        }
                    };

                    sender.send_start_match().await?;
                    match receiver.receive().await? {
                        net::protocol::Packet::StartMatch(_) => {},
                        p => anyhow::bail!("unexpected packet when expecting start match: {:?}", p),
                    }

                    log::info!("starting session");
                    let is_offerer = peer_conn.local_description().unwrap().sdp_type == datachannel_wrapper::SdpType::Offer;
                    main_view.lock().session = Some(session::Session::new_pvp(
                        handle,
                        audio_binder,
                        link_code,
                        patch
                            .and_then(|(patch_name, patch_version)| {
                                patches_scanner
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
                        remote_game,
                        &remote_rom,
                        &remote_negotiated_state.save_data,
                        emu_tps_counter.clone(),
                        sender,
                        receiver,
                        peer_conn,
                        is_offerer,
                        replays_path,
                        match_type,
                        config.read().input_delay,
                        rng_seed,
                        max_queue_length,
                    )?);
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

pub struct MainView {
    session_view: gui::session_view::SessionView,
    save_select_window: gui::save_select_window::SaveSelectWindow,
    patches_window: gui::patches_window::PatchesWindow,
    replays_window: gui::replays_window::ReplaysWindow,
    save_view: gui::save_view::SaveView,
}

impl MainView {
    pub fn new() -> Self {
        Self {
            session_view: gui::session_view::SessionView::new(),
            save_select_window: gui::save_select_window::SaveSelectWindow::new(),
            patches_window: gui::patches_window::PatchesWindow::new(),
            replays_window: gui::replays_window::ReplaysWindow::new(),
            save_view: gui::save_view::SaveView::new(),
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        font_families: &gui::FontFamilies,
        config: &mut config::Config,
        handle: tokio::runtime::Handle,
        window: &glutin::window::Window,
        input_state: &input::State,
        state: &mut gui::State,
    ) {
        let mut main_view = state.main_view.lock();
        if let Some(session) = main_view.session.as_ref() {
            self.session_view.show(
                ctx,
                input_state,
                &config.input_mapping,
                session,
                &config.video_filter,
                config.max_scale,
                &mut state.show_escape_window,
            );
            return;
        }

        let main_view = &mut *main_view;

        self.patches_window.show(
            ctx,
            &mut main_view.show_patches,
            &config.language,
            &config.patches_path(),
            state.patches_scanner.clone(),
        );

        self.replays_window
            .show(ctx, &mut main_view.show_replays, &config.language);

        let (selection_changed, has_selection) = {
            let mut selection = main_view.selection.lock();
            let selection = &mut *selection;

            let initial = selection
                .as_ref()
                .map(|selection| (selection.game, selection.patch.clone()));

            self.save_select_window.show(
                ctx,
                &mut main_view.show_save_select,
                selection,
                &config.language,
                &config.saves_path(),
                state.roms_scanner.clone(),
                state.saves_scanner.clone(),
            );

            egui::TopBottomPanel::top("main-top-panel").show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .selectable_label(state.show_settings.is_some(), "⚙️")
                                .on_hover_text_at_pointer(
                                    i18n::LOCALES.lookup(&config.language, "settings").unwrap(),
                                )
                                .clicked()
                            {
                                state.show_settings = if state.show_settings.is_none() {
                                    Some(gui::settings_window::State::new())
                                } else {
                                    None
                                };
                            }

                            if ui
                                .selectable_label(main_view.show_patches.is_some(), "🩹")
                                .on_hover_text_at_pointer(
                                    i18n::LOCALES.lookup(&config.language, "patches").unwrap(),
                                )
                                .clicked()
                            {
                                main_view.show_patches = if main_view.show_patches.is_none() {
                                    rayon::spawn({
                                        let patches_scanner = state.patches_scanner.clone();
                                        let patches_path = config.patches_path();
                                        move || {
                                            patches_scanner.rescan(move || {
                                                patch::scan(&patches_path).unwrap_or_default()
                                            });
                                        }
                                    });
                                    Some(gui::patches_window::State::new(
                                        selection
                                            .as_ref()
                                            .and_then(|s| s.patch.as_ref())
                                            .map(|(n, _)| n.to_string()),
                                    ))
                                } else {
                                    None
                                };
                            }

                            if ui
                                .selectable_label(main_view.show_replays.is_some(), "📽️")
                                .on_hover_text_at_pointer(
                                    i18n::LOCALES.lookup(&config.language, "replays").unwrap(),
                                )
                                .clicked()
                            {
                                main_view.show_replays = if main_view.show_replays.is_none() {
                                    Some(gui::replays_window::State::new())
                                } else {
                                    None
                                };
                            }
                        });
                    });

                    if ui
                        .horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.add({
                                    let text = egui::RichText::new(
                                        i18n::LOCALES
                                            .lookup(&config.language, "select-save.select-button")
                                            .unwrap(),
                                    );

                                    if main_view.show_save_select.is_some() {
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
                                                if let Some(selection) = selection {
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
                                                                .get(&egui::TextStyle::Small)
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
                            })
                            .inner
                        })
                        .inner
                        .clicked()
                    {
                        main_view.show_save_select = if main_view.show_save_select.is_none() {
                            rayon::spawn({
                                let roms_scanner = state.roms_scanner.clone();
                                let saves_scanner = state.saves_scanner.clone();
                                let roms_path = config.roms_path();
                                let saves_path = config.saves_path();
                                move || {
                                    roms_scanner.rescan(move || game::scan_roms(&roms_path));
                                    saves_scanner.rescan(move || save::scan_saves(&saves_path));
                                }
                            });
                            Some(gui::save_select_window::State::new(selection.as_ref().map(
                                |selection| {
                                    (selection.game, Some(selection.save.path.to_path_buf()))
                                },
                            )))
                        } else {
                            None
                        };
                    }
                });

                ui.horizontal_top(|ui| {
                    let patches = state.patches_scanner.read();

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
                    ui.add_enabled_ui(selection.is_some(), |ui| {
                        egui::ComboBox::from_id_source("patch-select-combobox")
                            .selected_text(
                                selection
                                    .as_ref()
                                    .and_then(|s| s.patch.as_ref().map(|(name, _)| name.as_str()))
                                    .unwrap_or(
                                        &i18n::LOCALES
                                            .lookup(&config.language, "main.no-patch")
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
                                            .lookup(&config.language, "main.no-patch")
                                            .unwrap(),
                                    )
                                    .clicked()
                                {
                                    let rom = {
                                        let roms = state.roms_scanner.read();
                                        roms.get(&selection.game).unwrap().clone()
                                    };

                                    *selection = gui::main_view::Selection::new(
                                        selection.game.clone(),
                                        selection.save.clone(),
                                        None,
                                        rom,
                                    );
                                }

                                for (name, (_, supported_versions)) in supported_patches.iter() {
                                    if ui
                                        .selectable_label(
                                            selection.patch.as_ref().map(|(name, _)| name)
                                                == Some(*name),
                                            *name,
                                        )
                                        .clicked()
                                    {
                                        let rom = {
                                            let roms = state.roms_scanner.read();
                                            roms.get(&selection.game).unwrap().clone()
                                        };
                                        let (rom_code, revision) =
                                            selection.game.rom_code_and_revision();
                                        let version = *supported_versions.first().unwrap();

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

                                        if let Some(show_patches) = main_view.show_patches.as_mut()
                                        {
                                            *show_patches = gui::patches_window::State::new(Some(
                                                (*name).clone(),
                                            ));
                                        }
                                        *selection = gui::main_view::Selection::new(
                                            selection.game.clone(),
                                            selection.save.clone(),
                                            Some(((*name).clone(), version.clone())),
                                            rom,
                                        );
                                    }
                                }
                            });
                        ui.add_enabled_ui(
                            selection
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
                                                    .map(|(_, version)| version.to_string())
                                            })
                                            .unwrap_or("".to_string()),
                                    )
                                    .show_ui(ui, |ui| {
                                        let selection = if let Some(selection) = selection.as_mut()
                                        {
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
                                                let rom = {
                                                    let roms = state.roms_scanner.read();
                                                    roms.get(&selection.game).unwrap().clone()
                                                };
                                                let (rom_code, revision) =
                                                    selection.game.rom_code_and_revision();

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

                                                *selection = gui::main_view::Selection::new(
                                                    selection.game.clone(),
                                                    selection.save.clone(),
                                                    Some((patch.0.clone(), (*version).clone())),
                                                    rom,
                                                );
                                            }
                                        }
                                    });
                            },
                        );
                    });
                });
            });

            (
                selection
                    .as_ref()
                    .map(|selection| (selection.game, selection.patch.clone()))
                    != initial,
                selection.is_some(),
            )
        };

        if selection_changed {
            let connection_task = main_view.connection_task.blocking_lock();
            if let Some(ConnectionTask::InProgress {
                state: ConnectionState::InLobby(lobby),
                ..
            }) = &*connection_task
            {
                handle.block_on(async {
                    let mut lobby = lobby.lock().await;
                    lobby.match_type = (
                        if main_view
                            .selection
                            .lock()
                            .as_ref()
                            .map(|selection| {
                                (lobby.match_type.0 as usize) < selection.game.match_types().len()
                            })
                            .unwrap_or(false)
                        {
                            lobby.match_type.0
                        } else {
                            0
                        },
                        0,
                    );
                    let settings = lobby.make_local_settings();
                    let _ = lobby.send_settings(settings).await;
                    let patches = state.patches_scanner.read();
                    if SimplifiedSettings::new(&lobby.make_local_settings(), &patches)
                        != SimplifiedSettings::new(&lobby.remote_settings, &patches)
                    {
                        lobby.remote_commitment = None;
                    }
                });
            }
        }

        egui::TopBottomPanel::bottom("main-bottom-panel").show(ctx, |ui| {
            ui.vertical(|ui| {
                {
                    let connection_task = main_view.connection_task.blocking_lock();
                    if let Some(ConnectionTask::InProgress {
                        state: connection_state,
                        ..
                    }) = &*connection_task
                    {
                        match connection_state {
                            ConnectionState::Starting => {
                                ui.horizontal(|ui| {
                                    ui.add(egui::Spinner::new().size(10.0));
                                    ui.label(
                                        i18n::LOCALES
                                            .lookup(
                                                &config.language,
                                                "main-connection-task.starting",
                                            )
                                            .unwrap(),
                                    );
                                });
                            }
                            ConnectionState::Signaling => {
                                ui.horizontal(|ui| {
                                    ui.add(egui::Spinner::new().size(10.0));
                                    ui.label(
                                        i18n::LOCALES
                                            .lookup(
                                                &config.language,
                                                "main-connection-task.signaling",
                                            )
                                            .unwrap(),
                                    );
                                });
                            }
                            ConnectionState::Waiting => {
                                ui.horizontal(|ui| {
                                    ui.add(egui::Spinner::new().size(10.0));
                                    ui.label(
                                        i18n::LOCALES
                                            .lookup(
                                                &config.language,
                                                "main-connection-task.waiting",
                                            )
                                            .unwrap(),
                                    );
                                });
                            }
                            ConnectionState::InLobby(lobby) => {
                                let mut lobby = lobby.blocking_lock();
                                if !lobby.attention_requested {
                                    window.request_user_attention(Some(
                                        glutin::window::UserAttentionType::Critical,
                                    ));
                                    lobby.attention_requested = true;
                                }

                                ui.add_enabled_ui(
                                    lobby.local_negotiated_state.is_none()
                                        && lobby.sender.is_some(),
                                    |ui| {
                                        egui_extras::TableBuilder::new(ui)
                                            .column(egui_extras::Size::remainder())
                                            .column(egui_extras::Size::exact(200.0))
                                            .column(egui_extras::Size::exact(200.0))
                                            .header(20.0, |mut header| {
                                                header.col(|_ui| {});
                                                header.col(|ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.strong(
                                                            i18n::LOCALES
                                                                .lookup(
                                                                    &config.language,
                                                                    "main.you",
                                                                )
                                                                .unwrap(),
                                                        );
                                                        if lobby.local_negotiated_state.is_some()
                                                            || lobby.sender.is_none()
                                                        {
                                                            ui.label("✅");
                                                        }
                                                    });
                                                });
                                                header.col(|ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.strong(
                                                            lobby.remote_settings.nickname.clone(),
                                                        );
                                                        if lobby.remote_commitment.is_some() {
                                                            ui.label("✅");
                                                        }
                                                    });
                                                });
                                            })
                                            .body(|mut body| {
                                                body.row(20.0, |mut row| {
                                                    row.col(|ui| {
                                                        ui.strong(
                                                            i18n::LOCALES
                                                                .lookup(
                                                                    &config.language,
                                                                    "main-details.game",
                                                                )
                                                                .unwrap(),
                                                        );
                                                    });
                                                    row.col(|ui| {
                                                        ui.label(
                                                            if let Some(selection) =
                                                                &*main_view.selection.lock()
                                                            {
                                                                let (family, _) = selection
                                                                    .game
                                                                    .family_and_variant();
                                                                i18n::LOCALES
                                                                    .lookup(
                                                                        &config.language,
                                                                        &format!("game-{}", family),
                                                                    )
                                                                    .unwrap()
                                                            } else {
                                                                i18n::LOCALES
                                                                    .lookup(
                                                                        &config.language,
                                                                        "main.no-game",
                                                                    )
                                                                    .unwrap()
                                                            },
                                                        );
                                                    });
                                                    row.col(|ui| {
                                                        ui.label(
                                                        if let Some(game) = lobby
                                                            .remote_settings
                                                            .game_info
                                                            .as_ref()
                                                            .and_then(|game_info| {
                                                                let (family, variant) =
                                                                    &game_info.family_and_variant;
                                                                game::find_by_family_and_variant(
                                                                    &family, *variant,
                                                                )
                                                            })
                                                        {
                                                            let (family, _) =
                                                                game.family_and_variant();
                                                            i18n::LOCALES
                                                                .lookup(
                                                                    &config.language,
                                                                    &format!("game-{}", family),
                                                                )
                                                                .unwrap()
                                                        } else {
                                                            i18n::LOCALES
                                                                .lookup(
                                                                    &config.language,
                                                                    "main.no-game",
                                                                )
                                                                .unwrap()
                                                        },
                                                    );
                                                    });
                                                });

                                                body.row(20.0, |mut row| {
                                                    row.col(|ui| {
                                                        ui.strong(
                                                            i18n::LOCALES
                                                                .lookup(
                                                                    &config.language,
                                                                    "main-details.match-type",
                                                                )
                                                                .unwrap(),
                                                        );
                                                    });
                                                    row.col(|ui| {
                                                        let game = lobby
                                                            .selection
                                                            .lock()
                                                            .as_ref()
                                                            .map(|selection| selection.game);
                                                        ui.add_enabled_ui(game.is_some(), |ui| {
                                                            egui::ComboBox::new(
                                                                "start-match-type-combobox",
                                                                "",
                                                            )
                                                            .width(150.0)
                                                            .selected_text(
                                                                if let Some(game) = game.as_ref() {
                                                                    i18n::LOCALES.lookup(&config.language,
                                                                        &format!(
                                                                            "game-{}.match-type-{}-{}",
                                                                            game.family_and_variant().0,
                                                                            lobby.match_type.0,
                                                                            lobby.match_type.1
                                                                        )).unwrap()
                                                                } else {
                                                                    "".to_string()
                                                                },
                                                            )
                                                            .show_ui(ui, |ui| {
                                                                if let Some(game) = game {
                                                                    let mut match_type =
                                                                        lobby.match_type;
                                                                    for (typ, subtype_count) in game
                                                                        .match_types()
                                                                        .iter()
                                                                        .enumerate()
                                                                    {
                                                                        for subtype in
                                                                            0..*subtype_count
                                                                        {
                                                                            ui.selectable_value(
                                                                                &mut match_type,
                                                                                (
                                                                                    typ as u8,
                                                                                    subtype as u8,
                                                                                ),
                                                                                i18n::LOCALES.lookup(&config.language,
                                                                                    &format!(
                                                                                        "game-{}.match-type-{}-{}",
                                                                                        game.family_and_variant().0,
                                                                                        typ,
                                                                                        subtype
                                                                                    )).unwrap(),
                                                                            );
                                                                        }
                                                                        config.default_match_type =
                                                                            match_type.0;
                                                                    }
                                                                    if match_type
                                                                        != lobby.match_type
                                                                    {
                                                                        handle.block_on(async {
                                                                            let _ = lobby
                                                                                .set_match_type(
                                                                                    match_type,
                                                                                )
                                                                                .await;
                                                                        });
                                                                    }
                                                                }
                                                            });
                                                        });
                                                    });
                                                    row.col(|ui| {
                                                        ui.label(
                                                            if let Some(game_info) = lobby
                                                                .remote_settings
                                                                .game_info
                                                                .as_ref()
                                                            {
                                                                i18n::LOCALES.lookup(&config.language,
                                                                    &format!(
                                                                        "game-{}.match-type-{}-{}",
                                                                        game_info.family_and_variant.0,
                                                                        lobby.remote_settings.match_type.0,
                                                                        lobby.remote_settings.match_type.1,
                                                                    )).unwrap()
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
                                                                .lookup(
                                                                    &config.language,
                                                                    "main-details.reveal-setup",
                                                                )
                                                                .unwrap(),
                                                        );
                                                    });
                                                    row.col(|ui| {
                                                        let mut checked = lobby.reveal_setup;
                                                        ui.checkbox(&mut checked, "");
                                                        handle.block_on(async {
                                                            let _ = lobby
                                                                .set_reveal_setup(checked)
                                                                .await;
                                                        });
                                                    });
                                                    row.col(|ui| {
                                                        ui.checkbox(
                                                            &mut lobby
                                                                .remote_settings
                                                                .reveal_setup
                                                                .clone(),
                                                            "",
                                                        );
                                                    });
                                                });
                                            });
                                    },
                                );
                            }
                        }
                    }
                }

                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let submit = |main_view: &State| {
                            if !main_view.link_code.is_empty() {
                                let cancellation_token = tokio_util::sync::CancellationToken::new();
                                *main_view.connection_task.blocking_lock() =
                                    Some(ConnectionTask::InProgress {
                                        state: ConnectionState::Starting,
                                        cancellation_token: cancellation_token.clone(),
                                    });

                                handle.spawn(run_connection_task(
                                    state.config.clone(),
                                    handle.clone(),
                                    ctx.clone(),
                                    state.audio_binder.clone(),
                                    state.emu_tps_counter.clone(),
                                    state.main_view.clone(),
                                    main_view.selection.clone(),
                                    state.roms_scanner.clone(),
                                    state.patches_scanner.clone(),
                                    if !config.matchmaking_endpoint.is_empty() {
                                        config.matchmaking_endpoint.clone()
                                    } else {
                                        config::DEFAULT_MATCHMAKING_ENDPOINT.to_string()
                                    },
                                    main_view.link_code.clone(),
                                    config.max_queue_length as usize,
                                    config.nickname.clone().unwrap_or_else(|| "".to_string()),
                                    config.replays_path(),
                                    main_view.connection_task.clone(),
                                    cancellation_token,
                                ));
                            } else if let Some(selection) = &*main_view.selection.lock() {
                                let audio_binder = state.audio_binder.clone();
                                let save_path = selection.save.path.clone();
                                let main_view = state.main_view.clone();
                                let emu_tps_counter = state.emu_tps_counter.clone();
                                let rom = selection.rom.clone();
                                let egui_ctx = ctx.clone();

                                // We have to run this in a thread in order to lock main_view safely. Furthermore, we have to use a real thread because of parking_lot::Mutex.
                                rayon::spawn(move || {
                                    main_view.lock().session = Some(
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
                        };

                        let (lobby, cancellation_token) = if let Some(connection_task) =
                            &*main_view.connection_task.blocking_lock()
                        {
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
                            let connection_task = main_view.connection_task.blocking_lock();
                            if let Some(ConnectionTask::Failed(err)) = &*connection_task {
                                let mut open = true;
                                egui::Window::new("")
                                    .id(egui::Id::new("connection-failed-window"))
                                    .open(&mut open)
                                    .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                                    .show(ctx, |ui| {
                                        // TODO: Localization
                                        ui.label(format!("{:?}", err));
                                    });
                                open
                            } else {
                                false
                            }
                        };

                        if !error_window_open {
                            let mut connection_task = main_view.connection_task.blocking_lock();
                            if let Some(ConnectionTask::Failed(_)) = &*connection_task {
                                *connection_task = None;
                            }
                        }

                        if let Some(cancellation_token) = &cancellation_token {
                            if ui
                                .add_enabled(
                                    !error_window_open,
                                    egui::Button::new(format!(
                                        "⏹️ {}",
                                        i18n::LOCALES
                                            .lookup(&config.language, "main.stop")
                                            .unwrap()
                                    )),
                                )
                                .clicked()
                            {
                                cancellation_token.cancel();
                            }
                        } else {
                            if ui
                                .add_enabled(
                                    !error_window_open
                                        && (!main_view.link_code.is_empty() || has_selection),
                                    egui::Button::new(if main_view.link_code.is_empty() {
                                        format!(
                                            "▶️ {}",
                                            i18n::LOCALES
                                                .lookup(&config.language, "main.play")
                                                .unwrap()
                                        )
                                    } else {
                                        format!(
                                            "🥊 {}",
                                            i18n::LOCALES
                                                .lookup(&config.language, "main.fight")
                                                .unwrap()
                                        )
                                    }),
                                )
                                .clicked()
                            {
                                submit(&main_view);
                            }

                            if ui
                                .add_enabled(!error_window_open, egui::Button::new("🎲"))
                                .on_hover_text(
                                    i18n::LOCALES
                                        .lookup(&config.language, "main.random")
                                        .unwrap(),
                                )
                                .clicked()
                            {
                                main_view.link_code = randomcode::generate(&config.language);
                                let _ = state.clipboard.set_text(main_view.link_code.clone());
                            }
                        }

                        if let Some(lobby) = lobby {
                            let mut lobby = lobby.blocking_lock();
                            let mut ready =
                                lobby.local_negotiated_state.is_some() || lobby.sender.is_none();
                            let was_ready = ready;
                            let patches = state.patches_scanner.read();
                            ui.add_enabled(
                                has_selection
                                    && lobby.remote_settings.game_info.is_some()
                                    && SimplifiedSettings::new(
                                        &lobby.make_local_settings(),
                                        &patches,
                                    ) == SimplifiedSettings::new(
                                        &lobby.remote_settings,
                                        &patches,
                                    )
                                    && lobby.sender.is_some(),
                                egui::Checkbox::new(
                                    &mut ready,
                                    i18n::LOCALES
                                        .lookup(&config.language, "main.ready")
                                        .unwrap(),
                                ),
                            );
                            if error_window_open {
                                ready = was_ready;
                            }
                            if lobby.sender.is_some() {
                                handle.block_on(async {
                                    if !was_ready && ready {
                                        let save_data = lobby
                                            .selection
                                            .lock()
                                            .as_ref()
                                            .map(|selection| selection.save.save.to_vec());
                                        if let Some(save_data) = save_data {
                                            let _ = lobby.commit(&save_data).await;
                                        }
                                    } else if !ready {
                                        let _ = lobby.uncommit().await;
                                    }
                                });
                            }
                        }

                        let input_resp = ui.add_enabled(
                            cancellation_token.is_none() && !error_window_open,
                            egui::TextEdit::singleline(&mut main_view.link_code)
                                .hint_text(
                                    i18n::LOCALES
                                        .lookup(&config.language, "main.link-code")
                                        .unwrap(),
                                )
                                .desired_width(f32::INFINITY),
                        );
                        main_view.link_code = main_view
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

                        if let Some(last) = main_view.link_code.chars().last() {
                            if last == '-' {
                                main_view.link_code = main_view
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

                        if input_resp.lost_focus() && ctx.input().key_pressed(egui::Key::Enter) {
                            submit(&main_view);
                        }
                    });
                });
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut selection = main_view.selection.lock();
            if let Some(selection) = &mut *selection {
                if let Some(assets) = selection.assets.as_ref() {
                    self.save_view.show(
                        ui,
                        &mut state.clipboard,
                        font_families,
                        &config.language,
                        selection.game,
                        &selection.save.save,
                        assets,
                        &mut selection.save_view_state,
                    );
                }
            }
        });
    }
}

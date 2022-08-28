use fluent_templates::Loader;
use rand::RngCore;
use sha3::digest::{ExtendableOutput, Update};

use crate::{audio, config, games, gui, i18n, input, net, patch, session, stats};

use super::save_select_window;

pub struct State {
    pub session: Option<session::Session>,
    link_code: String,
    selection: std::sync::Arc<parking_lot::Mutex<Option<Selection>>>,
    connection_task: std::sync::Arc<tokio::sync::Mutex<Option<ConnectionTask>>>,
    show_save_select: Option<gui::save_select_window::State>,
}

impl State {
    pub fn new() -> Self {
        Self {
            session: None,
            link_code: String::new(),
            selection: std::sync::Arc::new(parking_lot::Mutex::new(None)),
            connection_task: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            show_save_select: None,
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

#[derive(Clone)]
pub struct Selection {
    pub game: &'static (dyn games::Game + Send + Sync),
    pub rom: Vec<u8>,
    pub save_path: std::path::PathBuf,
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
}

#[derive(PartialEq)]
struct SimplifiedSettings {
    netplay_compatiblity: Option<String>,
    match_type: (u8, u8),
}

impl SimplifiedSettings {
    fn new(settings: &net::protocol::Settings) -> Self {
        Self {
            netplay_compatiblity: settings
                .game_info
                .as_ref()
                .map(|g| g.family_and_variant.0.clone()),
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
        net::protocol::Settings {
            nickname: self.nickname.clone(),
            match_type: self.match_type,
            game_info: self.selection.lock().as_ref().map(|selection| {
                let (family, variant) = selection.game.family_and_variant();
                net::protocol::GameInfo {
                    family_and_variant: (family.to_string(), variant),
                    patch: None,
                }
            }),
            available_games: vec![], // TODO
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

    fn set_remote_settings(&mut self, settings: net::protocol::Settings) {
        let old_reveal_setup = self.remote_settings.reveal_setup;
        self.remote_settings = settings;
        if SimplifiedSettings::new(&self.make_local_settings())
            != SimplifiedSettings::new(&self.remote_settings)
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
    audio_binder: audio::LateBinder,
    emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    main_view: std::sync::Arc<parking_lot::Mutex<State>>,
    selection: std::sync::Arc<parking_lot::Mutex<Option<Selection>>>,
    roms_scanner: gui::ROMsScanner,
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

                    let lobby = std::sync::Arc::new(tokio::sync::Mutex::new(Lobby{
                        attention_requested: false,
                        sender: Some(sender),
                        selection: selection.clone(),
                        nickname,
                        match_type: (0, 0), // TODO
                        reveal_setup: false,
                        remote_settings: net::protocol::Settings::default(),
                        remote_commitment: None,
                        latencies: stats::DeltaCounter::new(10),
                        local_negotiated_state: None,
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
                    const PING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
                    let mut ping_timer = tokio::time::interval(PING_INTERVAL);
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
                                        }
                                    },
                                    net::protocol::Packet::Settings(settings) => {
                                        let mut lobby = lobby.lock().await;
                                        lobby.set_remote_settings(settings);
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

                    let remote_commitment = make_commitment(&raw_remote_negotiated_state);
                    if !constant_time_eq::constant_time_eq_16(&remote_commitment, &received_remote_commitment) {
                        anyhow::bail!("commitment did not match");
                    }

                    let remote_negotiated_state = zstd::stream::decode_all(&raw_remote_negotiated_state[..]).map_err(|e| e.into()).and_then(|r| net::protocol::NegotiatedState::deserialize(&r))?;

                    let rng_seed = std::iter::zip(local_negotiated_state.nonce, remote_negotiated_state.nonce).map(|(x, y)| x ^ y).collect::<Vec<_>>().try_into().unwrap();
                    log::info!("session verified! rng seed = {:02x?}", rng_seed);

                    let (local_game, local_rom) = if let Some(selection) = selection.lock().as_ref() { // DEADLOCK HERE?
                        (selection.game, selection.rom.clone())
                    } else {
                        anyhow::bail!("attempted to start match in invalid state");
                    };

                    let remote_game = if let Some(game) = remote_settings.game_info.as_ref().and_then(|gi| {
                        let (family, variant) = &gi.family_and_variant;
                        games::find_by_family_and_variant(family, *variant)
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
}

impl MainView {
    pub fn new() -> Self {
        Self {
            session_view: gui::session_view::SessionView::new(),
            save_select_window: gui::save_select_window::SaveSelectWindow::new(),
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
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

        let (selection_changed, has_selection) = {
            let mut selection = main_view.selection.lock();
            let selection = &mut *selection;

            let initial_game = selection.as_ref().map(|selection| selection.game);

            self.save_select_window.show(
                ctx,
                &mut main_view.show_save_select,
                selection,
                &config.language,
                &config.saves_path(),
                state.roms_scanner.clone(),
                state.saves_scanner.clone(),
            );

            (
                selection.as_ref().map(|selection| selection.game) != initial_game,
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
                    let settings = lobby.make_local_settings();
                    let _ = lobby.send_settings(settings).await;
                    if SimplifiedSettings::new(&lobby.make_local_settings())
                        != SimplifiedSettings::new(&lobby.remote_settings)
                    {
                        lobby.remote_commitment = None;
                    }
                });
            }
        }

        egui::TopBottomPanel::top("main-top-panel")
            .frame(egui::Frame {
                inner_margin: egui::style::Margin::symmetric(8.0, 2.0),
                rounding: egui::Rounding::none(),
                fill: ctx.style().visuals.window_fill(),
                ..Default::default()
            })
            .show(ctx, |ui| {
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
                    });
                });
            });
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
                                                            ui.strong("✅");
                                                        }
                                                    });
                                                });
                                                header.col(|ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.strong(
                                                            lobby.remote_settings.nickname.clone(),
                                                        );
                                                        if lobby.remote_commitment.is_some() {
                                                            ui.strong("✅");
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
                                                                        &format!(
                                                                            "games.{}",
                                                                            family
                                                                        ),
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
                                                                games::find_by_family_and_variant(
                                                                    &family, *variant,
                                                                )
                                                            })
                                                        {
                                                            let (family, _) =
                                                                game.family_and_variant();
                                                            i18n::LOCALES
                                                                .lookup(
                                                                    &config.language,
                                                                    &format!("games.{}", family),
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
                                                        egui::ComboBox::new(
                                                            "start-match-type-combobox",
                                                            "",
                                                        )
                                                        .width(94.0)
                                                        .selected_text(format!(
                                                            "{:?}",
                                                            lobby.match_type
                                                        ))
                                                        .show_ui(ui, |ui| {
                                                            let game = lobby
                                                                .selection
                                                                .lock()
                                                                .as_ref()
                                                                .map(|selection| selection.game);
                                                            if let Some(game) = game {
                                                                let mut match_type =
                                                                    lobby.match_type;
                                                                for (typ, subtype_count) in game
                                                                    .match_types()
                                                                    .iter()
                                                                    .enumerate()
                                                                {
                                                                    for subtype in 0..*subtype_count
                                                                    {
                                                                        ui.selectable_value(
                                                                            &mut match_type,
                                                                            (
                                                                                typ as u8,
                                                                                subtype as u8,
                                                                            ),
                                                                            format!(
                                                                                "{:?}",
                                                                                (typ, subtype)
                                                                            ),
                                                                        );
                                                                    }
                                                                }
                                                                if match_type != lobby.match_type {
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
                                                    row.col(|ui| {
                                                        ui.label(format!(
                                                            "{:?}",
                                                            lobby.remote_settings.match_type
                                                        ));
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
                                    state.audio_binder.clone(),
                                    state.emu_tps_counter.clone(),
                                    state.main_view.clone(),
                                    main_view.selection.clone(),
                                    state.roms_scanner.clone(),
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
                                let save_path = selection.save_path.clone();
                                let main_view = state.main_view.clone();
                                let emu_tps_counter = state.emu_tps_counter.clone();
                                let rom = selection.rom.clone();

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
                        }

                        if let Some(lobby) = lobby {
                            let mut lobby = lobby.blocking_lock();
                            let mut ready =
                                lobby.local_negotiated_state.is_some() || lobby.sender.is_none();
                            let was_ready = ready;
                            ui.add_enabled(
                                has_selection
                                    && SimplifiedSettings::new(&lobby.make_local_settings())
                                        == SimplifiedSettings::new(&lobby.remote_settings)
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
                                        let save_path = lobby
                                            .selection
                                            .lock()
                                            .as_ref()
                                            .map(|selection| selection.save_path.clone());
                                        if let Some(save_path) = save_path {
                                            if let Ok(save_data) = std::fs::read(&save_path) {
                                                let _ = lobby.commit(&save_data).await;
                                            }
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
            ui.horizontal(|ui| {
                let resp = ui.group(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let resp = ui.add({
                            let button = egui::Button::new(
                                i18n::LOCALES
                                    .lookup(&config.language, "select-save.select-button")
                                    .unwrap(),
                            );

                            if main_view.show_save_select.is_some() {
                                button.fill(ui.ctx().style().visuals.selection.bg_fill)
                            } else {
                                button
                            }
                        });

                        ui.with_layout(
                            egui::Layout::top_down(egui::Align::Min).with_cross_justify(true),
                            |ui| {
                                if let Some(selection) = &*main_view.selection.lock() {
                                    ui.vertical(|ui| {
                                        ui.label(format!(
                                            "{}",
                                            selection
                                                .save_path
                                                .strip_prefix(&config.saves_path())
                                                .unwrap_or(selection.save_path.as_path())
                                                .display()
                                        ));

                                        let (family, variant) = selection.game.family_and_variant();
                                        ui.small(
                                            i18n::LOCALES
                                                .lookup(
                                                    &config.language,
                                                    &format!("games.{}-{}", family, variant),
                                                )
                                                .unwrap(),
                                        );
                                    });
                                } else {
                                    ui.label(
                                        i18n::LOCALES
                                            .lookup(
                                                &config.language,
                                                "select-save.no-game-selected",
                                            )
                                            .unwrap(),
                                    );
                                }
                            },
                        );

                        resp
                    })
                    .inner
                });

                if (resp.inner | resp.response).clicked() {
                    main_view.show_save_select = if main_view.show_save_select.is_none() {
                        rayon::spawn({
                            let roms_scanner = state.roms_scanner.clone();
                            let saves_scanner = state.saves_scanner.clone();
                            let patches_scanner = state.patches_scanner.clone();
                            let roms_path = config.roms_path();
                            let saves_path = config.saves_path();
                            let patches_path = config.patches_path();
                            move || {
                                roms_scanner.rescan(move || games::scan_roms(&roms_path));
                                saves_scanner.rescan(move || games::scan_saves(&saves_path));
                                patches_scanner
                                    .rescan(move || patch::scan(&patches_path).unwrap_or_default());
                            }
                        });
                        Some(save_select_window::State::new(
                            main_view.selection.lock().as_ref().map(|selection| {
                                (selection.game, Some(selection.save_path.to_path_buf()))
                            }),
                        ))
                    } else {
                        None
                    };
                }
            });
        });
    }
}

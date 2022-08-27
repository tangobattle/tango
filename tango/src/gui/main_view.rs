use fluent_templates::Loader;
use rand::RngCore;
use sha3::digest::{ExtendableOutput, Update};

use crate::{audio, games, gui, i18n, input, net, session, stats};

pub enum State {
    Session(session::Session),
    Start(Start),
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

struct Lobby {
    attention_requested: bool,
    sender: Option<net::Sender>,
    is_offerer: bool,
    input_delay: usize,
    nonce: [u8; 16],
    local_game: Option<(&'static (dyn games::Game + Send + Sync), Vec<u8>)>,
    nickname: String,
    match_type: (u8, u8),
    reveal_setup: bool,
    remote_settings: net::protocol::Settings,
    remote_commitment: Option<[u8; 16]>,
    latencies: stats::DeltaCounter,
    local_negotiated_state: Option<(net::protocol::NegotiatedState, Vec<u8>)>,
}

fn are_settings_compatible(
    local_settings: &net::protocol::Settings,
    remote_settings: &net::protocol::Settings,
) -> bool {
    // TODO: Check setting compatibility.
    false
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
        rand::thread_rng().fill_bytes(&mut self.nonce);
        let negotiated_state = net::protocol::NegotiatedState {
            nonce: self.nonce.clone(),
            save_data: save_data.to_vec(),
        };
        let buf = zstd::stream::encode_all(
            &net::protocol::NegotiatedState::serialize(&negotiated_state).unwrap()[..],
            0,
        )?;
        let commitment = make_commitment(&buf);

        log::info!(
            "nonce = {:02x?}, commitment = {:02x?}",
            self.nonce,
            commitment
        );

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
            game_info: self.local_game.as_ref().map(|(game, _)| {
                let (family, variant) = game.family_and_variant();
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

    async fn set_game(
        &mut self,
        game: &'static (dyn games::Game + Send + Sync),
        rom: Vec<u8>,
    ) -> Result<(), anyhow::Error> {
        if Some(game) == self.local_game.as_ref().map(|(g, _)| *g) {
            return Ok(());
        }
        self.send_settings(net::protocol::Settings {
            game_info: self.local_game.as_ref().map(|(game, _)| {
                let (family, variant) = game.family_and_variant();
                net::protocol::GameInfo {
                    family_and_variant: (family.to_string(), variant),
                    patch: None,
                }
            }),
            ..self.make_local_settings()
        })
        .await?;
        self.local_game = Some((game, rom));
        Ok(())
    }

    fn set_remote_settings(&mut self, settings: net::protocol::Settings) {
        self.remote_settings = settings;
        if !are_settings_compatible(&self.make_local_settings(), &self.remote_settings) {
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

pub struct Start {
    link_code: String,
    selected_game: Option<&'static (dyn games::Game + Send + Sync)>,
    connection_task: std::sync::Arc<tokio::sync::Mutex<Option<ConnectionTask>>>,
    show_save_select: Option<gui::save_select_window::State>,
}

async fn run_connection_task(
    handle: tokio::runtime::Handle,
    audio_binder: audio::LateBinder,
    emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    main_view: std::sync::Arc<parking_lot::Mutex<State>>,
    saves_list: gui::SavesListState,
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
                        input_delay: 2, // TODO
                        nonce: [0u8; 16],
                        local_game: None,
                        nickname,
                        match_type: (0, 0), // TODO
                        reveal_setup: false,
                        is_offerer: peer_conn.local_description().unwrap().sdp_type == datachannel_wrapper::SdpType::Offer,
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

                    let mut lobby = lobby.lock().await;
                    let mut sender = if let Some(sender) = lobby.sender.take() {
                        sender
                    } else {
                        anyhow::bail!("no sender?");
                    };

                    let (local_negotiated_state, raw_local_state) = if let Some((negotiated_state, raw_local_state)) = lobby.local_negotiated_state.take() {
                        (negotiated_state, raw_local_state)
                    } else {
                        anyhow::bail!("attempted to start match in invalid state");
                    };

                    log::info!("local state = {:02x?}", raw_local_state);

                    const CHUNK_SIZE: usize = 32 * 1024;
                    const CHUNKS_REQUIRED: usize = 5;
                    for (_, chunk) in std::iter::zip(
                        0..CHUNKS_REQUIRED,
                        raw_local_state.chunks(CHUNK_SIZE).chain(std::iter::repeat(&[][..]))
                     ) {
                        sender.send_chunk(chunk.to_vec()).await?;

                        if remote_chunks.len() < CHUNK_SIZE {
                            loop {
                                match receiver.receive().await? {
                                    net::protocol::Packet::Ping(ping) => {
                                        sender.send_pong(ping.ts).await?;
                                    },
                                    net::protocol::Packet::Pong(pong) => {
                                        if let Ok(d) = std::time::SystemTime::now().duration_since(pong.ts) {
                                            lobby.latencies.mark(d);
                                        }
                                    },
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
                    log::info!("remote state = {:02x?}", raw_remote_negotiated_state);

                    let received_remote_commitment = if let Some(commitment) = lobby.remote_commitment {
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

                    let (local_game, local_rom) = if let Some((game, rom)) = lobby.local_game.as_ref() {
                        (game, rom)
                    } else {
                        anyhow::bail!("attempted to start match in invalid state");
                    };

                    let shadow_game = if let Some(game) = lobby.remote_settings.game_info.as_ref().and_then(|(gi)| {
                        let (family, variant) = &gi.family_and_variant;
                        games::find_by_family_and_variant(family, *variant)
                    }) {
                        game
                    } else {
                        anyhow::bail!("attempted to start match in invalid state");
                    };

                    let shadow_rom = {
                        let saves_list = saves_list.read();
                        if let Some(shadow_rom) = saves_list.roms.get(&shadow_game).cloned() {
                            shadow_rom
                        } else {
                            anyhow::bail!("missing shadow rom");
                        }
                    };

                    sender.send_start_match().await?;
                    match receiver.receive().await? {
                        net::protocol::Packet::StartMatch(_) => {},
                        p => anyhow::bail!("unexpected packet when expecting start match: {:?}", p),
                    }

                    *main_view.lock() = State::Session(session::Session::new_pvp(
                        handle,
                        audio_binder,
                        *local_game,
                        &local_rom,
                        &local_negotiated_state.save_data,
                        shadow_game,
                        &shadow_rom,
                        &remote_negotiated_state.save_data,
                        emu_tps_counter.clone(),
                        sender,
                        receiver,
                        lobby.is_offerer,
                        replays_path,
                        lobby.match_type,
                        lobby.input_delay as u32,
                        std::iter::zip(lobby.nonce, remote_negotiated_state.nonce).map(|(x, y)| x ^ y).collect::<Vec<_>>().try_into().unwrap(),
                        max_queue_length,
                    )?);

                    return Ok(());
                })(
                )
            }
            => { r }
            _ = cancellation_token.cancelled() => {
                *connection_task.lock().await = None;
                return;
            }
        }
    } {
        log::info!("connection task failed: {:?}", e);
        *connection_task.lock().await = Some(ConnectionTask::Failed(e));
    }
}

impl Start {
    pub fn new() -> Self {
        Self {
            link_code: String::new(),
            selected_game: None,
            connection_task: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            show_save_select: None,
        }
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
        handle: tokio::runtime::Handle,
        window: &glutin::window::Window,
        input_state: &input::State,
        state: &mut gui::State,
    ) {
        match &mut *state.main_view.lock() {
            State::Session(session) => {
                self.session_view.show(
                    ctx,
                    input_state,
                    &state.config.input_mapping,
                    session,
                    &state.config.video_filter,
                    state.config.max_scale,
                );
            }
            State::Start(start) => {
                self.save_select_window.show(
                    ctx,
                    &mut start.show_save_select,
                    &mut start.selected_game,
                    &state.config.language,
                    &state.config.saves_path,
                    state.saves_list.clone(),
                );

                egui::TopBottomPanel::top("main-top-panel")
                    .frame(egui::Frame {
                        inner_margin: egui::style::Margin::symmetric(8.0, 2.0),
                        rounding: egui::Rounding::none(),
                        fill: ctx.style().visuals.window_fill(),
                        ..Default::default()
                    })
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .selectable_label(state.show_settings.is_some(), "⚙️")
                                        .on_hover_text_at_pointer(
                                            i18n::LOCALES
                                                .lookup(&state.config.language, "settings")
                                                .unwrap(),
                                        )
                                        .clicked()
                                    {
                                        state.show_settings = if state.show_settings.is_none() {
                                            Some(gui::settings_window::State::new())
                                        } else {
                                            None
                                        };
                                    }
                                },
                            );
                        });
                    });
                egui::TopBottomPanel::bottom("main-bottom-panel").show(ctx, |ui| {
                    ui.vertical(|ui| {
                        {
                            let connection_task = start.connection_task.blocking_lock();
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
                                                        &state.config.language,
                                                        "start-connection-task.starting",
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
                                                        &state.config.language,
                                                        "start-connection-task.signaling",
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
                                                        &state.config.language,
                                                        "start-connection-task.waiting",
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

                                        egui_extras::TableBuilder::new(ui)
                                            .column(egui_extras::Size::remainder())
                                            .column(egui_extras::Size::relative(0.1))
                                            .column(egui_extras::Size::relative(0.1))
                                            .header(20.0, |mut header| {
                                                header.col(|ui| {});
                                                header.col(|ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.strong("You");
                                                        if lobby.local_negotiated_state.is_some() {
                                                            ui.strong("✅");
                                                        }
                                                    });
                                                });
                                                header.col(|ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.strong("Remote");
                                                        if lobby.remote_commitment.is_some() {
                                                            ui.strong("✅");
                                                        }
                                                    });
                                                });
                                            })
                                            .body(|mut body| {
                                                body.row(20.0, |mut row| {
                                                    row.col(|ui| {
                                                        ui.strong("Game");
                                                    });
                                                    row.col(|ui| {
                                                        ui.label("A");
                                                    });
                                                    row.col(|ui| {
                                                        ui.label("B");
                                                    });
                                                });

                                                body.row(20.0, |mut row| {
                                                    row.col(|ui| {
                                                        ui.strong("Reveal setup");
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
                                    }
                                    _ => {}
                                }
                            }
                        }

                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let submit = |start: &Start| {
                                        if !start.link_code.is_empty() {
                                            let cancellation_token =
                                                tokio_util::sync::CancellationToken::new();
                                            *start.connection_task.blocking_lock() =
                                                Some(ConnectionTask::InProgress {
                                                    state: ConnectionState::Starting,
                                                    cancellation_token: cancellation_token.clone(),
                                                });

                                            handle.spawn(run_connection_task(
                                                handle.clone(),
                                                state.audio_binder.clone(),
                                                state.emu_tps_counter.clone(),
                                                state.main_view.clone(),
                                                state.saves_list.clone(),
                                                state.config.matchmaking_endpoint.clone(),
                                                start.link_code.clone(),
                                                state.config.max_queue_length as usize,
                                                state
                                                    .config
                                                    .nickname
                                                    .clone()
                                                    .unwrap_or_else(|| "".to_string()),
                                                state.config.replays_path.clone(),
                                                start.connection_task.clone(),
                                                cancellation_token,
                                            ));
                                        }
                                    };

                                    let (lobby, cancellation_token) = if let Some(connection_task) =
                                        &*start.connection_task.blocking_lock()
                                    {
                                        match connection_task {
                                            ConnectionTask::InProgress {
                                                state: task_state,
                                                cancellation_token,
                                            } => (
                                                if let ConnectionState::InLobby(lobby) = task_state
                                                {
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

                                    if let Some(cancellation_token) = &cancellation_token {
                                        if ui
                                            .button(format!(
                                                "⏹️ {}",
                                                i18n::LOCALES
                                                    .lookup(&state.config.language, "start.stop")
                                                    .unwrap()
                                            ))
                                            .clicked()
                                        {
                                            cancellation_token.cancel();
                                        }
                                    } else {
                                        if ui
                                            .button(if start.link_code.is_empty() {
                                                format!(
                                                    "▶️ {}",
                                                    i18n::LOCALES
                                                        .lookup(
                                                            &state.config.language,
                                                            "start.play"
                                                        )
                                                        .unwrap()
                                                )
                                            } else {
                                                format!(
                                                    "🥊 {}",
                                                    i18n::LOCALES
                                                        .lookup(
                                                            &state.config.language,
                                                            "start.fight"
                                                        )
                                                        .unwrap()
                                                )
                                            })
                                            .clicked()
                                        {
                                            submit(start);
                                        }
                                    }

                                    if let Some(lobby) = lobby {
                                        let mut lobby = lobby.blocking_lock();
                                        let mut ready = lobby.local_negotiated_state.is_some();
                                        let was_ready = ready;
                                        ui.checkbox(
                                            &mut ready,
                                            i18n::LOCALES
                                                .lookup(&state.config.language, "start.ready")
                                                .unwrap(),
                                        );
                                        handle.block_on(async {
                                            if !was_ready && ready {
                                                // TODO
                                                let _ = lobby.commit(&[]).await;
                                            } else if !ready {
                                                let _ = lobby.uncommit().await;
                                            }
                                        });
                                    }

                                    let input_resp = ui.add(
                                        egui::TextEdit::singleline(&mut start.link_code)
                                            .interactive(cancellation_token.is_none())
                                            .hint_text(
                                                i18n::LOCALES
                                                    .lookup(
                                                        &state.config.language,
                                                        "start.link-code",
                                                    )
                                                    .unwrap(),
                                            )
                                            .desired_width(f32::INFINITY),
                                    );
                                    start.link_code = start
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

                                    if let Some(last) = start.link_code.chars().last() {
                                        if last == '-' {
                                            start.link_code = start
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
                                        && ctx.input().key_pressed(egui::Key::Enter)
                                    {
                                        submit(start);
                                    }
                                },
                            );
                        });
                    });
                });
                egui::CentralPanel::default().show(ctx, |ui| {});
            }
        }
    }
}

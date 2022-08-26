use fluent_templates::Loader;

use crate::{games, gui, i18n, input, net, session, stats};

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
    InLobby(std::sync::Arc<tokio::sync::Mutex<Lobby>>),
    Failed(anyhow::Error),
}

enum ConnectionState {
    Starting,
    Signaling,
    Waiting,
}

struct Lobby {
    sender: net::Sender,
    is_offerer: bool,
    remote_settings: net::protocol::Settings,
    remote_commit: Option<net::protocol::Commit>,
    latencies: stats::DeltaCounter,
    raw_negotiated_state: Vec<u8>,
}

pub struct Start {
    link_code: String,
    connection_task: std::sync::Arc<tokio::sync::Mutex<Option<ConnectionTask>>>,
    show_save_select: Option<gui::save_select_window::State>,
}

async fn run_connection_task(
    saves_list: gui::SavesListState,
    matchmaking_addr: String,
    link_code: String,
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
                        sender,
                        is_offerer: peer_conn.local_description().unwrap().sdp_type == datachannel_wrapper::SdpType::Offer,
                        remote_settings: net::protocol::Settings::default(),
                        remote_commit: None,
                        latencies: stats::DeltaCounter::new(10),
                        raw_negotiated_state: vec![],
                    }));

                    *connection_task.lock().await = Some(ConnectionTask::InLobby(lobby.clone()));

                    loop {
                        match receiver.receive().await? {
                            net::protocol::Packet::Ping(ping) => {
                                lobby.lock().await.sender.send_pong(ping.ts).await?;
                            },
                            net::protocol::Packet::Pong(pong) => {
                                let mut lobby = lobby.lock().await;
                                if let Ok(d) = std::time::SystemTime::now().duration_since(pong.ts) {
                                    lobby.latencies.mark(d);
                                }
                            },
                            net::protocol::Packet::Settings(settings) => {
                                lobby.lock().await.remote_settings = settings;
                            },
                            net::protocol::Packet::Commit(commit) => {
                                lobby.lock().await.remote_commit = Some(commit);
                            },
                            net::protocol::Packet::Uncommit(_) => {
                                lobby.lock().await.remote_commit = None;
                            },
                            net::protocol::Packet::Chunk(chunk) => {
                                lobby.lock().await.raw_negotiated_state.extend(chunk.chunk);
                            },
                            net::protocol::Packet::StartMatch(_) => {
                                let lobby = lobby.lock().await;

                                let shadow_game = if let Some(game) = lobby.remote_settings.game_info.family_and_variant.as_ref().and_then(|(family, variant)| games::find_by_family_and_variant(family, *variant)) {
                                    game
                                } else {
                                    anyhow::bail!("attempted to start match in invalid state");
                                };

                                let shadow_rom = {
                                    let saves_list = saves_list.read();
                                    saves_list.roms.get(&shadow_game).cloned()
                                };

                                *connection_task.lock().await = None;
                                return Ok(());
                            },
                            p => {
                                anyhow::bail!("unexpected packet: {:?}", p);
                            }
                        }
                    }
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
        input_state: &input::State,
        state: &mut gui::State,
    ) {
        match &mut state.main_view {
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
                    &state.config.language,
                    &state.config.saves_path,
                    state.saves_list.clone(),
                    state.audio_binder.clone(),
                    state.emu_tps_counter.clone(),
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
                egui::TopBottomPanel::bottom("main-bottom-panel")
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
                                    let submit = |start: &Start| {
                                        if !start.link_code.is_empty() {
                                            let cancellation_token =
                                                tokio_util::sync::CancellationToken::new();
                                            let connection_task = start.connection_task.clone();

                                            *connection_task.blocking_lock() =
                                                Some(ConnectionTask::InProgress {
                                                    state: ConnectionState::Starting,
                                                    cancellation_token: cancellation_token.clone(),
                                                });

                                            handle.spawn(run_connection_task(
                                                state.saves_list.clone(),
                                                state.config.matchmaking_endpoint.clone(),
                                                start.link_code.clone(),
                                                connection_task,
                                                cancellation_token,
                                            ));
                                        }
                                    };

                                    let cancellation_token = if let Some(connection_task) =
                                        &*start.connection_task.blocking_lock()
                                    {
                                        match connection_task {
                                            ConnectionTask::InProgress {
                                                state: _,
                                                cancellation_token,
                                            } => Some(cancellation_token.clone()),
                                            ConnectionTask::InLobby(_) => None,
                                            ConnectionTask::Failed(_) => None,
                                        }
                                    } else {
                                        None
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
                egui::CentralPanel::default().show(ctx, |ui| {});
            }
        }
    }
}

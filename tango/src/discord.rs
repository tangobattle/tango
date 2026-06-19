//! Discord rich-presence client wrapper. Owns a background
//! tokio task that maintains the IPC connection (auto-reconnect
//! on failure, every 15 s), and exposes setter / poller methods
//! the UI calls from the main thread. Ported from
//! `tango/src/discord.rs`.

use discord_ipc as rpc;

pub use rpc::activity;

use crate::i18n;

const APP_ID: u64 = 974089681333534750;

pub struct GameInfo {
    pub title: String,
    pub family: String,
}

pub fn make_game_info(
    game: crate::rom::GameRef,
    patch: Option<(&str, &semver::Version)>,
    language: &unic_langid::LanguageIdentifier,
) -> GameInfo {
    // Play tab stores `&dyn tango_gamesupport::Game` directly so we
    // read `family_and_variant` straight off the gamedb trait.
    let family = game.family_and_variant().0.to_string();
    // Game-name localization goes through the per-family path, not the
    // app's general i18n loader.
    let mut title =
        crate::game::family_str(&family, language, "name").unwrap_or_else(|| format!("⟦game-{family}⟧"));
    if let Some((patch_name, patch_version)) = patch.as_ref() {
        title.push_str(&format!(" + {} v{}", patch_name, patch_version));
    }
    GameInfo { title, family }
}

pub fn make_base_activity(game_info: Option<GameInfo>) -> rpc::activity::Activity {
    rpc::activity::Activity {
        details: game_info.as_ref().map(|gi| gi.title.clone()),
        assets: Some(rpc::activity::Assets {
            small_image: Some("logo".to_string()),
            small_text: Some("Tango".to_string()),
            large_image: game_info.as_ref().map(|gi| gi.family.clone()),
            large_text: game_info.as_ref().map(|gi| gi.title.clone()),
        }),
        ..Default::default()
    }
}

pub fn make_looking_activity(
    ident: &crate::netplay::LinkIdent,
    lang: &unic_langid::LanguageIdentifier,
    game_info: Option<GameInfo>,
) -> rpc::activity::Activity {
    rpc::activity::Activity {
        state: Some(i18n::t!(lang, "discord-presence-looking")),
        // Neither transport carries a join secret — direct and lobby
        // sessions aren't joinable via Discord deep-link.
        secrets: ident.discord_join_secret().map(|s| rpc::activity::Secrets {
            join: Some(s.to_string()),
            ..Default::default()
        }),
        party: Some(rpc::activity::Party {
            id: party_id(ident),
            size: Some([1, 2]),
        }),
        ..make_base_activity(game_info)
    }
}

/// Rich-presence party identifier. Neither transport has a stable
/// cross-instance identity to group by (direct sessions are
/// machine-local; lobby challenges are per-peer and presence-driven),
/// so we return `None` and Discord drops the grouping.
fn party_id(ident: &crate::netplay::LinkIdent) -> Option<String> {
    match ident {
        crate::netplay::LinkIdent::Direct(_) | crate::netplay::LinkIdent::Lobby => None,
    }
}

pub fn make_single_player_activity(
    start_time: std::time::SystemTime,
    lang: &unic_langid::LanguageIdentifier,
    game_info: Option<GameInfo>,
) -> rpc::activity::Activity {
    rpc::activity::Activity {
        state: Some(i18n::t!(lang, "discord-presence-in-single-player")),
        timestamps: Some(rpc::activity::Timestamps {
            start: start_time
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_millis() as u64),

            end: None,
        }),
        ..make_base_activity(game_info)
    }
}

pub fn make_in_progress_activity(
    start_time: std::time::SystemTime,
    lang: &unic_langid::LanguageIdentifier,
    game_info: Option<GameInfo>,
) -> rpc::activity::Activity {
    rpc::activity::Activity {
        state: Some(i18n::t!(lang, "discord-presence-in-progress")),
        party: Some(rpc::activity::Party {
            id: None,
            size: Some([2, 2]),
        }),
        timestamps: Some(rpc::activity::Timestamps {
            start: start_time
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_millis() as u64),

            end: None,
        }),
        ..make_base_activity(game_info)
    }
}

pub struct Client {
    rpc: std::sync::Arc<tokio::sync::Mutex<Option<rpc::Client>>>,
    current_activity: std::sync::Arc<tokio::sync::Mutex<Option<rpc::activity::Activity>>>,
}

impl Client {
    pub fn new() -> Self {
        let current_activity: std::sync::Arc<tokio::sync::Mutex<Option<rpc::activity::Activity>>> =
            std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let rpc = std::sync::Arc::new(tokio::sync::Mutex::new(None));

        {
            let rpc = rpc.clone();
            let current_activity = current_activity.clone();

            tokio::task::spawn(async move {
                let mut last_err_summary = None;

                loop {
                    {
                        let mut events_rx = {
                            // Try to (re-)establish RPC connection.
                            let mut rpc_guard = rpc.lock().await;
                            let current_activity = current_activity.clone();

                            let (rpc, events_rx) = match async {
                                let (rpc, events_rx) = rpc::Client::connect(APP_ID).await?;
                                rpc.subscribe(rpc::Event::ActivityJoin).await?;
                                if let Some(activity) = current_activity.lock().await.as_ref() {
                                    rpc.set_activity(activity).await?;
                                }
                                Ok::<_, anyhow::Error>((rpc, events_rx))
                            }
                            .await
                            {
                                Ok((rpc, events_rx)) => {
                                    log::info!("connected to discord RPC");
                                    (rpc, events_rx)
                                }
                                Err(err) => {
                                    let err_message = format!("{err:?}");

                                    let err_summary = err_message
                                        .find("Stack backtrace:")
                                        .map(|i| &err_message[..i])
                                        .unwrap_or(&err_message);

                                    if last_err_summary.as_ref().is_none_or(|s| s != err_summary) {
                                        log::warn!("did not open discord RPC client: {err_message}");
                                        last_err_summary = Some(err_summary.to_string());
                                    }

                                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                                    continue;
                                }
                            };
                            *rpc_guard = Some(rpc);
                            events_rx
                        };

                        loop {
                            // Drain any pending events.
                            'l: loop {
                                let event = tokio::select! {
                                    event = events_rx.recv() => { event }
                                    else => { break 'l; }
                                };

                                let Some(_) = event else {
                                    break;
                                };
                                // We subscribe to ActivityJoin to keep the RPC
                                // event stream flowing, but no longer consume
                                // its join secret — link-code joins went away
                                // with matchmaking. Drain and ignore.
                            }

                            // Push the latest activity to the RPC.
                            if let Err(err) = async {
                                if let Some(rpc) = rpc.lock().await.as_ref() {
                                    if let Some(activity) = current_activity.lock().await.as_ref() {
                                        rpc.set_activity(activity).await?;
                                    }
                                }

                                Ok::<_, anyhow::Error>(())
                            }
                            .await
                            {
                                log::warn!("discord RPC client encountered error: {:?}", err);
                                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                                break;
                            }
                        }

                        {
                            let mut rpc_guard = rpc.lock().await;
                            *rpc_guard = None;
                        }
                    }
                }
            });
        }

        Self { rpc, current_activity }
    }

    pub fn set_current_activity(&self, activity: Option<rpc::activity::Activity>) {
        {
            let mut current_activity = self.current_activity.blocking_lock();
            if activity == *current_activity {
                return;
            }
            current_activity.clone_from(&activity);
        }

        if let Some(activity) = activity.as_ref() {
            let activity = activity.clone();
            let rpc = self.rpc.clone();
            // Don't block the UI thread on the actual write.
            tokio::task::spawn(async move {
                if let Some(rpc) = &*rpc.lock().await {
                    let _ = rpc.set_activity(&activity).await;
                }
            });
        }
    }

}

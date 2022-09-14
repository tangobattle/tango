mod rpc;

#[allow(dead_code)]
use fluent_templates::Loader;

use crate::{game, i18n};

const APP_ID: u64 = 974089681333534750;

pub struct GameInfo {
    pub title: String,
    pub family: String,
}

pub fn make_game_info(
    game: &'static (dyn game::Game + Send + Sync),
    patch: Option<(&str, &semver::Version)>,
    language: &unic_langid::LanguageIdentifier,
) -> GameInfo {
    let family = game.family_and_variant().0.to_string();
    let mut title = i18n::LOCALES.lookup(language, &format!("game-{}", family)).unwrap();
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
    link_code: &str,
    lang: &unic_langid::LanguageIdentifier,
    game_info: Option<GameInfo>,
) -> rpc::activity::Activity {
    rpc::activity::Activity {
        state: Some(i18n::LOCALES.lookup(lang, "discord-presence-looking").unwrap()),
        secrets: Some(rpc::activity::Secrets {
            join: Some(link_code.to_string()),
            ..Default::default()
        }),
        party: Some(rpc::activity::Party {
            id: Some(format!("party:{}", link_code)),
            size: Some([1, 2]),
        }),
        ..make_base_activity(game_info)
    }
}

pub fn make_single_player_activity(
    start_time: std::time::SystemTime,
    lang: &unic_langid::LanguageIdentifier,
    game_info: Option<GameInfo>,
) -> rpc::activity::Activity {
    rpc::activity::Activity {
        state: Some(i18n::LOCALES.lookup(lang, "discord-presence-in-single-player").unwrap()),
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

pub fn make_in_lobby_activity(
    link_code: &str,
    lang: &unic_langid::LanguageIdentifier,
    game_info: Option<GameInfo>,
) -> rpc::activity::Activity {
    rpc::activity::Activity {
        state: Some(i18n::LOCALES.lookup(lang, "discord-presence-in-lobby").unwrap()),
        party: Some(rpc::activity::Party {
            id: Some(format!("party:{}", link_code)),
            size: Some([2, 2]),
        }),
        ..make_base_activity(game_info)
    }
}

pub fn make_in_progress_activity(
    link_code: &str,
    start_time: std::time::SystemTime,
    lang: &unic_langid::LanguageIdentifier,
    game_info: Option<GameInfo>,
) -> rpc::activity::Activity {
    rpc::activity::Activity {
        state: Some(i18n::LOCALES.lookup(lang, "discord-presence-in-progress").unwrap()),
        party: Some(rpc::activity::Party {
            id: Some(format!("party:{}", link_code)),
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
    current_join_secret: std::sync::Arc<tokio::sync::Mutex<Option<String>>>,
}

impl Client {
    pub fn new() -> Self {
        let current_activity: std::sync::Arc<tokio::sync::Mutex<Option<rpc::activity::Activity>>> =
            std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let current_join_secret = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let rpc = std::sync::Arc::new(tokio::sync::Mutex::new(None));

        {
            let rpc = rpc.clone();
            let current_activity = current_activity.clone();
            let current_join_secret = current_join_secret.clone();

            tokio::task::spawn(async move {
                loop {
                    {
                        let mut events_rx = {
                            // Try establish RPC connection, if not already open.
                            let mut rpc_guard = rpc.lock().await;
                            let current_activity = current_activity.clone();

                            let (rpc, events_rx) = match (|| async {
                                let (rpc, events_rx) = rpc::Client::connect(APP_ID).await?;
                                rpc.subscribe(rpc::Event::ActivityJoin).await?;
                                if let Some(activity) = current_activity.lock().await.as_ref() {
                                    rpc.set_activity(activity).await?;
                                }
                                Ok::<_, anyhow::Error>((rpc, events_rx))
                            })()
                            .await
                            {
                                Ok((rpc, events_rx)) => {
                                    log::info!("connected to discord RPC");
                                    (rpc, events_rx)
                                }
                                Err(err) => {
                                    log::warn!("did not open discord RPC client: {:?}", err);
                                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                                    continue;
                                }
                            };
                            *rpc_guard = Some(rpc);
                            events_rx
                        };

                        loop {
                            // Service any events.
                            'l: loop {
                                let event = tokio::select! {
                                    event = events_rx.recv() => { event }
                                    else => { break 'l; }
                                };

                                let (event, v) = if let Some(event) = event {
                                    event
                                } else {
                                    break;
                                };

                                // We only care about activity joins.
                                if event != rpc::Event::ActivityJoin {
                                    continue;
                                }

                                let secret = if let Some(secret) =
                                    v.and_then(|v| v.get("secret").and_then(|v| v.as_str().map(|v| v.to_string())))
                                {
                                    secret
                                } else {
                                    continue;
                                };

                                *current_join_secret.lock().await = Some(secret);
                            }

                            // Do stuff with RPC connection.
                            if let Err(err) = (|| async {
                                if let Some(rpc) = rpc.lock().await.as_ref() {
                                    if let Some(activity) = current_activity.lock().await.as_ref() {
                                        rpc.set_activity(activity).await?;
                                    }
                                }

                                Ok::<_, anyhow::Error>(())
                            })()
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

        let client = Self {
            rpc,
            current_activity,
            current_join_secret,
        };
        client
    }

    pub fn set_current_activity(&self, activity: Option<rpc::activity::Activity>) {
        {
            let mut current_activity = self.current_activity.blocking_lock();
            if activity == *current_activity {
                return;
            }
            *current_activity = activity.clone();
        }

        if let Some(activity) = activity.as_ref() {
            let activity = activity.clone();
            let rpc = self.rpc.clone();
            // Do not block main thread on setting activity.
            tokio::task::spawn(async move {
                if let Some(rpc) = &*rpc.lock().await {
                    let _ = rpc.set_activity(&activity).await;
                }
            });
        }
    }

    pub fn has_current_join_secret(&self) -> bool {
        self.current_join_secret.blocking_lock().is_some()
    }

    pub fn take_current_join_secret(&self) -> Option<String> {
        self.current_join_secret.blocking_lock().take()
    }
}

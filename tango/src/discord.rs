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
    let mut title = i18n::LOCALES
        .lookup(language, &format!("game-{}", family))
        .unwrap();
    if let Some((patch_name, patch_version)) = patch.as_ref() {
        title.push_str(&format!(" + {} v{}", patch_name, patch_version));
    }
    GameInfo { title, family }
}

pub fn make_base_activity(game_info: Option<GameInfo>) -> discord_presence::models::Activity {
    discord_presence::models::Activity {
        details: game_info.as_ref().map(|gi| gi.title.clone()),
        assets: Some(discord_presence::models::ActivityAssets {
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
) -> discord_presence::models::Activity {
    discord_presence::models::Activity {
        state: Some(
            i18n::LOCALES
                .lookup(lang, "discord-presence.looking")
                .unwrap(),
        ),
        secrets: Some(discord_presence::models::ActivitySecrets {
            join: Some(link_code.to_string()),
            ..Default::default()
        }),
        party: Some(discord_presence::models::ActivityParty {
            id: Some(format!("party:{}", link_code)),
            size: Some((1, 2)),
        }),
        ..make_base_activity(game_info)
    }
}

pub fn make_single_player_activity(
    start_time: std::time::SystemTime,
    lang: &unic_langid::LanguageIdentifier,
    game_info: Option<GameInfo>,
) -> discord_presence::models::Activity {
    discord_presence::models::Activity {
        state: Some(
            i18n::LOCALES
                .lookup(lang, "discord-presence.in-single-player")
                .unwrap(),
        ),
        timestamps: Some(discord_presence::models::ActivityTimestamps {
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
) -> discord_presence::models::Activity {
    discord_presence::models::Activity {
        state: Some(
            i18n::LOCALES
                .lookup(lang, "discord-presence.in-lobby")
                .unwrap(),
        ),
        party: Some(discord_presence::models::ActivityParty {
            id: Some(format!("party:{}", link_code)),
            size: Some((2, 2)),
        }),
        ..make_base_activity(game_info)
    }
}

pub fn make_in_progress_activity(
    link_code: &str,
    start_time: std::time::SystemTime,
    lang: &unic_langid::LanguageIdentifier,
    game_info: Option<GameInfo>,
) -> discord_presence::models::Activity {
    discord_presence::models::Activity {
        state: Some(
            i18n::LOCALES
                .lookup(lang, "discord-presence.in-progress")
                .unwrap(),
        ),
        party: Some(discord_presence::models::ActivityParty {
            id: Some(format!("party:{}", link_code)),
            size: Some((2, 2)),
        }),
        timestamps: Some(discord_presence::models::ActivityTimestamps {
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
    handle: tokio::runtime::Handle,
    drpc: discord_presence::Client,
    current_activity:
        std::sync::Arc<parking_lot::Mutex<Option<discord_presence::models::Activity>>>,
    current_join_secret: std::sync::Arc<parking_lot::Mutex<Option<String>>>,
}

impl Client {
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        let drpc = discord_presence::Client::new(APP_ID);

        let current_activity: std::sync::Arc<
            parking_lot::Mutex<Option<discord_presence::models::Activity>>,
        > = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let current_join_secret = std::sync::Arc::new(parking_lot::Mutex::new(None));

        std::thread::spawn({
            let mut drpc = drpc.clone();
            let current_activity = current_activity.clone();
            let current_join_secret = current_join_secret.clone();
            move || {
                drpc.start();
                drpc.on_activity_join(move |e| {
                    *current_join_secret.lock() = e
                        .event
                        .get("secret")
                        .as_ref()
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                });

                loop {
                    let current_activity = current_activity.lock().clone();
                    if let Some(activity) = current_activity {
                        let _ = drpc.set_activity(|_| activity.clone());
                    } else {
                        let _ = drpc.clear_activity();
                    }
                    std::thread::sleep(std::time::Duration::from_secs(15));
                }
            }
        });

        let client = Self {
            handle,
            drpc,
            current_activity,
            current_join_secret,
        };
        client
    }

    pub fn set_current_activity(&self, activity: Option<discord_presence::models::Activity>) {
        let mut current_activity = self.current_activity.lock();
        if activity == *current_activity {
            return;
        }

        let mut drpc = self.drpc.clone();
        self.handle.spawn_blocking({
            let mut activity = activity.clone();
            move || {
                if let Some(activity) = activity.take() {
                    let _ = drpc.set_activity(move |_| activity);
                } else {
                    let _ = drpc.clear_activity();
                }
            }
        });
        *current_activity = activity;
    }

    pub fn has_current_join_secret(&self) -> bool {
        self.current_join_secret.lock().is_some()
    }

    pub fn take_current_join_secret(&self) -> Option<String> {
        self.current_join_secret.lock().take()
    }
}

fn make_activity(game_info: Option<GameInfo>) -> discord_presence::models::Activity {
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

pub struct GameInfo {
    pub family: String,
    pub title: String,
}

pub fn make_looking_activity(
    link_code: &str,
    game_info: Option<GameInfo>,
) -> discord_presence::models::Activity {
    discord_presence::models::Activity {
        state: Some("Looking for match".to_string()),
        secrets: Some(discord_presence::models::ActivitySecrets {
            join: Some(link_code.to_string()),
            ..Default::default()
        }),
        party: Some(discord_presence::models::ActivityParty {
            id: Some(format!("party:{}", link_code)),
            size: Some((1, 2)),
        }),
        ..make_activity(game_info)
    }
}

pub fn make_single_player_activity(
    link_code: &str,
    game_info: Option<GameInfo>,
) -> discord_presence::models::Activity {
    discord_presence::models::Activity {
        state: Some("In single player".to_string()),
        ..make_activity(game_info)
    }
}

pub fn make_in_lobby_activity(
    link_code: &str,
    game_info: Option<GameInfo>,
) -> discord_presence::models::Activity {
    discord_presence::models::Activity {
        state: Some("In lobby".to_string()),
        party: Some(discord_presence::models::ActivityParty {
            id: Some(format!("party:{}", link_code)),
            size: Some((2, 2)),
        }),
        ..make_activity(game_info)
    }
}

pub fn make_in_progress_activity(
    link_code: &str,
    start_time: std::time::SystemTime,
    game_info: Option<GameInfo>,
) -> discord_presence::models::Activity {
    discord_presence::models::Activity {
        state: Some("Match in progress".to_string()),
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
        ..make_activity(game_info)
    }
}

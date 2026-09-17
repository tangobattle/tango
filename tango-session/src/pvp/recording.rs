//! Replay storage adapters and recording metadata.

/// Where a match's recording goes.
///
/// The session composes the name — it encodes the timestamp, the
/// matchup and which seat we were, and both peers derive their own —
/// and hands over the bytes. What a name *refers to* is the host's:
/// a file under the replays directory on a desktop, a row in an object
/// store in a browser. This is the one place the live match assumed a
/// filesystem, and a browser is the host that doesn't have one.
pub trait ReplayStore: crate::platform::WasmNotSend + crate::platform::WasmNotSync {
    /// Open a recording. `name` carries no extension and no directory.
    fn create(&self, name: &str) -> std::io::Result<Recording>;
}

/// An open recording: where the bytes go, and how to find it again.
pub struct Recording {
    /// `Send` because [`tango_replay::Writer`] holds it as
    /// `Box<dyn Write + Send>`, which is what lets a desktop host move
    /// a match onto a thread.
    pub sink: Box<dyn std::io::Write + Send>,
    /// How this recording is identified afterwards — the file's path
    /// where there are files, and otherwise whatever key the host
    /// filed it under. Surfaced as
    /// [`PvpSession::replay_path`](super::PvpSession::replay_path).
    pub key: std::path::PathBuf,
}

/// Open the replay file + write its metadata frame, returning the writer
/// along with the path it records to (surfaced on the session so the
/// post-match results screen can offer playback). Everything the metadata
/// needs lives on `pre_match` (settings, seed, match clock, link code).
/// Name format mirrors the legacy app:
/// `YYYYMMDDhhmmss-<link_code>-<compat>-vs-<opponent>-p<idx>`.
pub(super) fn build_replay_writer(
    store: &dyn ReplayStore,
    pre_match: &crate::pvp::PreMatchData,
    // The sides' game impls, for each family's replay compatibility
    // version — the settings on `pre_match` only carry names.
    local_game: &'static tango_gamesupport::Game,
    remote_game: &'static tango_gamesupport::Game,
    local_player_index: u8,
    local_sram: &[u8],
    remote_sram: &[u8],
) -> Result<(tango_replay::Writer, std::path::PathBuf), crate::Error> {
    let link_code = &pre_match.terms.link_code;
    let local_settings = &pre_match.terms.local_settings;
    let remote_settings = &pre_match.terms.remote_settings;
    let local_gi = local_settings
        .game_info
        .as_ref()
        .ok_or(crate::Error::MissingGameInfo { side: "local" })?;
    let remote_gi = remote_settings
        .game_info
        .as_ref()
        .ok_or(crate::Error::MissingGameInfo { side: "remote" })?;
    let netplay_compat = local_gi
        .patch
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| local_gi.family_and_variant.0.clone());
    let ts = chrono::Local::now().format("%Y%m%d%H%M%S");
    // Direct sessions have no link code in their metadata —
    // substitute a stable placeholder here so the filename
    // doesn't end up with a double-dash where the slot would be.
    let filename_link_code = if link_code.is_empty() { "direct" } else { link_code };
    let raw_name = format!(
        "{ts}-{filename_link_code}-{netplay_compat}-vs-{}-p{}",
        remote_settings.nickname,
        local_player_index + 1
    );
    let safe_name: String = raw_name.chars().filter(|c| !"/\\?%*:|\"<>. ".contains(*c)).collect();
    let Recording { sink, key } = store.create(&safe_name)?;

    let local_side = Some(tango_replay::metadata::Side {
        nickname: local_settings.nickname.clone(),
        game_info: Some(tango_replay::metadata::GameInfo {
            rom_family: local_gi.family_and_variant.0.clone(),
            rom_variant: local_gi.family_and_variant.1 as u32,
            sim_version: local_game.pvp.sim_version(),
            patch: local_gi
                .patch
                .as_ref()
                .map(|p| tango_replay::metadata::game_info::Patch {
                    name: p.name.clone(),
                    version: p.version.to_string(),
                }),
        }),
        // The replay metadata proto (replay11) predates the
        // blind-setup inversion and still stores the
        // positive "reveal" sense.
        reveal_setup: !local_settings.blind_setup,
    });
    let remote_side = Some(tango_replay::metadata::Side {
        nickname: remote_settings.nickname.clone(),
        game_info: Some(tango_replay::metadata::GameInfo {
            rom_family: remote_gi.family_and_variant.0.clone(),
            rom_variant: remote_gi.family_and_variant.1 as u32,
            sim_version: remote_game.pvp.sim_version(),
            patch: remote_gi
                .patch
                .as_ref()
                .map(|p| tango_replay::metadata::game_info::Patch {
                    name: p.name.clone(),
                    version: p.version.to_string(),
                }),
        }),
        reveal_setup: !remote_settings.blind_setup,
    });
    // The recorder is where perspective enters the format: everything in
    // the file is absolute player order, so seat the two sides (and the
    // two saves) by our negotiated player index here and nowhere else.
    let (p1_side, p2_side, srams) = if local_player_index == 0 {
        (local_side, remote_side, [local_sram, remote_sram])
    } else {
        (remote_side, local_side, [remote_sram, local_sram])
    };
    let writer = tango_replay::Writer::new(
        sink,
        // SIO-engine stream: one continuous run of pair ticks.
        tango_replay::VERSION,
        local_player_index,
        tango_replay::Metadata {
            // The negotiated match clock, not the local wall clock: both
            // cores' cart RTC is pinned to this instant, and playback
            // re-primes pinned to `metadata.ts`, so recording the same
            // value is what makes playback reproduce the live match. Both
            // peers' replays of one match carry the identical ts.
            ts: pre_match.terms.match_ts,
            link_code: link_code.clone(),
            p1_side,
            p2_side,
            match_type: pre_match.terms.match_type.0 as u32,
            match_subtype: pre_match.terms.match_type.1 as u32,
        },
        pre_match.terms.rng_seed,
        [srams[0], srams[1]],
    )?;
    Ok((writer, key))
}

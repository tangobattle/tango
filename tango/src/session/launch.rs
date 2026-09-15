//! Prepare sessions from the desktop library and start their workers.

use super::runtime::{run_prefetch_pass, Pacer, RunningSession};
use super::{pvp, replay, singleplayer, training, PvpPanes, MAX_FRAME_DELAY, MIN_FRAME_DELAY};
use crate::library::Scanners;
use crate::platform::audio;
use crate::{config, selection};
use num_traits::ToPrimitive;

/// A fully owned session waiting to be installed. Dropping an unused launch
/// shuts down its workers; audio is connected only by `State::install`.
pub struct Launch {
    pub(super) runtime: RunningSession,
    pub(super) pvp_panes: Option<PvpPanes>,
    pub(super) replay_path: Option<std::path::PathBuf>,
}

impl std::fmt::Debug for Launch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Launch")
            .field("replay_path", &self.replay_path)
            .finish_non_exhaustive()
    }
}

/// Decode a replay, resolve its exact ROMs and patches, and start a
/// playback runtime ready for [`super::State::install`].
pub fn build_playback(
    scanners: &Scanners,
    config: &config::Config,
    audio_binder: &audio::LateBinder,
    path: &std::path::Path,
    // Have the prefetch pass double as the match-stats analysis — see
    // [`replay::PrefetchStatsJob`] and `App::replay_stats_takeover`.
    stats_job: Option<replay::PrefetchStatsJob>,
    // The recording's round boundaries when its analysis is already
    // cached, so the scrub bar draws them from the first frame.
    round_boundaries: Vec<u32>,
) -> anyhow::Result<Launch> {
    let f = std::fs::File::open(path)?;
    let replay = std::sync::Arc::new(tango_replay::Replay::decode(f)?);
    let resolved = crate::library::replays::resolve_roms(
        crate::library::storage(),
        &scanners.roms,
        &config.patches_path(),
        &replay.metadata,
    )?;
    let (session, workers, audio) = replay::ReplaySession::new(
        resolved.games,
        resolved.roms.map(std::sync::Arc::new),
        replay,
        // Both seats always share one engine, so either seat's rate is
        // the session's.
        resolved.games[0].pvp.tps().to_f32().unwrap(),
        audio_binder.sample_rate(),
        config.opponent_view != config::OpponentView::Off,
        stats_job,
        round_boundaries,
    )?;
    session.set_custom_screen_speedup(config.replay_custom_screen_speedup);
    // Three loops, three threads — ours to spawn, and ours to pace: the
    // playhead runs at the transport's speed, while the seek chase and
    // the prefetch pass run flat out.
    let (drive, seek, prefetch) = workers.split();
    let mut runtime = RunningSession::new(session, audio);
    runtime.add_thread(
        std::thread::Builder::new()
            .name("tango-sio-replay-drive".to_owned())
            .spawn(move || {
                let mut pacer = Pacer::new();
                let mut drive = drive;
                loop {
                    if drive.paused() {
                        // Park on the gate rather than spinning through
                        // ticks that do nothing; the cadence restarts on
                        // the wake so paused time accrues no debt.
                        drive.wait_while_paused();
                        pacer.resync();
                        continue;
                    }
                    use tango_session::Drive as _;
                    if !drive.tick() {
                        break;
                    }
                    pacer.wait(drive.fps_target());
                }
                use tango_session::Drive as _;
                drive.finish();
            })?,
    );
    runtime.add_thread(
        std::thread::Builder::new()
            .name("tango-sio-replay-seek".to_owned())
            .spawn(move || {
                // Park until the transport asks for a seek, then walk the
                // whole chase in one go — a thread has nothing better to
                // do, and the pair is better held once than per tick.
                while seek.wait_for_request() {
                    while seek.step(u32::MAX) {}
                }
            })?,
    );
    runtime.add_thread(
        std::thread::Builder::new()
            .name("tango-sio-replay-prefetch".to_owned())
            .spawn(move || run_prefetch_pass(prefetch))?,
    );
    Ok(Launch {
        runtime,
        pvp_panes: None,
        replay_path: Some(path.to_owned()),
    })
}

/// Build the live PvP session from the netplay handoff data
/// plus the local selection + scanners, along with the [`PvpPanes`]
/// presentation state (both sides' Loadeds + save-view panels) the
/// App installs beside it. Async because PvpSession::new awaits the
/// lobby loop's receiver handoff, and because remote-side rom
/// resolution might apply a patch.
pub async fn spawn_pvp(
    scanners: Scanners,
    config: config::Config,
    audio_binder: audio::LateBinder,
    local_game: crate::library::rom::GameRef,
    local_patch: Option<(String, semver::Version)>,
    pre_match: crate::netplay::PreMatchData,
) -> anyhow::Result<Launch> {
    let local_rom_bytes = crate::library::rom::load(
        crate::library::storage(),
        &scanners.roms,
        &config.patches_path(),
        local_game,
        local_patch.as_ref().map(|(name, version)| (name.as_str(), version)),
    )?;

    let remote_gi = pre_match
        .remote_settings
        .game_info
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("remote settings missing game info"))?;
    let remote_game = crate::library::game::find_by_family_and_variant(
        &remote_gi.family_and_variant.0,
        remote_gi.family_and_variant.1,
    )
    .ok_or_else(|| anyhow::anyhow!("unknown remote rom"))?;
    let remote_rom_bytes = crate::library::rom::load(
        crate::library::storage(),
        &scanners.roms,
        &config.patches_path(),
        remote_game,
        remote_gi.patch.as_ref().map(|p| (p.name.as_str(), &p.version)),
    )?;

    // Always build the remote model long enough to validate the exact committed
    // save the match will run. A valid blinded setup is still discarded from
    // the drawer below; an invalid one keeps only the structured violations
    // needed by the advisory warning.
    let remote_prepared = {
        let remote_save = remote_game
            .parse_save(&pre_match.remote_save_data)
            .map_err(|e| anyhow::anyhow!("parse remote save: {e:?}"))?;
        // `remote_rom_bytes` is already the patched image we run in the
        // session, so resolve the matching `rom_overrides` + charset and
        // hand both straight to preparation — no second BPS apply.
        let applied_patch = remote_gi.patch.as_ref().and_then(|p| {
            let patches = scanners.patches.read();
            let version_meta = patches.version(&p.name, &p.version)?;
            Some(crate::selection::AppliedPatch {
                name: p.name.clone(),
                version: p.version.clone(),
                rom_overrides: version_meta.rom_overrides_for(remote_game),
            })
        });
        crate::selection::prepare_from_patched_rom(
            remote_game,
            remote_rom_bytes.clone(),
            std::path::PathBuf::new(),
            remote_save,
            applied_patch,
        )
    };
    let opponent_build_warnings = selection::editor(remote_game).validate_save(&remote_prepared);
    let remote_loaded = selection::editor(remote_game).load(remote_prepared);
    let opponent_loaded = (!pre_match.remote_settings.blind_setup).then_some(remote_loaded);

    // Build the local-side LoadedSave so the in-session "my setup"
    // toggle can render the same save-view we use for the
    // opponent panel.
    let local_loaded = {
        let local_save = local_game
            .parse_save(&pre_match.local_save_data)
            .map_err(|e| anyhow::anyhow!("parse local save: {e:?}"))?;
        // Same as the opponent side: `local_rom_bytes` is already
        // patched, so layer the overrides on via `from_patched_rom`
        // instead of re-applying the BPS patch.
        let applied_patch = local_patch.as_ref().and_then(|(name, version)| {
            let patches = scanners.patches.read();
            let version_meta = patches.version(name, version)?;
            Some(crate::selection::AppliedPatch {
                name: name.clone(),
                version: version.clone(),
                rom_overrides: version_meta.rom_overrides_for(local_game),
            })
        });
        crate::selection::from_patched_rom(
            local_game,
            local_rom_bytes.clone(),
            std::path::PathBuf::new(),
            local_save,
            applied_patch,
        )
    };
    let (session, boot, audio) = pvp::PvpSession::new(pvp::PvpSessionArgs {
        local_game,
        local_rom: std::sync::Arc::new(local_rom_bytes),
        remote_game,
        remote_rom: std::sync::Arc::new(remote_rom_bytes),
        pre_match,
        // Presentation delay is purely local — read straight from config (clamped
        // to the supported range), not negotiated with the peer.
        frame_delay: config.frame_delay.clamp(MIN_FRAME_DELAY, MAX_FRAME_DELAY),
        disable_bgm: config.disable_bgm_in_pvp,
        replays: Some(&pvp::DirReplayStore(config.replays_path())),
        cache_path: &config.cache_path(),
        expected_fps: local_game.pvp.tps().to_f32().unwrap(),
        sample_rate: audio_binder.sample_rate(),
    })
    .await?;
    // The drive thread boots the pair on its first tick — priming is
    // seconds of emulation on a DS-class game, and the session is
    // installed and on screen (saying so) for all of it, rather than
    // the user waiting it out on the lobby.
    let mut runtime = RunningSession::new(session, audio);
    runtime.spawn_driver("tango-sio-drive", boot)?;
    Ok(Launch {
        runtime,
        pvp_panes: Some(PvpPanes {
            local_loaded: Some(local_loaded),
            opponent_loaded,
            opponent_build_warnings,
            build_warning_dismissed: false,
            build_warning_violations_expanded: false,
            // Clamped on the way in: the persisted pair predates the
            // current bounds on an older config, or the window it was
            // sized against is gone.
            pane_widths: [0, 1].map(|i| {
                config.pvp_setup_pane_widths[i]
                    .clamp(super::view::SETUP_PANE_MIN_WIDTH, super::view::SETUP_PANE_MAX_WIDTH)
            }),
            pane_drag: None,
        }),
        replay_path: None,
    })
}

/// Boot the supplied selection in single-player mode. Caller must
/// already have a complete (game + rom + save) LoadedSave — there's no
/// fallback for missing pieces, so the Play button is responsible for
/// gating.
pub fn spawn_singleplayer(
    scanners: &Scanners,
    config: &config::Config,
    audio_binder: &audio::LateBinder,
    loaded: &selection::LoadedSave,
) -> anyhow::Result<Launch> {
    let game = loaded.game;
    let rom_bytes = crate::library::rom::load(
        crate::library::storage(),
        &scanners.roms,
        &config.patches_path(),
        game,
        loaded.patch.as_ref().map(|p| (p.name.as_str(), &p.version)),
    )?;
    // Single-player runs with an in-memory save; the desktop runtime
    // periodically writes changes back to this file and flushes on close.
    let save = std::fs::read(&loaded.save_path)?;
    let (session, driver, audio) = singleplayer::SinglePlayerSession::new(
        game,
        std::sync::Arc::new(rom_bytes),
        Some(save.clone()),
        // Leave the cart clock on the real one, as it has always been.
        None,
        game.pvp.tps().to_f32().unwrap(),
        audio_binder.sample_rate(),
    )?;
    let mut runtime = RunningSession::new(session, audio);
    runtime.spawn_driver("singleplayer", driver)?;
    runtime.save_to(loaded.save_path.clone(), save);
    Ok(Launch {
        runtime,
        pvp_panes: None,
        replay_path: None,
    })
}

/// Boot the supplied selection in training mode — a local link battle
/// (both cores run this selection) against a do-nothing dummy controller
/// ([`training::NoopController`]) wired in as the integration seam. Same
/// gating contract as [`spawn_singleplayer`]: the caller must already
/// hold a complete (game + rom + save) LoadedSave.
pub fn spawn_training(
    scanners: &Scanners,
    config: &config::Config,
    audio_binder: &audio::LateBinder,
    loaded: &selection::LoadedSave,
) -> anyhow::Result<Launch> {
    let game = loaded.game;
    let rom_bytes = crate::library::rom::load(
        crate::library::storage(),
        &scanners.roms,
        &config.patches_path(),
        game,
        loaded.patch.as_ref().map(|p| (p.name.as_str(), &p.version)),
    )?;
    // The battle runs off an in-memory SRAM image (same as PvP), so
    // nothing training does is written back to the save file.
    let (session, driver, audio) = training::TrainingSession::new(
        game,
        std::sync::Arc::new(rom_bytes),
        loaded.editor.sram(loaded),
        std::time::SystemTime::now(),
        rand::random(),
        game.pvp.tps().to_f32().unwrap(),
        audio_binder.sample_rate(),
        Box::new(training::NoopController),
    )?;
    session.set_opponent_visible(config.opponent_view != config::OpponentView::Off);
    let mut runtime = RunningSession::new(session, audio);
    runtime.spawn_driver("training", driver)?;
    Ok(Launch {
        runtime,
        pvp_panes: None,
        replay_path: None,
    })
}

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
    pre_match: crate::netplay::PreMatchData,
) -> anyhow::Result<Launch> {
    let prepared = {
        tango_library::loadout::Resolver {
            storage: crate::library::storage(),
            roms: &scanners.roms,
            patches: &scanners.patches,
            patches_path: &config.patches_path(),
        }
        .prepare_match(
            &pre_match.terms.local_settings,
            &pre_match.terms.remote_settings,
            [&pre_match.terms.local_save_data, &pre_match.terms.remote_save_data],
        )?
    };
    let local_game = prepared.local.prepared.game;
    let remote_game = prepared.remote.prepared.game;
    let local_rom = prepared.local.rom;
    let remote_rom = prepared.remote.rom;
    let opponent_build_warnings =
        selection::editor(remote_game).build_warnings(&prepared.remote.prepared, prepared.remote.validation.as_ref());
    let opponent_loaded = (!pre_match.terms.remote_settings.blind_setup)
        .then(|| selection::editor(remote_game).load(prepared.remote.prepared));
    let local_loaded = selection::editor(local_game).load(prepared.local.prepared);
    let (session, boot, audio) = pvp::PvpSession::new(pvp::PvpSessionArgs {
        local_game,
        local_rom,
        remote_game,
        remote_rom,
        pre_match,
        // Presentation delay is purely local — read straight from config (clamped
        // to the supported range), not negotiated with the peer.
        frame_delay: config.frame_delay.clamp(MIN_FRAME_DELAY, MAX_FRAME_DELAY),
        disable_bgm: config.disable_bgm_in_pvp,
        replays: Some(&super::recording::DirReplayStore(config.replays_path())),
        stats_sink: Some(std::sync::Arc::new(super::recording::StatsCache {
            cache_path: config.cache_path(),
            replays_path: config.replays_path(),
        })),
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

/// Boot an exact selection from its saved file, independently of editor state.
pub fn spawn_singleplayer(
    scanners: &Scanners,
    config: &config::Config,
    audio_binder: &audio::LateBinder,
    selection: &tango_library::loadout::LoadoutSelection,
) -> anyhow::Result<Launch> {
    let resolved = tango_library::loadout::Resolver {
        storage: crate::library::storage(),
        roms: &scanners.roms,
        patches: &scanners.patches,
        patches_path: &config.patches_path(),
    }
    .resolve(selection, None)?;
    let game = resolved.prepared.game;
    let save = resolved.sram;
    let (session, driver, audio) = singleplayer::SinglePlayerSession::new(
        game,
        resolved.rom,
        Some(save.clone()),
        // Leave the cart clock on the real one, as it has always been.
        None,
        game.pvp.tps().to_f32().unwrap(),
        audio_binder.sample_rate(),
    )?;
    let mut runtime = RunningSession::new(session, audio);
    runtime.spawn_driver("singleplayer", driver)?;
    runtime.save_to(resolved.prepared.save_path, save);
    Ok(Launch {
        runtime,
        pvp_panes: None,
        replay_path: None,
    })
}

/// Boot the supplied selection in training mode — a local link battle
/// (both cores run this selection) against a do-nothing dummy controller
/// ([`training::NoopController`]). The caller supplies a checksum-correct
/// snapshot, which may include staged edits. Training never writes it back.
pub fn spawn_training(
    scanners: &Scanners,
    config: &config::Config,
    audio_binder: &audio::LateBinder,
    selection: &tango_library::loadout::LoadoutSelection,
    snapshot: &[u8],
) -> anyhow::Result<Launch> {
    let resolved = tango_library::loadout::Resolver {
        storage: crate::library::storage(),
        roms: &scanners.roms,
        patches: &scanners.patches,
        patches_path: &config.patches_path(),
    }
    .resolve(selection, Some(snapshot))?;
    let game = resolved.prepared.game;
    let save = resolved.sram;
    let (session, driver, audio) = training::TrainingSession::new(
        game,
        resolved.rom,
        save,
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
